//! 構文木の性質（Spec: parser-markdown §6）。Property-based Testing。

use parser_markdown::{Node, NodeKind, parse};
use proptest::prelude::*;

/// Markdown の記号を多く含む任意の文字列。
fn markdownish() -> impl Strategy<Value = String> {
    let atoms = prop::sample::select(vec![
        "*",
        "**",
        "_",
        "__",
        "`",
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
        "1. ",
        "2) ",
        "> ",
        "    ",
        "\t",
        " ",
        "  ",
        "\n",
        "\r\n",
        "\r",
        "\n\n",
        "a",
        "b",
        "日本",
        "語",
        "😀",
        "👨‍👩‍👧",
        "é",
        "e\u{301}",
        "http://x.y",
        "<a@b.c>",
        "<http://z>",
        "<div>",
        "</div>",
        "<!--",
        "-->",
        "[r]: /u \"t\"\n",
        "[r]",
        "===",
        "---",
        "***",
        "|",
        "\0",
        "&#35;",
        "\"",
        "'",
    ]);
    prop::collection::vec(atoms, 0..60).prop_map(|v| v.concat())
}

fn check(node: &Node, src: &str, parent: &std::ops::Range<usize>, problems: &mut Vec<String>) {
    let r = &node.range;
    if r.start > r.end
        || r.end > src.len()
        || !src.is_char_boundary(r.start)
        || !src.is_char_boundary(r.end)
    {
        problems.push(format!("bad range {r:?} for {:?}", node.kind));
        return;
    }
    if !matches!(node.kind, NodeKind::Document) && (r.start < parent.start || r.end > parent.end) {
        problems.push(format!("{:?} {r:?} outside parent {parent:?}", node.kind));
    }
    if let NodeKind::Text(t) = &node.kind {
        let s = &src[r.clone()];
        // エスケープ・文字参照・改行・NUL を含まない文字列は、原文の範囲と一致する
        if !s.contains(['\\', '&', '\n', '\r', '\0']) && s != t {
            problems.push(format!("text {t:?} != source {s:?} at {r:?}"));
        }
    }
    let mut prev_end = r.start;
    for c in &node.children {
        if c.range.start < prev_end {
            problems.push(format!(
                "{:?} {:?} overlaps previous sibling ending at {prev_end}",
                c.kind, c.range
            ));
        }
        prev_end = prev_end.max(c.range.end);
        check(c, src, r, problems);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, ..ProptestConfig::default() })]

    #[test]
    fn ranges_are_nested_ordered_and_match_text(src in markdownish()) {
        let doc = parse(&src);
        let mut problems = Vec::new();
        check(&doc, &src, &(0..src.len()), &mut problems);
        prop_assert!(problems.is_empty(), "{src:?}\n{}", problems.join("\n"));
    }

    #[test]
    fn line_endings_do_not_change_the_result(src in markdownish()) {
        let lf = src.replace("\r\n", "\n").replace('\r', "\n");
        let crlf = lf.replace('\n', "\r\n");
        let a = parser_markdown::html::render(&parse(&lf));
        prop_assert_eq!(&a, &parser_markdown::html::render(&parse(&crlf)), "LF と CRLF");
        prop_assert_eq!(&a, &parser_markdown::html::render(&parse(&lf.replace('\n', "\r"))), "LF と CR");
    }

    #[test]
    fn arbitrary_unicode_never_panics(src in "\\PC{0,200}") {
        let _ = parser_markdown::html::render(&parse(&src));
    }
}

#[test]
fn ranges_of_typical_document() {
    let src = "# 見出し\n\n本文の*強調*と[リンク](u)。\n\n- 項目\n";
    let doc = parse(src);
    let slices: Vec<(String, &str)> = {
        let mut v = Vec::new();
        doc.walk(&mut |n| {
            v.push((
                format!("{:?}", n.kind)
                    .chars()
                    .take_while(|c| c.is_alphanumeric())
                    .collect(),
                &src[n.range.clone()],
            ))
        });
        v
    };
    let find = |k: &str| {
        slices
            .iter()
            .find(|(kind, _)| kind == k)
            .map(|(_, s)| *s)
            .unwrap()
    };
    assert_eq!(find("Heading"), "# 見出し");
    assert_eq!(find("Emphasis"), "*強調*");
    assert_eq!(find("Link"), "[リンク](u)");
    assert_eq!(find("List"), "- 項目");
    assert_eq!(find("Item"), "- 項目");
    let paragraphs: Vec<_> = slices
        .iter()
        .filter(|(k, _)| k == "Paragraph")
        .map(|(_, s)| *s)
        .collect();
    assert_eq!(paragraphs, vec!["本文の*強調*と[リンク](u)。", "項目"]);
}

#[test]
fn nul_is_replaced_in_output_only() {
    let doc = parse("a\0b");
    let mut texts = Vec::new();
    doc.walk(&mut |n| {
        if let NodeKind::Text(t) = &n.kind {
            texts.push((t.clone(), n.range.clone()));
        }
    });
    assert_eq!(texts, vec![("a\u{FFFD}b".to_string(), 0..3)]);
}

#[test]
fn nesting_depth_is_limited() {
    let deep = format!("{}a", "> ".repeat(1000));
    let mut depth = 0;
    fn max_depth(n: &Node, d: usize, m: &mut usize) {
        *m = (*m).max(d);
        n.children.iter().for_each(|c| max_depth(c, d + 1, m));
    }
    max_depth(&parse(&deep), 0, &mut depth);
    assert!(depth <= 102, "depth {depth}");
    let mut idepth = 0;
    max_depth(
        &parse(&format!("{}a{}", "*".repeat(1000), "*".repeat(1000))),
        0,
        &mut idepth,
    );
    assert!(idepth <= 103, "inline depth {idepth}");
}
