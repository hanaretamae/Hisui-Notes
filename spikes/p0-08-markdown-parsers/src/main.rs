//! P0-08: Markdownパーサーの比較。製品コードではない。
//!
//! 使い方は README.md を参照。

mod inputs;
mod parsers;
mod wikilink_it;

use std::time::{Duration, Instant};

use parsers::ALL;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("spec") => spec(args.get(2).expect("spec.json のパス")),
        Some("bench") => bench(),
        Some("positions") => positions(),
        Some("obsidian") => obsidian(),
        Some("ext") => ext(),
        Some("patho") => patho(args.get(2), args.get(3)),
        Some("doc") => print!("{}", inputs::document(1 << 20)),
        _ => eprintln!(
            "usage: md-parsers spec <spec.json> | bench | positions | obsidian | ext | patho [<n> <pd|cm|mr|mi>]"
        ),
    }
}

// ---------------------------------------------------------------------------
// CommonMark spec tests

#[derive(serde::Deserialize)]
struct Example {
    markdown: String,
    html: String,
    example: u32,
    section: String,
}

/// CommonMark の normalize.py に近い簡易正規化。タグ間の空白と改行の差、空要素の `/>` の有無、
/// `"` と `&quot;` の差（HTML出力の差であり、解析結果の差ではない）を無視する。
fn normalize(html: &str) -> String {
    let s = html.replace(" />", ">").replace("&quot;", "\"");
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '>' {
            let mut ws = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_whitespace() {
                    ws.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek() != Some(&'<') {
                out.push_str(&ws);
            }
        }
    }
    out.trim().to_string()
}

fn spec(path: &str) {
    let examples: Vec<Example> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    println!("examples: {}", examples.len());
    for p in ALL {
        let mut exact = 0;
        let mut norm = 0;
        let mut failed_sections: std::collections::BTreeMap<&str, Vec<u32>> = Default::default();
        for ex in &examples {
            let got = p.html_commonmark(&ex.markdown);
            if got == ex.html {
                exact += 1;
            }
            if normalize(&got) == normalize(&ex.html) {
                norm += 1;
            } else {
                if std::env::var("SPEC_DIFF").is_ok_and(|v| v == p.short()) {
                    println!(
                        "  #{} md={:?}\n    expect={:?}\n    got   ={:?}",
                        ex.example, ex.markdown, ex.html, got
                    );
                }
                failed_sections
                    .entry(&ex.section)
                    .or_default()
                    .push(ex.example);
            }
        }
        let n = examples.len() as f64;
        println!(
            "{:<14} exact {:>3}/{} ({:.1}%)  normalized {:>3}/{} ({:.1}%)",
            p.name(),
            exact,
            examples.len(),
            exact as f64 / n * 100.0,
            norm,
            examples.len(),
            norm as f64 / n * 100.0
        );
        for (sec, ids) in failed_sections {
            println!("    fail [{sec}] {ids:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// 性能

fn time<F: FnMut()>(runs: usize, mut f: F) -> (Duration, Duration) {
    let mut v: Vec<Duration> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .collect();
    v.sort();
    (v[0], v[v.len() / 2])
}

fn bench() {
    let doc = inputs::document(1 << 20);
    println!(
        "doc: {} bytes, {} lines, {} chars",
        doc.len(),
        doc.lines().count(),
        doc.chars().count()
    );
    // 中央付近の行頭に1文字挿入した文書
    let mid = doc[..doc.len() / 2].rfind('\n').unwrap() + 1;
    let mut edited = doc.clone();
    edited.insert(mid, 'x');

    for p in ALL {
        for ext in [false, true] {
            let mut nodes = 0;
            let (min, med) = time(15, || nodes = p.parse_count(&doc, ext));
            let (emin, emed) = time(15, || {
                p.parse_count(&edited, ext);
            });
            println!(
                "{:<14} ext={:<5} nodes={:>7}  full parse min {:>7.2} ms med {:>7.2} ms | after 1-char edit min {:>7.2} ms med {:>7.2} ms",
                p.name(),
                ext,
                nodes,
                min.as_secs_f64() * 1e3,
                med.as_secs_f64() * 1e3,
                emin.as_secs_f64() * 1e3,
                emed.as_secs_f64() * 1e3
            );
        }
    }
    parsers::treesitter_incremental(&doc, mid);
}

// ---------------------------------------------------------------------------
// ソース位置

/// 各パーサーが返すリンクと見出しの原文範囲（バイト）が、原文の該当部分と一致するか。
fn positions() {
    let src = inputs::POSITION_SAMPLE;
    let expect_link = "[日本語のリンク](ノート/a.md#見出し)";
    let expect_heading = "## 見出し 🇯🇵 e\u{301}";
    println!("sample:\n{src}\n");
    for p in ALL {
        let r = p.ranges(src);
        let show = |label: &str, ranges: &[(usize, usize)], expect: &str| {
            let ok = ranges.iter().any(|&(s, e)| {
                src.get(s..e).map(|t| t.strip_suffix('\n').unwrap_or(t)) == Some(expect)
            });
            let shown: Vec<_> = ranges
                .iter()
                .map(|&(s, e)| format!("{s}..{e} {:?}", src.get(s..e)))
                .collect();
            println!("  {label}: {} {:?}", if ok { "OK " } else { "NG " }, shown);
        };
        println!("{}", p.name());
        show("link   ", &r.links, expect_link);
        show("heading", &r.headings, expect_heading);
    }
}

// ---------------------------------------------------------------------------
// Obsidian構文の組み込み対応

fn obsidian() {
    for (id, input) in inputs::OBSIDIAN_PROBES {
        println!("=== {id}\n--- input\n{input}");
        for p in ALL {
            let html = p.html_extended(input);
            println!("--- {}\n{}", p.name(), html.trim_end());
        }
        println!();
    }
}

// ---------------------------------------------------------------------------
// 構文の追加

fn ext() {
    println!("== markdown-it: 独自のインライン規則でWikilinkを追加");
    for input in inputs::WIKILINK_EXT_CASES {
        let (html, spans) = wikilink_it::parse(input);
        println!(
            "input: {input:?}\n  html: {}\n  spans: {:?}",
            html.trim_end(),
            spans
        );
        for (s, e) in spans {
            println!("    {s}..{e} = {:?}", &input[s..e]);
        }
    }
    println!(
        "\n== pulldown-cmark: 拡張フックが無い場合の後処理（Textイベントから [[...]] を探す）"
    );
    for input in inputs::WIKILINK_EXT_CASES {
        let found = parsers::pulldown_postprocess_wikilinks(input);
        println!("input: {input:?}\n  found: {found:?}");
    }
}

// ---------------------------------------------------------------------------
// 病的な入力

/// 引数なし: 入力の一覧。`patho <番号> <パーサー略号>`: 1組だけ計測する（無限に近い時間がかかる組を外部の timeout で打ち切るため）。
fn patho(case: Option<&String>, parser: Option<&String>) {
    let cases = inputs::pathological();
    let (Some(i), Some(ps)) = (case, parser) else {
        for (i, (name, input)) in cases.iter().enumerate() {
            println!("{i} {name} {} B", input.len());
        }
        return;
    };
    let (name, input) = &cases[i.parse::<usize>().unwrap()];
    let p = ALL
        .into_iter()
        .find(|p| p.short() == ps)
        .expect("pd|cm|mr|mi");
    let t = Instant::now();
    p.parse_count(input, true);
    println!(
        "{name:<20} {:<14} {:>10.2} ms",
        p.name(),
        t.elapsed().as_secs_f64() * 1e3
    );
}
