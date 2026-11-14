//! GPUIの Platform のAndroid向け実装（検証用）。iOSの実装（gpui_ios）を雛形にした。

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use android_activity::input::{InputEvent, KeyAction, KeyMapChar, Keycode, MotionAction};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use anyhow::anyhow;
use futures::channel::oneshot;
use gpui::{
    Action, AnyWindowHandle, BackgroundExecutor, ClipboardItem, CursorStyle, DummyKeyboardMapper, ForegroundExecutor,
    Keymap, Menu, MenuItem, PathPromptOptions, Platform, PlatformDisplay, PlatformInput, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Result, Task, ThermalState, TouchEvent, TouchId,
    TouchPhase, WindowAppearance, WindowParams, point, px,
};
use gpui_wgpu::{CosmicTextSystem, GpuContext};

use crate::dispatcher::AndroidDispatcher;
use crate::window::{AndroidWindow, RawWindow};

pub struct AndroidPlatform(Rc<State>);

struct State {
    app: AndroidApp,
    dispatcher: Arc<AndroidDispatcher>,
    background: BackgroundExecutor,
    foreground: ForegroundExecutor,
    text_system: Arc<CosmicTextSystem>,
    gpu: GpuContext,
    window: RefCell<Option<AndroidWindow>>,
    quit: Cell<bool>,
    /// IME の編集バッファ（GameTextInput）のうち、確定として GPUI に渡し済みのバイト数
    ime_committed: Cell<usize>,
    /// GPUI に変換中の文字列を渡しているか
    ime_marked: Cell<bool>,
}

/// Java の文字列の位置（UTF-16）を UTF-8 のバイト位置に変換する。
fn utf16_to_byte(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= u {
            return i;
        }
        n += c.len_utf16();
    }
    s.len()
}

/// Android の画面密度（dpi）から GPUI の倍率を求める（160 dpi = 1.0）。
fn scale_of(app: &AndroidApp) -> f32 {
    app.config().density().map(|d| d as f32 / 160.0).unwrap_or(1.0)
}

impl AndroidPlatform {
    pub fn new(app: AndroidApp) -> Self {
        let dispatcher = AndroidDispatcher::new(app.create_waker());
        let text_system = Arc::new(CosmicTextSystem::new("Roboto"));
        // システムフォントを明示的に読み込む（fontdb のシステムフォントの探索が Android の場所を含むか未確認のため）
        let mut fonts: Vec<Cow<'static, [u8]>> = vec![];
        for f in ["Roboto-Regular.ttf", "NotoSansCJK-Regular.ttc", "NotoColorEmoji.ttf", "NotoNaskhArabic-Regular.ttf"] {
            match std::fs::read(format!("/system/fonts/{f}")) {
                Ok(b) => fonts.push(Cow::Owned(b)),
                Err(e) => log::warn!("font {f}: {e}"),
            }
        }
        if let Err(e) = gpui::PlatformTextSystem::add_fonts(text_system.as_ref(), fonts) {
            log::warn!("add_fonts: {e:#}");
        }
        Self(Rc::new(State {
            app,
            background: BackgroundExecutor::new(dispatcher.clone()),
            foreground: ForegroundExecutor::new(dispatcher.clone()),
            dispatcher,
            text_system,
            gpu: Default::default(),
            window: RefCell::new(None),
            quit: Cell::new(false),
            ime_committed: Cell::new(0),
            ime_marked: Cell::new(false),
        }))
    }

    fn handle_input(&self, w: &AndroidWindow) {
        let app = &self.0.app;
        let scale = w.0.borrow().scale;
        let Ok(mut iter) = app.input_events_iter() else { return };
        let mut events: Vec<PlatformInput> = vec![];
        let mut texts: Vec<String> = vec![];
        let mut ime_states: Vec<android_activity::input::TextInputState> = vec![];
        let mut show_keyboard = false;
        loop {
            let more = iter.next(|event| {
                match event {
                    InputEvent::MotionEvent(m) => {
                        let phase = match m.action() {
                            MotionAction::Down | MotionAction::PointerDown => Some(TouchPhase::Started),
                            MotionAction::Move => Some(TouchPhase::Moved),
                            MotionAction::Up | MotionAction::PointerUp => Some(TouchPhase::Ended),
                            MotionAction::Cancel => Some(TouchPhase::Cancelled),
                            _ => None,
                        };
                        if let Some(phase) = phase {
                            let idx = match phase {
                                TouchPhase::Moved => None,
                                _ => Some(m.pointer_index()),
                            };
                            for p in m.pointers() {
                                if idx.is_some_and(|i| i != p.pointer_index()) {
                                    continue;
                                }
                                let position = point(px(p.x() / scale), px(p.y() / scale));
                                events.push(PlatformInput::Touch(TouchEvent {
                                    id: TouchId(p.pointer_id() as u64),
                                    phase,
                                    position,
                                    predicted_position: None,
                                    force: None,
                                }));
                            }
                            if phase == TouchPhase::Ended {
                                show_keyboard = true;
                            }
                        }
                        InputStatus::Handled
                    }
                    InputEvent::KeyEvent(k) => {
                        if k.action() == KeyAction::Down {
                            if let Ok(map) = app.device_key_character_map(k.device_id()) {
                                if let Ok(KeyMapChar::Unicode(c)) = map.get(k.key_code(), k.meta_state()) {
                                    texts.push(c.to_string());
                                    return InputStatus::Handled;
                                }
                            }
                            // 文字を持たないキーは GPUI のキーストロークとして渡す（キーバインドで処理される）
                            let name = match k.key_code() {
                                Keycode::Del => Some("backspace"),
                                Keycode::ForwardDel => Some("delete"),
                                Keycode::Enter | Keycode::NumpadEnter => Some("enter"),
                                Keycode::DpadLeft => Some("left"),
                                Keycode::DpadRight => Some("right"),
                                Keycode::MoveHome => Some("home"),
                                Keycode::MoveEnd => Some("end"),
                                _ => None,
                            };
                            if let Some(name) = name
                                && let Ok(keystroke) = gpui::Keystroke::parse(name)
                            {
                                events.push(PlatformInput::KeyDown(gpui::KeyDownEvent { keystroke, is_held: false, prefer_character_input: false }));
                                return InputStatus::Handled;
                            }
                            log::info!("key without char: {:?}", k.key_code());
                        }
                        InputStatus::Unhandled
                    }
                    InputEvent::TextEvent(state) => {
                        // GameActivity の場合のみ届く（NativeActivity では届かない）
                        log::info!("ime state: text={:?} sel={:?} compose={:?}", state.text, state.selection, state.compose_region);
                        ime_states.push(state.clone());
                        InputStatus::Handled
                    }
                    _ => InputStatus::Unhandled,
                }
            });
            if !more {
                break;
            }
        }
        for e in events {
            if let PlatformInput::Touch(t) = &e {
                w.0.borrow_mut().mouse = t.position;
            }
            w.dispatch(e);
        }
        for t in texts {
            w.commit_text(&t);
        }
        for st in ime_states {
            self.apply_ime(w, st);
        }
        if show_keyboard {
            app.show_soft_input(true);
        }
    }
}

impl AndroidPlatform {
    /// IME の編集バッファの状態を、GPUI の確定（replace_text_in_range）と変換中（replace_and_mark_text_in_range）に変換する。
    fn apply_ime(&self, w: &AndroidWindow, st: android_activity::input::TextInputState) {
        let text = st.text.as_str();
        let compose = st.compose_region.map(|r| {
            let (a, b) = (r.start.min(r.end), r.start.max(r.end));
            (utf16_to_byte(text, a), utf16_to_byte(text, b))
        });
        let commit_end = compose.map_or(text.len(), |c| c.0);
        let done = self.0.ime_committed.get().min(text.len());
        if commit_end > done && text.is_char_boundary(done) {
            let s = &text[done..commit_end];
            log::info!("ime commit {s:?}");
            w.commit_text(s);
            self.0.ime_marked.set(false);
            self.0.ime_committed.set(commit_end);
        }
        match compose {
            Some((a, b)) if a < b => {
                w.set_marked_text(&text[a..b]);
                self.0.ime_marked.set(true);
            }
            _ => {
                if self.0.ime_marked.get() {
                    // 変換が取り消された（変換中の文字列が空になった）
                    w.set_marked_text("");
                    self.0.ime_marked.set(false);
                }
                // 変換が終わったら IME のバッファを空に戻す（次の入力が二重に確定しないように）
                if !text.is_empty() {
                    self.0.app.set_text_input_state(android_activity::input::TextInputState::default());
                }
                self.0.ime_committed.set(0);
            }
        }
    }
}

struct Layout;
impl PlatformKeyboardLayout for Layout {
    fn id(&self) -> &str {
        "android"
    }
    fn name(&self) -> &str {
        "Android"
    }
}

impl Platform for AndroidPlatform {
    fn platform_info(&self) -> gpui::PlatformInfo {
        gpui::PlatformInfo { os: gpui::OperatingSystem::Linux, target: gpui::PlatformTarget::Native }
    }
    fn background_executor(&self) -> BackgroundExecutor {
        self.0.background.clone()
    }
    fn foreground_executor(&self) -> ForegroundExecutor {
        self.0.foreground.clone()
    }
    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.0.text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        let mut on_finish = Some(on_finish_launching);
        let app = self.0.app.clone();
        while !self.0.quit.get() {
            let next_timer = self.0.dispatcher.run_main_tasks();
            let has_window = self.0.window.borrow().is_some();
            let timeout = if has_window {
                Some(next_timer.map_or(Duration::from_millis(16), |t| t.min(Duration::from_millis(16))))
            } else {
                next_timer.or(Some(Duration::from_millis(100)))
            };
            let mut init_window = false;
            let mut resized = false;
            let mut destroy = false;
            app.poll_events(timeout, |event| {
                if let PollEvent::Main(m) = event {
                    match m {
                        MainEvent::InitWindow { .. } => init_window = true,
                        MainEvent::WindowResized { .. } | MainEvent::ConfigChanged { .. } => resized = true,
                        MainEvent::TerminateWindow { .. } => log::warn!("TerminateWindow（表面の再作成は未実装）"),
                        MainEvent::Destroy => destroy = true,
                        _ => {}
                    }
                }
            });
            if destroy {
                self.0.quit.set(true);
                break;
            }
            if init_window && let Some(f) = on_finish.take() {
                f();
            }
            let w = self.0.window.borrow().clone();
            if let Some(w) = w {
                if resized && let Some(nw) = app.native_window() {
                    w.resized(nw.width(), nw.height());
                }
                self.handle_input(&w);
                w.frame();
            }
        }
    }

    fn quit(&self) {
        self.0.quit.set(true);
    }
    fn restart(&self, _binary_path: Option<PathBuf>, _arguments: Vec<std::ffi::OsString>) {}
    fn activate(&self, _ignoring_other_apps: bool) {}
    fn hide(&self) {}
    fn hide_other_apps(&self) {}
    fn unhide_other_apps(&self) {}
    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![]
    }
    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        None
    }
    fn on_displays_changed(&self, _callback: Box<dyn FnMut()>) {}
    fn active_window(&self) -> Option<AnyWindowHandle> {
        None
    }
    fn open_window(&self, _handle: AnyWindowHandle, options: WindowParams) -> anyhow::Result<Box<dyn PlatformWindow>> {
        let nw = self.0.app.native_window().ok_or_else(|| anyhow!("native window is not ready"))?;
        let raw = RawWindow(nw.ptr().as_ptr().cast());
        let w = AndroidWindow::new(options, raw, nw.width(), nw.height(), scale_of(&self.0.app), self.0.gpu.clone())?;
        log::info!("window {}x{} scale {}", nw.width(), nw.height(), scale_of(&self.0.app));
        // NativeWindow の参照を保持する（ANativeWindow の解放を防ぐ）
        std::mem::forget(nw);
        self.0.window.replace(Some(w.clone()));
        Ok(Box::new(w))
    }
    fn window_appearance(&self) -> WindowAppearance {
        WindowAppearance::Light
    }
    fn open_url(&self, _url: &str) {}
    fn on_open_urls(&self, _callback: Box<dyn FnMut(Vec<String>)>) {}
    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }
    fn prompt_for_paths(&self, _options: PathPromptOptions) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Err(anyhow!("not implemented")));
        rx
    }
    fn prompt_for_new_path(&self, _directory: &Path, _suggested_name: Option<&str>) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Err(anyhow!("not implemented")));
        rx
    }
    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }
    fn reveal_path(&self, _path: &Path) {}
    fn open_with_system(&self, _path: &Path) {}
    fn on_quit(&self, _callback: Box<dyn FnMut() -> bool>) {}
    fn on_reopen(&self, _callback: Box<dyn FnMut()>) {}
    fn on_system_sleep(&self, _callback: Box<dyn FnMut()>) {}
    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}
    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}
    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}
    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {}
    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}
    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}
    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }
    fn on_thermal_state_change(&self, _callback: Box<dyn FnMut()>) {}
    fn prevent_idle_sleep(&self, _reason: &str) -> Task<Result<gpui::ActivityGuard>> {
        Task::ready(Err(anyhow!("not implemented")))
    }
    fn app_path(&self) -> Result<PathBuf> {
        Err(anyhow!("not implemented"))
    }
    fn path_for_auxiliary_executable(&self, _name: &str) -> Result<PathBuf> {
        Err(anyhow!("not implemented"))
    }
    fn set_cursor_style(&self, _style: CursorStyle) {}
    fn hide_cursor_until_mouse_moves(&self) {}
    fn is_cursor_visible(&self) -> bool {
        false
    }
    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }
    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        None
    }
    fn write_to_clipboard(&self, _item: ClipboardItem) {}
    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("not implemented")))
    }
    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Ok(None))
    }
    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }
    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(Layout)
    }
    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }
    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}
}
