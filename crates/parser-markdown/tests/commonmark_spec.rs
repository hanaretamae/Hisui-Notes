//! CommonMark spec 0.31.2 の全例への適合（ADR-0004 §6）。データは `tests/data/`（CC BY-SA 4.0）。

use serde::Deserialize;

#[derive(Deserialize)]
struct Example {
    markdown: String,
    html: String,
    example: u32,
    section: String,
}

fn examples() -> Vec<Example> {
    serde_json::from_str(include_str!("data/commonmark-0.31.2.json")).unwrap()
}

#[test]
fn all_spec_examples_pass() {
    let mut failed = Vec::new();
    let all = examples();
    for ex in &all {
        let got = parser_markdown::html::render(&parser_markdown::parse(&ex.markdown));
        if got != ex.html {
            failed.push(format!(
                "example {} ({}):\n--- markdown\n{:?}\n--- expected\n{:?}\n--- got\n{:?}",
                ex.example, ex.section, ex.markdown, ex.html, got
            ));
        }
    }
    if !failed.is_empty() {
        let n = failed.len();
        let shown: Vec<_> = failed.into_iter().take(15).collect();
        panic!(
            "{n} / {} examples failed:\n{}",
            all.len(),
            shown.join("\n\n")
        );
    }
}
