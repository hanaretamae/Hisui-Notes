//! 字句の走査と文字の分類（CommonMark spec 0.31.2）。

use std::collections::HashMap;
use std::sync::OnceLock;

use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

/// バックスラッシュでエスケープできる ASCII の句読点。
pub fn is_escapable(b: u8) -> bool {
    b.is_ascii_punctuation()
}

pub fn is_space_or_tab(b: Option<u8>) -> bool {
    matches!(b, Some(b' ' | b'\t'))
}

/// Unicode の空白（一般カテゴリー Zs と、タブ・改行・改ページ・復帰）。
pub fn is_unicode_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0c' | '\r')
        || c.general_category() == unicode_properties::GeneralCategory::SpaceSeparator
}

/// Unicode の句読点（一般カテゴリー P と S。spec 0.31.2）。
pub fn is_unicode_punctuation(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_punctuation();
    }
    matches!(
        c.general_category_group(),
        GeneralCategoryGroup::Punctuation | GeneralCategoryGroup::Symbol
    )
}

/// HTML の空白（spec の whitespace: 空白、タブ、改行、垂直タブ、改ページ、復帰）。
fn is_html_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && is_html_ws(s[i]) {
        i += 1;
    }
    i
}

fn starts_with_ci(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn find(s: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    s.get(from..)?
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| p + from)
}

fn tag_name_end(s: &[u8], i: usize) -> Option<usize> {
    if !s.get(i)?.is_ascii_alphabetic() {
        return None;
    }
    let mut j = i + 1;
    while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'-') {
        j += 1;
    }
    Some(j)
}

/// 開始タグ（`<` から）。終わりの次の位置。
fn open_tag(s: &[u8]) -> Option<usize> {
    let mut i = tag_name_end(s, 1)?;
    loop {
        let ws_end = skip_ws(s, i);
        // 属性名の前には空白が必要
        if ws_end > i
            && let Some(&c) = s.get(ws_end)
            && (c.is_ascii_alphabetic() || c == b'_' || c == b':')
        {
            let mut j = ws_end + 1;
            while j < s.len()
                && (s[j].is_ascii_alphanumeric() || matches!(s[j], b'_' | b'.' | b':' | b'-'))
            {
                j += 1;
            }
            // 属性値
            let k = skip_ws(s, j);
            if s.get(k) == Some(&b'=') {
                let v = skip_ws(s, k + 1);
                match s.get(v)? {
                    q @ (b'"' | b'\'') => {
                        let close = s[v + 1..].iter().position(|c| c == q)? + v + 1;
                        i = close + 1;
                    }
                    _ => {
                        let mut e = v;
                        while e < s.len()
                            && !is_html_ws(s[e])
                            && !matches!(s[e], b'"' | b'\'' | b'=' | b'<' | b'>' | b'`')
                        {
                            e += 1;
                        }
                        if e == v {
                            return None;
                        }
                        i = e;
                    }
                }
            } else {
                i = j;
            }
            continue;
        }
        let j = ws_end;
        return match s.get(j)? {
            b'>' => Some(j + 1),
            b'/' if s.get(j + 1) == Some(&b'>') => Some(j + 2),
            _ => None,
        };
    }
}

/// 終了タグ（`</` から）。
fn close_tag(s: &[u8]) -> Option<usize> {
    if s.get(1) != Some(&b'/') {
        return None;
    }
    let j = skip_ws(s, tag_name_end(s, 2)?);
    (s.get(j) == Some(&b'>')).then_some(j + 1)
}

/// 開始タグまたは終了タグ。
pub fn open_or_close_tag(s: &[u8]) -> Option<usize> {
    if s.first() != Some(&b'<') {
        return None;
    }
    open_tag(s).or_else(|| close_tag(s))
}

/// インラインの HTML（`<` から）。終わりの次の位置。
pub fn html_tag(s: &[u8]) -> Option<usize> {
    if s.first() != Some(&b'<') {
        return None;
    }
    if let Some(e) = open_or_close_tag(s) {
        return Some(e);
    }
    if s.starts_with(b"<!--") {
        if s[4..].starts_with(b">") {
            return Some(5);
        }
        if s[4..].starts_with(b"->") {
            return Some(6);
        }
        return find(s, 4, b"-->").map(|p| p + 3);
    }
    if s.starts_with(b"<?") {
        return find(s, 2, b"?>").map(|p| p + 2);
    }
    if s.starts_with(b"<![CDATA[") {
        return find(s, 9, b"]]>").map(|p| p + 3);
    }
    if s.starts_with(b"<!") && s.get(2).is_some_and(|c| c.is_ascii_alphabetic()) {
        return find(s, 2, b">").map(|p| p + 1);
    }
    None
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

const RAW_TAGS: &[&str] = &["pre", "script", "style", "textarea"];

/// HTML ブロックの開始条件（spec 4.6）。種類（1〜7）を返す。`s` は行の非空白の位置から。
pub fn html_block_start(s: &[u8], can_interrupt_paragraph: bool) -> Option<u8> {
    if s.first() != Some(&b'<') {
        return None;
    }
    for t in RAW_TAGS {
        let n = 1 + t.len();
        if starts_with_ci(&s[1..], t.as_bytes())
            && matches!(s.get(n), None | Some(b' ' | b'\t' | b'>' | b'\n' | b'\r'))
        {
            return Some(1);
        }
    }
    if s.starts_with(b"<!--") {
        return Some(2);
    }
    if s.starts_with(b"<?") {
        return Some(3);
    }
    if s.starts_with(b"<!") && s.get(2).is_some_and(|c| c.is_ascii_alphabetic()) {
        return Some(4);
    }
    if s.starts_with(b"<![CDATA[") {
        return Some(5);
    }
    let name_start = if s.get(1) == Some(&b'/') { 2 } else { 1 };
    if let Some(end) = tag_name_end(s, name_start) {
        let name = &s[name_start..end];
        if BLOCK_TAGS
            .iter()
            .any(|t| name.eq_ignore_ascii_case(t.as_bytes()))
        {
            let ok = match s.get(end) {
                None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'>') => true,
                Some(b'/') => s.get(end + 1) == Some(&b'>'),
                _ => false,
            };
            if ok {
                return Some(6);
            }
        }
    }
    if !can_interrupt_paragraph {
        return None;
    }
    // 7: 完全な開始タグ・終了タグ（1 の名前を除く）の後に空白のみ
    let end = open_or_close_tag(s)?;
    let name = &s[name_start..tag_name_end(s, name_start)?];
    if RAW_TAGS
        .iter()
        .any(|t| name.eq_ignore_ascii_case(t.as_bytes()))
    {
        return None;
    }
    s[end..]
        .iter()
        .all(|&c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
        .then_some(7)
}

/// HTML ブロックの終了条件（種類 1〜5）。
pub fn html_block_end(kind: u8, line: &[u8]) -> bool {
    match kind {
        1 => {
            let lower = line.to_ascii_lowercase();
            [
                b"</pre>".as_slice(),
                b"</script>",
                b"</style>",
                b"</textarea>",
            ]
            .iter()
            .any(|p| find(&lower, 0, p).is_some())
        }
        2 => find(line, 0, b"-->").is_some(),
        3 => find(line, 0, b"?>").is_some(),
        4 => line.contains(&b'>'),
        5 => find(line, 0, b"]]>").is_some(),
        _ => false,
    }
}

fn entity_table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        entities::ENTITIES
            .iter()
            .filter(|e| e.entity.ends_with(';'))
            .map(|e| (&e.entity[1..e.entity.len() - 1], e.characters))
            .collect()
    })
}

/// 文字参照（`&` から）。長さと、表す文字列。
pub fn entity(s: &str) -> Option<(usize, String)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'&') {
        return None;
    }
    if b.get(1) == Some(&b'#') {
        let (start, radix, max) = if matches!(b.get(2), Some(b'x' | b'X')) {
            (3, 16, 6)
        } else {
            (2, 10, 7)
        };
        let digits = b[start..]
            .iter()
            .take_while(|c| {
                if radix == 16 {
                    c.is_ascii_hexdigit()
                } else {
                    c.is_ascii_digit()
                }
            })
            .count();
        if digits == 0 || digits > max || b.get(start + digits) != Some(&b';') {
            return None;
        }
        let n = u32::from_str_radix(&s[start..start + digits], radix).ok()?;
        let c = if n == 0 {
            '\u{FFFD}'
        } else {
            char::from_u32(n).unwrap_or('\u{FFFD}')
        };
        return Some((start + digits + 1, c.to_string()));
    }
    let len = b[1..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    if len == 0 || b.get(1 + len) != Some(&b';') {
        return None;
    }
    entity_table()
        .get(&s[1..1 + len])
        .map(|v| (len + 2, (*v).to_string()))
}

/// バックスラッシュのエスケープと文字参照を解決する。
pub fn unescape(s: &str) -> String {
    if !s.contains(['\\', '&']) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        match b[i] {
            b'\\' if b.get(i + 1).is_some_and(|&c| is_escapable(c)) => {
                out.push(b[i + 1] as char);
                i += 2;
            }
            b'&' => match entity(&s[i..]) {
                Some((n, v)) => {
                    out.push_str(&v);
                    i += n;
                }
                None => {
                    out.push('&');
                    i += 1;
                }
            },
            _ => {
                let w = s[i..].chars().next().map_or(1, char::len_utf8);
                let next = s[i + w..].find(['\\', '&']).map_or(s.len(), |p| p + i + w);
                out.push_str(&s[i..next]);
                i = next;
            }
        }
    }
    out
}

/// リンクのラベルの正規化（大文字小文字の同一視と空白の畳み込み）。
pub fn normalize_label(label: &str) -> String {
    let collapsed = label
        .split(is_unicode_whitespace)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    // Unicode の大文字小文字の畳み込みの近似（ẞ と SS を同一視するため、小文字・大文字・小文字の順に変換する）
    collapsed.to_lowercase().to_uppercase().to_lowercase()
}

/// 行が空白（空白・タブ）だけか。
pub fn is_blank(s: &str) -> bool {
    s.bytes().all(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_tags() {
        assert_eq!(html_tag(b"<a href=\"x\">rest"), Some(12));
        assert_eq!(html_tag(b"<a/>"), Some(4));
        assert_eq!(html_tag(b"<a  b c=d e='f'\ng>"), Some(18));
        assert_eq!(html_tag(b"</a >"), Some(5));
        assert_eq!(html_tag(b"<!-- x -->"), Some(10));
        assert_eq!(html_tag(b"<!-->"), Some(5));
        assert_eq!(html_tag(b"<a b=>"), None);
        assert_eq!(html_tag(b"<a\"b>"), None);
        assert_eq!(html_tag(b"<33>"), None);
    }

    #[test]
    fn entities_and_unescape() {
        assert_eq!(entity("&amp;x"), Some((5, "&".into())));
        assert_eq!(entity("&#35;"), Some((5, "#".into())));
        assert_eq!(entity("&#X22;"), Some((6, "\"".into())));
        assert_eq!(entity("&#0;"), Some((4, "\u{FFFD}".into())));
        assert_eq!(entity("&nosuch;"), None);
        assert_eq!(unescape(r"\*a&lt;\b"), r"*a<\b");
        assert_eq!(
            unescape("&😀\\é"),
            "&😀\\é",
            "文字参照でない `&` の直後の多バイト文字"
        );
    }

    #[test]
    fn labels_fold_case() {
        assert_eq!(normalize_label("ẞ"), normalize_label("SS"));
        assert_eq!(normalize_label(" Foo \n bar "), "foo bar");
    }

    #[test]
    fn html_block_kinds() {
        assert_eq!(html_block_start(b"<div>", false), Some(6));
        assert_eq!(html_block_start(b"<pre>", false), Some(1));
        assert_eq!(html_block_start(b"<a href=\"x\">  ", true), Some(7));
        assert_eq!(html_block_start(b"<a href=\"x\">  ", false), None);
        assert_eq!(html_block_start(b"<a> x", true), None);
    }
}
