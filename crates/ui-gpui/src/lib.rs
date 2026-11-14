//! GPUI のフロントエンド（`docs/spec/editor-ui-m2.md`）。Core とは `app-api` と、
//! `app-core` の公開API（`LocalAppApi`）だけを通して連携する（ADR-0002）。

mod display;
mod editor;
mod line;

use std::sync::Arc;

use app_api::{ApiError, AppInfo, Command, ErrorCode, Event, Query, QueryResponse};
use app_core::LocalAppApi;
use futures::StreamExt as _;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, PathPromptOptions, PromptLevel,
    SharedString, Subscription, Task, Window, actions, div, prelude::*, px, rgb,
};

pub use display::DisplayLine;
pub use editor::{Editor, EditorEvent};

actions!(workspace, [Save, OpenVault]);

/// macOS では Cmd、それ以外では Ctrl を修飾キーにしたキー操作。
macro_rules! sec {
    ($k:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $k)
        } else {
            concat!("ctrl-", $k)
        }
    };
}

/// キー操作の割り当て（Spec §4.2）。
pub fn bind_keys(cx: &mut App) {
    use editor::*;
    let e = Some("Editor");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, e),
        KeyBinding::new("delete", Delete, e),
        KeyBinding::new("enter", Newline, e),
        KeyBinding::new("tab", Tab, e),
        KeyBinding::new("left", Left, e),
        KeyBinding::new("right", Right, e),
        KeyBinding::new("up", Up, e),
        KeyBinding::new("down", Down, e),
        KeyBinding::new("shift-left", SelectLeft, e),
        KeyBinding::new("shift-right", SelectRight, e),
        KeyBinding::new("shift-up", SelectUp, e),
        KeyBinding::new("shift-down", SelectDown, e),
        KeyBinding::new("home", LineStart, e),
        KeyBinding::new("end", LineEnd, e),
        KeyBinding::new("shift-home", SelectLineStart, e),
        KeyBinding::new("shift-end", SelectLineEnd, e),
        KeyBinding::new(sec!("home"), DocumentStart, e),
        KeyBinding::new(sec!("end"), DocumentEnd, e),
        KeyBinding::new(sec!("shift-home"), SelectDocumentStart, e),
        KeyBinding::new(sec!("shift-end"), SelectDocumentEnd, e),
        KeyBinding::new(sec!("a"), SelectAll, e),
        KeyBinding::new(sec!("z"), Undo, e),
        KeyBinding::new(sec!("shift-z"), Redo, e),
        KeyBinding::new(sec!("y"), Redo, e),
        KeyBinding::new(sec!("c"), Copy, e),
        KeyBinding::new(sec!("x"), Cut, e),
        KeyBinding::new(sec!("v"), Paste, e),
        KeyBinding::new(sec!("s"), Save, Some("Workspace")),
        KeyBinding::new(sec!("o"), OpenVault, Some("Workspace")),
    ]);
}

/// 未保存の確認の選択肢（Spec §2-4）。
pub const UNSAVED_ANSWERS: [&str; 3] = ["保存", "破棄", "キャンセル"];
/// 保存の衝突の選択肢（Spec §5）。
pub const CONFLICT_ANSWERS: [&str; 3] = ["上書き", "再読み込み", "キャンセル"];

type Continuation = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// 画面全体（Spec §3）。
pub struct Workspace {
    api: Arc<dyn LocalAppApi>,
    info: AppInfo,
    vault_root: Option<String>,
    files: Vec<String>,
    skipped: usize,
    editor: Option<Entity<Editor>>,
    focus_editor: bool,
    status: SharedString,
    focus: FocusHandle,
    /// 画面の文字のフォント（`None` は GPUI の既定。Spec §2-5）。
    font_family: Option<SharedString>,
    _editor_sub: Option<Subscription>,
    _events: Task<()>,
}

fn is_markdown(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("md"))
}

impl Workspace {
    /// Query で初期の状態を取得し、Event の購読を始める。
    pub fn new(
        api: Arc<dyn LocalAppApi>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, ApiError> {
        let QueryResponse::AppInfo(info) = api.query(Query::AppInfo)? else {
            return Err(ApiError::new(
                ErrorCode::Internal,
                "unexpected response to AppInfo",
            ));
        };
        let mut events = api.subscribe();
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |view, cx| view.apply(event, cx)).is_err() {
                    break;
                }
            }
        });
        let this = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            this.update(cx, |ws, cx| ws.should_close(window, cx))
                .unwrap_or(true)
        });
        Ok(Self {
            api,
            info,
            vault_root: None,
            files: Vec::new(),
            skipped: 0,
            editor: None,
            focus_editor: false,
            status: SharedString::default(),
            focus: cx.focus_handle(),
            font_family: None,
            _editor_sub: None,
            _events: task,
        })
    }

    /// 画面の文字のフォントを設定する（OS の UI フォント。Spec §2-5）。
    pub fn with_font_family(mut self, family: Option<String>) -> Self {
        self.font_family = family.map(Into::into);
        self
    }

    /// 見出し（アプリ名と版）。
    pub fn title(&self) -> String {
        format!("{} {}", self.info.name, self.info.version)
    }

    /// 状態表示の文字列。
    pub fn status(&self) -> &str {
        &self.status
    }

    /// 一覧に表示するファイル（`.md` のみ）。
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files
            .iter()
            .map(String::as_str)
            .filter(|p| is_markdown(p))
    }

    pub fn editor(&self) -> Option<&Entity<Editor>> {
        self.editor.as_ref()
    }

    fn report(&mut self, e: ApiError, cx: &mut Context<Self>) {
        self.status = e.to_string().into();
        cx.notify();
    }

    /// Event を表示に反映する。
    fn apply(&mut self, event: Event, cx: &mut Context<Self>) {
        match &event {
            Event::StatusMessageChanged { text } => self.status = text.clone().into(),
            Event::VaultOpened { root } => {
                self.vault_root = Some(root.clone());
                self.refresh_files(cx);
            }
            Event::DocumentOpened {
                document,
                view,
                path,
            } => {
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.read(cx).document() == *document)
                {
                    return;
                }
                match Editor::build(self.api.clone(), *document, *view, path.clone(), cx) {
                    Ok(editor) => {
                        self._editor_sub =
                            Some(cx.subscribe(&editor, |this, _, e: &EditorEvent, cx| {
                                let EditorEvent::Error(e) = e;
                                this.report(e.clone(), cx);
                            }));
                        self.editor = Some(editor);
                        self.focus_editor = true;
                    }
                    Err(e) => self.report(e, cx),
                }
            }
            Event::DocumentClosed { document } => {
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.read(cx).document() == *document)
                {
                    self.editor = None;
                    self._editor_sub = None;
                }
            }
            Event::DocumentChanged { document, .. }
            | Event::ViewStateChanged { document, .. }
            | Event::DirtyChanged { document, .. } => {
                if let Some(editor) = &self.editor
                    && editor.read(cx).document() == *document
                {
                    editor.update(cx, |e, cx| e.apply(&event, cx));
                }
            }
            _ => return,
        }
        cx.notify();
    }

    fn refresh_files(&mut self, cx: &mut Context<Self>) {
        match self.api.query(Query::VaultFiles) {
            Ok(QueryResponse::VaultFiles(f)) => {
                self.files = f.files;
                self.skipped = f.skipped;
            }
            Ok(_) => {}
            Err(e) => self.report(e, cx),
        }
    }

    /// 開いている文書が未保存か（Core に問い合わせる）。
    fn is_dirty(&self, cx: &App) -> bool {
        let Some(editor) = &self.editor else {
            return false;
        };
        let document = editor.read(cx).document();
        matches!(self.api.query(Query::DocumentInfo { document }), Ok(QueryResponse::DocumentInfo(i)) if i.dirty)
    }

    /// 保存する。衝突の場合は `Err` に `Conflict` を返す。
    fn save_now(&mut self, cx: &mut Context<Self>, overwrite: bool) -> Result<(), ApiError> {
        let Some(editor) = &self.editor else {
            return Ok(());
        };
        let document = editor.read(cx).document();
        self.api.execute(Command::SaveDocument {
            document,
            overwrite,
        })
    }

    /// 保存（Ctrl+S）。衝突の場合は確認する（Spec §5）。
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.save_now(cx, false) {
            Ok(()) => {
                self.status = "保存しました".into();
                cx.notify();
            }
            Err(e) if e.code == ErrorCode::Conflict => {
                let rx = window.prompt(
                    PromptLevel::Warning,
                    "ファイルが外部で変更されています",
                    Some(&e.message),
                    &CONFLICT_ANSWERS,
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let Ok(answer) = rx.await else { return };
                    let _ = this.update_in(cx, |this, _window, cx| {
                        let Some(editor) = &this.editor else { return };
                        let document = editor.read(cx).document();
                        let r = match answer {
                            0 => this.api.execute(Command::SaveDocument {
                                document,
                                overwrite: true,
                            }),
                            1 => this.api.execute(Command::ReloadDocument { document }),
                            _ => Ok(()),
                        };
                        if let Err(e) = r {
                            this.report(e, cx);
                        }
                    });
                })
                .detach();
            }
            Err(e) => self.report(e, cx),
        }
    }

    /// 未保存の変更があれば確認してから `then` を行う（Spec §2-4）。
    fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>, then: Continuation) {
        if !self.is_dirty(cx) {
            then(self, window, cx);
            return;
        }
        let path = self
            .editor
            .as_ref()
            .map(|e| e.read(cx).path().to_string())
            .unwrap_or_default();
        let rx = window.prompt(
            PromptLevel::Warning,
            "未保存の変更があります",
            Some(&path),
            &UNSAVED_ANSWERS,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = rx.await else { return };
            let _ = this.update_in(cx, |this, window, cx| match answer {
                0 => match this.save_now(cx, false) {
                    Ok(()) => then(this, window, cx),
                    Err(e) => this.report(e, cx),
                },
                1 => then(this, window, cx),
                _ => {}
            });
        })
        .detach();
    }

    /// Vault 内のファイルを開く。開いている文書は閉じる（同時に1つ。Spec §2-2）。
    pub fn open_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .editor
            .as_ref()
            .is_some_and(|e| e.read(cx).path() == path)
        {
            return;
        }
        self.confirm_discard(
            window,
            cx,
            Box::new(move |this, _window, cx| {
                if let Some(editor) = &this.editor {
                    let document = editor.read(cx).document();
                    if let Err(e) = this.api.execute(Command::CloseDocument { document }) {
                        return this.report(e, cx);
                    }
                }
                if let Err(e) = this.api.execute(Command::OpenDocument { path }) {
                    this.report(e, cx);
                }
            }),
        );
    }

    /// OS のパスのディレクトリを Vault として開く。
    pub fn open_vault_path(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(
            window,
            cx,
            Box::new(move |this, _window, cx| {
                if let Err(e) = this.api.execute(Command::OpenVault { path }) {
                    this.report(e, cx);
                }
            }),
        );
    }

    /// フォルダーの選択ダイアログで Vault を開く。
    pub fn prompt_open_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Vaultを開く".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| match path.to_str() {
                Some(p) => this.open_vault_path(p.to_string(), window, cx),
                None => this.report(
                    ApiError::new(ErrorCode::InvalidArgument, "path is not UTF-8"),
                    cx,
                ),
            });
        })
        .detach();
    }

    /// ウィンドウを閉じてよいか。未保存なら確認し、確認の後に閉じる。
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.is_dirty(cx) {
            return true;
        }
        self.confirm_discard(window, cx, Box::new(|_, window, _| window.remove_window()));
        false
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_editor)
            && let Some(editor) = &self.editor
        {
            window.focus(&editor.read(cx).focus_handle(cx), cx);
        }
        let border = rgb(0xd0d7de);
        let header = match &self.editor {
            Some(e) => {
                let e = e.read(cx);
                format!("{}{}", e.path(), if e.is_dirty() { " ●" } else { "" })
            }
            None => self.title(),
        };
        let files = self.files().map(str::to_string).collect::<Vec<_>>();
        let current = self.editor.as_ref().map(|e| e.read(cx).path().to_string());
        div()
            .key_context("Workspace")
            .when_some(self.font_family.clone(), |d, f| d.font_family(f))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save(window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenVault, window, cx| this.prompt_open_vault(window, cx)),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x1f2328))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .items_center()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .id("open-vault")
                            .debug_selector(|| "open-vault".into())
                            .px_2()
                            .border_1()
                            .border_color(border)
                            .rounded_md()
                            .cursor_pointer()
                            .child("Vaultを開く")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prompt_open_vault(window, cx)
                            })),
                    )
                    .child(
                        div()
                            .id("header")
                            .debug_selector(|| "header".into())
                            .child(header),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("files")
                            .w(px(240.))
                            .flex_none()
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(border)
                            .py_1()
                            .children(files.into_iter().enumerate().map(|(i, path)| {
                                let selected = current.as_deref() == Some(path.as_str());
                                div()
                                    .id(("file", i))
                                    .px_3()
                                    .cursor_pointer()
                                    .when(selected, |d| d.bg(rgb(0xddf4ff)))
                                    .hover(|d| d.bg(rgb(0xf6f8fa)))
                                    .child(path.clone())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_file(path.clone(), window, cx)
                                    }))
                            }))
                            .when(self.skipped > 0, |d| {
                                d.child(div().px_3().text_color(rgb(0x6e7781)).child(format!(
                                    "{} 件を一覧から除外しました（名前が対応外）",
                                    self.skipped
                                )))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .map(|d| match &self.editor {
                                Some(e) => d.child(e.clone()),
                                None => d.p_4().text_color(rgb(0x6e7781)).child(
                                    if self.vault_root.is_some() {
                                        "ファイルを選択してください"
                                    } else {
                                        "Vaultを開いてください（Ctrl+O）"
                                    },
                                ),
                            }),
                    ),
            )
            .child(
                div()
                    .id("status")
                    .debug_selector(|| "status".into())
                    .px_3()
                    .border_t_1()
                    .border_color(border)
                    .min_h(px(22.))
                    .child(self.status.clone()),
            )
    }
}
