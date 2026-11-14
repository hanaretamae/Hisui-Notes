//! P0-06: WASMランタイムの比較。製品コードではない。使い方は ../README.md を参照。

#[cfg(feature = "wasmi")]
mod rt_wasmi;
#[cfg(any(feature = "wasmtime-cranelift", feature = "wasmtime-pulley-runtime"))]
mod rt_wasmtime;

use std::time::{Duration, Instant};

/// 各ランタイムが実装する操作。
pub trait Runtime {
    fn name(&self) -> String;
    /// 文書を両インスタンスへ書き込む（拡張側は `set_doc` で写しを持つ）。
    fn load_doc(&mut self, doc: &str);
    /// 書き込み済み文書の [start, end) を解析する。
    fn parse(&mut self, start: usize, end: usize) -> u32;
    /// `[` ごとの走査。mode 0: ホストを呼ばない基準、1: ホストの関数だけを呼ぶ、2: ホストから拡張インスタンスの `rule` も呼ぶ。
    fn scan(&mut self, mode: u8) -> (u64, u64);
}

pub struct Timing {
    pub min: Duration,
    pub med: Duration,
}

pub fn time<F: FnMut()>(runs: usize, mut f: F) -> Timing {
    let mut v: Vec<Duration> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .collect();
    v.sort();
    Timing {
        min: v[0],
        med: v[v.len() / 2],
    }
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// 常駐メモリ（KiB）。
pub fn rss_kib() -> u64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap();
    s.split_whitespace().nth(1).unwrap().parse::<u64>().unwrap() * 4
}

/// 中央付近の見出しから始まり、4,096バイト以上の後の見出しの直前で終わる範囲。
fn block_range(doc: &str) -> (usize, usize) {
    let start = doc[doc.len() / 2..].find("\n## ").unwrap() + doc.len() / 2 + 1;
    let end = doc[start + 4096..].find("\n## ").unwrap() + start + 4096 + 1;
    (start, end)
}

fn native_parse(s: &str) -> u32 {
    use pulldown_cmark::{Options, Parser};
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_WIKILINKS;
    Parser::new_ext(s, opts).into_offset_iter().count() as u32
}

fn bench(rt: &mut dyn Runtime, doc: &str, native_events: u32) {
    let (bs, be) = block_range(doc);
    rt.load_doc(doc);
    let events = rt.parse(0, doc.len());
    assert_eq!(
        events,
        native_events,
        "{}: イベント数がネイティブと一致しない",
        rt.name()
    );
    let full = time(10, || {
        rt.parse(0, doc.len());
    });
    let block = time(10, || {
        for _ in 0..100 {
            rt.parse(bs, be);
        }
    });
    let (calls, total) = rt.scan(1);
    let base = time(10, || {
        rt.scan(0);
    });
    let host_only = time(10, || {
        rt.scan(1);
    });
    let (calls2, total2) = rt.scan(2);
    assert_eq!(calls, calls2);
    let cross = time(10, || {
        rt.scan(2);
    });
    let per_call = |t: &Timing| (t.med.as_secs_f64() - base.med.as_secs_f64()) * 1e9 / calls as f64;
    println!(
        "{:<26} 1MB parse min {:>8.2} med {:>8.2} ms | {}B block {:>7.1} µs | hooks: {} calls, scan only {:>6.2} ms, +host call {:>5.0} ns/call, +cross-module {:>5.0} ns/call, matched {}→{}",
        rt.name(),
        ms(full.min),
        ms(full.med),
        be - bs,
        block.med.as_secs_f64() * 1e6 / 100.0,
        calls,
        ms(base.med),
        per_call(&host_only),
        per_call(&cross),
        total,
        total2,
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    let wasm_path = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("../guest/target/wasm32-unknown-unknown/release/wasm_guest.wasm");
    let doc_path = args.get(3).map(String::as_str).unwrap_or("../doc-1mb.md");
    match cmd {
        "bench" => {
            let wasm = std::fs::read(wasm_path).unwrap();
            let doc = std::fs::read_to_string(doc_path).unwrap();
            let (bs, be) = block_range(&doc);
            let native_events = native_parse(&doc);
            let nf = time(10, || {
                native_parse(&doc);
            });
            let nb = time(10, || {
                for _ in 0..100 {
                    native_parse(&doc[bs..be]);
                }
            });
            println!(
                "{:<26} 1MB parse min {:>8.2} med {:>8.2} ms | {}B block {:>7.1} µs | events {}",
                "native",
                ms(nf.min),
                ms(nf.med),
                be - bs,
                nb.med.as_secs_f64() * 1e6 / 100.0,
                native_events
            );
            #[cfg(feature = "wasmi")]
            for mode in [
                wasmi::CompilationMode::Eager,
                wasmi::CompilationMode::LazyTranslation,
            ] {
                let mut rt = rt_wasmi::Wasmi::new(&wasm, mode, false);
                bench(&mut rt, &doc, native_events);
            }
            #[cfg(feature = "wasmtime-cranelift")]
            for pulley in [false, true] {
                let mut rt = rt_wasmtime::Wasmtime::from_wasm(&wasm, pulley, false);
                bench(&mut rt, &doc, native_events);
            }
            #[cfg(any(feature = "wasmtime-cranelift", feature = "wasmtime-pulley-runtime"))]
            if let Ok(cwasm) = std::fs::read("../guest.pulley64.cwasm") {
                let mut rt = rt_wasmtime::Wasmtime::from_precompiled(&cwasm, false);
                bench(&mut rt, &doc, native_events);
            }
        }
        "startup" => {
            let wasm = std::fs::read(wasm_path).unwrap();
            #[cfg(feature = "wasmi")]
            for mode in [
                wasmi::CompilationMode::Eager,
                wasmi::CompilationMode::LazyTranslation,
            ] {
                rt_wasmi::startup(&wasm, mode);
            }
            #[cfg(feature = "wasmtime-cranelift")]
            for pulley in [false, true] {
                rt_wasmtime::startup(&wasm, pulley);
            }
            #[cfg(any(feature = "wasmtime-cranelift", feature = "wasmtime-pulley-runtime"))]
            if let Ok(cwasm) = std::fs::read("../guest.pulley64.cwasm") {
                rt_wasmtime::startup_precompiled(&cwasm);
            }
        }
        "limits" => {
            let wasm = std::fs::read(wasm_path).unwrap();
            let doc = std::fs::read_to_string(doc_path).unwrap();
            #[cfg(feature = "wasmi")]
            rt_wasmi::limits(&wasm, &doc);
            #[cfg(feature = "wasmtime-cranelift")]
            for pulley in [false, true] {
                rt_wasmtime::limits(&wasm, &doc, pulley);
            }
        }
        #[cfg(feature = "wasmtime-cranelift")]
        "precompile" => {
            let wasm = std::fs::read(wasm_path).unwrap();
            let bytes = rt_wasmtime::precompile_pulley(&wasm);
            std::fs::write("../guest.pulley64.cwasm", &bytes).unwrap();
            println!("wrote ../guest.pulley64.cwasm ({} bytes)", bytes.len());
        }
        "none" => {
            // バイナリサイズの基準（ランタイムを含まない構成）。
            let doc = std::fs::read_to_string(doc_path).unwrap();
            println!("native events {}", native_parse(&doc));
        }
        _ => {
            eprintln!("usage: wasm-host bench|startup|limits|precompile|none [guest.wasm] [doc.md]")
        }
    }
}
