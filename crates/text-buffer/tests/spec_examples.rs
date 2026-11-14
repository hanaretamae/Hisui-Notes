//! `docs/spec/text-buffer.md` の個別の規則の検証（例による）。性質による検証は `properties.rs`。

// 逆転した範囲の拒否（Spec §3.1）を検証するため、空の範囲を意図的に作る
#![allow(clippy::reversed_empty_ranges)]

use text_buffer::{Bias, Edit, HistoryId, LineCol, TextBuffer, TextError, UndoGroup, map_offset};

const NEW: UndoGroup = UndoGroup::NewGroup;
const MERGE: UndoGroup = UndoGroup::MergeWithPrevious;

fn buf(s: &str) -> TextBuffer {
    TextBuffer::new(s)
}

// --- §2 原文の保持 ---

#[test]
fn content_is_not_normalized() {
    // NFD の「が」、CRLF、BOM、NEL をそのまま保持する
    let s = "\u{feff}か\u{3099}\r\nx\u{85}y";
    assert_eq!(buf(s).text().to_string(), s);
}

// --- §3.1 位置の検査 ---

#[test]
fn invalid_positions_return_errors() {
    let b = buf("aあ");
    let t = b.text();
    assert_eq!(
        t.byte_to_char(5),
        Err(TextError::OutOfBounds { offset: 5, len: 4 })
    );
    assert_eq!(
        t.byte_to_char(2),
        Err(TextError::NotCharBoundary { offset: 2 })
    );
    assert_eq!(
        t.slice(3..1),
        Err(TextError::InvalidRange { start: 3, end: 1 })
    );
    assert_eq!(
        t.char_to_byte(3),
        Err(TextError::CharIndexOutOfBounds { index: 3, len: 2 })
    );
}

// --- §3.2 行 ---

#[test]
fn only_lf_cr_crlf_break_lines() {
    let t = buf("a\nb\rc\r\nd\u{b}e\u{c}f\u{85}g\u{2028}h\u{2029}i").snapshot();
    assert_eq!(t.line_count(), 4);
    assert_eq!(
        t.slice(t.line_range(3).unwrap()).unwrap(),
        "d\u{b}e\u{c}f\u{85}g\u{2028}h\u{2029}i"
    );
}

#[test]
fn line_ranges_exclude_breaks() {
    let t = buf("ab\r\ncd\n\nef\r").snapshot();
    assert_eq!(t.line_count(), 5, "末尾が行区切りなら最後は空の行");
    assert_eq!(t.line_range(0), Ok(0..2));
    assert_eq!(t.line_range_with_break(0), Ok(0..4));
    assert_eq!(t.line_range(1), Ok(4..6));
    assert_eq!(t.line_range(2), Ok(7..7));
    assert_eq!(t.line_range(3), Ok(8..10));
    assert_eq!(t.line_range_with_break(3), Ok(8..11));
    assert_eq!(t.line_range(4), Ok(11..11));
    assert_eq!(
        t.line_range(5),
        Err(TextError::LineOutOfBounds {
            line: 5,
            line_count: 5
        })
    );
    assert_eq!(buf("").text().line_count(), 1, "空の内容は1行");
}

#[test]
fn position_between_cr_and_lf_belongs_to_preceding_line() {
    let t = buf("ab\r\ncd").snapshot();
    assert_eq!(t.byte_to_line(3), Ok(0));
    assert_eq!(t.byte_to_line_col(3), Ok(LineCol::new(0, 3)));
    assert_eq!(t.line_col_to_byte(LineCol::new(0, 3)), Ok(3));
    assert_eq!(t.byte_to_line(4), Ok(1));
}

// --- §3.3 位置変換 ---

#[test]
fn column_ranges_follow_spec() {
    let t = buf("ab\ncd\r\nef").snapshot();
    // LF の行: 0..=2（2 は LF の前）
    assert_eq!(t.line_col_to_byte(LineCol::new(0, 2)), Ok(2));
    assert_eq!(
        t.line_col_to_byte(LineCol::new(0, 3)),
        Err(TextError::ColumnOutOfBounds {
            line: 0,
            column: 3,
            max: 2
        })
    );
    // CRLF の行: 0..=3（3 は CR と LF の間）
    assert_eq!(t.line_col_to_byte(LineCol::new(1, 3)), Ok(6));
    assert!(t.line_col_to_byte(LineCol::new(1, 4)).is_err());
    // 最後の行: 0..=2
    assert_eq!(t.line_col_to_byte(LineCol::new(2, 2)), Ok(9));
    assert!(t.line_col_to_byte(LineCol::new(2, 3)).is_err());
}

#[test]
fn utf16_conversions_handle_surrogate_pairs() {
    // "a😀b": 😀 は UTF-8 で4バイト、UTF-16 で2単位
    let t = buf("a😀b\nx😀").snapshot();
    assert_eq!(t.byte_to_utf16(5), Ok(3));
    assert_eq!(t.utf16_to_byte(3), Ok(5));
    assert_eq!(
        t.utf16_to_byte(2),
        Err(TextError::NotUtf16Boundary { index: 2 })
    );
    assert_eq!(t.byte_to_line_utf16(12), Ok(LineCol::new(1, 3)));
    assert_eq!(t.line_utf16_to_byte(LineCol::new(1, 3)), Ok(12));
    assert_eq!(
        t.line_utf16_to_byte(LineCol::new(1, 2)),
        Err(TextError::NotUtf16Boundary { index: 7 })
    );
}

#[test]
fn line_col_rejects_non_char_boundary() {
    let t = buf("あい").snapshot();
    assert_eq!(
        t.line_col_to_byte(LineCol::new(0, 1)),
        Err(TextError::NotCharBoundary { offset: 1 })
    );
}

// --- §3.4 書記素クラスタ ---

#[test]
fn grapheme_boundaries_for_complex_clusters() {
    // e + 結合アキュート、ZWJ 連結、国旗、ヒンディー語の結合子音、CRLF
    let s = "e\u{301}👩‍💻🇯🇵क्षx\r\ny";
    let t = buf(s).snapshot();
    let mut got = vec![0];
    let mut o = 0;
    while let Some(n) = t.next_grapheme_boundary(o).unwrap() {
        got.push(n);
        o = n;
    }
    let mut expected: Vec<usize> =
        unicode_segmentation::UnicodeSegmentation::grapheme_indices(s, true)
            .map(|(i, _)| i)
            .collect();
    expected.push(s.len());
    assert_eq!(got, expected);
    assert_eq!(t.prev_grapheme_boundary(0), Ok(None));
    assert_eq!(t.next_grapheme_boundary(s.len()), Ok(None));
    // e の直後（結合文字の前）は書記素の境界ではないが、文字の境界ではある
    assert_eq!(t.is_grapheme_boundary(1), Ok(false));
    assert_eq!(t.next_char_boundary(1), Ok(Some(3)));
    assert_eq!(t.prev_char_boundary(3), Ok(Some(1)));
}

#[test]
fn grapheme_queries_check_offsets() {
    let t = buf("あ").snapshot();
    assert_eq!(
        t.is_grapheme_boundary(1),
        Err(TextError::NotCharBoundary { offset: 1 })
    );
    assert_eq!(
        t.next_grapheme_boundary(9),
        Err(TextError::OutOfBounds { offset: 9, len: 3 })
    );
}

// --- §5 編集 ---

#[test]
fn transaction_edits_are_sequential() {
    let mut b = buf("abc");
    // 2つ目の範囲は1つ目を適用した後の内容に対する位置
    let change = b
        .apply([Edit::insert(0, "XY"), Edit::delete(2..3)], NEW)
        .unwrap()
        .unwrap();
    assert_eq!(b.text().to_string(), "XYbc");
    assert_eq!(change.version, 1);
    assert_eq!(change.edits.len(), 2);
    assert_eq!(change.edits[1].deleted, "a");
}

#[test]
fn failed_transaction_changes_nothing() {
    let mut b = buf("abc");
    b.apply([Edit::insert(0, "x")], NEW).unwrap();
    let before = (b.text().to_string(), b.version(), b.history().len());
    let err = b
        .apply([Edit::insert(0, "y"), Edit::delete(1..99)], NEW)
        .unwrap_err();
    assert_eq!(err, TextError::OutOfBounds { offset: 99, len: 5 });
    assert_eq!(
        (b.text().to_string(), b.version(), b.history().len()),
        before
    );

    let mut b = buf("あ");
    assert_eq!(
        b.apply([Edit::insert(1, "z")], NEW),
        Err(TextError::NotCharBoundary { offset: 1 })
    );
    assert_eq!(
        b.apply([Edit::delete(3..0)], NEW),
        Err(TextError::InvalidRange { start: 3, end: 0 })
    );
    assert_eq!((b.text().to_string(), b.version()), ("あ".to_string(), 0));
}

#[test]
fn noop_transactions_are_not_applied() {
    let mut b = buf("abc");
    assert_eq!(b.apply([], NEW), Ok(None));
    assert_eq!(b.apply([Edit::insert(1, "")], NEW), Ok(None));
    assert_eq!(b.version(), 0);
    assert!(b.history().is_empty());
}

#[test]
fn snapshots_are_immutable() {
    let mut b = buf("abc");
    let snap = b.snapshot();
    b.apply([Edit::insert(3, "d")], NEW).unwrap();
    assert_eq!(snap.to_string(), "abc");
    assert_eq!(snap.version(), 0);
    assert_eq!(b.text().version(), 1);
    // 別スレッドへ渡せる
    let handle = std::thread::spawn(move || snap.len_bytes());
    assert_eq!(handle.join().unwrap(), 3);
}

#[test]
fn map_offset_keeps_cursor_after_insertion() {
    let mut b = buf("hello");
    let change = b.apply([Edit::insert(5, " world")], NEW).unwrap().unwrap();
    assert_eq!(map_offset(5, Bias::After, &change), 11);
    assert_eq!(map_offset(5, Bias::Before, &change), 5);
}

// --- §6 編集履歴 ---

#[test]
fn merge_groups_transactions_into_one_undo_step() {
    let mut b = buf("");
    b.apply([Edit::insert(0, "a")], NEW).unwrap();
    b.apply([Edit::insert(1, "b")], MERGE).unwrap();
    b.apply([Edit::insert(2, "c")], MERGE).unwrap();
    assert_eq!(b.history().len(), 2);
    let change = b.undo().unwrap();
    assert_eq!(b.text().to_string(), "");
    assert_eq!(change.version, 4, "Undo も版番号を1増やす");
    assert!(b.undo().is_none(), "根では Undo できない");
}

#[test]
fn merge_after_undo_starts_new_group() {
    let mut b = buf("");
    b.apply([Edit::insert(0, "a")], NEW).unwrap();
    b.apply([Edit::insert(1, "b")], NEW).unwrap();
    b.undo().unwrap();
    b.redo().unwrap();
    b.apply([Edit::insert(2, "c")], MERGE).unwrap();
    assert_eq!(b.history().len(), 4, "Redo の直後は追加せず新しい項目");
    // 根の直後の MERGE も新しい項目
    let mut b = buf("");
    b.apply([Edit::insert(0, "a")], MERGE).unwrap();
    assert_eq!(b.history().len(), 2);
}

#[test]
fn undo_tree_keeps_branches() {
    let mut b = buf("");
    b.apply([Edit::insert(0, "a")], NEW).unwrap(); // 1: "a"
    b.apply([Edit::insert(1, "b")], NEW).unwrap(); // 2: "ab"
    b.undo().unwrap(); // 1
    b.apply([Edit::insert(1, "c")], NEW).unwrap(); // 3: "ac"（分岐）
    let h = b.history();
    let n1 = h.node(HistoryId::ROOT).unwrap().children[0];
    assert_eq!(
        h.node(n1).unwrap().children.len(),
        2,
        "Undo 後の編集で分岐を破棄しない"
    );
    let (n2, n3) = (
        h.node(n1).unwrap().children[0],
        h.node(n1).unwrap().children[1],
    );

    b.undo().unwrap();
    assert_eq!(b.history().node(n1).unwrap().last_child, Some(n3));
    b.redo().unwrap();
    assert_eq!(b.text().to_string(), "ac", "Redo は最後に通った子へ");

    b.undo().unwrap();
    b.redo_to(n2).unwrap();
    assert_eq!(b.text().to_string(), "ab");
    assert_eq!(b.history().node(n1).unwrap().last_child, Some(n2));

    assert_eq!(b.redo_to(n3).unwrap_err(), TextError::NotAChild { id: n3 });
}

#[test]
fn goto_moves_across_branches_in_one_change() {
    let mut b = buf("x");
    b.apply([Edit::insert(1, "a")], NEW).unwrap(); // "xa"
    let a = b.history().current();
    b.apply([Edit::insert(2, "b")], NEW).unwrap(); // "xab"
    let ab = b.history().current();
    b.undo().unwrap();
    b.undo().unwrap();
    b.apply([Edit::delete(0..1)], NEW).unwrap(); // ""（根からの別の分岐）
    let other = b.history().current();
    let v = b.version();

    let before = b.text().to_string();
    let change = b.goto(ab).unwrap().unwrap();
    assert_eq!(b.text().to_string(), "xab");
    assert_eq!(change.version, v + 1, "移動は1つのトランザクション");
    // Change の逆を適用すると移動前に戻る
    let mut s = b.text().to_string();
    for e in change.inverse_edits() {
        s.replace_range(e.start..e.end(), &e.inserted);
    }
    assert_eq!(s, before);
    // 経路上の「最後に通った子」が更新される
    assert_eq!(
        b.history().node(HistoryId::ROOT).unwrap().last_child,
        Some(a)
    );

    assert_eq!(b.goto(ab), Ok(None));
    b.goto(other).unwrap();
    assert_eq!(b.text().to_string(), "");
    assert_eq!(
        b.goto(HistoryId::ROOT)
            .unwrap()
            .map(|_| b.text().to_string()),
        Some("x".to_string())
    );
}

#[test]
fn unknown_history_ids_are_rejected() {
    let mut b = buf("a");
    let bogus = {
        let mut other = buf("");
        for i in 0..5 {
            other.apply([Edit::insert(i, "z")], NEW).unwrap();
        }
        other.history().current()
    };
    assert_eq!(
        b.goto(bogus),
        Err(TextError::UnknownHistoryId { id: bogus })
    );
    assert_eq!(
        b.redo_to(bogus).unwrap_err(),
        TextError::UnknownHistoryId { id: bogus }
    );
    assert_eq!(b.version(), 0);
}
