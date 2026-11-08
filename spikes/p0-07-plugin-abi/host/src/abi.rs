//! コアモジュール + 独自ABI（ADR-0005）の検証用実装。
//!
//! ABI v1:
//! - インポートはモジュール名 `hisui:v1` の関数のみ。メモリ・テーブル・グローバルのインポート、WASI等の他モジュールは拒否する。
//! - エクスポート: `memory`、`hisui_abi_version() -> i32`（1）、`run(i32) -> i64`。
//! - 文字列・バッファは (ポインター, 長さ) で受け渡す。戻り値が負の場合はエラーコード。

use std::collections::HashMap;
use std::sync::Arc;

use wasmi::{Caller, Config, Engine, Extern, ExternType, Instance, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder};

use crate::broker::{Broker, Decision};

pub const ABI_MODULE: &str = "hisui:v1";
pub const ABI_VERSION: i32 = 1;

pub const E_DENIED: i64 = -1;
pub const E_NOT_FOUND: i64 = -2;
pub const E_BAD_ARGS: i64 = -3;
pub const E_BUF_TOO_SMALL: i64 = -4;
pub const E_CONFIRM_UNAVAILABLE: i64 = -5;

/// ホスト関数とその実行に必要な権限。
pub const HOST_FUNCTIONS: &[(&str, Option<&str>)] = &[("log", None), ("vault_read", Some("vault.read")), ("net_connect", Some("net.connect"))];

/// 1回の受け渡しの上限（ホストのメモリを守る）。
const MAX_ARG: usize = 64 * 1024;

pub struct Manifest {
    pub id: &'static str,
    pub capabilities: &'static [&'static str],
}

pub struct HostState {
    /// 主体ID。ホストが読み込み時に割り当て、拡張からは指定できない。
    pub principal: String,
    pub broker: Arc<Broker>,
    pub confirm_ui_available: bool,
    pub vault: Arc<HashMap<String, String>>,
    pub log: Vec<String>,
    pub audit: Vec<String>,
    limits: StoreLimits,
}

#[derive(Debug)]
pub enum LoadError {
    Invalid(String),
    ForbiddenImport(String),
    UnknownHostFunction(String),
    UndeclaredCapability(String, &'static str),
    MissingExport(&'static str),
    AbiVersion(i32),
    Instantiate(String),
}

pub fn engine() -> Engine {
    let mut c = Config::default();
    c.consume_fuel(true);
    Engine::new(&c)
}

/// 読み込み時の検査（インスタンス化の前）。
pub fn validate(module: &Module, manifest: &Manifest) -> Result<(), LoadError> {
    for imp in module.imports() {
        let (m, n) = (imp.module(), imp.name());
        if m != ABI_MODULE || !matches!(imp.ty(), ExternType::Func(_)) {
            return Err(LoadError::ForbiddenImport(format!("{m}.{n}")));
        }
        let Some((_, cap)) = HOST_FUNCTIONS.iter().find(|(f, _)| *f == n) else {
            return Err(LoadError::UnknownHostFunction(n.into()));
        };
        if let Some(cap) = cap
            && !manifest.capabilities.contains(cap)
        {
            return Err(LoadError::UndeclaredCapability(n.into(), cap));
        }
    }
    for name in ["memory", "hisui_abi_version", "run"] {
        if module.get_export(name).is_none() {
            return Err(LoadError::MissingExport(name));
        }
    }
    Ok(())
}

fn mem(caller: &Caller<'_, HostState>) -> Option<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Some(m),
        _ => None,
    }
}

/// 拡張のメモリから範囲を検査して読む。範囲外・上限超過は None。
fn read_guest(caller: &Caller<'_, HostState>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let (ptr, len) = (ptr as u32 as usize, len as u32 as usize);
    if len > MAX_ARG {
        return None;
    }
    let m = mem(caller)?;
    m.data(caller).get(ptr..ptr.checked_add(len)?).map(<[u8]>::to_vec)
}

fn write_guest(caller: &mut Caller<'_, HostState>, ptr: i32, cap: i32, bytes: &[u8]) -> i64 {
    let (ptr, cap) = (ptr as u32 as usize, cap as u32 as usize);
    let Some(m) = mem(caller) else { return E_BAD_ARGS };
    let data = m.data_mut(&mut *caller);
    let Some(end) = ptr.checked_add(cap) else { return E_BAD_ARGS };
    let Some(dst) = data.get_mut(ptr..end) else { return E_BAD_ARGS };
    if bytes.len() > dst.len() {
        return E_BUF_TOO_SMALL;
    }
    dst[..bytes.len()].copy_from_slice(bytes);
    bytes.len() as i64
}

fn decide(state: &mut HostState, cap: &str, scope: &str) -> Result<(), i64> {
    let d = state.broker.check(&state.principal, cap, scope);
    // 監査ログには主体・権限・結果・範囲（Vault相対パス／オリジン）のみ。内容は記録しない。
    state.audit.push(format!("{} {cap} {scope:?} -> {d:?}", state.principal));
    match d {
        Decision::Allow => Ok(()),
        Decision::Deny => Err(E_DENIED),
        // 確認UIを表示できない場合は拒否（ADR-0008 §4）。この検証では確認UIは無い。
        Decision::Confirm if !state.confirm_ui_available => Err(E_CONFIRM_UNAVAILABLE),
        Decision::Confirm => Err(E_DENIED),
    }
}

fn linker(engine: &Engine) -> Linker<HostState> {
    let mut l = Linker::new(engine);
    l.func_wrap(ABI_MODULE, "log", |caller: Caller<'_, HostState>, ptr: i32, len: i32| -> i32 {
        let Some(b) = read_guest(&caller, ptr, len) else { return E_BAD_ARGS as i32 };
        let s = String::from_utf8_lossy(&b).into_owned();
        let mut caller = caller;
        caller.data_mut().log.push(s);
        0
    })
    .unwrap();
    l.func_wrap(
        ABI_MODULE,
        "vault_read",
        |mut caller: Caller<'_, HostState>, pptr: i32, plen: i32, optr: i32, ocap: i32| -> i64 {
            let Some(p) = read_guest(&caller, pptr, plen) else { return E_BAD_ARGS };
            let Ok(path) = String::from_utf8(p) else { return E_BAD_ARGS };
            if let Err(e) = decide(caller.data_mut(), "vault.read", &path) {
                return e;
            }
            // 許可された場合も、正規化後のパスで読む
            let key = crate::broker::normalize_vault_path(&path).unwrap().join("/");
            let Some(content) = caller.data().vault.get(&key).cloned() else { return E_NOT_FOUND };
            write_guest(&mut caller, optr, ocap, content.as_bytes())
        },
    )
    .unwrap();
    l.func_wrap(ABI_MODULE, "net_connect", |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> i32 {
        let Some(b) = read_guest(&caller, ptr, len) else { return E_BAD_ARGS as i32 };
        let Ok(origin) = String::from_utf8(b) else { return E_BAD_ARGS as i32 };
        match decide(caller.data_mut(), "net.connect", &origin) {
            Ok(()) => 0,
            Err(e) => e as i32,
        }
    })
    .unwrap();
    l
}

pub struct Plugin {
    pub store: Store<HostState>,
    pub instance: Instance,
}

pub fn load(
    engine: &Engine,
    wasm: &[u8],
    manifest: &Manifest,
    broker: Arc<Broker>,
    vault: Arc<HashMap<String, String>>,
) -> Result<Plugin, LoadError> {
    let module = Module::new(engine, wasm).map_err(|e| LoadError::Invalid(e.to_string()))?;
    validate(&module, manifest)?;
    let state = HostState {
        principal: format!("extension:{}", manifest.id),
        broker,
        confirm_ui_available: false,
        vault,
        log: vec![],
        audit: vec![],
        limits: StoreLimitsBuilder::new().memory_size(16 << 20).build(),
    };
    let mut store = Store::new(engine, state);
    store.limiter(|s| &mut s.limits);
    store.set_fuel(100_000_000).unwrap();
    let instance = linker(engine).instantiate_and_start(&mut store, &module).map_err(|e| LoadError::Instantiate(e.to_string()))?;
    let v = instance
        .get_typed_func::<(), i32>(&store, "hisui_abi_version")
        .map_err(|_| LoadError::MissingExport("hisui_abi_version"))?
        .call(&mut store, ())
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;
    if v != ABI_VERSION {
        return Err(LoadError::AbiVersion(v));
    }
    Ok(Plugin { store, instance })
}

impl Plugin {
    pub fn run(&mut self, scenario: i32) -> Result<i64, String> {
        self.store.set_fuel(1_000_000_000).unwrap();
        let f = self.instance.get_typed_func::<i32, i64>(&self.store, "run").map_err(|e| e.to_string())?;
        f.call(&mut self.store, scenario).map_err(|e| e.to_string())
    }
}
