//! エディター（Spec: editor-ui-m2 §4）。文書の内容はスナップショットで読み、操作は Command で送る。

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use app_api::{
    ApiError, Command, DocumentId, EditCommand, ErrorCode, Event, Movement, Query, QueryResponse,
    Selection, ViewId, ViewState,
};
use app_core::{LocalAppApi, TextSnapshot};
use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClipboardItem, Context, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, ListAlignment, ListState,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, UTF16Selection,
    Window, actions, canvas, div, list, point, prelude::*, px, rgb, rgba, size,
};

use crate::display::{self, DisplayLine};
use crate::line::{LayoutCache, LineElement, LineStyle};

actions!(
    editor,
    [
        Backspace,
        Delete,
        Newline,
        Tab,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DocumentStart,
        DocumentEnd,
        SelectDocumentStart,
        SelectDocumentEnd,
        SelectAll,
        Undo,
        Redo,
        Copy,
        Cut,
        Paste,
    ]
);

/// 暫定の文字の大きさと行の高さ（Spec §8-2）。
const FONT_SIZE: f32 = 15.;
const LINE_HEIGHT: f32 = 22.;

/// エディターから Workspace への通知。
#[derive(Debug, Clone)]
pub enum EditorEvent {
    Error(ApiError),
}

pub struct Editor {
    api: Arc<dyn LocalAppApi>,
    document: DocumentId,
    view: ViewId,
    path: String,
    snapshot: TextSnapshot,
    state: ViewState,
    dirty: bool,
    list: ListState,
    focus: FocusHandle,
    cache: Rc<RefCell<LayoutCache>>,
    /// 上下の移動の目標の横位置（§4.3）。
    goal_x: Option<Pixels>,
    /// ドラッグ中の選択の起点。
    drag_anchor: Option<usize>,
}

impl EventEmitter<EditorEvent> for Editor {}

fn unexpected() -> ApiError {
    ApiError::new(ErrorCode::Internal, "unexpected query response")
}

impl Editor {
    /// 初期の状態を Core から読み、エディターを作る。
    pub fn build(
        api: Arc<dyn LocalAppApi>,
        document: DocumentId,
        view: ViewId,
        path: String,
        cx: &mut App,
    ) -> Result<Entity<Self>, ApiError> {
        let snapshot = api
            .document_snapshot(document)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "document is not open"))?;
        let QueryResponse::ViewState(state) = api.query(Query::ViewState { document, view })?
        else {
            return Err(unexpected());
        };
        let QueryResponse::DocumentInfo(info) = api.query(Query::DocumentInfo { document })? else {
            return Err(unexpected());
        };
        Ok(cx.new(|cx| Self {
            list: ListState::new(snapshot.line_count(), ListAlignment::Top, px(400.)),
            api,
            document,
            view,
            path,
            snapshot,
            state,
            dirty: info.dirty,
            focus: cx.focus_handle(),
            cache: Rc::default(),
            goal_x: None,
            drag_anchor: None,
        }))
    }

    pub fn document(&self) -> DocumentId {
        self.document
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// 表示中の内容が未保存か（Event から更新する）。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 論理行の表示の内容（表示に使うものと同じ）。
    pub fn display_line(&self, line: usize) -> Option<DisplayLine> {
        display::display_line(&self.snapshot, &self.state, line)
    }

    /// 表示中の論理行の数。
    pub fn line_count(&self) -> usize {
        self.list.item_count()
    }

    /// 文書の Event を表示に反映する。
    pub fn apply(&mut self, event: &Event, cx: &mut Context<Self>) {
        match event {
            Event::DocumentChanged { version, edits, .. } => {
                let Some(latest) = self.api.document_snapshot(self.document) else {
                    return;
                };
                if latest.version() <= self.snapshot.version() {
                    return;
                }
                let old = &self.snapshot;
                match edits.as_slice() {
                    [e] if latest.version() == *version => {
                        let lines = (
                            old.byte_to_line(e.start),
                            old.byte_to_line(e.start + e.old_len),
                            latest.byte_to_line(e.start + e.new_len),
                        );
                        match lines {
                            (Ok(first), Ok(old_last), Ok(new_last)) => {
                                self.list.splice(first..old_last + 1, new_last - first + 1)
                            }
                            _ => self.list.reset(latest.line_count()),
                        }
                    }
                    _ => self.list.reset(latest.line_count()),
                }
                self.snapshot = latest;
                self.refresh_state();
            }
            Event::ViewStateChanged { .. } => {
                let before = self.composition_lines();
                self.refresh_state();
                // 変換中の文字列の表示で行の高さが変わる
                for r in [before, self.composition_lines()].into_iter().flatten() {
                    self.list.remeasure_items(r);
                }
                if let Some(s) = self.state.selections.get(self.state.primary)
                    && let Ok(line) = self.snapshot.byte_to_line(s.head)
                {
                    self.list.scroll_to_reveal_item(line);
                }
            }
            Event::DirtyChanged { dirty, .. } => self.dirty = *dirty,
            _ => return,
        }
        cx.notify();
    }

    fn composition_lines(&self) -> Option<Range<usize>> {
        let c = self.state.composition.as_ref()?;
        let first = self.snapshot.byte_to_line(c.range.start).ok()?;
        let last = self.snapshot.byte_to_line(c.range.end).ok()?;
        Some(first..(last + 1).min(self.list.item_count()))
    }

    fn refresh_state(&mut self) {
        if let Some(latest) = self.api.document_snapshot(self.document) {
            self.snapshot = latest;
        }
        if let Ok(s) = self.current_state() {
            self.state = s;
        }
    }

    /// Core の現在のビューの状態（Event の到着を待たない）。
    fn current_state(&self) -> Result<ViewState, ApiError> {
        match self.api.query(Query::ViewState {
            document: self.document,
            view: self.view,
        })? {
            QueryResponse::ViewState(s) => Ok(s),
            _ => Err(unexpected()),
        }
    }

    fn query(&self, q: Query, cx: &mut Context<Self>) -> Option<QueryResponse> {
        self.api
            .query(q)
            .map_err(|e| cx.emit(EditorEvent::Error(e)))
            .ok()
    }

    pub fn run(&mut self, command: EditCommand, cx: &mut Context<Self>) {
        let r = self.api.execute(Command::Edit {
            document: self.document,
            view: self.view,
            command,
        });
        if let Err(e) = r {
            cx.emit(EditorEvent::Error(e));
        }
    }

    fn act(&mut self, command: EditCommand, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.run(command, cx);
    }

    fn move_cursor(&mut self, movement: Movement, extend: bool, cx: &mut Context<Self>) {
        self.act(EditCommand::MoveCursor { movement, extend }, cx);
    }

    fn set_cursor(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        self.run(
            EditCommand::SetSelections {
                selections: vec![Selection { anchor, head }],
                primary: 0,
            },
            cx,
        );
    }

    /// 表示上の上下の行への移動（§4.3）。
    fn move_vertically(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
        let Ok(state) = self.current_state() else {
            return;
        };
        if let Some(c) = state.composition {
            self.run(EditCommand::CommitComposition { text: c.text }, cx);
            return;
        }
        let Some(snapshot) = self.api.document_snapshot(self.document) else {
            return;
        };
        let Some(sel) = state.selections.get(state.primary).copied() else {
            return;
        };
        let Ok(line) = snapshot.byte_to_line(sel.head) else {
            return;
        };
        let lh = px(LINE_HEIGHT);
        let cache = self.cache.borrow();
        let here = cache.lines.get(&line);
        let p = here
            .map(|l| l.position(l.line.buffer_to_display(sel.head), lh))
            .unwrap_or_default();
        let x = self.goal_x.unwrap_or(p.x);
        let target_y = if down { p.y + lh } else { p.y - lh };
        let target = match here {
            Some(l) if target_y >= px(0.) && target_y < l.height => l
                .line
                .display_to_buffer(l.index_at(point(x, target_y + lh / 2.), lh)),
            _ => {
                let next = if down {
                    line.checked_add(1).filter(|&n| n < snapshot.line_count())
                } else {
                    line.checked_sub(1)
                };
                match next {
                    None if down => snapshot.len_bytes(),
                    None => 0,
                    Some(n) => match cache.lines.get(&n) {
                        Some(l) => {
                            let y = if down { lh / 2. } else { l.height - lh / 2. };
                            l.line.display_to_buffer(l.index_at(point(x, y), lh))
                        }
                        None => snapshot.line_range(n).map(|r| r.start).unwrap_or(0),
                    },
                }
            }
        };
        drop(cache);
        let anchor = if extend { sel.anchor } else { target };
        self.set_cursor(anchor, target, cx);
        self.goal_x = Some(x);
    }

    fn selected_text(&self) -> Option<String> {
        let state = self.current_state().ok()?;
        let s = state.selections.get(state.primary)?;
        let range = s.anchor.min(s.head)..s.anchor.max(s.head);
        if range.is_empty() {
            return None;
        }
        self.api.document_snapshot(self.document)?.slice(range).ok()
    }

    /// ウィンドウ座標の位置に最も近いバッファの位置。描画した行が無い場合は `None`。
    fn buffer_offset_at(&self, p: Point<Pixels>) -> Option<usize> {
        let cache = self.cache.borrow();
        let lh = px(LINE_HEIGHT);
        let mut lines: Vec<_> = cache.lines.iter().filter(|(_, l)| !l.line.hidden).collect();
        lines.sort_by_key(|(i, _)| **i);
        let (_, first) = lines.first()?;
        let (_, last) = lines.last()?;
        let l = if p.y < first.origin.y {
            first
        } else {
            lines
                .iter()
                .map(|(_, l)| l)
                .find(|l| p.y >= l.origin.y && p.y < l.origin.y + l.height)
                .unwrap_or(last)
        };
        let local = point(
            p.x - l.origin.x,
            (p.y - l.origin.y).clamp(px(0.), (l.height - px(1.)).max(px(0.))),
        );
        Some(l.line.display_to_buffer(l.index_at(local, lh)))
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(offset) = self.buffer_offset_at(e.position) else {
            return;
        };
        let anchor = if e.modifiers.shift {
            self.current_state()
                .ok()
                .and_then(|s| s.selections.get(s.primary).map(|s| s.anchor))
                .unwrap_or(offset)
        } else {
            offset
        };
        self.drag_anchor = Some(anchor);
        self.goal_x = None;
        self.set_cursor(anchor, offset, cx);
    }

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let (Some(anchor), true) = (self.drag_anchor, e.dragging())
            && let Some(offset) = self.buffer_offset_at(e.position)
        {
            self.set_cursor(anchor, offset, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag_anchor = None;
    }

    fn render_line(
        &mut self,
        ix: usize,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(line) = self.display_line(ix) else {
            return div().into_any_element();
        };
        let style = LineStyle {
            font_size: px(FONT_SIZE),
            line_height: px(LINE_HEIGHT),
            text: rgb(0x1f2328).into(),
            selection: rgba(0x0969da40).into(),
            cursor: rgb(0x0969da).into(),
            focused: self.focus.is_focused(window),
        };
        LineElement {
            index: ix,
            line,
            style,
            cache: self.cache.clone(),
        }
        .into_any_element()
    }

    /// 入力用の位置（UTF-16）の、描画した行と表示上の位置。
    fn input_to_display(&self, u: usize) -> Option<(usize, usize)> {
        let state = self.current_state().ok()?;
        let snapshot = self.api.document_snapshot(self.document)?;
        if let Some(c) = &state.composition {
            let start16 = snapshot.byte_to_utf16(c.range.start).ok()?;
            let len16 = c.text.encode_utf16().count();
            if u >= start16 && u <= start16 + len16 {
                let line = snapshot.byte_to_line(c.range.start).ok()?;
                let cs = c.range.start - snapshot.line_range(line).ok()?.start;
                return Some((line, cs + utf16_to_byte_in(&c.text, u - start16)));
            }
        }
        let QueryResponse::Offset(b) = self
            .api
            .query(Query::InputToBuffer {
                document: self.document,
                view: self.view,
                offset: u,
            })
            .ok()?
        else {
            return None;
        };
        let line = snapshot.byte_to_line(b).ok()?;
        let cache = self.cache.borrow();
        Some((line, cache.lines.get(&line)?.line.buffer_to_display(b)))
    }

    fn input_query(&self, q: Query) -> Option<QueryResponse> {
        self.api.query(q).ok()
    }
}

/// 文字列の中の UTF-16 位置をバイト位置にする（文字の途中は次の文字境界）。
fn utf16_to_byte_in(s: &str, u: usize) -> usize {
    let mut n16 = 0;
    for (i, ch) in s.char_indices() {
        if n16 >= u {
            return i;
        }
        n16 += ch.len_utf16();
    }
    s.len()
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let QueryResponse::Text(s) = self.query(
            Query::InputTextForRange {
                document: self.document,
                view: self.view,
                range: range.clone(),
            },
            cx,
        )?
        else {
            return None;
        };
        actual.replace(range.start..range.start + s.encode_utf16().count());
        Some(s)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        match self.query(
            Query::InputSelectedRange {
                document: self.document,
                view: self.view,
            },
            cx,
        )? {
            QueryResponse::SelectedRange { range, reversed } => {
                Some(UTF16Selection { range, reversed })
            }
            _ => None,
        }
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        match self.input_query(Query::InputMarkedRange {
            document: self.document,
            view: self.view,
        })? {
            QueryResponse::MarkedRange(r) => r,
            _ => None,
        }
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        // 変換中の文字列をそのまま確定する
        if let Ok(ViewState {
            composition: Some(c),
            ..
        }) = self.current_state()
        {
            self.run(EditCommand::CommitComposition { text: c.text }, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.goal_x = None;
        let composing = self.current_state().is_ok_and(|s| s.composition.is_some());
        if let (Some(r), false) = (range, composing) {
            self.select_input_range(r, cx);
        }
        self.run(
            EditCommand::CommitComposition {
                text: text.to_string(),
            },
            cx,
        );
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() {
            self.run(EditCommand::CancelComposition, cx);
            return;
        }
        let composing = self.current_state().is_ok_and(|s| s.composition.is_some());
        if let (Some(r), false) = (range, composing) {
            self.select_input_range(r, cx);
        }
        let selected =
            selected.map(|r| utf16_to_byte_in(text, r.start)..utf16_to_byte_in(text, r.end));
        self.run(
            EditCommand::SetComposition {
                text: text.to_string(),
                selected,
            },
            cx,
        );
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let lh = px(LINE_HEIGHT);
        let (line, start) = self.input_to_display(range.start)?;
        let end = self
            .input_to_display(range.end)
            .filter(|(l, _)| *l == line)
            .map(|(_, d)| d)
            .unwrap_or(start);
        let cache = self.cache.borrow();
        let l = cache.lines.get(&line)?;
        let (a, b) = (l.position(start, lh), l.position(end, lh));
        let width = if a.y == b.y {
            (b.x - a.x).max(px(2.))
        } else {
            px(2.)
        };
        Some(Bounds::new(l.origin + a, size(width, lh)))
    }

    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let b = self.buffer_offset_at(p)?;
        match self.query(
            Query::BufferToInput {
                document: self.document,
                view: self.view,
                offset: b,
            },
            cx,
        )? {
            QueryResponse::Offset(u) => Some(u),
            _ => None,
        }
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let state = self.current_state().ok()?;
        Some(display::input_len_utf16(
            &self.api.document_snapshot(self.document)?,
            &state,
        ))
    }
}

impl Editor {
    /// 入力用の範囲（UTF-16）を選択する（変換中でない場合のみ呼ぶ）。
    fn select_input_range(&mut self, r: Range<usize>, cx: &mut Context<Self>) {
        let to_buffer = |this: &Self, offset| match this.api.query(Query::InputToBuffer {
            document: this.document,
            view: this.view,
            offset,
        }) {
            Ok(QueryResponse::Offset(b)) => Some(b),
            _ => None,
        };
        if let (Some(a), Some(b)) = (to_buffer(self, r.start), to_buffer(self, r.end)) {
            self.set_cursor(a, b, cx);
        }
    }
}

macro_rules! handlers {
    ($el:expr, $cx:expr, { $($action:ident => |$this:ident, $c:ident| $body:expr),* $(,)? }) => {
        $el$(.on_action($cx.listener(|$this: &mut Editor, _: &$action, _: &mut Window, $c: &mut Context<Editor>| $body)))*
    };
}

impl Render for Editor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cache = self.cache.clone();
        let focus = self.focus.clone();
        let entity = cx.entity();
        let el = div()
            .id("editor")
            .key_context("Editor")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .px_3()
            .bg(rgb(0xffffff))
            .cursor_text()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up));
        let el = handlers!(el, cx, {
            Backspace => |this, cx| this.act(EditCommand::DeleteBackward, cx),
            Delete => |this, cx| this.act(EditCommand::DeleteForward, cx),
            Newline => |this, cx| this.act(EditCommand::InsertNewline, cx),
            Tab => |this, cx| this.act(EditCommand::InsertText { text: "\t".into() }, cx),
            Left => |this, cx| this.move_cursor(Movement::Left, false, cx),
            Right => |this, cx| this.move_cursor(Movement::Right, false, cx),
            SelectLeft => |this, cx| this.move_cursor(Movement::Left, true, cx),
            SelectRight => |this, cx| this.move_cursor(Movement::Right, true, cx),
            Up => |this, cx| this.move_vertically(false, false, cx),
            Down => |this, cx| this.move_vertically(true, false, cx),
            SelectUp => |this, cx| this.move_vertically(false, true, cx),
            SelectDown => |this, cx| this.move_vertically(true, true, cx),
            LineStart => |this, cx| this.move_cursor(Movement::LineStart, false, cx),
            LineEnd => |this, cx| this.move_cursor(Movement::LineEnd, false, cx),
            SelectLineStart => |this, cx| this.move_cursor(Movement::LineStart, true, cx),
            SelectLineEnd => |this, cx| this.move_cursor(Movement::LineEnd, true, cx),
            DocumentStart => |this, cx| this.move_cursor(Movement::DocumentStart, false, cx),
            DocumentEnd => |this, cx| this.move_cursor(Movement::DocumentEnd, false, cx),
            SelectDocumentStart => |this, cx| this.move_cursor(Movement::DocumentStart, true, cx),
            SelectDocumentEnd => |this, cx| this.move_cursor(Movement::DocumentEnd, true, cx),
            SelectAll => |this, cx| this.act(EditCommand::SelectAll, cx),
            Undo => |this, cx| this.act(EditCommand::Undo, cx),
            Redo => |this, cx| this.act(EditCommand::Redo, cx),
            Copy => |this, cx| {
                if let Some(t) = this.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(t));
                }
            },
            Cut => |this, cx| {
                if let Some(t) = this.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(t));
                    this.act(EditCommand::DeleteSelection, cx);
                }
            },
            Paste => |this, cx| {
                if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    this.act(EditCommand::InsertText { text: t }, cx);
                }
            },
        });
        el.child(list(self.list.clone(), cx.processor(Self::render_line)).size_full())
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        cache.borrow_mut().finish_frame();
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}
