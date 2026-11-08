//! markdown-it の拡張フック（インライン規則）で `[[target|alias]]` を追加できるかの確認。

use markdown_it::parser::inline::{InlineRule, InlineState};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

#[derive(Debug)]
pub struct WikiLink {
    pub target: String,
    pub alias: Option<String>,
}

impl NodeValue for WikiLink {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push(("data-wikilink", self.target.clone()));
        fmt.open("a", &attrs);
        fmt.text(self.alias.as_deref().unwrap_or(&self.target));
        fmt.close("a");
    }
}

struct WikiLinkScanner;

impl InlineRule for WikiLinkScanner {
    const MARKER: char = '[';

    fn run(state: &mut InlineState) -> Option<(Node, usize)> {
        let input = &state.src[state.pos..state.pos_max];
        let body = input.strip_prefix("[[")?;
        let end = body.find("]]")?;
        let inner = &body[..end];
        if inner.is_empty() || inner.contains('\n') || inner.contains('[') {
            return None;
        }
        let (target, alias) = match inner.split_once('|') {
            Some((t, a)) => (t.to_string(), Some(a.to_string())),
            None => (inner.to_string(), None),
        };
        Some((Node::new(WikiLink { target, alias }), end + 4))
    }
}

pub fn add(md: &mut MarkdownIt) {
    // 標準のリンク規則より先に評価する。
    md.inline.add_rule::<WikiLinkScanner>().before_all();
}

/// HTMLと、WikiLinkノードの原文範囲（バイト）を返す。
pub fn parse(src: &str) -> (String, Vec<(usize, usize)>) {
    let mut md = MarkdownIt::new();
    markdown_it::plugins::cmark::add(&mut md);
    add(&mut md);
    let ast = md.parse(src);
    let mut spans = vec![];
    ast.walk(|n, _| {
        if n.is::<WikiLink>() {
            if let Some(m) = n.srcmap {
                spans.push(m.get_byte_offsets());
            }
        }
    });
    (ast.render(), spans)
}
