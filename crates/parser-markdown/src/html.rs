//! 構文木の HTML への変換（CommonMark の参照実装と同じ出力。spec tests の検証に使う）。

use crate::ast::{Node, NodeKind};

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

/// URL の正規化（安全な文字と既存の `%XX` 以外を百分率符号化する）。
pub fn normalize_url(s: &str) -> String {
    const SAFE: &[u8] = b";/?:@&=+$,-_.!~*'()#";
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'%'
            && b.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
            && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            out.push_str(&s[i..i + 3]);
            i += 3;
            continue;
        }
        if c.is_ascii_alphanumeric() || SAFE.contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
        i += 1;
    }
    out
}

struct Renderer {
    out: String,
}

impl Renderer {
    fn cr(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn plain(&mut self, n: &Node) {
        match &n.kind {
            NodeKind::Text(t) | NodeKind::Code(t) | NodeKind::HtmlInline(t) => {
                escape(t, &mut self.out)
            }
            NodeKind::SoftBreak | NodeKind::HardBreak => self.out.push(' '),
            _ => n.children.iter().for_each(|c| self.plain(c)),
        }
    }

    fn children(&mut self, n: &Node, tight: bool) {
        for c in &n.children {
            self.node(c, tight);
        }
    }

    fn node(&mut self, n: &Node, tight: bool) {
        match &n.kind {
            NodeKind::Document => self.children(n, false),
            NodeKind::Paragraph => {
                if tight {
                    self.children(n, false);
                } else {
                    self.cr();
                    self.out.push_str("<p>");
                    self.children(n, false);
                    self.out.push_str("</p>");
                    self.cr();
                }
            }
            NodeKind::BlockQuote => {
                self.cr();
                self.out.push_str("<blockquote>");
                self.cr();
                self.children(n, false);
                self.cr();
                self.out.push_str("</blockquote>");
                self.cr();
            }
            NodeKind::List(info) => {
                self.cr();
                let tag = if info.ordered { "ol" } else { "ul" };
                if info.ordered && info.start != 1 {
                    self.out.push_str(&format!("<ol start=\"{}\">", info.start));
                } else {
                    self.out.push_str(&format!("<{tag}>"));
                }
                self.cr();
                for item in &n.children {
                    self.node(item, info.tight);
                }
                self.cr();
                self.out.push_str(&format!("</{tag}>"));
                self.cr();
            }
            NodeKind::Item => {
                self.out.push_str("<li>");
                self.children(n, tight);
                self.out.push_str("</li>");
                self.cr();
            }
            NodeKind::Heading { level, .. } => {
                self.cr();
                self.out.push_str(&format!("<h{level}>"));
                self.children(n, false);
                self.out.push_str(&format!("</h{level}>"));
                self.cr();
            }
            NodeKind::ThematicBreak => {
                self.cr();
                self.out.push_str("<hr />");
                self.cr();
            }
            NodeKind::CodeBlock { info, literal } => {
                self.cr();
                self.out.push_str("<pre><code");
                if let Some(lang) = info.as_deref().and_then(|i| i.split_whitespace().next()) {
                    self.out.push_str(" class=\"language-");
                    escape(lang, &mut self.out);
                    self.out.push('"');
                }
                self.out.push('>');
                escape(literal, &mut self.out);
                self.out.push_str("</code></pre>");
                self.cr();
            }
            NodeKind::HtmlBlock { literal } => {
                self.cr();
                self.out.push_str(literal);
                self.cr();
            }
            NodeKind::Text(t) => escape(t, &mut self.out),
            NodeKind::SoftBreak => self.out.push('\n'),
            NodeKind::HardBreak => self.out.push_str("<br />\n"),
            NodeKind::Code(c) => {
                self.out.push_str("<code>");
                escape(c, &mut self.out);
                self.out.push_str("</code>");
            }
            NodeKind::Emphasis => {
                self.out.push_str("<em>");
                self.children(n, false);
                self.out.push_str("</em>");
            }
            NodeKind::Strong => {
                self.out.push_str("<strong>");
                self.children(n, false);
                self.out.push_str("</strong>");
            }
            NodeKind::Link { destination, title } => {
                self.out.push_str("<a href=\"");
                escape(&normalize_url(destination), &mut self.out);
                self.out.push('"');
                if !title.is_empty() {
                    self.out.push_str(" title=\"");
                    escape(title, &mut self.out);
                    self.out.push('"');
                }
                self.out.push('>');
                self.children(n, false);
                self.out.push_str("</a>");
            }
            NodeKind::Image { destination, title } => {
                self.out.push_str("<img src=\"");
                escape(&normalize_url(destination), &mut self.out);
                self.out.push_str("\" alt=\"");
                for c in &n.children {
                    self.plain(c);
                }
                self.out.push('"');
                if !title.is_empty() {
                    self.out.push_str(" title=\"");
                    escape(title, &mut self.out);
                    self.out.push('"');
                }
                self.out.push_str(" />");
            }
            NodeKind::HtmlInline(h) => self.out.push_str(h),
        }
    }
}

/// 構文木を HTML にする。
pub fn render(doc: &Node) -> String {
    let mut r = Renderer { out: String::new() };
    r.node(doc, false);
    r.out
}
