//! `docs/spec/text-buffer.md` §8 の Property-based Testing。期待値は `String` と標準ライブラリ・
//! `unicode-segmentation` で独立に求める。ropey の `small_chunks`（dev-dependencies）により、
//! 短い内容でもチャンク境界をまたぐ書記素・CRLF が生じる。

use std::collections::HashMap;

use proptest::prelude::*;
use proptest::sample::Index;
use text_buffer::{
    Bias, Change, Edit, HistoryId, LineCol, Snapshot, TextBuffer, TextError, UndoGroup, map_offset,
};
use unicode_segmentation::UnicodeSegmentation;

/// 境界の扱いが難しい断片を多く含む文字列の部品。
fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => prop::sample::select(vec![
            "a", "Z", " ", "\n", "\r", "\r\n", "あ", "漢", "😀", "👩‍💻", "🇯🇵", "\u{1F1EF}", "e\u{301}", "\u{301}",
            "क्ष", "\u{94d}", "\u{200d}", "\u{85}", "\u{2028}", "ﷺ", "\u{feff}",
        ])
        .prop_map(String::from),
        1 => any::<char>().prop_map(String::from),
    ]
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(piece(), 0..80).prop_map(|v| v.concat())
}

/// 文字境界の一覧（末尾を含む）。
fn char_boundaries(s: &str) -> Vec<usize> {
    s.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(s.len()))
        .collect()
}

/// 抽象的な編集。適用時の内容に合わせて文字境界へ写す。
#[derive(Debug, Clone)]
struct AbstractEdit(Index, Index, String);

fn abstract_edit() -> impl Strategy<Value = AbstractEdit> {
    (
        any::<Index>(),
        any::<Index>(),
        prop::collection::vec(piece(), 0..4).prop_map(|v| v.concat()),
    )
        .prop_map(|(a, b, s)| AbstractEdit(a, b, s))
}

impl AbstractEdit {
    fn concrete(&self, s: &str) -> Edit {
        let bs = char_boundaries(s);
        let (a, b) = (bs[self.0.index(bs.len())], bs[self.1.index(bs.len())]);
        Edit::new(a.min(b)..a.max(b), self.2.clone())
    }
}

/// モデル（`String`）に範囲置換を適用する。
fn apply_model(s: &mut String, start: usize, end: usize, insert: &str) {
    s.replace_range(start..end, insert);
}

/// `Change` の性質（Spec §5.4）と `map_offset` の性質（§5.5）を検査する。
fn check_change(before: &str, after: &str, change: &Change) {
    let mut s = before.to_string();
    for e in &change.edits {
        assert_eq!(&s[e.start..e.end()], e.deleted);
        apply_model(&mut s, e.start, e.end(), &e.inserted);
    }
    assert_eq!(s, after, "edits を順に適用すると適用後の内容になる");
    for e in change.inverse_edits() {
        apply_model(&mut s, e.start, e.end(), &e.inserted);
    }
    assert_eq!(s, before, "逆を適用すると適用前の内容に戻る");
    for o in char_boundaries(before) {
        for bias in [Bias::Before, Bias::After] {
            let m = map_offset(o, bias, change);
            assert!(
                after.is_char_boundary(m),
                "map_offset({o}, {bias:?}) = {m} は文字境界"
            );
        }
    }
}

#[derive(Debug, Clone)]
enum Op {
    Edit(Vec<AbstractEdit>, bool),
    Undo,
    Redo,
    RedoTo(Index),
    Goto(Index),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (prop::collection::vec(abstract_edit(), 0..3), any::<bool>()).prop_map(|(e, m)| Op::Edit(e, m)),
        3 => Just(Op::Undo),
        2 => Just(Op::Redo),
        1 => any::<Index>().prop_map(Op::RedoTo),
        2 => any::<Index>().prop_map(Op::Goto),
    ]
}

/// 行の先頭のバイト位置（モデル。LF・CR・CRLF のみを行区切りとする。Spec §3.2）。
fn line_starts(s: &str) -> Vec<usize> {
    let b = s.as_bytes();
    let mut starts = vec![0];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' if b.get(i + 1) == Some(&b'\n') => {
                starts.push(i + 2);
                i += 2;
                continue;
            }
            b'\r' | b'\n' => starts.push(i + 1),
            _ => {}
        }
        i += 1;
    }
    starts
}

fn check_reads(t: &Snapshot, s: &str) {
    assert_eq!(t.to_string(), s);
    assert_eq!(t.len_bytes(), s.len());
    assert_eq!(t.len_chars(), s.chars().count());
    assert_eq!(t.len_utf16(), s.encode_utf16().count());
    assert_eq!(t.line_count(), line_starts(s).len());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// 任意の操作列で、内容がモデルと一致し、Undo tree の不変条件（§6.1）と Change の性質が成り立つ。
    #[test]
    fn edits_and_history_match_model(initial in text(), ops in prop::collection::vec(op(), 0..40)) {
        let mut b = TextBuffer::new(&initial);
        let mut model = initial.clone();
        // 各項目の内容（§6.1: 根から項目までの経路の適用結果）
        let mut node_content: HashMap<HistoryId, String> = HashMap::from([(HistoryId::ROOT, initial.clone())]);
        let mut ids = vec![HistoryId::ROOT];
        for op in ops {
            let before = model.clone();
            let version = b.version();
            let result: Option<Change> = match op {
                Op::Edit(edits, merge) => {
                    let group = if merge { UndoGroup::MergeWithPrevious } else { UndoGroup::NewGroup };
                    // 各範囲は前の編集の適用後の内容に対する位置（§5.2）
                    let mut concrete = Vec::new();
                    for e in &edits {
                        let c = e.concrete(&model);
                        apply_model(&mut model, c.range.start, c.range.end, &c.insert);
                        concrete.push(c);
                    }
                    let r = b.apply(concrete, group).unwrap();
                    if r.is_some() {
                        let cur = b.history().current();
                        if !ids.contains(&cur) { ids.push(cur); }
                        node_content.insert(cur, model.clone());
                    }
                    r
                }
                Op::Undo => b.undo(),
                Op::Redo => b.redo(),
                Op::RedoTo(i) => {
                    let children = b.history().node(b.history().current()).unwrap().children.to_vec();
                    if children.is_empty() { None } else { Some(b.redo_to(children[i.index(children.len())]).unwrap()) }
                }
                Op::Goto(i) => b.goto(ids[i.index(ids.len())]).unwrap(),
            };
            let cur = b.history().current();
            model = node_content[&cur].clone();
            prop_assert_eq!(b.text().to_string(), model.clone(), "内容は現在の項目の内容に等しい");
            match &result {
                Some(change) => {
                    prop_assert_eq!(change.version, version + 1);
                    prop_assert_eq!(b.version(), version + 1);
                    check_change(&before, &model, change);
                }
                None => {
                    prop_assert_eq!(b.version(), version, "内容を変えない操作は版を変えない");
                    prop_assert_eq!(&before, &model);
                }
            }
        }
        check_reads(b.text(), &model);
    }

    /// Undo をすべて行うと最初の内容に、Redo をすべて行うと最後の内容に戻る。
    #[test]
    fn undo_all_and_redo_all(initial in text(), edits in prop::collection::vec(abstract_edit(), 1..30)) {
        let mut b = TextBuffer::new(&initial);
        for e in &edits {
            let c = e.concrete(&b.text().to_string());
            b.apply([c], UndoGroup::NewGroup).unwrap();
        }
        let last = b.text().to_string();
        while b.undo().is_some() {}
        prop_assert_eq!(b.text().to_string(), initial);
        while b.redo().is_some() {}
        prop_assert_eq!(b.text().to_string(), last);
    }

    /// 位置変換が往復で一致し、標準ライブラリで数えた値と一致する（§3.3）。
    #[test]
    fn position_conversions_match_std(s in text()) {
        let t = TextBuffer::new(&s).snapshot();
        let starts = line_starts(&s);
        for o in 0..=s.len() + 1 {
            if o > s.len() {
                prop_assert_eq!(t.byte_to_char(o), Err(TextError::OutOfBounds { offset: o, len: s.len() }));
                continue;
            }
            if !s.is_char_boundary(o) {
                prop_assert_eq!(t.byte_to_char(o), Err(TextError::NotCharBoundary { offset: o }));
                prop_assert_eq!(t.byte_to_line(o), Err(TextError::NotCharBoundary { offset: o }));
                continue;
            }
            let c = s[..o].chars().count();
            let u = s[..o].encode_utf16().count();
            prop_assert_eq!(t.byte_to_char(o), Ok(c));
            prop_assert_eq!(t.char_to_byte(c), Ok(o));
            prop_assert_eq!(t.byte_to_utf16(o), Ok(u));
            prop_assert_eq!(t.utf16_to_byte(u), Ok(o));

            let line = starts.partition_point(|&st| st <= o) - 1;
            let lc = t.byte_to_line_col(o).unwrap();
            prop_assert_eq!(lc, LineCol::new(line, o - starts[line]));
            prop_assert_eq!(t.line_col_to_byte(lc), Ok(o));
            let lu = t.byte_to_line_utf16(o).unwrap();
            prop_assert_eq!(lu, LineCol::new(line, s[starts[line]..o].encode_utf16().count()));
            prop_assert_eq!(t.line_utf16_to_byte(lu), Ok(o));
        }
        for (line, &st) in starts.iter().enumerate() {
            let full_end = starts.get(line + 1).copied().unwrap_or(s.len());
            prop_assert_eq!(t.line_range_with_break(line), Ok(st..full_end));
            let content = t.slice(t.line_range(line).unwrap()).unwrap();
            prop_assert!(!content.contains(['\n', '\r']), "行の範囲は行区切りを含まない");
        }
    }

    /// 書記素境界が、文字列全体での `unicode-segmentation` の結果と一致する（§3.4）。
    #[test]
    fn grapheme_boundaries_match_unicode_segmentation(s in text()) {
        let t = TextBuffer::new(&s).snapshot();
        let mut bounds: Vec<usize> = s.grapheme_indices(true).map(|(i, _)| i).collect();
        bounds.push(s.len());
        for o in char_boundaries(&s) {
            prop_assert_eq!(t.is_grapheme_boundary(o), Ok(bounds.contains(&o)), "is_grapheme_boundary({})", o);
            let next = bounds.iter().copied().find(|&b| b > o);
            let prev = bounds.iter().copied().rev().find(|&b| b < o);
            prop_assert_eq!(t.next_grapheme_boundary(o), Ok(next), "next_grapheme_boundary({})", o);
            prop_assert_eq!(t.prev_grapheme_boundary(o), Ok(prev), "prev_grapheme_boundary({})", o);
            let next_c = s[o..].chars().next().map(|ch| o + ch.len_utf8());
            let prev_c = s[..o].chars().next_back().map(|ch| o - ch.len_utf8());
            prop_assert_eq!(t.next_char_boundary(o), Ok(next_c));
            prop_assert_eq!(t.prev_char_boundary(o), Ok(prev_c));
        }
    }
}
