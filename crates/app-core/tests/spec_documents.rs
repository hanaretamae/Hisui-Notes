//! `docs/spec/app-api.md` §5A（版0.2: Vault・文書）の検証。一時ディレクトリの実ファイルを使う。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use app_api::{
    AppApi, Command, DocumentId, EditCommand, ErrorCode, Event, EventReceiver, Movement, Query,
    QueryResponse, Selection, TextEdit, ViewId,
};
use app_core::{Core, LocalAppApi};
use platform::DesktopPlatform;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 一時ディレクトリの Vault（終了時に削除する）。
struct TempVault(PathBuf);

impl TempVault {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "hisui-core-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        for (p, data) in files {
            let full = root.join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, data).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn read(&self, p: &str) -> Vec<u8> {
        std::fs::read(self.0.join(p)).unwrap()
    }

    fn write(&self, p: &str, data: &[u8]) {
        std::fs::write(self.0.join(p), data).unwrap();
    }
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn drain(rx: &mut EventReceiver) -> Vec<Event> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// Vault と文書を開いた状態の Core。
fn setup(
    files: &[(&str, &[u8])],
    open: &str,
) -> (TempVault, Core, EventReceiver, DocumentId, ViewId) {
    let v = TempVault::new(files);
    let core = Core::new(Arc::new(DesktopPlatform));
    let mut rx = core.subscribe();
    core.execute(Command::OpenVault {
        path: v.0.to_string_lossy().into(),
    })
    .unwrap();
    core.execute(Command::OpenDocument { path: open.into() })
        .unwrap();
    let events = drain(&mut rx);
    let Some(Event::DocumentOpened { document, view, .. }) = events.last().cloned() else {
        panic!("{events:?}")
    };
    (v, core, rx, document, view)
}

fn edit(core: &Core, d: DocumentId, v: ViewId, command: EditCommand) {
    core.execute(Command::Edit {
        document: d,
        view: v,
        command,
    })
    .unwrap();
}

fn text(core: &Core, d: DocumentId) -> String {
    core.document_snapshot(d).unwrap().to_string()
}

#[test]
fn open_vault_lists_files_and_opens_document() {
    let v = TempVault::new(&[("b.md", b""), ("a/n.md", b"x"), (".obsidian/c.json", b"{}")]);
    let core = Core::new(Arc::new(DesktopPlatform));
    let mut rx = core.subscribe();
    let root = v.0.to_string_lossy().to_string();
    core.execute(Command::OpenVault { path: root.clone() })
        .unwrap();
    assert_eq!(drain(&mut rx), vec![Event::VaultOpened { root }]);
    let QueryResponse::VaultFiles(files) = core.query(Query::VaultFiles).unwrap() else {
        panic!()
    };
    assert_eq!(files.files, vec!["a/n.md", "b.md"]);
    assert_eq!(files.skipped, 0);

    core.execute(Command::OpenDocument {
        path: "a/n.md".into(),
    })
    .unwrap();
    let [
        Event::DocumentOpened {
            document,
            view,
            path,
        },
    ] = &drain(&mut rx)[..]
    else {
        panic!()
    };
    let (document, view) = (*document, *view);
    assert_eq!(path, "a/n.md");
    assert_eq!(text(&core, document), "x");
    // すでに開いているパスは同じ文書とビュー
    core.execute(Command::OpenDocument {
        path: "a/n.md".into(),
    })
    .unwrap();
    assert_eq!(
        drain(&mut rx),
        vec![Event::DocumentOpened {
            document,
            view,
            path: "a/n.md".into()
        }]
    );
    // Vault を開き直すと文書を閉じる
    core.execute(Command::OpenVault {
        path: v.0.to_string_lossy().into(),
    })
    .unwrap();
    assert!(
        matches!(drain(&mut rx)[..], [Event::DocumentClosed { document: d }, Event::VaultOpened { .. }] if d == document)
    );
    assert!(core.document_snapshot(document).is_none());
}

#[test]
fn open_errors_map_to_codes_and_publish_nothing() {
    let core = Core::new(Arc::new(DesktopPlatform));
    let mut rx = core.subscribe();
    let code = |c: Command| core.execute(c).unwrap_err().code;
    assert_eq!(
        code(Command::OpenDocument {
            path: "n.md".into()
        }),
        ErrorCode::NotFound,
        "Vault が無い"
    );
    assert_eq!(
        code(Command::OpenVault {
            path: "/nonexistent/hisui".into()
        }),
        ErrorCode::NotFound
    );
    let v = TempVault::new(&[("sjis.md", b"\x82\xa0"), ("dir/x.md", b"")]);
    core.execute(Command::OpenVault {
        path: v.0.to_string_lossy().into(),
    })
    .unwrap();
    drain(&mut rx);
    assert_eq!(
        code(Command::OpenDocument {
            path: "sjis.md".into()
        }),
        ErrorCode::InvalidData
    );
    assert_eq!(
        code(Command::OpenDocument {
            path: "none.md".into()
        }),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(Command::OpenDocument {
            path: "../x.md".into()
        }),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        code(Command::OpenDocument { path: "dir".into() }),
        ErrorCode::Unsupported
    );
    assert_eq!(
        code(Command::CloseDocument {
            document: DocumentId(99)
        }),
        ErrorCode::NotFound
    );
    assert!(
        drain(&mut rx).is_empty(),
        "失敗した Command は Event を発行しない"
    );
}

#[test]
fn edits_publish_changes_view_state_and_dirty() {
    let (_v, core, mut rx, d, v) = setup(&[("n.md", "前後".as_bytes())], "n.md");
    edit(
        &core,
        d,
        v,
        EditCommand::SetSelections {
            selections: vec![Selection { anchor: 3, head: 3 }],
            primary: 0,
        },
    );
    drain(&mut rx);
    edit(&core, d, v, EditCommand::InsertText { text: "中".into() });
    let events = drain(&mut rx);
    assert_eq!(
        events[0],
        Event::DocumentChanged {
            document: d,
            version: 1,
            edits: vec![TextEdit {
                start: 3,
                old_len: 0,
                new_len: 3
            }]
        }
    );
    assert!(
        matches!(&events[1], Event::ViewStateChanged { state, .. } if state.selections == vec![Selection { anchor: 6, head: 6 }])
    );
    assert_eq!(
        events[2],
        Event::DirtyChanged {
            document: d,
            dirty: true
        }
    );
    assert_eq!(text(&core, d), "前中後");
    let snap = core.document_snapshot(d).unwrap();
    assert_eq!(
        snap.version(),
        1,
        "スナップショットの版番号は DocumentChanged と一致する"
    );

    // 変更の無い操作は ViewStateChanged だけ（変わった場合）
    edit(
        &core,
        d,
        v,
        EditCommand::MoveCursor {
            movement: Movement::DocumentStart,
            extend: false,
        },
    );
    assert!(matches!(
        drain(&mut rx)[..],
        [Event::ViewStateChanged { .. }]
    ));
    edit(
        &core,
        d,
        v,
        EditCommand::MoveCursor {
            movement: Movement::DocumentStart,
            extend: false,
        },
    );
    assert!(drain(&mut rx).is_empty());

    edit(&core, d, v, EditCommand::Undo);
    let events = drain(&mut rx);
    assert!(events.contains(&Event::DirtyChanged {
        document: d,
        dirty: false
    }));
    assert_eq!(text(&core, d), "前後");

    let r = core.execute(Command::Edit {
        document: d,
        view: v,
        command: EditCommand::SetSelections {
            selections: vec![Selection { anchor: 1, head: 1 }],
            primary: 0,
        },
    });
    assert_eq!(r.unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(
        core.execute(Command::Edit {
            document: d,
            view: ViewId(7),
            command: EditCommand::Undo
        })
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
}

#[test]
fn ime_queries_reflect_composition() {
    let (_v, core, mut rx, d, v) = setup(&[("n.md", "a😀b".as_bytes())], "n.md");
    edit(
        &core,
        d,
        v,
        EditCommand::SetSelections {
            selections: vec![Selection { anchor: 5, head: 5 }],
            primary: 0,
        },
    );
    edit(
        &core,
        d,
        v,
        EditCommand::SetComposition {
            text: "にほ".into(),
            selected: None,
        },
    );
    let events = drain(&mut rx);
    assert!(
        matches!(events.last(), Some(Event::ViewStateChanged { state, .. }) if state.composition.as_ref().unwrap().text == "にほ")
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::DocumentChanged { .. })),
        "変換中は内容を変えない"
    );
    let q = |query| core.query(query).unwrap();
    assert_eq!(
        q(Query::InputMarkedRange {
            document: d,
            view: v
        }),
        QueryResponse::MarkedRange(Some(3..5))
    );
    assert_eq!(
        q(Query::InputTextForRange {
            document: d,
            view: v,
            range: 0..6
        }),
        QueryResponse::Text("a😀にほb".into())
    );
    assert_eq!(
        q(Query::InputSelectedRange {
            document: d,
            view: v
        }),
        QueryResponse::SelectedRange {
            range: 5..5,
            reversed: false
        }
    );
    assert_eq!(
        q(Query::InputToBuffer {
            document: d,
            view: v,
            offset: 6
        }),
        QueryResponse::Offset(6)
    );
    assert_eq!(
        q(Query::BufferToInput {
            document: d,
            view: v,
            offset: 6
        }),
        QueryResponse::Offset(6)
    );
    edit(
        &core,
        d,
        v,
        EditCommand::CommitComposition {
            text: "日本".into(),
        },
    );
    assert_eq!(text(&core, d), "a😀日本b");
}

#[test]
fn save_writes_file_and_detects_conflicts() {
    let (vault, core, mut rx, d, v) = setup(&[("n.md", b"\xEF\xBB\xBFline\r\n")], "n.md");
    edit(
        &core,
        d,
        v,
        EditCommand::MoveCursor {
            movement: Movement::DocumentEnd,
            extend: false,
        },
    );
    edit(
        &core,
        d,
        v,
        EditCommand::InsertText {
            text: "追加".into(),
        },
    );
    drain(&mut rx);
    core.execute(Command::SaveDocument {
        document: d,
        overwrite: false,
    })
    .unwrap();
    assert_eq!(
        drain(&mut rx),
        vec![
            Event::DocumentSaved { document: d },
            Event::DirtyChanged {
                document: d,
                dirty: false
            }
        ]
    );
    assert_eq!(
        vault.read("n.md"),
        "\u{feff}line\r\n追加".as_bytes(),
        "BOM を戻して保存する"
    );
    let QueryResponse::DocumentInfo(info) =
        core.query(Query::DocumentInfo { document: d }).unwrap()
    else {
        panic!()
    };
    assert!(!info.dirty);

    // 外部での変更 → 衝突（書き込まず、Event も無い）
    vault.write("n.md", b"external");
    edit(&core, d, v, EditCommand::InsertText { text: "!".into() });
    drain(&mut rx);
    let e = core
        .execute(Command::SaveDocument {
            document: d,
            overwrite: false,
        })
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert!(
        !e.message.contains("追加"),
        "メッセージに文書の内容を含めない"
    );
    assert_eq!(vault.read("n.md"), b"external");
    assert!(drain(&mut rx).is_empty());
    // 上書き
    core.execute(Command::SaveDocument {
        document: d,
        overwrite: true,
    })
    .unwrap();
    assert_eq!(vault.read("n.md"), "\u{feff}line\r\n追加!".as_bytes());
    // 上書き後は通常の保存ができる
    edit(&core, d, v, EditCommand::InsertText { text: "?".into() });
    core.execute(Command::SaveDocument {
        document: d,
        overwrite: false,
    })
    .unwrap();
}

#[test]
fn reload_replaces_content_as_undoable_change() {
    let (vault, core, mut rx, d, v) = setup(&[("n.md", b"one")], "n.md");
    edit(
        &core,
        d,
        v,
        EditCommand::InsertText {
            text: "mine ".into(),
        },
    );
    vault.write("n.md", b"one two");
    drain(&mut rx);
    core.execute(Command::ReloadDocument { document: d })
        .unwrap();
    let events = drain(&mut rx);
    assert!(matches!(
        events[0],
        Event::DocumentChanged { version: 2, .. }
    ));
    assert!(events.contains(&Event::DirtyChanged {
        document: d,
        dirty: false
    }));
    assert_eq!(text(&core, d), "one two");
    edit(&core, d, v, EditCommand::Undo);
    assert_eq!(text(&core, d), "mine one");
    // 再読み込み後は、読み直した内容を基準に衝突を検出する
    edit(&core, d, v, EditCommand::Redo);
    edit(&core, d, v, EditCommand::InsertText { text: "!".into() });
    core.execute(Command::SaveDocument {
        document: d,
        overwrite: false,
    })
    .unwrap();
}

#[test]
fn close_document_discards_it() {
    let (_v, core, mut rx, d, _) = setup(&[("n.md", b"x")], "n.md");
    core.execute(Command::CloseDocument { document: d })
        .unwrap();
    assert_eq!(drain(&mut rx), vec![Event::DocumentClosed { document: d }]);
    assert_eq!(
        core.query(Query::DocumentInfo { document: d })
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}
