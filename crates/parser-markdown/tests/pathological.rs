//! 病的な入力（cmark の pathological_tests.py を参考にした入力。P0-08 §3.6、ADR-0004 §6）。
//! 入力の長さに対しておおむね線形の時間で終わり、スタックが溢れないことを確かめる。

use std::time::{Duration, Instant};

const N: usize = 30_000;

fn cases() -> Vec<(&'static str, String)> {
    let r = |s: &str, n: usize| s.repeat(n);
    vec![
        (
            "nested strong emph",
            format!("{}a{}", r("*a **a ", N), r(" a** a*", N)),
        ),
        ("many emph closers with no openers", r("a_ ", N)),
        ("many emph openers with no closers", r("_a ", N)),
        ("many link closers with no openers", r("a]", N)),
        ("many link openers with no closers", r("[a", N)),
        ("mismatched openers and closers", r("*a_ ", N)),
        (
            "openers and closers multiple of 3",
            format!("a**b{}", r("c* ", N)),
        ),
        ("link openers and emph closers", r("[ a_", N)),
        ("pattern [ (]( repeated", r("[ (](", N)),
        ("nested brackets", format!("{}a{}", r("[", N), r("]", N))),
        ("nested block quotes", format!("{}a", r("> ", N))),
        (
            "deeply nested lists",
            (0..2000)
                .map(|i| format!("{}* a\n", "  ".repeat(i)))
                .collect(),
        ),
        ("U+0000 in input", r("abc\u{0}de\u{0}", N)),
        (
            "backticks",
            (1..2000).map(|i| format!("e{}", "`".repeat(i))).collect(),
        ),
        ("unclosed links A", r("[a](<b", N)),
        ("unclosed links B", r("[a](b", N)),
        ("unclosed <!--", r("</ <!-- ", N)),
        ("unclosed <? and <![CDATA[", r("<? <![CDATA[ <!A ", N)),
        (
            "many references",
            (0..N).map(|i| format!("[a{i}]: u\n")).collect(),
        ),
        ("nested emphasis", format!("{}a{}", r("*", N), r("*", N))),
        ("nested links", format!("{}a{}", r("[", N), r("](b)", N))),
    ]
}

#[test]
fn pathological_inputs_finish_quickly() {
    // デバッグビルドでも数秒以内（線形でない場合は数分以上かかる）
    let limit = Duration::from_secs(if cfg!(debug_assertions) { 10 } else { 1 });
    let mut slow = Vec::new();
    for (name, input) in cases() {
        let t = Instant::now();
        let doc = parser_markdown::parse(&input);
        let _ = parser_markdown::html::render(&doc);
        let e = t.elapsed();
        eprintln!("{name}: {e:?}");
        if e > limit {
            slow.push(format!("{name}: {e:?} ({} bytes)", input.len()));
        }
    }
    assert!(slow.is_empty(), "too slow:\n{}", slow.join("\n"));
}
