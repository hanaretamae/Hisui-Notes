//! 参照実装との差分試験（ADR-0004 §6）。comrak・pulldown-cmark・markdown-rs の少なくとも1つと同じ HTML になることを確かめる。
//! 参照実装にも誤りがある（例: comrak 0.59 は `` `~~~```~~~```~~~```~~~``` `` の2つ目のコードスパンを認識しない）ため、
//! いずれか1つとの一致を条件とする（pulldown-cmark は段落の継続行の先頭の空白を残すことがある）。

use proptest::prelude::*;

/// HTML の書式の差（解析の差ではない）を揃える。
/// コードスパン（`<pre>` の外の `<code>`）の連続する空白・タブは1つの空白にする。段落の継続行の先頭の空白を
/// cmark・commonmark.js は除き、pulldown-cmark・markdown-rs は残すため（spec 4.8 は除く）。
fn normalize(html: &str) -> String {
    let html = html
        .trim_end()
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        // alt の HTML を markdown-rs はエスケープしない（解析の差ではない）
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        // pulldown-cmark は tight なリストの項目の HTML ブロックの前で改行しない
        .replace("<li>\n", "<li>")
        // 改行の無い最後の行で終わるコードブロックの末尾の改行（実装により異なり、spec の例に無い）
        .replace("\n</code></pre>", "</code></pre>");
    let mut out = String::new();
    let mut rest = html.as_str();
    while let Some(p) = rest.find("<code>") {
        let (before, after) = rest.split_at(p + "<code>".len());
        out.push_str(before);
        let end = after.find("</code>").unwrap_or(after.len());
        if before.ends_with("<pre><code>") {
            out.push_str(&after[..end]);
        } else {
            let mut prev_space = false;
            for ch in after[..end].chars() {
                let space = ch == ' ' || ch == '\t';
                if !(space && prev_space) {
                    out.push(if space { ' ' } else { ch });
                }
                prev_space = space;
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn comrak(src: &str) -> String {
    let mut opts = comrak::Options::default();
    opts.render.r#unsafe = true;
    normalize(&comrak::markdown_to_html(src, &opts))
}

fn pulldown(src: &str) -> String {
    let mut out = String::new();
    pulldown_cmark::html::push_html(
        &mut out,
        pulldown_cmark::Parser::new_ext(src, pulldown_cmark::Options::empty()),
    );
    normalize(&out)
}

fn markdown_rs(src: &str) -> String {
    let opts = markdown::Options {
        compile: markdown::CompileOptions {
            allow_dangerous_html: true,
            allow_dangerous_protocol: true,
            ..Default::default()
        },
        ..Default::default()
    };
    normalize(&markdown::to_html_with_options(src, &opts).unwrap_or_default())
}

fn ours(src: &str) -> String {
    normalize(&parser_markdown::html::render(&parser_markdown::parse(src)))
}

/// HTML コメント（`<!--`）は含めない。comrak 0.59 は spec 0.30 の規則（内容に `--` を含めない）で判定し、
/// spec 0.31.2 と異なる（例: `<!--<!---->`）。コメントは spec tests で検証する。
fn markdownish() -> impl Strategy<Value = String> {
    let atoms = prop::sample::select(vec![
        "*",
        "**",
        "_",
        "__",
        "`",
        "``",
        "```",
        "~~~",
        "[",
        "]",
        "(",
        ")",
        "![",
        "<",
        ">",
        "&amp;",
        "&",
        "\\",
        "#",
        "## ",
        "- ",
        "+ ",
        "* ",
        "1. ",
        "2) ",
        "> ",
        "    ",
        "\t",
        " ",
        "  ",
        "\n",
        "\n\n",
        "a",
        "b",
        "日本",
        "、",
        "。",
        "「",
        "」",
        "😀",
        "é",
        "http://x.y",
        "<a@b.c>",
        "<http://z>",
        "<div>",
        "</div>",
        "<span>",
        "[r]: /u \"t\"\n",
        "[r]",
        "[r][]",
        "](u)",
        "](<u>)",
        "===",
        "---",
        "***",
        "\"",
        "'",
        ":",
    ]);
    prop::collection::vec(atoms, 0..40).prop_map(|v| v.concat())
}

proptest! {
    // 参照実装どうしでも結果が分かれる入力がある（Spec §7）ため、CI では種を固定して決定的にする。
    // 調査時は PROPTEST_CASES を増やし、種を変えて実行する（種の固定は rng_seed を外す）。
    #![proptest_config(ProptestConfig { cases: 4096, rng_seed: proptest::test_runner::RngSeed::Fixed(0x4869_7375_6921), ..ProptestConfig::default() })]

    #[test]
    fn same_html_as_a_reference_implementation(src in markdownish()) {
        // 改行の無い最後の行が空白だけの場合、spec は空行とし（4.9）、cmark 系は空行としない。比較から除く
        let last = src.rsplit(['\n', '\r']).next().unwrap_or("");
        let src = if !last.is_empty() && last.bytes().all(|c| c == b' ' || c == b'\t') { src + "\n" } else { src };
        let o = ours(&src);
        let (c, p, m) = (comrak(&src), pulldown(&src), markdown_rs(&src));
        prop_assert!(o == c || o == p || o == m, "input: {:?}\nours:     {:?}\ncomrak:   {:?}\npulldown: {:?}\nmarkdown: {:?}", src, o, c, p, m);
    }
}
