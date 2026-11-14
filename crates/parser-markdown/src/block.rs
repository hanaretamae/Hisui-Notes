//! ブロック構造の解析（CommonMark spec 0.31.2 の「解析の方針」の第1段階）。

use std::collections::HashMap;

use crate::ast::{ListInfo, Node, NodeKind};
use crate::inline::{self, RefMap};
use crate::scan;

const CODE_INDENT: usize = 4;

/// コンテナーの入れ子の深さの上限（Spec §5）。超える分のブロック引用・リストは開始しない。
pub(crate) const MAX_DEPTH: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ListData {
    ordered: bool,
    bullet_char: u8,
    start: u64,
    delimiter: u8,
    padding: usize,
    marker_offset: usize,
}

#[derive(Debug)]
enum Kind {
    Document,
    BlockQuote,
    List {
        data: ListData,
        tight: bool,
    },
    Item(ListData),
    Paragraph,
    Heading {
        level: u8,
        setext: bool,
    },
    ThematicBreak,
    CodeBlock {
        fenced: bool,
        fence_char: u8,
        fence_length: usize,
        fence_offset: usize,
        info: Option<String>,
        literal: String,
    },
    HtmlBlock {
        html_type: u8,
        literal: String,
    },
}

/// 内容の文字列と、その位置から原文の位置への対応。
#[derive(Debug, Default, Clone)]
pub(crate) struct Content {
    pub text: String,
    /// (内容の位置, 原文の位置) の昇順の組。
    pub map: Vec<(usize, usize)>,
    /// 内容の先頭から解析済み（リンク参照定義）の長さ。
    pub skip: usize,
}

impl Content {
    fn push(&mut self, s: &str, src: usize) {
        self.map.push((self.text.len(), src));
        self.text.push_str(s);
    }

    /// 内容の位置を原文の位置にする。
    pub fn to_source(&self, off: usize) -> usize {
        let i = self.map.partition_point(|&(c, _)| c <= off);
        match i.checked_sub(1) {
            Some(i) => self.map[i].1 + (off - self.map[i].0),
            None => self.map.first().map_or(0, |m| m.1),
        }
    }

    pub fn rest(&self) -> &str {
        &self.text[self.skip..]
    }
}

struct Block {
    kind: Kind,
    parent: Option<usize>,
    children: Vec<usize>,
    open: bool,
    start_line: usize,
    end_line: usize,
    start: usize,
    content: Content,
    removed: bool,
}

struct Line {
    start: usize,
    end: usize,
}

struct Parser<'a> {
    src: &'a str,
    lines: Vec<Line>,
    blocks: Vec<Block>,
    tip: usize,
    oldtip: usize,
    line: &'a str,
    line_start: usize,
    line_number: usize,
    offset: usize,
    column: usize,
    next_nonspace: usize,
    next_nonspace_column: usize,
    indent: usize,
    indented: bool,
    blank: bool,
    partially_consumed_tab: bool,
    all_closed: bool,
    last_matched_container: usize,
    refmap: RefMap,
}

fn split_lines(src: &str) -> Vec<Line> {
    let b = src.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\n' => {
                lines.push(Line { start, end: i });
                i += 1;
                start = i;
            }
            b'\r' => {
                lines.push(Line { start, end: i });
                i += if b.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < b.len() {
        lines.push(Line {
            start,
            end: b.len(),
        });
    }
    lines
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        let doc = Block {
            kind: Kind::Document,
            parent: None,
            children: Vec::new(),
            open: true,
            start_line: 1,
            end_line: 0,
            start: 0,
            content: Content::default(),
            removed: false,
        };
        Self {
            src,
            lines: split_lines(src),
            blocks: vec![doc],
            tip: 0,
            oldtip: 0,
            line: "",
            line_start: 0,
            line_number: 0,
            offset: 0,
            column: 0,
            next_nonspace: 0,
            next_nonspace_column: 0,
            indent: 0,
            indented: false,
            blank: false,
            partially_consumed_tab: false,
            all_closed: true,
            last_matched_container: 0,
            refmap: HashMap::new(),
        }
    }

    fn peek(&self, i: usize) -> Option<u8> {
        self.line.as_bytes().get(i).copied()
    }

    fn last_child(&self, b: usize) -> Option<usize> {
        self.blocks[b].children.last().copied()
    }

    fn find_next_nonspace(&mut self) {
        let b = self.line.as_bytes();
        let mut i = self.offset;
        let mut cols = self.column;
        while let Some(&c) = b.get(i) {
            match c {
                b' ' => {
                    i += 1;
                    cols += 1;
                }
                b'\t' => {
                    i += 1;
                    cols += 4 - cols % 4;
                }
                _ => break,
            }
        }
        self.blank = i >= b.len();
        self.next_nonspace = i;
        self.next_nonspace_column = cols;
        self.indent = cols - self.column;
        self.indented = self.indent >= CODE_INDENT;
    }

    fn advance_next_nonspace(&mut self) {
        self.offset = self.next_nonspace;
        self.column = self.next_nonspace_column;
        self.partially_consumed_tab = false;
    }

    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        while count > 0 {
            let Some(c) = self.line[self.offset..].chars().next() else {
                break;
            };
            if c == '\t' {
                let chars_to_tab = 4 - self.column % 4;
                if columns {
                    self.partially_consumed_tab = chars_to_tab > count;
                    let adv = chars_to_tab.min(count);
                    self.column += adv;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= adv;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += chars_to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                self.offset += c.len_utf8();
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn add_line(&mut self) {
        let tip = self.tip;
        if self.partially_consumed_tab {
            self.offset += 1;
            let chars_to_tab = 4 - self.column % 4;
            let src = self.line_start + self.offset;
            self.blocks[tip]
                .content
                .push(&" ".repeat(chars_to_tab), src);
        }
        let rest = &self.line[self.offset..];
        let src = self.line_start + self.offset;
        let content = &mut self.blocks[tip].content;
        content.push(rest, src);
        // 原文に改行がある行だけ改行を加える（最後の行に改行が無い場合の内容を原文と一致させる）
        if self.line_start + self.line.len() < self.src.len() {
            content.push("\n", self.line_start + self.line.len());
        }
    }

    fn add_child(&mut self, kind: Kind, offset: usize) -> usize {
        while !self.can_contain(self.tip, &kind) {
            self.finalize(self.tip, self.line_number - 1);
        }
        let id = self.blocks.len();
        self.blocks.push(Block {
            kind,
            parent: Some(self.tip),
            children: Vec::new(),
            open: true,
            start_line: self.line_number,
            end_line: self.line_number,
            start: self.line_start + offset,
            content: Content::default(),
            removed: false,
        });
        self.blocks[self.tip].children.push(id);
        self.tip = id;
        id
    }

    fn can_contain(&self, parent: usize, child: &Kind) -> bool {
        match self.blocks[parent].kind {
            Kind::Document | Kind::BlockQuote | Kind::Item(_) => !matches!(child, Kind::Item(_)),
            Kind::List { .. } => matches!(child, Kind::Item(_)),
            _ => false,
        }
    }

    fn accepts_lines(&self, b: usize) -> bool {
        matches!(
            self.blocks[b].kind,
            Kind::Paragraph | Kind::CodeBlock { .. } | Kind::HtmlBlock { .. }
        )
    }

    fn close_unmatched_blocks(&mut self) {
        if !self.all_closed {
            while self.oldtip != self.last_matched_container {
                let parent = self.blocks[self.oldtip].parent.expect("not document");
                self.finalize(self.oldtip, self.line_number - 1);
                self.oldtip = parent;
            }
            self.all_closed = true;
        }
    }

    /// ブロックの継続の判定。0: 一致、1: 不一致、2: 行を消費した（フェンスの閉じ）。
    fn continue_block(&mut self, b: usize) -> u8 {
        match &self.blocks[b].kind {
            Kind::Document | Kind::List { .. } => 0,
            Kind::BlockQuote => {
                if !self.indented && self.peek(self.next_nonspace) == Some(b'>') {
                    self.advance_next_nonspace();
                    self.advance_offset(1, false);
                    if scan::is_space_or_tab(self.peek(self.offset)) {
                        self.advance_offset(1, true);
                    }
                    0
                } else {
                    1
                }
            }
            Kind::Item(data) => {
                let need = data.marker_offset + data.padding;
                // 空行の判定より字下げを先に見る（cmark と同じ。コードブロック内の空行の空白を残す）
                if self.blank && self.blocks[b].children.is_empty() {
                    return 1;
                } else if self.indent >= need {
                    self.advance_offset(need, true);
                } else if self.blank {
                    self.advance_next_nonspace();
                } else {
                    return 1;
                }
                0
            }
            Kind::Heading { .. } | Kind::ThematicBreak => 1,
            &Kind::CodeBlock {
                fenced: true,
                fence_char,
                fence_length,
                fence_offset,
                ..
            } => {
                let rest = &self.line.as_bytes()[self.next_nonspace..];
                let closing = self.indent <= 3 && rest.first() == Some(&fence_char) && {
                    let n = rest.iter().take_while(|&&c| c == fence_char).count();
                    n >= fence_length && rest[n..].iter().all(|&c| c == b' ' || c == b'\t')
                };
                if closing {
                    self.finalize(b, self.line_number);
                    return 2;
                }
                let mut i = fence_offset;
                while i > 0 && scan::is_space_or_tab(self.peek(self.offset)) {
                    self.advance_offset(1, true);
                    i -= 1;
                }
                0
            }
            Kind::CodeBlock { fenced: false, .. } => {
                if self.indent >= CODE_INDENT {
                    self.advance_offset(CODE_INDENT, true);
                } else if self.blank {
                    self.advance_next_nonspace();
                } else {
                    return 1;
                }
                0
            }
            &Kind::HtmlBlock { html_type, .. } => {
                u8::from(self.blank && (html_type == 6 || html_type == 7))
            }
            Kind::Paragraph => u8::from(self.blank),
        }
    }

    fn finalize(&mut self, b: usize, line_number: usize) {
        let above = self.blocks[b].parent;
        self.blocks[b].open = false;
        self.blocks[b].end_line = line_number;
        match &self.blocks[b].kind {
            Kind::Paragraph => {
                let mut has_refs = false;
                loop {
                    // 遅延継続の行は先頭の空白を残すため、定義の前の空白を読み飛ばす
                    let rest = self.blocks[b].content.rest();
                    let ws = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                    if !rest[ws..].starts_with('[') {
                        break;
                    }
                    let n = inline::parse_reference(&rest[ws..], &mut self.refmap);
                    if n == 0 {
                        break;
                    }
                    let n = ws + n;
                    self.blocks[b].content.skip += n;
                    has_refs = true;
                }
                // 内容がリンク参照定義だけの段落は取り除く（setext 見出しの判定で定義を読んだ場合を含む）
                if (has_refs || self.blocks[b].content.skip > 0)
                    && scan::is_blank(self.blocks[b].content.rest())
                {
                    self.blocks[b].removed = true;
                }
            }
            Kind::CodeBlock { fenced, .. } => {
                let fenced = *fenced;
                let text = std::mem::take(&mut self.blocks[b].content.text);
                let (info, literal, end_line) = if fenced {
                    let nl = text.find('\n').unwrap_or(text.len());
                    let info = scan::unescape(text[..nl].trim_matches([' ', '\t']));
                    let rest = text.get(nl + 1..).unwrap_or("").to_string();
                    (Some(info), rest, line_number)
                } else {
                    let mut lines: Vec<&str> = text.split('\n').collect();
                    while lines
                        .last()
                        .is_some_and(|l| l.bytes().all(|c| c == b' ' || c == b'\t'))
                    {
                        lines.pop();
                    }
                    let start = self.blocks[b].start_line;
                    let end = start + lines.len().saturating_sub(1);
                    (None, lines.join("\n") + "\n", end)
                };
                self.blocks[b].end_line = end_line;
                if let Kind::CodeBlock {
                    info: i,
                    literal: l,
                    ..
                } = &mut self.blocks[b].kind
                {
                    *i = info;
                    *l = literal;
                }
            }
            Kind::HtmlBlock { .. } => {
                let text = std::mem::take(&mut self.blocks[b].content.text);
                // `(\n *)+$` を取り除く（末尾の空白だけの行と改行）
                let mut end = text.len();
                let bytes = text.as_bytes();
                loop {
                    let mut j = end;
                    while j > 0 && bytes[j - 1] == b' ' {
                        j -= 1;
                    }
                    if j > 0 && bytes[j - 1] == b'\n' {
                        end = j - 1;
                    } else {
                        break;
                    }
                }
                if let Kind::HtmlBlock { literal, .. } = &mut self.blocks[b].kind {
                    *literal = text[..end].to_string();
                }
            }
            Kind::Item(_) => {
                // 項目の終わりは最後の子の終わり（参照定義だけの段落を含む。spec 5.3 の空行の判定に使う）
                self.blocks[b].end_line = match self.blocks[b].children.last() {
                    Some(&last) => self.blocks[last].end_line,
                    None => self.blocks[b].start_line,
                };
            }
            Kind::List { .. } => {
                let items: Vec<usize> = self.blocks[b].children.clone();
                let mut tight = true;
                'outer: for (i, &item) in items.iter().enumerate() {
                    if i + 1 < items.len() && self.ends_with_blank_line(item, items[i + 1]) {
                        tight = false;
                        break;
                    }
                    // 参照定義だけの段落（出力からは除く）もブロックとして数える（spec 5.3）
                    let subs: Vec<usize> = self.blocks[item].children.clone();
                    for (j, &sub) in subs.iter().enumerate() {
                        if j + 1 < subs.len() && self.ends_with_blank_line(sub, subs[j + 1]) {
                            tight = false;
                            break 'outer;
                        }
                    }
                }
                if let Some(&last) = items.last() {
                    self.blocks[b].end_line = self.blocks[last].end_line;
                }
                if let Kind::List { tight: t, .. } = &mut self.blocks[b].kind {
                    *t = tight;
                }
            }
            _ => {}
        }
        if let Some(above) = above {
            self.tip = above;
        }
    }

    fn depth(&self, mut b: usize) -> usize {
        let mut d = 0;
        while let Some(p) = self.blocks[b].parent {
            d += 1;
            b = p;
        }
        d
    }

    fn ends_with_blank_line(&self, b: usize, next: usize) -> bool {
        self.blocks[b].end_line + 1 != self.blocks[next].start_line
    }

    /// ブロックの開始の判定。0: 無し、1: コンテナーを開始、2: 葉を開始。
    fn try_starts(&mut self, container: usize) -> u8 {
        let rest = &self.line.as_bytes()[self.next_nonspace..];
        let first = rest.first().copied();
        let is_paragraph = matches!(self.blocks[container].kind, Kind::Paragraph);

        // ブロック引用
        let depth = self.depth(container);
        if !self.indented && first == Some(b'>') && depth < MAX_DEPTH {
            self.advance_next_nonspace();
            self.advance_offset(1, false);
            if scan::is_space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
            self.close_unmatched_blocks();
            self.add_child(Kind::BlockQuote, self.next_nonspace);
            return 1;
        }

        // ATX 見出し
        if !self.indented && first == Some(b'#') {
            let n = rest.iter().take_while(|&&c| c == b'#').count();
            if n <= 6 && rest.get(n).is_none_or(|&c| c == b' ' || c == b'\t') {
                self.advance_next_nonspace();
                self.advance_offset(n, false);
                self.close_unmatched_blocks();
                let h = self.add_child(
                    Kind::Heading {
                        level: n as u8,
                        setext: false,
                    },
                    self.next_nonspace,
                );
                let text = &self.line[self.offset..];
                let content = strip_atx_closing(text);
                let lead = content.len() - content.trim_start_matches([' ', '\t']).len();
                let src = self.line_start + self.offset + lead;
                self.blocks[h]
                    .content
                    .push(content.trim_start_matches([' ', '\t']), src);
                self.offset = self.line.len();
                return 2;
            }
        }

        // フェンスによるコードブロック
        if !self.indented && matches!(first, Some(b'`' | b'~')) {
            let fc = first.unwrap_or(b'`');
            let n = rest.iter().take_while(|&&c| c == fc).count();
            if n >= 3 && !(fc == b'`' && rest[n..].contains(&b'`')) {
                self.close_unmatched_blocks();
                let indent = self.indent;
                self.add_child(
                    Kind::CodeBlock {
                        fenced: true,
                        fence_char: fc,
                        fence_length: n,
                        fence_offset: indent,
                        info: None,
                        literal: String::new(),
                    },
                    self.next_nonspace,
                );
                self.advance_next_nonspace();
                self.advance_offset(n, false);
                return 2;
            }
        }

        // HTML ブロック
        if !self.indented && first == Some(b'<') {
            let lazy_paragraph = !self.all_closed
                && !self.blank
                && matches!(self.blocks[self.tip].kind, Kind::Paragraph);
            let can_interrupt = !is_paragraph && !lazy_paragraph;
            if let Some(t) = scan::html_block_start(rest, can_interrupt) {
                self.close_unmatched_blocks();
                self.add_child(
                    Kind::HtmlBlock {
                        html_type: t,
                        literal: String::new(),
                    },
                    self.offset,
                );
                return 2;
            }
        }

        // setext 見出し
        if !self.indented && is_paragraph && matches!(first, Some(b'=' | b'-')) {
            let c = first.unwrap_or(b'=');
            let n = rest.iter().take_while(|&&x| x == c).count();
            if rest[n..].iter().all(|&x| x == b' ' || x == b'\t') {
                self.close_unmatched_blocks();
                loop {
                    let rest = self.blocks[container].content.rest();
                    let ws = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                    if !rest[ws..].starts_with('[') {
                        break;
                    }
                    let k = inline::parse_reference(&rest[ws..], &mut self.refmap);
                    if k == 0 {
                        break;
                    }
                    let k = ws + k;
                    self.blocks[container].content.skip += k;
                }
                if !self.blocks[container].content.rest().is_empty() {
                    self.blocks[container].kind = Kind::Heading {
                        level: if c == b'=' { 1 } else { 2 },
                        setext: true,
                    };
                    self.blocks[container].end_line = self.line_number;
                    self.offset = self.line.len();
                    return 2;
                }
            }
        }

        // 区切り線
        if !self.indented && matches!(first, Some(b'*' | b'_' | b'-')) {
            let c = first.unwrap_or(b'*');
            let mut count = 0;
            let ok = rest.iter().all(|&x| {
                if x == c {
                    count += 1;
                    true
                } else {
                    x == b' ' || x == b'\t'
                }
            });
            if ok && count >= 3 {
                self.close_unmatched_blocks();
                self.add_child(Kind::ThematicBreak, self.next_nonspace);
                self.offset = self.line.len();
                return 2;
            }
        }

        // リストの項目
        if (!self.indented || matches!(self.blocks[container].kind, Kind::List { .. }))
            && depth + 2 <= MAX_DEPTH
            && let Some(data) = self.parse_list_marker(container)
        {
            self.close_unmatched_blocks();
            let matches_list = match self.blocks[self.tip].kind {
                Kind::List { data: d, .. } => {
                    d.ordered == data.ordered
                        && d.delimiter == data.delimiter
                        && d.bullet_char == data.bullet_char
                }
                _ => false,
            };
            if !matches_list {
                self.add_child(Kind::List { data, tight: true }, self.next_nonspace);
            }
            self.add_child(Kind::Item(data), self.next_nonspace);
            return 1;
        }

        // インデントによるコードブロック
        if self.indented && !matches!(self.blocks[self.tip].kind, Kind::Paragraph) && !self.blank {
            self.advance_offset(CODE_INDENT, true);
            self.close_unmatched_blocks();
            self.add_child(
                Kind::CodeBlock {
                    fenced: false,
                    fence_char: 0,
                    fence_length: 0,
                    fence_offset: 0,
                    info: None,
                    literal: String::new(),
                },
                self.offset,
            );
            return 2;
        }
        0
    }

    fn parse_list_marker(&mut self, container: usize) -> Option<ListData> {
        if self.indent >= 4 {
            return None;
        }
        let rest = &self.line.as_bytes()[self.next_nonspace..];
        let is_paragraph = matches!(self.blocks[container].kind, Kind::Paragraph);
        let (mut data, len) = match rest.first()? {
            c @ (b'*' | b'+' | b'-') => (
                ListData {
                    ordered: false,
                    bullet_char: *c,
                    start: 0,
                    delimiter: 0,
                    padding: 0,
                    marker_offset: self.indent,
                },
                1,
            ),
            c if c.is_ascii_digit() => {
                let n = rest.iter().take_while(|c| c.is_ascii_digit()).count();
                if n > 9 {
                    return None;
                }
                let delim = *rest.get(n)?;
                if delim != b'.' && delim != b')' {
                    return None;
                }
                let start: u64 = std::str::from_utf8(&rest[..n]).ok()?.parse().ok()?;
                if is_paragraph && start != 1 {
                    return None;
                }
                (
                    ListData {
                        ordered: true,
                        bullet_char: 0,
                        start,
                        delimiter: delim,
                        padding: 0,
                        marker_offset: self.indent,
                    },
                    n + 1,
                )
            }
            _ => return None,
        };
        let next = rest.get(len).copied();
        if !matches!(next, None | Some(b'\t' | b' ')) {
            return None;
        }
        if is_paragraph && rest[len..].iter().all(|&c| c == b' ' || c == b'\t') {
            return None;
        }
        self.advance_next_nonspace();
        self.advance_offset(len, true);
        let spaces_start_col = self.column;
        let spaces_start_offset = self.offset;
        loop {
            self.advance_offset(1, true);
            let c = self.peek(self.offset);
            if !(self.column - spaces_start_col < 5 && scan::is_space_or_tab(c)) {
                break;
            }
        }
        let blank_item = self.peek(self.offset).is_none();
        let spaces_after = self.column - spaces_start_col;
        if !(1..5).contains(&spaces_after) || blank_item {
            data.padding = len + 1;
            self.column = spaces_start_col;
            self.offset = spaces_start_offset;
            self.partially_consumed_tab = false;
            if scan::is_space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
        } else {
            data.padding = len + spaces_after;
        }
        Some(data)
    }

    fn incorporate_line(&mut self, idx: usize) {
        let l = &self.lines[idx];
        self.line = &self.src[l.start..l.end];
        self.line_start = l.start;
        self.line_number = idx + 1;
        self.oldtip = self.tip;
        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;

        let mut all_matched = true;
        let mut container = 0;
        while let Some(last) = self.last_child(container) {
            if !self.blocks[last].open {
                break;
            }
            container = last;
            self.find_next_nonspace();
            match self.continue_block(container) {
                0 => {}
                1 => all_matched = false,
                _ => return,
            }
            if !all_matched {
                container = self.blocks[container].parent.unwrap_or(0);
                break;
            }
        }
        self.all_closed = container == self.oldtip;
        self.last_matched_container = container;

        let mut matched_leaf = !matches!(self.blocks[container].kind, Kind::Paragraph)
            && self.accepts_lines(container);
        // 遅延継続の行は先頭の空白を残す（cmark と同じ。commonmark.js は除く）
        let mut lazy_from = None;
        while !matched_leaf {
            self.find_next_nonspace();
            let maybe_special = self
                .peek(self.next_nonspace)
                .is_some_and(|c| b"#`~*+_=<>0123456789-".contains(&c));
            if !self.indented && !maybe_special {
                lazy_from = Some((self.offset, self.column, self.partially_consumed_tab));
                self.advance_next_nonspace();
                break;
            }
            match self.try_starts(container) {
                1 => container = self.tip,
                2 => {
                    container = self.tip;
                    matched_leaf = true;
                }
                _ => {
                    lazy_from = Some((self.offset, self.column, self.partially_consumed_tab));
                    self.advance_next_nonspace();
                    break;
                }
            }
        }

        if !self.all_closed && !self.blank && matches!(self.blocks[self.tip].kind, Kind::Paragraph)
        {
            // 段落の遅延継続
            if let Some((o, c, t)) = lazy_from {
                (self.offset, self.column, self.partially_consumed_tab) = (o, c, t);
            }
            self.add_line();
        } else {
            self.close_unmatched_blocks();
            if self.accepts_lines(container) {
                self.add_line();
                if let Kind::HtmlBlock { html_type, .. } = self.blocks[container].kind
                    && (1..=5).contains(&html_type)
                    && scan::html_block_end(html_type, &self.line.as_bytes()[self.offset..])
                {
                    self.finalize(container, self.line_number);
                }
            } else if self.offset < self.line.len() && !self.blank {
                container = self.add_child(Kind::Paragraph, self.offset);
                let _ = container;
                self.advance_next_nonspace();
                self.add_line();
            }
        }
    }

    fn run(mut self) -> Node {
        for i in 0..self.lines.len() {
            self.incorporate_line(i);
        }
        let n = self.lines.len();
        while self.tip != 0 || self.blocks[0].open {
            let t = self.tip;
            self.finalize(t, n);
            if t == 0 {
                break;
            }
        }
        let refmap = std::mem::take(&mut self.refmap);
        self.build(0, &refmap)
    }

    fn line_end(&self, line: usize) -> usize {
        match line.checked_sub(1).and_then(|i| self.lines.get(i)) {
            Some(l) => l.end,
            None => self.src.len(),
        }
    }

    fn build(&self, b: usize, refmap: &RefMap) -> Node {
        let blk = &self.blocks[b];
        let end = if b == 0 {
            self.src.len()
        } else {
            self.line_end(blk.end_line).max(blk.start)
        };
        let range = blk.start..end;
        let kind = match &blk.kind {
            Kind::Document => NodeKind::Document,
            Kind::BlockQuote => NodeKind::BlockQuote,
            Kind::List { data, tight } => NodeKind::List(ListInfo {
                ordered: data.ordered,
                start: data.start,
                marker: if data.ordered {
                    data.delimiter
                } else {
                    data.bullet_char
                },
                tight: *tight,
            }),
            Kind::Item(_) => NodeKind::Item,
            Kind::Paragraph => NodeKind::Paragraph,
            Kind::Heading { level, setext } => NodeKind::Heading {
                level: *level,
                setext: *setext,
            },
            Kind::ThematicBreak => NodeKind::ThematicBreak,
            Kind::CodeBlock { info, literal, .. } => NodeKind::CodeBlock {
                info: info.clone(),
                literal: literal.clone(),
            },
            Kind::HtmlBlock { literal, .. } => NodeKind::HtmlBlock {
                literal: literal.clone(),
            },
        };
        let mut node = Node::new(kind, range);
        match blk.kind {
            Kind::Paragraph | Kind::Heading { .. } => {
                node.children = inline::parse(&blk.content, refmap);
            }
            _ => {
                node.children = blk
                    .children
                    .iter()
                    .filter(|&&c| !self.blocks[c].removed)
                    .map(|&c| self.build(c, refmap))
                    .collect();
            }
        }
        node
    }
}

/// ATX 見出しの閉じの `#` の列を取り除く。
fn strip_atx_closing(s: &str) -> &str {
    let t = s.trim_end_matches([' ', '\t']);
    let without = t.trim_end_matches('#');
    if without.len() == t.len() {
        return t;
    }
    if without.is_empty() {
        return "";
    }
    if without.ends_with([' ', '\t']) {
        return without.trim_end_matches([' ', '\t']);
    }
    t
}

/// 文書を解析する。
pub fn parse(src: &str) -> Node {
    Parser::new(src).run()
}
