//! P0-07の検証用プラグイン（ABI v1、WASIなし）。製品コードではない。
//! マニフェストの宣言: `vault.read`、`net.connect`。

#[link(wasm_import_module = "hisui:v1")]
unsafe extern "C" {
    fn log(ptr: *const u8, len: usize) -> i32;
    fn vault_read(path_ptr: *const u8, path_len: usize, out_ptr: *mut u8, out_cap: usize) -> i64;
    fn net_connect(ptr: *const u8, len: usize) -> i32;
}

fn read(path: &str) -> i64 {
    let mut buf = [0u8; 256];
    let r = unsafe { vault_read(path.as_ptr(), path.len(), buf.as_mut_ptr(), buf.len()) };
    if r > 0 {
        let msg = format!("read {path}: {:?}", core::str::from_utf8(&buf[..r as usize]).unwrap_or("?"));
        unsafe { log(msg.as_ptr(), msg.len()) };
    }
    r
}

#[unsafe(no_mangle)]
pub extern "C" fn hisui_abi_version() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn run(scenario: i32) -> i64 {
    match scenario {
        1 => read("notes/a.md"),
        2 => read("notes/../secret.md"),
        3 => read("/etc/passwd"),
        4 => read("private/b.md"),
        5 => unsafe { net_connect(b"https://example.com".as_ptr(), 19) as i64 },
        6 => read("notes2/x.md"),
        7 => read("notes/./a.md"),
        8 => read("Notes/a.md"),
        9 => read("notes/missing.md"),
        // 性能: 許可されるパスを10万回読む
        100 => {
            let mut buf = [0u8; 256];
            let p = "notes/a.md";
            let mut ok = 0;
            for _ in 0..100_000 {
                if unsafe { vault_read(p.as_ptr(), p.len(), buf.as_mut_ptr(), buf.len()) } > 0 {
                    ok += 1;
                }
            }
            ok
        }
        _ => -100,
    }
}
