//! `docs/spec/document-session.md` §9 の Property-based Testing。2つのビューで任意のコマンド列を実行する。

use std::collections::HashMap;

use document::{Change, DocumentSession, HistoryId, Movement, Selection, ViewId};
use proptest::prelude::*;
use proptest::sample::Index;

fn piece() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "a",
        "Z",
        " ",
        "\n",
        "\r",
        "\r\n",
        "あ",
        "😀",
        "👩‍💻",
        "🇯🇵",
        "e\u{301}",
        "\u{301}",
        "क्ष",
        "❤\u{fe0f}",
        "👍🏽",
    ])
    .prop_map(String::from)
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(piece(), 0..20).prop_map(|v| v.concat())
}

#[derive(Debug, Clone)]
enum Cmd {
    Insert(String),
    Newline,
    Backspace,
    Delete,
    DeleteSelection,
    Move(Movement, bool),
    SelectAll,
    SetSelections(Vec<(Index, Index)>, Index),
    Undo,
    Redo,
    Compose(String),
    Commit(String),
    Cancel,
    Reload(String),
}

fn cmd() -> impl Strategy<Value = Cmd> {
    let movement = prop::sample::select(vec![
        Movement::Left,
        Movement::Right,
        Movement::LineStart,
        Movement::LineEnd,
        Movement::DocumentStart,
        Movement::DocumentEnd,
    ]);
    prop_oneof![
        4 => piece().prop_map(Cmd::Insert),
        1 => Just(Cmd::Newline),
        3 => Just(Cmd::Backspace),
        2 => Just(Cmd::Delete),
        1 => Just(Cmd::DeleteSelection),
        3 => (movement, any::<bool>()).prop_map(|(m, e)| Cmd::Move(m, e)),
        1 => Just(Cmd::SelectAll),
        2 => (prop::collection::vec((any::<Index>(), any::<Index>()), 1..4), any::<Index>())
            .prop_map(|(s, p)| Cmd::SetSelections(s, p)),
        2 => Just(Cmd::Undo),
        1 => Just(Cmd::Redo),
        2 => piece().prop_map(Cmd::Compose),
        1 => piece().prop_map(Cmd::Commit),
        1 => Just(Cmd::Cancel),
        1 => text().prop_map(Cmd::Reload),
    ]
}

fn boundaries(s: &str) -> Vec<usize> {
    s.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(s.len()))
        .collect()
}

/// §3 の選択範囲の不変条件と、変換中の文字列の範囲の妥当性。
fn check_invariants(s: &DocumentSession, views: &[ViewId]) -> Result<(), TestCaseError> {
    let content = s.text().to_string();
    for &v in views {
        let sels = s.selections(v).unwrap();
        prop_assert!(!sels.is_empty());
        prop_assert!(s.primary_index(v).unwrap() < sels.len());
        for x in sels {
            prop_assert!(
                x.anchor <= content.len() && content.is_char_boundary(x.anchor),
                "{:?}",
                x
            );
            prop_assert!(
                x.head <= content.len() && content.is_char_boundary(x.head),
                "{:?}",
                x
            );
        }
        for w in sels.windows(2) {
            let (a, b) = (w[0], w[1]);
            prop_assert!(a.start() <= b.start(), "並び順 {:?}", sels);
            let overlaps =
                b.start() < a.end() || (b.start() == a.end() && (a.is_empty() || b.is_empty()));
            prop_assert!(!overlaps, "重なり {:?}", sels);
        }
        if let Some(c) = s.composition(v).unwrap() {
            prop_assert!(c.range.start <= c.range.end && c.range.end <= content.len());
            prop_assert!(
                content.is_char_boundary(c.range.start) && content.is_char_boundary(c.range.end)
            );
        }
    }
    Ok(())
}

fn apply_changes(before: &str, changes: &[Change]) -> String {
    let mut s = before.to_string();
    for c in changes {
        for e in &c.edits {
            s.replace_range(e.start..e.end(), &e.inserted);
        }
    }
    s
}

fn run(s: &mut DocumentSession, v: ViewId, cmd: &Cmd) -> Vec<Change> {
    let r = match cmd {
        Cmd::Insert(t) => s.insert_text(v, t),
        Cmd::Newline => s.insert_newline(v),
        Cmd::Backspace => s.delete_backward(v),
        Cmd::Delete => s.delete_forward(v),
        Cmd::DeleteSelection => s.delete_selection(v),
        Cmd::Move(m, e) => s.move_cursor(v, *m, *e),
        Cmd::SelectAll => s.select_all(v),
        Cmd::SetSelections(sel, p) => {
            let bs = boundaries(&s.text().to_string());
            let sels: Vec<Selection> = sel
                .iter()
                .map(|(a, h)| Selection::new(bs[a.index(bs.len())], bs[h.index(bs.len())]))
                .collect();
            let primary = p.index(sels.len());
            s.set_selections(v, sels, primary)
        }
        Cmd::Undo => s.undo(v),
        Cmd::Redo => s.redo(v),
        Cmd::Compose(t) => s.set_composition(v, t, None).map(|_| vec![]),
        Cmd::Commit(t) => s.commit_composition(v, t),
        Cmd::Cancel => s.cancel_composition(v).map(|_| vec![]),
        Cmd::Reload(t) => s.reload(t).map(|c| c.into_iter().collect()),
    };
    r.expect("valid commands do not fail")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn invariants_hold_and_undo_all_restores(
        initial in text(),
        cmds in prop::collection::vec((any::<bool>(), cmd()), 0..40),
    ) {
        let mut s = DocumentSession::new(&initial);
        let views = [s.open_view(), s.open_view()];
        // 1つのコマンドで1つの項目だけが作られた場合の、作ったビューの編集前の選択範囲
        let mut before_of: HashMap<HistoryId, (ViewId, Vec<Selection>)> = HashMap::new();
        let mut has_reload = false;
        for (which, c) in &cmds {
            let v = views[*which as usize];
            let before = s.text().to_string();
            let (version, history_len) = (s.version(), s.history().len());
            let sels_before = s.selections(v).unwrap().to_vec();
            let had_composition = s.composition(v).unwrap().is_some();
            let changes = run(&mut s, v, c);
            check_invariants(&s, &views)?;
            prop_assert_eq!(apply_changes(&before, &changes), s.text().to_string(), "返した Change の列で内容を再現できる");
            prop_assert_eq!(s.version(), version + changes.len() as u64);
            if matches!(c, Cmd::Compose(_) | Cmd::Cancel) {
                prop_assert_eq!(s.version(), version, "変換中の文字列はバッファに影響しない");
                prop_assert_eq!(s.history().len(), history_len);
            }
            if matches!(c, Cmd::Reload(_)) {
                has_reload = true;
                prop_assert!(!s.is_dirty());
            }
            if s.history().len() == history_len + 1 && !had_composition && !matches!(c, Cmd::Reload(_)) {
                before_of.insert(s.history().current(), (v, sels_before));
            }
        }
        // 先に変換中の文字列を取り消し、ビュー0で Undo をすべて行う
        for &v in &views {
            s.cancel_composition(v).unwrap();
        }
        let mut last_undone = None;
        loop {
            let cur = s.history().current();
            if s.undo(views[0]).unwrap().is_empty() {
                break;
            }
            last_undone = Some(cur);
            check_invariants(&s, &views)?;
        }
        prop_assert_eq!(s.text().to_string(), initial.clone(), "Undo をすべて行うと最初の内容に戻る");
        prop_assert!(!s.is_dirty() || has_reload);
        // 最後に取り消した項目の編集前の選択範囲が、Undo したビューに復元される（§5.6）
        if let Some((_, sels)) = last_undone.and_then(|id| before_of.get(&id)) {
            let mut expected = sels.clone();
            expected.sort_by_key(|x| (x.start(), x.end()));
            prop_assert_eq!(s.selections(views[0]).unwrap().to_vec(), expected);
        }
    }
}
