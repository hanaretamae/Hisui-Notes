//! `docs/spec/document-session.md` の個別の規則の検証（例による）。性質による検証は `session_properties.rs`。

use document::{DocumentError, DocumentSession, LineEnding, Movement, Selection, TextError};

fn session(text: &str) -> (DocumentSession, document::ViewId) {
    let mut s = DocumentSession::new(text);
    let v = s.open_view();
    (s, v)
}

fn content(s: &DocumentSession) -> String {
    s.text().to_string()
}

fn cursor_at(s: &mut DocumentSession, v: document::ViewId, offset: usize) {
    s.set_selections(v, vec![Selection::cursor(offset)], 0)
        .unwrap();
}

fn cursors(s: &DocumentSession, v: document::ViewId) -> Vec<usize> {
    s.selections(v).unwrap().iter().map(|x| x.head).collect()
}

// --- §2 ビュー ---

#[test]
fn views_open_at_start_and_unknown_views_are_rejected() {
    let (mut s, v) = session("abc");
    assert_eq!(s.selections(v).unwrap(), &[Selection::cursor(0)]);
    s.close_view(v).unwrap();
    assert_eq!(s.insert_text(v, "x"), Err(DocumentError::UnknownView(v)));
    assert_eq!(content(&s), "abc");
}

// --- §3 選択範囲 ---

#[test]
fn set_selections_validates_without_side_effects() {
    let (mut s, v) = session("あい");
    assert_eq!(
        s.set_selections(v, vec![], 0),
        Err(DocumentError::EmptySelections)
    );
    assert_eq!(
        s.set_selections(v, vec![Selection::cursor(0)], 1),
        Err(DocumentError::PrimaryOutOfRange { primary: 1, len: 1 })
    );
    assert_eq!(
        s.set_selections(v, vec![Selection::cursor(1)], 0),
        Err(DocumentError::Text(TextError::NotCharBoundary {
            offset: 1
        }))
    );
    assert_eq!(s.selections(v).unwrap(), &[Selection::cursor(0)]);
}

#[test]
fn set_selections_sorts_and_merges() {
    let (mut s, v) = session("abcdefgh");
    s.set_selections(
        v,
        vec![
            Selection::new(6, 4),
            Selection::cursor(1),
            Selection::new(5, 8),
            Selection::cursor(1),
        ],
        2,
    )
    .unwrap();
    assert_eq!(
        s.selections(v).unwrap(),
        &[Selection::cursor(1), Selection::new(4, 8)]
    );
    assert_eq!(s.primary_index(v), Ok(1));
}

// --- §5.1・§5.2 入力 ---

#[test]
fn insert_replaces_each_selection_and_places_cursors_after() {
    let (mut s, v) = session("ab cd");
    s.set_selections(v, vec![Selection::cursor(0), Selection::new(3, 5)], 0)
        .unwrap();
    s.insert_text(v, "XY").unwrap();
    assert_eq!(content(&s), "XYab XY");
    assert_eq!(cursors(&s, v), vec![2, 7]);
}

#[test]
fn inserted_text_line_endings_are_preserved_and_newline_follows_document() {
    let (mut s, v) = session("a\r\nb");
    assert_eq!(s.line_ending(), LineEnding::CrLf);
    cursor_at(&mut s, v, 4);
    s.insert_text(v, "\nc").unwrap();
    s.insert_newline(v).unwrap();
    assert_eq!(content(&s), "a\r\nb\nc\r\n");
    let (mut s, v) = session("no breaks");
    s.insert_newline(v).unwrap();
    assert_eq!(content(&s), "\nno breaks", "行区切りが無い文書は LF");
}

// --- §5.3 削除 ---

fn backspace_at_end(text: &str) -> String {
    let (mut s, v) = session(text);
    cursor_at(&mut s, v, text.len());
    s.delete_backward(v).unwrap();
    content(&s)
}

#[test]
fn backspace_deletes_code_points_except_special_clusters() {
    assert_eq!(backspace_at_end("xe\u{301}"), "xe", "結合アキュートだけ");
    assert_eq!(backspace_at_end("क्ष"), "क्", "結合子音の最後の子音だけ");
    assert_eq!(
        backspace_at_end("\u{1112}\u{1161}"),
        "\u{1112}",
        "ハングルの字母"
    );
    assert_eq!(backspace_at_end("x👩‍💻"), "x", "ZWJ 連結は全体");
    assert_eq!(backspace_at_end("x🇯🇵"), "x", "国旗は全体");
    assert_eq!(
        backspace_at_end("x❤\u{fe0f}"),
        "x",
        "異体字セレクターを含む場合は全体"
    );
    assert_eq!(
        backspace_at_end("x👍🏽"),
        "x",
        "肌の色の修飾子を含む場合は全体"
    );
    assert_eq!(
        backspace_at_end("x🏴\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}"),
        "x",
        "タグ文字"
    );
    assert_eq!(backspace_at_end("a\r\n"), "a", "CRLF は全体");
    assert_eq!(backspace_at_end("a\n"), "a");
}

#[test]
fn backspace_inside_cluster_deletes_previous_code_point() {
    // カーソルを e と結合アキュートの間に置く（他のビューの編集などで起こりうる。§3）
    let (mut s, v) = session("e\u{301}x");
    cursor_at(&mut s, v, 1);
    s.delete_backward(v).unwrap();
    assert_eq!(content(&s), "\u{301}x");
}

#[test]
fn delete_forward_removes_grapheme_and_stops_at_end() {
    let (mut s, v) = session("🇯🇵e\u{301}");
    s.delete_forward(v).unwrap();
    assert_eq!(content(&s), "e\u{301}");
    s.delete_forward(v).unwrap();
    assert_eq!(content(&s), "");
    assert_eq!(s.delete_forward(v), Ok(vec![]));
    assert_eq!(s.delete_backward(v), Ok(vec![]), "先頭では何もしない");
}

#[test]
fn deleting_selection_and_multiple_cursors() {
    let (mut s, v) = session("abcdef");
    s.set_selections(
        v,
        vec![
            Selection::cursor(1),
            Selection::cursor(2),
            Selection::new(5, 3),
        ],
        0,
    )
    .unwrap();
    s.delete_backward(v).unwrap();
    assert_eq!(content(&s), "cf");
    assert_eq!(
        cursors(&s, v),
        vec![0, 1],
        "削除後に重なったカーソル（1と2）はまとまる"
    );

    let (mut s, v) = session("abc");
    assert_eq!(s.delete_selection(v), Ok(vec![]), "空の選択では何もしない");
    s.set_selections(v, vec![Selection::new(0, 2)], 0).unwrap();
    s.insert_text(v, "").unwrap();
    assert_eq!(content(&s), "c");
}

// --- §5.4 移動 ---

#[test]
fn cursor_movement_follows_graphemes_and_lines() {
    let (mut s, v) = session("🇯🇵a\r\nbc");
    s.move_cursor(v, Movement::Right, false).unwrap();
    assert_eq!(cursors(&s, v), vec![8]);
    s.move_cursor(v, Movement::Right, true).unwrap();
    assert_eq!(s.selections(v).unwrap(), &[Selection::new(8, 9)]);
    s.move_cursor(v, Movement::Left, false).unwrap();
    assert_eq!(
        cursors(&s, v),
        vec![8],
        "選択範囲がある Left は先頭へ縮める"
    );
    s.move_cursor(v, Movement::Right, false).unwrap();
    s.move_cursor(v, Movement::Right, false).unwrap();
    assert_eq!(cursors(&s, v), vec![11], "CRLF は1つの書記素");
    s.move_cursor(v, Movement::LineEnd, false).unwrap();
    assert_eq!(cursors(&s, v), vec![13]);
    s.move_cursor(v, Movement::LineStart, true).unwrap();
    assert_eq!(s.selections(v).unwrap(), &[Selection::new(13, 11)]);
    s.move_cursor(v, Movement::Right, false).unwrap();
    assert_eq!(
        cursors(&s, v),
        vec![13],
        "選択範囲がある Right は末尾へ縮める"
    );
    s.move_cursor(v, Movement::Right, false).unwrap();
    assert_eq!(cursors(&s, v), vec![13], "末尾を越えない");
    s.move_cursor(v, Movement::DocumentStart, false).unwrap();
    s.move_cursor(v, Movement::Left, false).unwrap();
    assert_eq!(cursors(&s, v), vec![0]);
    s.select_all(v).unwrap();
    assert_eq!(s.selections(v).unwrap(), &[Selection::new(0, 13)]);
    s.move_cursor(v, Movement::DocumentEnd, false).unwrap();
    assert_eq!(cursors(&s, v), vec![13]);
}

// --- §5.5 Undo の区切り ---

#[test]
fn consecutive_typing_is_one_undo_step() {
    let (mut s, v) = session("");
    for c in ["あ", "い", "う"] {
        s.insert_text(v, c).unwrap();
    }
    s.undo(v).unwrap();
    assert_eq!(content(&s), "");
}

#[test]
fn undo_groups_break_on_newline_move_kind_and_other_views() {
    let (mut s, v) = session("");
    s.insert_text(v, "a").unwrap();
    s.insert_newline(v).unwrap();
    s.insert_text(v, "b").unwrap();
    s.move_cursor(v, Movement::Left, false).unwrap();
    s.move_cursor(v, Movement::Right, false).unwrap();
    s.insert_text(v, "c").unwrap();
    s.delete_backward(v).unwrap();
    let w = s.open_view();
    s.insert_text(w, "W").unwrap();
    s.set_selections(v, vec![Selection::cursor(s.text().len_bytes())], 0)
        .unwrap();
    s.insert_text(v, "d").unwrap();
    let steps = ["Wa\nb", "a\nb", "a\nbc", "a\nb", "a\n", "a", ""];
    let mut got = vec![];
    while !s.undo(v).unwrap().is_empty() {
        got.push(content(&s));
    }
    assert_eq!(got, steps);
}

#[test]
fn replacing_a_selection_is_its_own_undo_step() {
    let (mut s, v) = session("abc");
    s.insert_text(v, "x").unwrap();
    s.set_selections(v, vec![Selection::new(1, 4)], 0).unwrap();
    s.insert_text(v, "y").unwrap();
    s.insert_text(v, "z").unwrap();
    s.undo(v).unwrap();
    assert_eq!(content(&s), "xy");
    s.undo(v).unwrap();
    assert_eq!(content(&s), "xabc");
}

// --- §5.6 Undo・Redo と選択範囲 ---

#[test]
fn undo_restores_selection_before_edit_and_redo_after() {
    let (mut s, v) = session("hello world");
    s.set_selections(v, vec![Selection::new(11, 6)], 0).unwrap();
    s.insert_text(v, "there").unwrap();
    let w = s.open_view();
    s.set_selections(w, vec![Selection::cursor(0)], 0).unwrap();
    s.move_cursor(v, Movement::DocumentStart, false).unwrap();

    s.undo(v).unwrap();
    assert_eq!(content(&s), "hello world");
    assert_eq!(
        s.selections(v).unwrap(),
        &[Selection::new(11, 6)],
        "編集前の選択範囲（向きも）"
    );
    s.redo(v).unwrap();
    assert_eq!(s.selections(v).unwrap(), &[Selection::cursor(11)]);
    assert_eq!(cursors(&s, w), vec![0], "他のビューは補正だけ");
}

#[test]
fn other_views_follow_edits() {
    let (mut s, a) = session("abcd");
    let b = s.open_view();
    s.set_selections(b, vec![Selection::cursor(2), Selection::new(3, 4)], 1)
        .unwrap();
    cursor_at(&mut s, a, 2);
    s.insert_text(a, "XX").unwrap();
    assert_eq!(
        s.selections(b).unwrap(),
        &[Selection::cursor(4), Selection::new(5, 6)],
        "挿入位置のカーソルは後ろへ"
    );
    cursor_at(&mut s, a, 6);
    s.delete_backward(a).unwrap();
    s.delete_backward(a).unwrap();
    assert_eq!(
        s.selections(b).unwrap(),
        &[Selection::cursor(4)],
        "削除で重なった選択範囲はまとまる"
    );
    assert_eq!(s.primary_index(b), Ok(0));
}

// --- §6 IME ---

#[test]
fn composition_stays_out_of_buffer_until_commit() {
    // P0-02 で記録した流れ: n → に → にh → にほ → … → 日本語 → 確定
    let (mut s, v) = session("前後");
    cursor_at(&mut s, v, 3);
    let (version, history_len) = (s.version(), s.history().len());
    for t in [
        "n",
        "に",
        "にh",
        "にほ",
        "にほん",
        "にほんご",
        "日本語",
        "日本語",
    ] {
        s.set_composition(v, t, None).unwrap();
        assert_eq!(content(&s), "前後");
    }
    assert_eq!(
        (s.version(), s.history().len()),
        (version, history_len),
        "履歴・版は変わらない"
    );
    assert_eq!(s.composition(v).unwrap().unwrap().range, 3..3);
    let changes = s.commit_composition(v, "日本語").unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(content(&s), "前日本語後");
    assert_eq!(cursors(&s, v), vec![12]);
    assert!(s.composition(v).unwrap().is_none());
    s.undo(v).unwrap();
    assert_eq!(content(&s), "前後");
    assert_eq!(cursors(&s, v), vec![3]);
}

#[test]
fn composition_deleted_to_empty_commits_nothing() {
    let (mut s, v) = session("ab");
    s.set_composition(v, "にほ", None).unwrap();
    s.set_composition(v, "に", None).unwrap();
    assert_eq!(s.commit_composition(v, ""), Ok(vec![]));
    assert_eq!(content(&s), "ab");
    assert_eq!(s.version(), 0);
}

#[test]
fn composition_replaces_selection_on_commit_and_cancel_restores() {
    let (mut s, v) = session("abc");
    s.set_selections(v, vec![Selection::new(0, 3)], 0).unwrap();
    s.set_composition(v, "か", None).unwrap();
    s.cancel_composition(v).unwrap();
    assert_eq!(
        (content(&s), s.selections(v).unwrap().to_vec()),
        ("abc".into(), vec![Selection::new(0, 3)])
    );
    s.set_composition(v, "か", None).unwrap();
    s.commit_composition(v, "").unwrap();
    assert_eq!(content(&s), "", "空で確定すると選択していた範囲は消える");
}

#[test]
fn other_operations_commit_composition_first() {
    let (mut s, v) = session("xyz");
    s.set_composition(v, "か", None).unwrap();
    let changes = s.insert_text(v, "!").unwrap();
    assert_eq!(changes.len(), 2, "確定と入力の2つ");
    assert_eq!(content(&s), "か!xyz");

    // 選択範囲の位置は呼び出し時点の内容に対するもの。確定で補正される
    let (mut s, v) = session("xyz");
    s.set_composition(v, "あい", None).unwrap();
    s.set_selections(v, vec![Selection::cursor(3)], 0).unwrap();
    assert_eq!(content(&s), "あいxyz");
    assert_eq!(cursors(&s, v), vec![9]);
}

#[test]
fn composition_follows_other_views_edits() {
    let (mut s, a) = session("abcd");
    let b = s.open_view();
    cursor_at(&mut s, a, 2);
    s.set_composition(a, "に", None).unwrap();
    cursor_at(&mut s, b, 0);
    s.insert_text(b, "XY").unwrap();
    assert_eq!(s.composition(a).unwrap().unwrap().range, 4..4);
    s.commit_composition(a, "日").unwrap();
    assert_eq!(content(&s), "XYab日cd");
}

#[test]
fn closing_view_discards_composition() {
    let (mut s, v) = session("a");
    s.set_composition(v, "に", None).unwrap();
    s.close_view(v).unwrap();
    assert_eq!(content(&s), "a");
}

#[test]
fn composition_selection_is_validated() {
    let (mut s, v) = session("");
    assert_eq!(
        s.set_composition(v, "にほ", Some(1..3)),
        Err(DocumentError::InvalidCompositionSelection {
            selected: 1..3,
            len: 6
        })
    );
    assert!(s.composition(v).unwrap().is_none());
}

#[test]
fn input_positions_include_composition() {
    // バッファ "a😀b"（UTF-16 で 4）。😀 の後ろ（バイト5、UTF-16 3）で「にほ」を変換中
    let (mut s, v) = session("a😀b");
    cursor_at(&mut s, v, 5);
    assert_eq!(s.input_marked_range(v), Ok(None));
    assert_eq!(s.input_selected_range(v), Ok((3..3, false)));
    s.set_composition(v, "にほ", Some(3..3)).unwrap();
    assert_eq!(s.input_len_utf16(v), Ok(6));
    assert_eq!(s.input_marked_range(v), Ok(Some(3..5)));
    assert_eq!(s.input_selected_range(v), Ok((4..4, false)));
    assert_eq!(s.input_text_for_range(v, 0..6).as_deref(), Ok("a😀にほb"));
    assert_eq!(
        s.input_text_for_range(v, 2..4).as_deref(),
        Ok("😀に"),
        "サロゲートペアの途中は外側へ"
    );
    assert_eq!(
        s.input_text_for_range(v, 5..99).as_deref(),
        Ok("b"),
        "長さを超える部分は切り詰める"
    );
    assert_eq!(s.input_to_buffer(v, 3), Ok(5));
    assert_eq!(
        s.input_to_buffer(v, 4),
        Ok(5),
        "変換中の文字列の内側は range.start"
    );
    assert_eq!(s.input_to_buffer(v, 5), Ok(5));
    assert_eq!(s.input_to_buffer(v, 6), Ok(6));
    assert_eq!(s.buffer_to_input(v, 6), Ok(6));
    assert_eq!(s.buffer_to_input(v, 1), Ok(1));

    // 選択範囲を置き換える変換: range の内側のバッファ位置は変換中の文字列の先頭
    let (mut s, v) = session("abcd");
    s.set_selections(v, vec![Selection::new(1, 3)], 0).unwrap();
    s.set_composition(v, "に", None).unwrap();
    assert_eq!(s.input_text_for_range(v, 0..3).as_deref(), Ok("aにd"));
    assert_eq!(s.buffer_to_input(v, 2), Ok(1));
    assert_eq!(s.buffer_to_input(v, 3), Ok(2));
    assert_eq!(s.input_to_buffer(v, 2), Ok(3));
}

// --- §8 保存済みの状態 ---

#[test]
fn dirty_state_follows_history() {
    let (mut s, v) = session("a");
    assert!(!s.is_dirty());
    s.insert_text(v, "b").unwrap();
    assert!(s.is_dirty());
    s.undo(v).unwrap();
    assert!(!s.is_dirty(), "保存済みの項目へ戻ると未保存でなくなる");
    s.redo(v).unwrap();
    let (snap, id) = s.save_point();
    s.insert_newline(v).unwrap();
    s.mark_saved(id).unwrap();
    assert_eq!(snap.to_string(), "ba");
    assert!(s.is_dirty(), "書き込み中の編集は未保存のまま");
    s.undo(v).unwrap();
    assert!(!s.is_dirty());
}

#[test]
fn mark_saved_rejects_unknown_items() {
    let (mut s, _) = session("a");
    let mut other = DocumentSession::new("");
    let ov = other.open_view();
    other.insert_text(ov, "x").unwrap();
    let id = other.history().current();
    assert!(matches!(
        s.mark_saved(id),
        Err(DocumentError::Text(TextError::UnknownHistoryId { .. }))
    ));
}

#[test]
fn reload_is_undoable_and_marks_saved() {
    let (mut s, v) = session("one\r\ntwo\r\n");
    let w = s.open_view();
    cursor_at(&mut s, w, 10);
    cursor_at(&mut s, v, 3);
    s.insert_text(v, "!").unwrap();
    s.set_composition(v, "に", None).unwrap();
    let change = s.reload("one\ntwo\nthree\n").unwrap().unwrap();
    assert_eq!(change.edits.len(), 1, "1つの範囲置換");
    assert_eq!(content(&s), "one\ntwo\nthree\n");
    assert!(!s.is_dirty());
    assert_eq!(s.line_ending(), LineEnding::Lf);
    assert!(
        s.composition(v).unwrap().is_none(),
        "変換中の文字列は取り消す"
    );
    assert!(s.text().is_char_boundary(cursors(&s, w)[0]));
    s.undo(v).unwrap();
    assert_eq!(content(&s), "one!\r\ntwo\r\n");
    assert!(s.is_dirty());

    // 同じ内容の再読み込み: 内容・履歴は変えず、保存済みにする
    let len = s.history().len();
    assert_eq!(s.reload("one!\r\ntwo\r\n"), Ok(None));
    assert_eq!(s.history().len(), len);
    assert!(!s.is_dirty());
}

#[test]
fn reload_keeps_char_boundaries() {
    // 共通の先頭・末尾が文字の途中で終わる場合（あ=E3 81 82、ぃ=E3 81 83）
    let (mut s, _) = session("xあy");
    let change = s.reload("xぃy").unwrap().unwrap();
    assert_eq!(change.edits[0].deleted, "あ");
    assert_eq!(change.edits[0].inserted, "ぃ");
}
