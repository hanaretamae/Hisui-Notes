//! P0-07: プラグインABI（コアモジュール + 独自ABI）と権限制御の検証。製品コードではない。

mod abi;
mod broker;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use abi::*;
use broker::{Broker, Decision};

const SAMPLE: Manifest = Manifest { id: "sample", capabilities: &["vault.read", "net.connect"] };
const READ_ONLY: Manifest = Manifest { id: "read-only", capabilities: &["vault.read"] };

static mut FAILED: u32 = 0;

fn expect(name: &str, ok: bool, detail: String) {
    println!("{} {name:<58} {detail}", if ok { "PASS" } else { "FAIL" });
    if !ok {
        unsafe { FAILED += 1 };
    }
}

fn wat(src: &str) -> Vec<u8> {
    wat::parse_str(src).unwrap()
}

/// 正しいエクスポートを持つ最小モジュールの雛形。`imports` と `body`（run の本体）を差し込む。
fn module_with(imports: &str, body: &str, version: i32) -> Vec<u8> {
    wat(&format!(
        r#"(module
            {imports}
            (memory (export "memory") 1)
            (func (export "hisui_abi_version") (result i32) i32.const {version})
            (func (export "run") (param i32) (result i64) {body}))"#
    ))
}

fn main() {
    let guest = std::fs::read(
        std::env::args().nth(1).unwrap_or("../guest/target/wasm32-unknown-unknown/release/sample_plugin.wasm".into()),
    )
    .unwrap();
    let engine = engine();
    let broker = Arc::new(Broker::new());
    let vault: Arc<HashMap<String, String>> = Arc::new(
        [("notes/a.md", "# A"), ("secret.md", "SECRET"), ("private/b.md", "PRIVATE"), ("notes2/x.md", "X")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    );
    // 利用者による付与（Vault範囲 notes/ の読み取り、example.com への接続）
    broker.grant("extension:sample", "vault.read", "notes");
    broker.grant("extension:sample", "net.connect", "https://example.com");

    println!("== 1. 実行時の権限検査（Rustのプラグイン）");
    let mut p = load(&engine, &guest, &SAMPLE, broker.clone(), vault.clone()).expect("load");
    let cases: &[(i32, &str, i64)] = &[
        (1, "範囲内の読み取り notes/a.md", 3),
        (7, "正規化で範囲内になる notes/./a.md", 3),
        (9, "範囲内だが存在しない notes/missing.md", E_NOT_FOUND),
        (2, "`..` を含む notes/../secret.md", E_DENIED),
        (3, "絶対パス /etc/passwd", E_DENIED),
        (4, "範囲外 private/b.md", E_DENIED),
        (6, "接頭辞が文字列として一致するだけ notes2/x.md", E_DENIED),
        (8, "大文字・小文字の違い Notes/a.md", E_DENIED),
        (5, "external（net.connect）、確認UIなし", E_CONFIRM_UNAVAILABLE),
    ];
    for &(s, name, want) in cases {
        let got = p.run(s);
        expect(name, got == Ok(want), format!("expected {want}, got {got:?}"));
    }
    println!("    plugin log: {:?}", p.store.data().log);
    println!("    audit: {:?}", p.store.data().audit.iter().take(3).collect::<Vec<_>>());
    let leaked = p.store.data().audit.iter().any(|a| a.contains("SECRET") || a.contains("# A"));
    expect("監査ログに文書内容が含まれない", !leaked, String::new());

    broker.revoke("extension:sample", "vault.read");
    let got = p.run(1);
    expect("取り消しの直後の読み取りは拒否", got == Ok(E_DENIED), format!("{got:?}"));
    broker.grant("extension:sample", "vault.read", "notes");

    println!("\n== 2. 読み込み時の検査（WATの不正なモジュール）");
    let load_case = |name: &str, wasm: Vec<u8>, manifest: &Manifest, want: &str| {
        let r = load(&engine, &wasm, manifest, broker.clone(), vault.clone());
        let got = match &r {
            Ok(_) => "Ok".to_string(),
            Err(e) => format!("{e:?}"),
        };
        expect(name, got.starts_with(want), got);
    };
    load_case(
        "宣言していない権限の関数をインポート",
        module_with(r#"(import "hisui:v1" "net_connect" (func (param i32 i32) (result i32)))"#, "i64.const 0", 1),
        &READ_ONLY,
        "UndeclaredCapability",
    );
    load_case(
        "WASIの関数をインポート",
        module_with(r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#, "i64.const 0", 1),
        &SAMPLE,
        "ForbiddenImport",
    );
    load_case(
        "存在しないホスト関数をインポート",
        module_with(r#"(import "hisui:v1" "process_spawn" (func (param i32 i32) (result i32)))"#, "i64.const 0", 1),
        &SAMPLE,
        "UnknownHostFunction",
    );
    load_case(
        "ホストのメモリをインポート",
        wat(r#"(module (import "hisui:v1" "memory" (memory 1))
                (func (export "hisui_abi_version") (result i32) i32.const 1)
                (func (export "run") (param i32) (result i64) i64.const 0))"#),
        &SAMPLE,
        "ForbiddenImport",
    );
    load_case("ABIの版が異なる", module_with("", "i64.const 0", 2), &SAMPLE, "AbiVersion(2)");
    load_case(
        "必須のエクスポートが無い",
        wat(r#"(module (memory (export "memory") 1) (func (export "run") (param i32) (result i64) i64.const 0))"#),
        &SAMPLE,
        "MissingExport",
    );
    load_case("WASMとして不正なバイト列", b"\0asm\x01\0\0\0garbage".to_vec(), &SAMPLE, "Invalid");

    println!("\n== 3. 不正な引数（ホストが停止しないこと）");
    let vr = r#"(import "hisui:v1" "vault_read" (func $vr (param i32 i32 i32 i32) (result i64)))
                (data (i32.const 16) "notes/a.md")"#;
    let arg_case = |name: &str, body: &str, want: i64| {
        let mut p = load(&engine, &module_with(vr, body, 1), &SAMPLE, broker.clone(), vault.clone()).unwrap();
        let got = p.run(0);
        expect(name, got == Ok(want), format!("expected {want}, got {got:?}"));
    };
    arg_case("パスのポインターが範囲外", "i32.const 0xFFFFFFF0 i32.const 10 i32.const 1024 i32.const 64 call $vr", E_BAD_ARGS);
    arg_case("パスの長さが上限超過", "i32.const 16 i32.const 0x7FFFFFFF i32.const 1024 i32.const 64 call $vr", E_BAD_ARGS);
    arg_case("ポインター＋長さが桁あふれ", "i32.const 16 i32.const -1 i32.const 1024 i32.const 64 call $vr", E_BAD_ARGS);
    arg_case("出力先が範囲外", "i32.const 16 i32.const 10 i32.const 65530 i32.const 64 call $vr", E_BAD_ARGS);
    arg_case("出力先が小さい", "i32.const 16 i32.const 10 i32.const 1024 i32.const 1 call $vr", E_BUF_TOO_SMALL);
    arg_case("UTF-8として不正なパス", "(i32.store8 (i32.const 0) (i32.const 0xFF)) i32.const 0 i32.const 1 i32.const 1024 i32.const 64 call $vr", E_BAD_ARGS);

    println!("\n== 4. 性能");
    let n = 1_000_000;
    let t = Instant::now();
    let mut allow = 0;
    for _ in 0..n {
        if broker.check("extension:sample", "vault.read", "notes/a.md") == Decision::Allow {
            allow += 1;
        }
    }
    let per = t.elapsed().as_secs_f64() * 1e9 / n as f64;
    expect("check() 1回 ≤ 1 µs（ADR-0008 §9）", allow == n && per <= 1000.0, format!("{per:.0} ns/check"));
    let t = Instant::now();
    let r = p.run(100);
    let per = t.elapsed().as_secs_f64() * 1e9 / 100_000.0;
    println!("     拡張からの vault_read 1回（検査・コピーを含む） {per:.0} ns, result {r:?}");

    let failed = unsafe { FAILED };
    println!("\nfailed: {failed}");
    std::process::exit(if failed == 0 { 0 } else { 1 });
}
