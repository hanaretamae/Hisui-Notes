//! `docs/spec/editor-ui-m2.md` §7 の検証。一時ディレクトリの実ファイルと GPUI のテスト環境を使う。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use app_core::{Core, LocalAppApi};
use gpui::{Entity, EntityInputHandler, TestAppContext, VisualTestContext};
use platform::DesktopPlatform;
use ui_gpui::{Editor, Workspace};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempVault(PathBuf);

impl TempVault {
    fn new(files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "hisui-ui-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        for (p, data) in files {
            let full = root.join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, data).unwrap();
        }
        Self(root)
    }
    fn read(&self, p: &str) -> String {
        std::fs::read_to_string(self.0.join(p)).unwrap()
    }
    fn write(&self, p: &str, data: &str) {
        std::fs::write(self.0.join(p), data).unwrap();
    }
    fn root(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Vault を開いた画面。
fn setup<'a>(
    cx: &'a mut TestAppContext,
    vault: &TempVault,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.update(ui_gpui::bind_keys);
    let api: Arc<dyn LocalAppApi> = Arc::new(Core::new(Arc::new(DesktopPlatform)));
    let (ws, cx) = cx.add_window_view(|window, cx| Workspace::new(api, window, cx).unwrap());
    let root = vault.root();
    ws.update_in(cx, |ws, window, cx| ws.open_vault_path(root, window, cx));
    cx.run_until_parked();
    (ws, cx)
}

fn open(ws: &Entity<Workspace>, cx: &mut VisualTestContext, path: &str) -> Entity<Editor> {
    let path = path.to_string();
    ws.update_in(cx, |ws, window, cx| ws.open_file(path, window, cx));
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| ws.editor().cloned())
        .expect("editor")
}

fn line(editor: &Entity<Editor>, cx: &mut VisualTestContext, n: usize) -> String {
    editor.read_with(cx, |e, _| {
        e.display_line(n).map(|l| l.text).unwrap_or_default()
    })
}

fn header(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    ws.read_with(cx, |ws, cx| {
        ws.editor()
            .map(|e| {
                format!(
                    "{}{}",
                    e.read(cx).path(),
                    if e.read(cx).is_dirty() { " ●" } else { "" }
                )
            })
            .unwrap_or_default()
    })
}

#[gpui::test]
fn lists_markdown_files_and_opens_one(cx: &mut TestAppContext) {
    let v = TempVault::new(&[
        ("b.md", "# B\n本文"),
        ("a/c.MD", ""),
        ("img.png", "x"),
        (".obsidian/x.md", ""),
    ]);
    let (ws, cx) = setup(cx, &v);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.files().collect::<Vec<_>>(), vec!["a/c.MD", "b.md"])
    });
    let editor = open(&ws, cx, "b.md");
    assert_eq!(line(&editor, cx, 0), "# B");
    assert_eq!(line(&editor, cx, 1), "本文");
    assert_eq!(editor.read_with(cx, |e, _| e.line_count()), 2);
    assert_eq!(header(&ws, cx), "b.md");
}

#[gpui::test]
fn open_errors_are_shown_in_status(cx: &mut TestAppContext) {
    let v = TempVault::new(&[("ok.md", "")]);
    std::fs::write(v.0.join("bad.md"), b"\xff\xfe").unwrap();
    let (ws, cx) = setup(cx, &v);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file("bad.md".into(), window, cx)
    });
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.editor().is_none());
        assert!(ws.status().starts_with("InvalidData"), "{}", ws.status());
    });
}

#[gpui::test]
fn keys_edit_document_and_save_writes_file(cx: &mut TestAppContext) {
    let v = TempVault::new(&[("n.md", "ab")]);
    let (ws, cx) = setup(cx, &v);
    let editor = open(&ws, cx, "n.md");
    cx.simulate_keystrokes("ctrl-end enter");
    cx.simulate_input("x");
    cx.run_until_parked();
    assert_eq!(
        (line(&editor, cx, 0), line(&editor, cx, 1)),
        ("ab".into(), "x".into())
    );
    assert_eq!(header(&ws, cx), "n.md ●");
    cx.simulate_keystrokes("backspace backspace left shift-left");
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |e, _| e.line_count()),
        1,
        "改行の削除で行が減る"
    );
    let sel = editor.read_with(cx, |e, _| e.display_line(0).unwrap().selections);
    assert_eq!(sel, vec![(0..1, false)]);
    // 連続した Backspace は1つの項目（文書セッション §5.5）
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(line(&editor, cx, 1), "x");
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.line_count()), 2);
    assert_eq!(line(&editor, cx, 1), "");
    cx.simulate_keystrokes("ctrl-shift-z ctrl-s");
    cx.run_until_parked();
    assert_eq!(v.read("n.md"), "ab\nx");
    assert_eq!(header(&ws, cx), "n.md");
}

#[gpui::test]
fn ime_composition_is_displayed_and_committed(cx: &mut TestAppContext) {
    let v = TempVault::new(&[("n.md", "前後")]);
    let (ws, cx) = setup(cx, &v);
    let editor = open(&ws, cx, "n.md");
    cx.simulate_keystrokes("right");
    editor.update_in(cx, |e, window, cx| {
        e.replace_and_mark_text_in_range(None, "にほ", None, window, cx);
        assert_eq!(e.marked_text_range(window, cx), Some(1..3));
        assert_eq!(e.text_length_utf16(window, cx), Some(4));
        assert_eq!(
            e.text_for_range(0..4, &mut None, window, cx).as_deref(),
            Some("前にほ後")
        );
    });
    cx.run_until_parked();
    let l = editor.read_with(cx, |e, _| e.display_line(0).unwrap());
    assert_eq!(l.text, "前にほ後");
    assert_eq!(l.cursors, vec![9]);
    assert_eq!(header(&ws, cx), "n.md", "変換中は内容を変えない");
    editor.update_in(cx, |e, window, cx| {
        e.replace_text_in_range(None, "日本", window, cx)
    });
    cx.run_until_parked();
    let l = editor.read_with(cx, |e, _| e.display_line(0).unwrap());
    assert_eq!(
        (l.text.as_str(), l.composition.is_none()),
        ("前日本後", true)
    );
    // 取り消し
    editor.update_in(cx, |e, window, cx| {
        e.replace_and_mark_text_in_range(None, "x", None, window, cx);
        e.replace_and_mark_text_in_range(None, "", None, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(line(&editor, cx, 0), "前日本後");
}

#[gpui::test]
fn save_conflict_offers_overwrite_and_reload(cx: &mut TestAppContext) {
    let v = TempVault::new(&[("n.md", "one")]);
    let (ws, cx) = setup(cx, &v);
    let editor = open(&ws, cx, "n.md");
    cx.simulate_input("A");
    v.write("n.md", "external");
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("キャンセル");
    cx.run_until_parked();
    assert_eq!(v.read("n.md"), "external");

    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    cx.simulate_prompt_answer("再読み込み");
    cx.run_until_parked();
    assert_eq!(line(&editor, cx, 0), "external");
    assert_eq!(header(&ws, cx), "n.md");

    cx.simulate_keystrokes("ctrl-z");
    v.write("n.md", "again");
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    cx.simulate_prompt_answer("上書き");
    cx.run_until_parked();
    assert_eq!(v.read("n.md"), "Aone");
}

#[gpui::test]
fn unsaved_changes_are_confirmed_before_opening_another_file(cx: &mut TestAppContext) {
    let v = TempVault::new(&[("a.md", "a"), ("b.md", "b")]);
    let (ws, cx) = setup(cx, &v);
    open(&ws, cx, "a.md");
    cx.simulate_input("1");
    cx.run_until_parked();

    let try_open = |cx: &mut VisualTestContext| {
        ws.update_in(cx, |ws, window, cx| ws.open_file("b.md".into(), window, cx));
        cx.run_until_parked();
        assert!(cx.has_pending_prompt());
    };
    try_open(cx);
    cx.simulate_prompt_answer("キャンセル");
    cx.run_until_parked();
    assert_eq!(header(&ws, cx), "a.md ●");

    try_open(cx);
    cx.simulate_prompt_answer("保存");
    cx.run_until_parked();
    assert_eq!(v.read("a.md"), "1a");
    assert_eq!(header(&ws, cx), "b.md");

    cx.simulate_input("2");
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| ws.open_file("a.md".into(), window, cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("破棄");
    cx.run_until_parked();
    assert_eq!(header(&ws, cx), "a.md");
    assert_eq!(v.read("b.md"), "b", "破棄した変更は保存しない");
}

/// 1つの変更ごとに表示の行の一覧を差分で更新する（行の追加・削除）。
#[gpui::test]
fn line_list_follows_single_edits(cx: &mut TestAppContext) {
    let v = TempVault::new(&[("n.md", "a\nb\nc")]);
    let (ws, cx) = setup(cx, &v);
    let editor = open(&ws, cx, "n.md");
    let lines = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| {
            (0..e.line_count())
                .map(|i| e.display_line(i).unwrap().text)
                .collect::<Vec<_>>()
        })
    };
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(lines(cx), vec!["a", "b", "", "c"]);
    cx.simulate_input("新");
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-home");
    cx.run_until_parked();
    cx.simulate_keystrokes("shift-down");
    cx.run_until_parked();
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    assert_eq!(lines(cx), vec!["b", "新", "c"]);
}
