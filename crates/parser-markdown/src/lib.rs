//! 標準Markdownパーサー（`docs/spec/parser-markdown.md`、ADR-0004）。
//!
//! CommonMark spec 0.31.2 に基づく独自の実装。位置は原文の UTF-8 のバイト位置。

mod ast;
mod block;
pub mod html;
mod inline;
mod scan;

pub use ast::{ListInfo, Node, NodeKind};

/// 文書を解析して構文木を返す。どの入力でも失敗しない（CommonMark ではすべての文字列が正しい文書）。
pub fn parse(text: &str) -> Node {
    let mut doc = block::parse(text);
    if text.contains('\0') {
        replace_nul(&mut doc);
    }
    doc
}

/// 構文木の文字列の U+0000 を U+FFFD にする（spec 2.3。原文と位置は変えない）。
fn replace_nul(node: &mut Node) {
    let fix = |s: &mut String| {
        if s.contains('\0') {
            *s = s.replace('\0', "\u{FFFD}");
        }
    };
    match &mut node.kind {
        NodeKind::Text(s)
        | NodeKind::Code(s)
        | NodeKind::HtmlInline(s)
        | NodeKind::HtmlBlock { literal: s } => fix(s),
        NodeKind::CodeBlock { info, literal } => {
            fix(literal);
            if let Some(i) = info {
                fix(i);
            }
        }
        NodeKind::Link { destination, title } | NodeKind::Image { destination, title } => {
            fix(destination);
            fix(title);
        }
        _ => {}
    }
    node.children.iter_mut().for_each(replace_nul);
}
