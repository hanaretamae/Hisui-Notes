//! 差分試験（tests/differential.rs）で見つかった差の回帰試験。期待値は参照実装（cmark・commonmark.js・
//! pulldown-cmark・markdown-rs）のうち spec に沿うものに合わせる（Spec: parser-markdown §7）。

fn html(src: &str) -> String {
    parser_markdown::html::render(&parser_markdown::parse(src))
}

#[test]
fn code_block_ending_at_eof_without_newline() {
    assert_eq!(html("```\n*"), "<pre><code>*</code></pre>\n");
}

#[test]
fn image_alt_keeps_escaped_inline_html_and_breaks_become_spaces() {
    assert_eq!(
        html("![<div>*](u)"),
        "<p><img src=\"u\" alt=\"&lt;div&gt;*\" /></p>\n"
    );
    assert_eq!(html("![a\nb](u)"), "<p><img src=\"u\" alt=\"a b\" /></p>\n");
}

#[test]
fn trailing_tabs_before_soft_break_are_removed() {
    assert_eq!(html("**\t\na"), "<p>**\na</p>\n");
    // 行末の空白2つの後にタブがある場合はハード改行ではない
    assert_eq!(html("a  \t\nb"), "<p>a\nb</p>\n");
    assert_eq!(html("a \t  \nb"), "<p>a<br />\nb</p>\n");
}

#[test]
fn spaces_after_backslash_hard_break_are_skipped() {
    assert_eq!(
        html("1. - \\\n    a"),
        "<ol>\n<li>\n<ul>\n<li><br />\na</li>\n</ul>\n</li>\n</ol>\n"
    );
}

#[test]
fn lazy_continuation_keeps_leading_spaces_in_code_span() {
    assert_eq!(
        html("> ``a\n  b``"),
        "<blockquote>\n<p><code>a   b</code></p>\n</blockquote>\n"
    );
    // 遅延継続でない継続行は先頭の空白を除く
    assert_eq!(html("``a\n  b``"), "<p><code>a b</code></p>\n");
}

#[test]
fn blank_line_in_fenced_code_in_list_item_keeps_extra_indent() {
    assert_eq!(
        html("- ```\n    \n  a\n"),
        "<ul>\n<li>\n<pre><code>  \na\n</code></pre>\n</li>\n</ul>\n"
    );
}

#[test]
fn blank_brackets_are_not_a_link_label() {
    assert_eq!(
        html("[r][    ](u)\n\n[r]: /v"),
        "<p><a href=\"/v\">r</a><a href=\"u\">    </a></p>\n"
    );
}

#[test]
fn reference_definitions_on_lazy_lines() {
    assert_eq!(
        html("- [r]: /u \"t\"\n [r]: /v\n"),
        "<ul>\n<li></li>\n</ul>\n"
    );
}

#[test]
fn reference_definitions_count_for_list_tightness() {
    // 定義だけの段落の後の空行で項目は loose になる（spec 5.3）
    assert_eq!(
        html("- [r]: /u\n\n\n    a"),
        "<ul>\n<li>\n<p>a</p>\n</li>\n</ul>\n"
    );
    // 定義が遅延継続の行に続いても、項目の間に空行は無い
    assert_eq!(
        html("* [r]: /u\n[s]: /v\n* a"),
        "<ul>\n<li></li>\n<li>a</li>\n</ul>\n"
    );
}

#[test]
fn link_label_limit_is_999_characters_not_bytes() {
    for ch in ["a", "あ"] {
        let ok = ch.repeat(999);
        assert_eq!(
            html(&format!("[{ok}]\n\n[{ok}]: /u")),
            format!("<p><a href=\"/u\">{ok}</a></p>\n"),
            "{ch} x 999"
        );
        let long = ch.repeat(1000);
        assert_eq!(
            html(&format!("[{long}]\n\n[{long}]: /u")),
            format!("<p>[{long}]</p>\n<p>[{long}]: /u</p>\n"),
            "{ch} x 1000"
        );
    }
}
