//! インラインの解析（CommonMark spec 0.31.2 の第2段階）。

use std::collections::HashMap;

use crate::ast::{Node, NodeKind};
use crate::block::Content;
use crate::scan;

/// リンク参照定義（正規化したラベル → (リンク先, タイトル)）。
pub(crate) type RefMap = HashMap<String, (String, String)>;

#[derive(Debug, Clone)]
enum IKind {
    Root,
    Text(String),
    SoftBreak,
    HardBreak,
    Code(String),
    Emph,
    Strong,
    Link { dest: String, title: String },
    Image { dest: String, title: String },
    Html(String),
}

#[derive(Debug)]
struct Inl {
    /// 部分木の高さ（葉は 1）。入れ子の深さの上限の判定に使う。
    height: usize,
    kind: IKind,
    start: usize,
    end: usize,
    parent: Option<usize>,
    first: Option<usize>,
    last: Option<usize>,
    prev: Option<usize>,
    next: Option<usize>,
}

struct Delim {
    cc: u8,
    numdelims: usize,
    origdelims: usize,
    node: usize,
    previous: Option<usize>,
    next: Option<usize>,
    can_open: bool,
    can_close: bool,
}

struct Bracket {
    node: usize,
    previous: Option<usize>,
    previous_delimiter: Option<usize>,
    index: usize,
    image: bool,
    active: bool,
    bracket_after: bool,
}

struct Parser<'a> {
    subject: &'a str,
    pos: usize,
    nodes: Vec<Inl>,
    delims: Vec<Delim>,
    delimiters: Option<usize>,
    brackets_v: Vec<Bracket>,
    brackets: Option<usize>,
    refmap: &'a RefMap,
    /// バッククォートの列（位置, 長さ）。コードスパンの閉じの検索に使う（二乗の時間を避ける）。
    tick_runs: Option<Vec<(usize, usize)>>,
    /// 閉じの検索に失敗した HTML の種類（コメント、処理命令、CDATA、宣言）。以降の位置でも閉じは無い。
    html_unclosed: [bool; 4],
}

/// インラインの入れ子の深さの上限（Spec §5）。
const MAX_INLINE_DEPTH: usize = 100;

/// リンクラベルの文字数の上限（spec 4.7）と、括弧を含むバイト数の上限（1文字は4バイト以下）。
const MAX_LABEL_CHARS: usize = 999;
const MAX_LABEL_BYTES: usize = MAX_LABEL_CHARS * 4 + 2;

/// リンク先の括弧の入れ子の上限（spec 6.3 が実装に認める制限）。
const MAX_LINK_PARENS: usize = 32;

impl<'a> Parser<'a> {
    fn new(subject: &'a str, refmap: &'a RefMap) -> Self {
        let root = Inl {
            height: 1,
            kind: IKind::Root,
            start: 0,
            end: subject.len(),
            parent: None,
            first: None,
            last: None,
            prev: None,
            next: None,
        };
        Self {
            subject,
            pos: 0,
            nodes: vec![root],
            delims: Vec::new(),
            delimiters: None,
            brackets_v: Vec::new(),
            brackets: None,
            refmap,
            tick_runs: None,
            html_unclosed: [false; 4],
        }
    }

    fn bytes(&self) -> &'a [u8] {
        self.subject.as_bytes()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes().get(self.pos).copied()
    }

    fn new_node(&mut self, kind: IKind, start: usize, end: usize) -> usize {
        self.nodes.push(Inl {
            height: 1,
            kind,
            start,
            end,
            parent: None,
            first: None,
            last: None,
            prev: None,
            next: None,
        });
        self.nodes.len() - 1
    }

    fn append_child(&mut self, parent: usize, child: usize) {
        self.unlink(child);
        self.nodes[child].parent = Some(parent);
        match self.nodes[parent].last {
            Some(last) => {
                self.nodes[last].next = Some(child);
                self.nodes[child].prev = Some(last);
            }
            None => self.nodes[parent].first = Some(child),
        }
        self.nodes[parent].last = Some(child);
    }

    fn insert_after(&mut self, node: usize, sibling: usize) {
        self.unlink(sibling);
        let next = self.nodes[node].next;
        let parent = self.nodes[node].parent;
        self.nodes[sibling].next = next;
        self.nodes[sibling].prev = Some(node);
        self.nodes[sibling].parent = parent;
        self.nodes[node].next = Some(sibling);
        match next {
            Some(n) => self.nodes[n].prev = Some(sibling),
            None => {
                if let Some(p) = parent {
                    self.nodes[p].last = Some(sibling);
                }
            }
        }
    }

    fn unlink(&mut self, node: usize) {
        let (prev, next, parent) = (
            self.nodes[node].prev,
            self.nodes[node].next,
            self.nodes[node].parent,
        );
        match prev {
            Some(p) => self.nodes[p].next = next,
            None => {
                if let Some(par) = parent {
                    self.nodes[par].first = next;
                }
            }
        }
        match next {
            Some(n) => self.nodes[n].prev = prev,
            None => {
                if let Some(par) = parent {
                    self.nodes[par].last = prev;
                }
            }
        }
        self.nodes[node].prev = None;
        self.nodes[node].next = None;
        self.nodes[node].parent = None;
    }

    /// 兄弟の節 `from` から `until` の手前までの高さの最大値。
    fn span_height(&self, from: Option<usize>, until: Option<usize>) -> usize {
        let mut h = 0;
        let mut cur = from;
        while let Some(i) = cur {
            if Some(i) == until {
                break;
            }
            h = h.max(self.nodes[i].height);
            cur = self.nodes[i].next;
        }
        h
    }

    fn text(&mut self, s: impl Into<String>, start: usize, end: usize) -> usize {
        let n = self.new_node(IKind::Text(s.into()), start, end);
        self.append_child(0, n);
        n
    }

    fn append(&mut self, kind: IKind, start: usize, end: usize) -> usize {
        let n = self.new_node(kind, start, end);
        self.append_child(0, n);
        n
    }

    fn parse_inline(&mut self) -> bool {
        let Some(c) = self.peek() else { return false };
        let handled = match c {
            b'\n' => self.parse_newline(),
            b'\\' => self.parse_backslash(),
            b'`' => self.parse_backticks(),
            b'*' | b'_' => self.handle_delim(c),
            b'[' => self.parse_open_bracket(),
            b'!' => self.parse_bang(),
            b']' => self.parse_close_bracket(),
            b'<' => self.parse_autolink() || self.parse_html_tag(),
            b'&' => self.parse_entity(),
            _ => self.parse_string(),
        };
        if !handled {
            let start = self.pos;
            let ch = self.subject[start..].chars().next().unwrap_or('\0');
            self.pos += ch.len_utf8();
            self.text(ch.to_string(), start, self.pos);
        }
        true
    }

    fn parse_string(&mut self) -> bool {
        let start = self.pos;
        let rest = &self.bytes()[start..];
        let n = rest
            .iter()
            .position(|c| {
                matches!(
                    c,
                    b'\n' | b'`' | b'[' | b']' | b'\\' | b'!' | b'<' | b'&' | b'*' | b'_'
                )
            })
            .unwrap_or(rest.len());
        if n == 0 {
            return false;
        }
        self.pos += n;
        let s = self.subject[start..self.pos].to_string();
        self.text(s, start, self.pos);
        true
    }

    fn parse_newline(&mut self) -> bool {
        let nl = self.pos;
        self.pos += 1;
        let b = self.bytes();
        // 改行の前の空白・タブ（原文で数える）。行末の2つ以上の空白はハード改行（spec 6.7）
        let ws = b[..nl]
            .iter()
            .rev()
            .take_while(|&&c| c == b' ' || c == b'\t')
            .count();
        let hard = nl >= 2 && b[nl - 1] == b' ' && b[nl - 2] == b' ';
        let mut start = nl;
        if ws > 0
            && let Some(l) = self.nodes[0].last
            && self.nodes[l].end == nl
            && self.nodes[l].start <= nl - ws
            && let IKind::Text(t) = &mut self.nodes[l].kind
        {
            t.truncate(t.len() - ws);
            self.nodes[l].end -= ws;
            start = nl - ws;
        }
        self.append(
            if hard {
                IKind::HardBreak
            } else {
                IKind::SoftBreak
            },
            start,
            nl + 1,
        );
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
        true
    }

    fn parse_backslash(&mut self) -> bool {
        let start = self.pos;
        self.pos += 1;
        match self.peek() {
            Some(b'\n') => {
                self.pos += 1;
                self.append(IKind::HardBreak, start, self.pos);
                // 次の行の先頭の空白は読み飛ばす（spec 6.7）
                while matches!(self.peek(), Some(b' ' | b'\t')) {
                    self.pos += 1;
                }
            }
            Some(c) if scan::is_escapable(c) => {
                self.pos += 1;
                self.text((c as char).to_string(), start, self.pos);
            }
            _ => {
                self.text("\\", start, self.pos);
            }
        }
        true
    }

    fn parse_backticks(&mut self) -> bool {
        let start = self.pos;
        let b = self.bytes();
        let n = b[start..].iter().take_while(|&&c| c == b'`').count();
        let after_open = start + n;
        let runs = self.tick_runs.get_or_insert_with(|| {
            let mut v = Vec::new();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'`' {
                    let m = b[i..].iter().take_while(|&&c| c == b'`').count();
                    v.push((i, m));
                    i += m;
                } else {
                    i += 1;
                }
            }
            v
        });
        let k = runs.partition_point(|&(p, _)| p < after_open);
        if let Some(&(i, m)) = runs[k..].iter().find(|&&(_, m)| m == n) {
            let raw = self.subject[after_open..i].replace('\n', " ");
            let literal = if raw.len() >= 2
                && raw.bytes().any(|c| c != b' ')
                && raw.starts_with(' ')
                && raw.ends_with(' ')
            {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw
            };
            self.pos = i + m;
            self.append(IKind::Code(literal), start, self.pos);
            return true;
        }
        self.pos = after_open;
        let ticks = self.subject[start..after_open].to_string();
        self.text(ticks, start, after_open);
        true
    }

    fn scan_delims(&self, cc: u8) -> (usize, bool, bool) {
        let start = self.pos;
        let b = self.bytes();
        let n = b[start..].iter().take_while(|&&c| c == cc).count();
        let before = self.subject[..start].chars().next_back().unwrap_or('\n');
        let after = self.subject[start + n..].chars().next().unwrap_or('\n');
        let after_ws = scan::is_unicode_whitespace(after);
        let after_p = scan::is_unicode_punctuation(after);
        let before_ws = scan::is_unicode_whitespace(before);
        let before_p = scan::is_unicode_punctuation(before);
        let left = !after_ws && (!after_p || before_ws || before_p);
        let right = !before_ws && (!before_p || after_ws || after_p);
        if cc == b'_' {
            (n, left && (!right || before_p), right && (!left || after_p))
        } else {
            (n, left, right)
        }
    }

    fn handle_delim(&mut self, cc: u8) -> bool {
        let (n, can_open, can_close) = self.scan_delims(cc);
        let start = self.pos;
        self.pos += n;
        let node = self.text(self.subject[start..self.pos].to_string(), start, self.pos);
        if can_open || can_close {
            let d = self.delims.len();
            self.delims.push(Delim {
                cc,
                numdelims: n,
                origdelims: n,
                node,
                previous: self.delimiters,
                next: None,
                can_open,
                can_close,
            });
            if let Some(p) = self.delimiters {
                self.delims[p].next = Some(d);
            }
            self.delimiters = Some(d);
        }
        true
    }

    fn remove_delimiter(&mut self, d: usize) {
        let (prev, next) = (self.delims[d].previous, self.delims[d].next);
        if let Some(p) = prev {
            self.delims[p].next = next;
        }
        match next {
            Some(n) => self.delims[n].previous = prev,
            None => self.delimiters = prev,
        }
    }

    fn add_bracket(&mut self, node: usize, index: usize, image: bool) {
        if let Some(b) = self.brackets {
            self.brackets_v[b].bracket_after = true;
        }
        self.brackets_v.push(Bracket {
            node,
            previous: self.brackets,
            previous_delimiter: self.delimiters,
            index,
            image,
            active: true,
            bracket_after: false,
        });
        self.brackets = Some(self.brackets_v.len() - 1);
    }

    fn remove_bracket(&mut self) {
        if let Some(b) = self.brackets {
            self.brackets = self.brackets_v[b].previous;
        }
    }

    fn parse_open_bracket(&mut self) -> bool {
        let start = self.pos;
        self.pos += 1;
        let node = self.text("[", start, self.pos);
        self.add_bracket(node, start, false);
        true
    }

    fn parse_bang(&mut self) -> bool {
        let start = self.pos;
        self.pos += 1;
        if self.peek() == Some(b'[') {
            self.pos += 1;
            let node = self.text("![", start, self.pos);
            self.add_bracket(node, start + 1, true);
        } else {
            self.text("!", start, self.pos);
        }
        true
    }

    /// 空白・タブと、高々1つの改行を読み飛ばす。
    fn spnl(&mut self) {
        let b = self.bytes();
        while matches!(b.get(self.pos), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
        if b.get(self.pos) == Some(&b'\n') {
            self.pos += 1;
            while matches!(b.get(self.pos), Some(b' ' | b'\t')) {
                self.pos += 1;
            }
        }
    }

    fn parse_link_destination(&mut self) -> Option<String> {
        let b = self.bytes();
        let start = self.pos;
        if b.get(start) == Some(&b'<') {
            let mut i = start + 1;
            while let Some(&c) = b.get(i) {
                match c {
                    b'>' => {
                        self.pos = i + 1;
                        return Some(scan::unescape(&self.subject[start + 1..i]));
                    }
                    b'\\' if b.get(i + 1).is_some_and(|&n| scan::is_escapable(n)) => i += 2,
                    b'<' | b'\n' => return None,
                    _ => i += 1,
                }
            }
            return None;
        }
        let mut open = 0usize;
        let mut i = start;
        while let Some(&c) = b.get(i) {
            match c {
                b'\\' if b.get(i + 1).is_some_and(|&n| scan::is_escapable(n)) => i += 2,
                b'(' => {
                    open += 1;
                    if open > MAX_LINK_PARENS {
                        return None;
                    }
                    i += 1;
                }
                b')' => {
                    if open == 0 {
                        break;
                    }
                    open -= 1;
                    i += 1;
                }
                c if c <= b' ' || c == 0x7f => break,
                _ => i += 1,
            }
        }
        if i == start && b.get(i) != Some(&b')') {
            return None;
        }
        if open != 0 {
            return None;
        }
        self.pos = i;
        Some(scan::unescape(&self.subject[start..i]))
    }

    fn parse_link_title(&mut self) -> Option<String> {
        let b = self.bytes();
        let start = self.pos;
        let close = match b.get(start)? {
            b'"' => b'"',
            b'\'' => b'\'',
            b'(' => b')',
            _ => return None,
        };
        let mut i = start + 1;
        while let Some(&c) = b.get(i) {
            if c == b'\\' && i + 1 < b.len() {
                i += 2;
            } else if c == close {
                self.pos = i + 1;
                return Some(scan::unescape(&self.subject[start + 1..i]));
            } else if close == b')' && c == b'(' {
                return None;
            } else {
                i += 1;
            }
        }
        None
    }

    /// リンクのラベル（`[` から `]` まで）の長さ。無ければ 0。
    fn parse_link_label(&mut self) -> usize {
        let b = self.bytes();
        let start = self.pos;
        if b.get(start) != Some(&b'[') {
            return 0;
        }
        let mut i = start + 1;
        while let Some(&c) = b.get(i) {
            match c {
                b'\\' if i + 1 < b.len() => i += 2,
                b'[' => return 0,
                b']' => {
                    let len = i + 1 - start;
                    // 空白だけの括弧はリンクラベルではない（spec 6.3）。`[]` は省略形の参照として呼び出し側で扱う
                    let blank = len > 2
                        && b[start + 1..i]
                            .iter()
                            .all(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'));
                    let chars = b[start + 1..i]
                        .iter()
                        .filter(|&&c| c & 0xC0 != 0x80)
                        .count();
                    if chars > MAX_LABEL_CHARS || blank {
                        return 0;
                    }
                    self.pos = i + 1;
                    return len;
                }
                _ => i += 1,
            }
            if i - start > MAX_LABEL_BYTES {
                return 0;
            }
        }
        0
    }

    fn parse_close_bracket(&mut self) -> bool {
        let close_start = self.pos;
        self.pos += 1;
        let startpos = self.pos;
        let Some(opener) = self.brackets else {
            self.text("]", close_start, startpos);
            return true;
        };
        if !self.brackets_v[opener].active {
            self.text("]", close_start, startpos);
            self.remove_bracket();
            return true;
        }
        let is_image = self.brackets_v[opener].image;
        let savepos = self.pos;
        let mut matched: Option<(String, String)> = None;

        if self.peek() == Some(b'(') {
            self.pos += 1;
            self.spnl();
            if let Some(dest) = self.parse_link_destination() {
                let before_title = self.pos;
                self.spnl();
                let ws_before = self.pos > before_title
                    && self
                        .subject
                        .as_bytes()
                        .get(self.pos - 1)
                        .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n'));
                let title = if ws_before {
                    self.parse_link_title()
                } else {
                    None
                };
                if title.is_none() {
                    // タイトルが無い場合も、空白の後に `)` があればよい
                }
                self.spnl();
                if self.peek() == Some(b')') {
                    self.pos += 1;
                    matched = Some((dest, title.unwrap_or_default()));
                } else {
                    self.pos = savepos;
                }
            } else {
                self.pos = savepos;
            }
        }

        if matched.is_none() {
            let before_label = self.pos;
            let n = self.parse_link_label();
            let reflabel = if n > 2 {
                Some(self.subject[before_label..before_label + n].to_string())
            } else if !self.brackets_v[opener].bracket_after
                && startpos - self.brackets_v[opener].index <= MAX_LABEL_BYTES
            {
                Some(self.subject[self.brackets_v[opener].index..startpos].to_string())
            } else {
                None
            };
            if n == 0 {
                self.pos = savepos;
            }
            if let Some(label) = reflabel {
                let inner = &label[1..label.len() - 1];
                if inner.chars().count() <= MAX_LABEL_CHARS
                    && let Some((d, t)) = self.refmap.get(&scan::normalize_label(inner))
                {
                    matched = Some((d.clone(), t.clone()));
                }
            }
        }

        let opener_node = self.brackets_v[opener].node;
        let height = if matched.is_some() {
            1 + self.span_height(self.nodes[opener_node].next, None)
        } else {
            0
        };
        let matched = matched.filter(|_| height <= MAX_INLINE_DEPTH);
        let Some((dest, title)) = matched else {
            self.remove_bracket();
            self.pos = startpos;
            self.text("]", close_start, startpos);
            return true;
        };
        let start = self.nodes[opener_node].start;
        let kind = if is_image {
            IKind::Image { dest, title }
        } else {
            IKind::Link { dest, title }
        };
        let link = self.new_node(kind, start, self.pos);
        self.nodes[link].height = height;
        let mut tmp = self.nodes[opener_node].next;
        while let Some(t) = tmp {
            let next = self.nodes[t].next;
            self.append_child(link, t);
            tmp = next;
        }
        self.append_child(0, link);
        let bottom = self.brackets_v[opener].previous_delimiter;
        self.process_emphasis(bottom);
        self.remove_bracket();
        self.unlink(opener_node);
        if !is_image {
            let mut o = self.brackets;
            while let Some(b) = o {
                if !self.brackets_v[b].image {
                    self.brackets_v[b].active = false;
                }
                o = self.brackets_v[b].previous;
            }
        }
        true
    }

    fn parse_autolink(&mut self) -> bool {
        let b = &self.bytes()[self.pos..];
        let start = self.pos;
        // メールアドレス
        if let Some(n) = email_autolink(b) {
            let addr = self.subject[start + 1..start + n - 1].to_string();
            self.pos += n;
            let link = self.new_node(
                IKind::Link {
                    dest: format!("mailto:{addr}"),
                    title: String::new(),
                },
                start,
                self.pos,
            );
            let t = self.new_node(IKind::Text(addr), start + 1, self.pos - 1);
            self.append_child(link, t);
            self.append_child(0, link);
            return true;
        }
        if let Some(n) = uri_autolink(b) {
            let uri = self.subject[start + 1..start + n - 1].to_string();
            self.pos += n;
            let link = self.new_node(
                IKind::Link {
                    dest: uri.clone(),
                    title: String::new(),
                },
                start,
                self.pos,
            );
            let t = self.new_node(IKind::Text(uri), start + 1, self.pos - 1);
            self.append_child(link, t);
            self.append_child(0, link);
            return true;
        }
        false
    }

    fn parse_html_tag(&mut self) -> bool {
        let start = self.pos;
        let rest = &self.bytes()[start..];
        // 閉じを探す種類（一度見つからなければ、以降の位置でも見つからない）
        let kind = if rest.starts_with(b"<!--") {
            Some(0)
        } else if rest.starts_with(b"<?") {
            Some(1)
        } else if rest.starts_with(b"<![CDATA[") {
            Some(2)
        } else if rest.starts_with(b"<!") && rest.get(2).is_some_and(|c| c.is_ascii_alphabetic()) {
            Some(3)
        } else {
            None
        };
        if kind.is_some_and(|k| self.html_unclosed[k]) {
            return false;
        }
        match scan::html_tag(rest) {
            Some(n) => {
                self.pos += n;
                self.append(
                    IKind::Html(self.subject[start..self.pos].to_string()),
                    start,
                    self.pos,
                );
                true
            }
            None => {
                if let Some(k) = kind {
                    self.html_unclosed[k] = true;
                }
                false
            }
        }
    }

    fn parse_entity(&mut self) -> bool {
        let start = self.pos;
        match scan::entity(&self.subject[start..]) {
            Some((n, v)) => {
                self.pos += n;
                self.text(v, start, self.pos);
                true
            }
            None => false,
        }
    }

    fn process_emphasis(&mut self, stack_bottom: Option<usize>) {
        let mut openers_bottom = [stack_bottom; 14];
        let mut closer = self.delimiters;
        while let Some(c) = closer {
            if self.delims[c].previous == stack_bottom {
                break;
            }
            closer = self.delims[c].previous;
        }
        while let Some(c) = closer {
            if !self.delims[c].can_close {
                closer = self.delims[c].next;
                continue;
            }
            let cc = self.delims[c].cc;
            let idx = if cc == b'_' { 2 } else { 8 }
                + if self.delims[c].can_open { 3 } else { 0 }
                + self.delims[c].origdelims % 3;
            let mut opener = self.delims[c].previous;
            let mut found = false;
            while let Some(o) = opener {
                if Some(o) == stack_bottom || Some(o) == openers_bottom[idx] {
                    break;
                }
                let odd = (self.delims[c].can_open || self.delims[o].can_close)
                    && !self.delims[c].origdelims.is_multiple_of(3)
                    && (self.delims[o].origdelims + self.delims[c].origdelims).is_multiple_of(3);
                if self.delims[o].cc == cc && self.delims[o].can_open && !odd {
                    found = true;
                    break;
                }
                opener = self.delims[o].previous;
            }
            // 入れ子の深さの上限を超える強調は作らない
            let height = match opener {
                Some(o) if found => {
                    1 + self.span_height(
                        self.nodes[self.delims[o].node].next,
                        Some(self.delims[c].node),
                    )
                }
                _ => 0,
            };
            let found = found && height <= MAX_INLINE_DEPTH;
            let old_closer = c;
            if found {
                let o = opener.expect("found");
                let use_delims = if self.delims[c].numdelims >= 2 && self.delims[o].numdelims >= 2 {
                    2
                } else {
                    1
                };
                let (oi, ci) = (self.delims[o].node, self.delims[c].node);
                self.delims[o].numdelims -= use_delims;
                self.delims[c].numdelims -= use_delims;
                if let IKind::Text(t) = &mut self.nodes[oi].kind {
                    t.truncate(t.len() - use_delims);
                }
                if let IKind::Text(t) = &mut self.nodes[ci].kind {
                    t.truncate(t.len() - use_delims);
                }
                self.nodes[oi].end -= use_delims;
                let emph_start = self.nodes[oi].end;
                let emph_end = self.nodes[ci].start + use_delims;
                self.nodes[ci].start += use_delims;
                let kind = if use_delims == 1 {
                    IKind::Emph
                } else {
                    IKind::Strong
                };
                let emph = self.new_node(kind, emph_start, emph_end);
                self.nodes[emph].height = height;
                let mut tmp = self.nodes[oi].next;
                while let Some(t) = tmp {
                    if t == ci {
                        break;
                    }
                    let next = self.nodes[t].next;
                    self.append_child(emph, t);
                    tmp = next;
                }
                self.insert_after(oi, emph);
                // opener と closer の間の区切りを取り除く
                let mut d = self.delims[c].previous;
                while let Some(x) = d {
                    if x == o {
                        break;
                    }
                    let prev = self.delims[x].previous;
                    self.remove_delimiter(x);
                    d = prev;
                }
                if self.delims[o].numdelims == 0 {
                    self.unlink(oi);
                    self.remove_delimiter(o);
                }
                if self.delims[c].numdelims == 0 {
                    self.unlink(ci);
                    let next = self.delims[c].next;
                    self.remove_delimiter(c);
                    closer = next;
                }
            } else {
                closer = self.delims[c].next;
                openers_bottom[idx] = self.delims[old_closer].previous;
                if !self.delims[old_closer].can_open {
                    self.remove_delimiter(old_closer);
                }
            }
        }
        while let Some(d) = self.delimiters {
            if Some(d) == stack_bottom {
                break;
            }
            self.remove_delimiter(d);
        }
    }

    /// 配列の節を構文木の節にする。隣り合う文字列は結合する。
    fn convert(&self, parent: usize, map: &impl Fn(usize) -> usize) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        let mut cur = self.nodes[parent].first;
        while let Some(i) = cur {
            let n = &self.nodes[i];
            let range = map(n.start)..map(n.end);
            let kind = match &n.kind {
                IKind::Root => unreachable!(),
                IKind::Text(t) => {
                    if let Some(Node {
                        kind: NodeKind::Text(prev),
                        range: r,
                        ..
                    }) = out.last_mut()
                    {
                        prev.push_str(t);
                        r.end = range.end;
                        cur = n.next;
                        continue;
                    }
                    NodeKind::Text(t.clone())
                }
                IKind::SoftBreak => NodeKind::SoftBreak,
                IKind::HardBreak => NodeKind::HardBreak,
                IKind::Code(c) => NodeKind::Code(c.clone()),
                IKind::Emph => NodeKind::Emphasis,
                IKind::Strong => NodeKind::Strong,
                IKind::Link { dest, title } => NodeKind::Link {
                    destination: dest.clone(),
                    title: title.clone(),
                },
                IKind::Image { dest, title } => NodeKind::Image {
                    destination: dest.clone(),
                    title: title.clone(),
                },
                IKind::Html(h) => NodeKind::HtmlInline(h.clone()),
            };
            let mut node = Node::new(kind, range);
            node.children = self.convert(i, map);
            out.push(node);
            cur = n.next;
        }
        out
    }
}

fn uri_autolink(b: &[u8]) -> Option<usize> {
    if b.first() != Some(&b'<') || !b.get(1)?.is_ascii_alphabetic() {
        return None;
    }
    let mut i = 2;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'+' | b'.' | b'-')) {
        i += 1;
    }
    let scheme_len = i - 1;
    if !(2..=32).contains(&scheme_len) || b.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    while let Some(&c) = b.get(i) {
        match c {
            b'>' => return Some(i + 1),
            b'<' => return None,
            c if c <= b' ' => return None,
            _ => i += 1,
        }
    }
    None
}

fn email_autolink(b: &[u8]) -> Option<usize> {
    if b.first() != Some(&b'<') {
        return None;
    }
    let mut i = 1;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&b[i])) {
        i += 1;
    }
    if i == 1 || b.get(i) != Some(&b'@') {
        return None;
    }
    i += 1;
    loop {
        // ラベル: 英数字で始まり終わり、途中に `-`、63 文字以下
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-') {
            i += 1;
        }
        let label = &b[start..i];
        if label.is_empty()
            || label.len() > 63
            || label[0] == b'-'
            || label[label.len() - 1] == b'-'
        {
            return None;
        }
        match b.get(i)? {
            b'.' => i += 1,
            b'>' => return Some(i + 1),
            _ => return None,
        }
    }
}

/// 段落・見出しの内容を解析する。位置は原文の位置にする。
pub(crate) fn parse(content: &Content, refmap: &RefMap) -> Vec<Node> {
    let rest = content.rest();
    let trimmed_start = rest.trim_start_matches([' ', '\t', '\n', '\r']);
    let lead = rest.len() - trimmed_start.len();
    let subject = trimmed_start.trim_end_matches([' ', '\t', '\n', '\r']);
    let base = content.skip + lead;
    let mut p = Parser::new(subject, refmap);
    while p.parse_inline() {}
    p.process_emphasis(None);
    let map = |off: usize| content.to_source(base + off);
    p.convert(0, &map)
}

/// リンク参照定義を解析し、`refmap` に登録する（既に定義済みのラベルは変えない）。読んだ長さを返す。0 は定義でない。
pub(crate) fn parse_reference(s: &str, refmap: &mut RefMap) -> usize {
    let empty = RefMap::new();
    let mut p = Parser::new(s, &empty);
    let n = p.parse_link_label();
    if n == 0 {
        return 0;
    }
    let rawlabel = &s[1..n - 1];
    if p.peek() != Some(b':') {
        return 0;
    }
    p.pos += 1;
    p.spnl();
    let Some(dest) = p.parse_link_destination() else {
        return 0;
    };
    let before_title = p.pos;
    p.spnl();
    let mut title = None;
    if p.pos != before_title {
        title = p.parse_link_title();
    }
    if title.is_none() {
        p.pos = before_title;
    }
    let at_line_end = |p: &mut Parser| {
        let b = p.bytes();
        let mut i = p.pos;
        while matches!(b.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        match b.get(i) {
            None => {
                p.pos = i;
                true
            }
            Some(b'\n') => {
                p.pos = i + 1;
                true
            }
            _ => false,
        }
    };
    if !at_line_end(&mut p) {
        if title.is_none() {
            return 0;
        }
        title = None;
        p.pos = before_title;
        if !at_line_end(&mut p) {
            return 0;
        }
    }
    let label = scan::normalize_label(rawlabel);
    if label.is_empty() {
        return 0;
    }
    refmap
        .entry(label)
        .or_insert((dest, title.unwrap_or_default()));
    p.pos
}
