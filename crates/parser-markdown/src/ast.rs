//! 構文木（Spec: parser-markdown §3）。

use std::ops::Range;

/// 構文木の節。`range` は原文の UTF-8 のバイト位置（ADR-0004 §1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub range: Range<usize>,
    pub children: Vec<Node>,
}

/// リストの情報。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListInfo {
    /// 番号付きか。
    pub ordered: bool,
    /// 番号付きリストの開始番号（番号なしでは 0）。
    pub start: u64,
    /// 記号（番号なし: `-`・`+`・`*`、番号付き: `.`・`)`）。
    pub marker: u8,
    /// 項目の間に空行が無い（tight）か。
    pub tight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeKind {
    // ブロック
    Document,
    BlockQuote,
    List(ListInfo),
    Item,
    Paragraph,
    /// 見出し（レベル 1〜6）。`setext` は下線による見出し。
    Heading {
        level: u8,
        setext: bool,
    },
    ThematicBreak,
    /// コードブロック。`info` はフェンスの情報文字列（エスケープ等を解決済み）。インデントによる場合は `None`。
    CodeBlock {
        info: Option<String>,
        literal: String,
    },
    HtmlBlock {
        literal: String,
    },
    // インライン
    /// 文字列（エスケープ・文字参照を解決済み）。
    Text(String),
    SoftBreak,
    HardBreak,
    Code(String),
    Emphasis,
    Strong,
    /// リンク。`destination` はエスケープ・文字参照を解決済みで、URL の正規化はしない。
    Link {
        destination: String,
        title: String,
    },
    Image {
        destination: String,
        title: String,
    },
    HtmlInline(String),
}

impl Node {
    pub(crate) fn new(kind: NodeKind, range: Range<usize>) -> Self {
        Self {
            kind,
            range,
            children: Vec::new(),
        }
    }

    /// 子孫を含めた深さ優先の順序で走査する。
    pub fn walk(&self, f: &mut impl FnMut(&Node)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }
}
