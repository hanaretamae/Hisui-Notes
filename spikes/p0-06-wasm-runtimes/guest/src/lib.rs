//! P0-06の計測用ゲスト（wasm32-unknown-unknown、WASIなし）。製品コードではない。
//!
//! ABI: ホストは `alloc` で確保した領域に文書を書き込み、ポインターと長さを渡す。
//! 同じモジュールを2つインスタンス化し、片方を「パーサー」、もう片方を「拡張」として使う。

use std::cell::RefCell;

thread_local! {
    static DOC: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[link(wasm_import_module = "host")]
unsafe extern "C" {
    /// 拡張パーサーの規則をホスト経由で呼び出す（ADR-0004 §3）。戻り値は認識した長さ（0は不認識）。
    fn call_rule(pos: u32) -> u32;
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len as usize);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

fn input(ptr: *const u8, len: u32) -> &'static str {
    // ホストは有効なUTF-8を書き込む。計測用のため検査を省かない。
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    std::str::from_utf8(bytes).expect("utf-8")
}

/// Markdownを解析し、イベント数を返す（解析の負荷の代わり）。
#[unsafe(no_mangle)]
pub extern "C" fn parse(ptr: *const u8, len: u32) -> u32 {
    use pulldown_cmark::{Options, Parser};
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_WIKILINKS;
    Parser::new_ext(input(ptr, len), opts)
        .into_offset_iter()
        .count() as u32
}

/// 開始文字 `[` の位置ごとにホスト経由で拡張の規則を呼ぶ。呼び出し回数と認識した長さの合計を返す。
#[unsafe(no_mangle)]
pub extern "C" fn scan_hooks(ptr: *const u8, len: u32) -> u64 {
    let s = input(ptr, len).as_bytes();
    let (mut calls, mut total) = (0u64, 0u64);
    for (i, &b) in s.iter().enumerate() {
        if b == b'[' {
            calls += 1;
            total += unsafe { call_rule(i as u32) } as u64;
        }
    }
    (calls << 32) | (total & 0xffff_ffff)
}

/// `scan_hooks` と同じ走査で、ホストを呼ばない場合（呼び出しの費用を差し引くための基準）。
#[unsafe(no_mangle)]
pub extern "C" fn scan_base(ptr: *const u8, len: u32) -> u64 {
    let s = input(ptr, len).as_bytes();
    let (mut calls, mut total) = (0u64, 0u64);
    for (i, &b) in s.iter().enumerate() {
        if b == b'[' {
            calls += 1;
            total += std::hint::black_box(i as u32 & 0) as u64;
        }
    }
    (calls << 32) | (total & 0xffff_ffff)
}

/// 拡張側: 文書の写しを保持する。
#[unsafe(no_mangle)]
pub extern "C" fn set_doc(ptr: *const u8, len: u32) {
    let s = input(ptr, len).as_bytes().to_vec();
    DOC.with(|d| *d.borrow_mut() = s);
}

/// 拡張側の規則: `pos` から `[[...]]` を認識したらその長さを返す。
#[unsafe(no_mangle)]
pub extern "C" fn rule(pos: u32) -> u32 {
    DOC.with(|d| {
        let d = d.borrow();
        let s = &d[pos as usize..];
        if !s.starts_with(b"[[") {
            return 0;
        }
        // 同じ行の中だけを探す（長い探索を避ける）
        for (i, w) in s.windows(2).enumerate().skip(2).take(256) {
            if w == b"]]" {
                return i as u32 + 2;
            }
            if w[0] == b'\n' {
                break;
            }
        }
        0
    })
}

/// 実行時間の上限の確認用: 終わらないループ。
#[unsafe(no_mangle)]
pub extern "C" fn spin() -> u32 {
    let mut x: u32 = 1;
    loop {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        if x == 0 {
            return x;
        }
    }
}

/// メモリの上限の確認用: 64 KiBのページを増やし続け、失敗するまでに増やせたページ数を返す。
#[unsafe(no_mangle)]
pub extern "C" fn grow() -> u32 {
    let mut n = 0;
    while core::arch::wasm32::memory_grow(0, 1) != usize::MAX {
        n += 1;
    }
    n
}
