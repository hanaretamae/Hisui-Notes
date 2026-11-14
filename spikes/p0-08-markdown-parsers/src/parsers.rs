//! 各パーサーの呼び出しを同じ形にそろえる。

use std::time::Instant;

#[derive(Clone, Copy)]
pub enum Parser {
    Pulldown,
    Comrak,
    MarkdownRs,
    MarkdownIt,
}

pub const ALL: [Parser; 4] = [
    Parser::Pulldown,
    Parser::Comrak,
    Parser::MarkdownRs,
    Parser::MarkdownIt,
];

#[derive(Default)]
pub struct Ranges {
    pub links: Vec<(usize, usize)>,
    pub headings: Vec<(usize, usize)>,
}

// --- pulldown-cmark -------------------------------------------------------

fn pd_opts(ext: bool) -> pulldown_cmark::Options {
    use pulldown_cmark::Options as O;
    if !ext {
        return O::empty();
    }
    O::ENABLE_TABLES
        | O::ENABLE_FOOTNOTES
        | O::ENABLE_STRIKETHROUGH
        | O::ENABLE_TASKLISTS
        | O::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | O::ENABLE_MATH
        | O::ENABLE_GFM
        | O::ENABLE_WIKILINKS
}

// --- comrak ---------------------------------------------------------------

fn cm_opts(ext: bool) -> comrak::Options<'static> {
    let mut o = comrak::Options::default();
    o.render.r#unsafe = true;
    if ext {
        let e = &mut o.extension;
        e.table = true;
        e.strikethrough = true;
        e.tasklist = true;
        e.autolink = true;
        e.footnotes = true;
        e.inline_footnotes = true;
        e.front_matter_delimiter = Some("---".into());
        e.math_dollars = true;
        e.ofm_callouts = true;
        e.ofm_comments = true;
        e.highlight = true;
        e.wikilinks_title_after_pipe = true;
        e.cjk_friendly_emphasis = true;
        o.parse.relaxed_tasklist_matching = true;
    }
    o
}

/// comrakの1始まりの行・列（UTF-8バイト）を、文書先頭からのバイト位置に変換する。
fn line_starts(src: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(src.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

// --- markdown-rs ----------------------------------------------------------

fn mr_opts(ext: bool) -> markdown::Options {
    let mut o = if ext {
        markdown::Options::gfm()
    } else {
        markdown::Options::default()
    };
    o.compile.allow_dangerous_html = true;
    o.compile.allow_dangerous_protocol = true;
    if ext {
        o.parse.constructs.frontmatter = true;
        o.parse.constructs.math_flow = true;
        o.parse.constructs.math_text = true;
    }
    o
}

// --- markdown-it ----------------------------------------------------------

fn mi(ext: bool) -> markdown_it::MarkdownIt {
    let mut md = markdown_it::MarkdownIt::new();
    markdown_it::plugins::cmark::add(&mut md);
    markdown_it::plugins::html::add(&mut md);
    if ext {
        markdown_it::plugins::extra::tables::add(&mut md);
        markdown_it::plugins::extra::strikethrough::add(&mut md);
        crate::wikilink_it::add(&mut md);
    }
    md
}

impl Parser {
    pub fn name(self) -> &'static str {
        match self {
            Parser::Pulldown => "pulldown-cmark",
            Parser::Comrak => "comrak",
            Parser::MarkdownRs => "markdown-rs",
            Parser::MarkdownIt => "markdown-it",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Parser::Pulldown => "pd",
            Parser::Comrak => "cm",
            Parser::MarkdownRs => "mr",
            Parser::MarkdownIt => "mi",
        }
    }

    pub fn html_commonmark(self, src: &str) -> String {
        self.html(src, false)
    }
    pub fn html_extended(self, src: &str) -> String {
        self.html(src, true)
    }

    fn html(self, src: &str, ext: bool) -> String {
        match self {
            Parser::Pulldown => {
                let mut s = String::new();
                pulldown_cmark::html::push_html(
                    &mut s,
                    pulldown_cmark::Parser::new_ext(src, pd_opts(ext)),
                );
                s
            }
            Parser::Comrak => comrak::markdown_to_html(src, &cm_opts(ext)),
            Parser::MarkdownRs => markdown::to_html_with_options(src, &mr_opts(ext))
                .unwrap_or_else(|e| format!("ERROR {e}")),
            Parser::MarkdownIt => mi(ext).parse(src).render(),
        }
    }

    /// 構文木（イベント列）を作るまでの処理。HTMLは生成しない。戻り値はノード（イベント）数。
    pub fn parse_count(self, src: &str, ext: bool) -> usize {
        match self {
            Parser::Pulldown => pulldown_cmark::Parser::new_ext(src, pd_opts(ext))
                .into_offset_iter()
                .count(),
            Parser::Comrak => {
                let arena = comrak::Arena::new();
                let root = comrak::parse_document(&arena, src, &cm_opts(ext));
                root.descendants().count()
            }
            Parser::MarkdownRs => {
                let ast = markdown::to_mdast(src, &mr_opts(ext).parse).unwrap();
                fn count(n: &markdown::mdast::Node) -> usize {
                    1 + n.children().map(|c| c.iter().map(count).sum()).unwrap_or(0)
                }
                count(&ast)
            }
            Parser::MarkdownIt => {
                // MarkdownItの構築（規則の登録）は計測から除くため、呼び出し側で再利用したいが、
                // 1 MBの解析時間に対して無視できる大きさなので、ここでは毎回作る。
                let ast = mi(ext).parse(src);
                let mut n = 0;
                ast.walk(|_, _| n += 1);
                n
            }
        }
    }

    pub fn ranges(self, src: &str) -> Ranges {
        let mut r = Ranges::default();
        match self {
            Parser::Pulldown => {
                use pulldown_cmark::{Event, Tag};
                for (ev, range) in
                    pulldown_cmark::Parser::new_ext(src, pd_opts(true)).into_offset_iter()
                {
                    match ev {
                        Event::Start(Tag::Link { .. }) => r.links.push((range.start, range.end)),
                        Event::Start(Tag::Heading { .. }) => {
                            r.headings.push((range.start, range.end))
                        }
                        _ => {}
                    }
                }
            }
            Parser::Comrak => {
                use comrak::nodes::NodeValue;
                let ls = line_starts(src);
                let arena = comrak::Arena::new();
                let root = comrak::parse_document(&arena, src, &cm_opts(true));
                for n in root.descendants() {
                    let d = n.data();
                    let sp = d.sourcepos;
                    // 列は1始まりのUTF-8バイト。終了位置は最後のバイトを指す（閉区間）。
                    let s = ls[sp.start.line - 1] + sp.start.column - 1;
                    let e = ls[sp.end.line - 1] + sp.end.column;
                    match d.value {
                        NodeValue::Link(..) => r.links.push((s, e)),
                        NodeValue::Heading(..) => r.headings.push((s, e)),
                        _ => {}
                    }
                }
            }
            Parser::MarkdownRs => {
                use markdown::mdast::Node;
                fn walk(n: &Node, r: &mut Ranges) {
                    let pos = n.position().map(|p| (p.start.offset, p.end.offset));
                    match (n, pos) {
                        (Node::Link(_), Some(p)) => r.links.push(p),
                        (Node::Heading(_), Some(p)) => r.headings.push(p),
                        _ => {}
                    }
                    if let Some(c) = n.children() {
                        c.iter().for_each(|c| walk(c, r));
                    }
                }
                walk(
                    &markdown::to_mdast(src, &mr_opts(true).parse).unwrap(),
                    &mut r,
                );
            }
            Parser::MarkdownIt => {
                use markdown_it::plugins::cmark::block::heading::ATXHeading;
                use markdown_it::plugins::cmark::inline::link::Link;
                let ast = mi(true).parse(src);
                ast.walk(|n, _| {
                    let Some(m) = n.srcmap else { return };
                    let p = m.get_byte_offsets();
                    if n.is::<Link>() {
                        r.links.push(p);
                    } else if n.is::<ATXHeading>() {
                        r.headings.push(p);
                    }
                });
            }
        }
        r
    }
}

/// 拡張フックが無いパーサーで、Textイベントを後処理してWikilinkを探す場合の結果。
pub fn pulldown_postprocess_wikilinks(src: &str) -> Vec<String> {
    use pulldown_cmark::Event;
    // pulldown-cmarkは `[` の位置でTextイベントを分割するため、連続するTextを結合してから探す。
    let mut found = vec![];
    let mut buf = String::new();
    let mut start = 0;
    let flush = |buf: &mut String, start: usize, found: &mut Vec<String>| {
        let mut rest = buf.as_str();
        while let Some(s) = rest.find("[[") {
            let Some(e) = rest[s..].find("]]") else { break };
            found.push(format!(
                "{:?} (merged text from {start})",
                &rest[s..s + e + 2]
            ));
            rest = &rest[s + e + 2..];
        }
        buf.clear();
    };
    for (ev, range) in pulldown_cmark::Parser::new_ext(src, pd_opts(false)).into_offset_iter() {
        if let Event::Text(t) = ev {
            if buf.is_empty() {
                start = range.start;
            }
            buf.push_str(&t);
        } else {
            flush(&mut buf, start, &mut found);
        }
    }
    flush(&mut buf, start, &mut found);
    found
}

// --- tree-sitter-md -------------------------------------------------------

/// tree-sitter-mdの全体解析と、1文字挿入後の差分解析の時間。HTML出力が無いため spec テストは行わない。
pub fn treesitter_incremental(doc: &str, at: usize) {
    use tree_sitter::{InputEdit, Point};
    let mut p = tree_sitter_md::MarkdownParser::default();
    let mut full = vec![];
    let mut tree = None;
    for _ in 0..15 {
        let t = Instant::now();
        tree = p.parse(doc.as_bytes(), None);
        full.push(t.elapsed());
    }
    let mut edited = doc.to_string();
    edited.insert(at, 'x');
    let row = doc[..at].matches('\n').count();
    let base = tree.unwrap();
    let mut inc = vec![];
    for _ in 0..15 {
        let mut old = base.clone();
        old.edit(&InputEdit {
            start_byte: at,
            old_end_byte: at,
            new_end_byte: at + 1,
            start_position: Point::new(row, 0),
            old_end_position: Point::new(row, 0),
            new_end_position: Point::new(row, 1),
        });
        let t = Instant::now();
        let _ = p.parse(edited.as_bytes(), Some(&old));
        inc.push(t.elapsed());
    }
    full.sort();
    inc.sort();
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
    println!(
        "{:<14} full parse min {:>7.2} ms med {:>7.2} ms | incremental after 1-char edit min {:>7.2} ms med {:>7.2} ms",
        "tree-sitter-md",
        ms(full[0]),
        ms(full[7]),
        ms(inc[0]),
        ms(inc[7])
    );
}
