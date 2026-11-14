//! 文書セッション（`docs/spec/document-session.md`）。

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use text_buffer::{
    Bias, Change, Edit, History, HistoryId, Snapshot, TextBuffer, TextError, UndoGroup, map_offset,
};

use crate::{Composition, DocumentError, LineEnding, Movement, Selection, ViewId};

/// Undo の区切りの判定に使うコマンドの種類（Spec §5.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpKind {
    InsertText,
    DeleteBackward,
    DeleteForward,
}

#[derive(Debug, Clone)]
struct ViewState {
    selections: Vec<Selection>,
    primary: usize,
    composition: Option<Composition>,
}

/// 履歴の項目ごとの、編集したビューの編集前・編集後の選択範囲（Spec §5.6）。
#[derive(Debug, Clone)]
struct SelectionRecord {
    before: (Vec<Selection>, usize),
    after: (Vec<Selection>, usize),
}

/// 1つの文書の編集状態（Spec §1）。
#[derive(Debug, Clone)]
pub struct DocumentSession {
    buffer: TextBuffer,
    views: BTreeMap<ViewId, ViewState>,
    next_view: u64,
    line_ending: LineEnding,
    saved: HistoryId,
    records: HashMap<HistoryId, SelectionRecord>,
    /// 直前の操作が Undo の区切りの対象のコマンドだった場合の、そのビューと種類。
    last_op: Option<(ViewId, OpKind)>,
}

type Result<T> = std::result::Result<T, DocumentError>;

impl DocumentSession {
    /// `text` を内容とするセッションを作る。保存済みの状態は読み込んだ内容（Spec §8.1）。
    pub fn new(text: &str) -> Self {
        Self {
            buffer: TextBuffer::new(text),
            views: BTreeMap::new(),
            next_view: 0,
            line_ending: LineEnding::detect(text),
            saved: HistoryId::ROOT,
            records: HashMap::new(),
            last_op: None,
        }
    }

    // --- 参照 ---

    /// 現在の内容（変換中の文字列を含まない）。
    pub fn text(&self) -> &Snapshot {
        self.buffer.text()
    }

    pub fn version(&self) -> u64 {
        self.buffer.version()
    }

    pub fn history(&self) -> &History {
        self.buffer.history()
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn views(&self) -> impl Iterator<Item = ViewId> + '_ {
        self.views.keys().copied()
    }

    /// ビューの選択範囲の列（始まりの位置の順）。
    pub fn selections(&self, view: ViewId) -> Result<&[Selection]> {
        Ok(&self.view(view)?.selections)
    }

    /// ビューの主選択範囲。
    pub fn primary_selection(&self, view: ViewId) -> Result<Selection> {
        let st = self.view(view)?;
        Ok(st.selections[st.primary])
    }

    /// 主選択範囲の列の中の位置。
    pub fn primary_index(&self, view: ViewId) -> Result<usize> {
        Ok(self.view(view)?.primary)
    }

    pub fn composition(&self, view: ViewId) -> Result<Option<&Composition>> {
        Ok(self.view(view)?.composition.as_ref())
    }

    fn view(&self, view: ViewId) -> Result<&ViewState> {
        self.views
            .get(&view)
            .ok_or(DocumentError::UnknownView(view))
    }

    fn view_mut(&mut self, view: ViewId) -> &mut ViewState {
        self.views.get_mut(&view).expect("view checked by caller")
    }

    // --- ビュー（Spec §2） ---

    pub fn open_view(&mut self) -> ViewId {
        let id = ViewId(self.next_view);
        self.next_view += 1;
        self.views.insert(
            id,
            ViewState {
                selections: vec![Selection::cursor(0)],
                primary: 0,
                composition: None,
            },
        );
        id
    }

    /// ビューを閉じる。変換中の文字列は文書に入れない。
    pub fn close_view(&mut self, view: ViewId) -> Result<()> {
        self.views
            .remove(&view)
            .map(|_| ())
            .ok_or(DocumentError::UnknownView(view))
    }

    // --- 選択範囲（Spec §3、§5.4） ---

    /// 選択範囲の列を設定する。位置は呼び出し時点の内容（変換中の文字列を含まない）に対するもの。
    /// 変換中の場合は先に確定し、確定による変更で位置を補正する（Spec §6.2）。
    pub fn set_selections(
        &mut self,
        view: ViewId,
        selections: Vec<Selection>,
        primary: usize,
    ) -> Result<Vec<Change>> {
        self.view(view)?;
        if selections.is_empty() {
            return Err(DocumentError::EmptySelections);
        }
        if primary >= selections.len() {
            return Err(DocumentError::PrimaryOutOfRange {
                primary,
                len: selections.len(),
            });
        }
        for s in &selections {
            self.text().byte_to_char(s.anchor)?;
            self.text().byte_to_char(s.head)?;
        }
        let changes: Vec<Change> = self.commit_pending(view).into_iter().collect();
        let selections = selections
            .into_iter()
            .map(|s| {
                changes
                    .iter()
                    .fold(s, |s, c| map_selection(s, c, Bias::After))
            })
            .collect();
        self.set_view_selections(view, selections, primary);
        self.last_op = None;
        Ok(changes)
    }

    /// カーソルを移動する（Spec §5.4）。
    pub fn move_cursor(
        &mut self,
        view: ViewId,
        movement: Movement,
        extend: bool,
    ) -> Result<Vec<Change>> {
        self.view(view)?;
        let changes: Vec<Change> = self.commit_pending(view).into_iter().collect();
        let st = self.view(view)?;
        let t = self.buffer.text();
        let moved = st
            .selections
            .iter()
            .map(|&s| -> Result<Selection> {
                if !extend && !s.is_empty() {
                    match movement {
                        Movement::Left => return Ok(Selection::cursor(s.start())),
                        Movement::Right => return Ok(Selection::cursor(s.end())),
                        _ => {}
                    }
                }
                let target = match movement {
                    Movement::Left => t.prev_grapheme_boundary(s.head)?.unwrap_or(0),
                    Movement::Right => t.next_grapheme_boundary(s.head)?.unwrap_or(t.len_bytes()),
                    Movement::LineStart => t.line_range(t.byte_to_line(s.head)?)?.start,
                    Movement::LineEnd => t.line_range(t.byte_to_line(s.head)?)?.end,
                    Movement::DocumentStart => 0,
                    Movement::DocumentEnd => t.len_bytes(),
                };
                Ok(if extend {
                    Selection::new(s.anchor, target)
                } else {
                    Selection::cursor(target)
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let primary = st.primary;
        self.set_view_selections(view, moved, primary);
        self.last_op = None;
        Ok(changes)
    }

    /// 文書全体を選択する。
    pub fn select_all(&mut self, view: ViewId) -> Result<Vec<Change>> {
        self.view(view)?;
        let changes: Vec<Change> = self.commit_pending(view).into_iter().collect();
        let len = self.text().len_bytes();
        self.set_view_selections(view, vec![Selection::new(0, len)], 0);
        self.last_op = None;
        Ok(changes)
    }

    fn set_view_selections(&mut self, view: ViewId, selections: Vec<Selection>, primary: usize) {
        let (selections, primary) = normalize(selections, primary);
        let st = self.view_mut(view);
        st.selections = selections;
        st.primary = primary;
    }

    // --- 編集コマンド（Spec §5） ---

    /// 各選択範囲を `text` で置き換える（Spec §5.1）。改行コードは変換しない。
    pub fn insert_text(&mut self, view: ViewId, text: &str) -> Result<Vec<Change>> {
        if text.is_empty() {
            return self.delete_selection(view);
        }
        let kind = if text.contains(['\n', '\r']) {
            None
        } else {
            Some(OpKind::InsertText)
        };
        self.edit(view, kind, |_, s| Ok(Some((s.range(), text.to_string()))))
    }

    /// 文書の改行コードを入力する（Spec §5.2）。
    pub fn insert_newline(&mut self, view: ViewId) -> Result<Vec<Change>> {
        let nl = self.line_ending.as_str();
        self.edit(view, None, |_, s| Ok(Some((s.range(), nl.to_string()))))
    }

    /// Backspace（Spec §5.3）。
    pub fn delete_backward(&mut self, view: ViewId) -> Result<Vec<Change>> {
        self.edit(view, Some(OpKind::DeleteBackward), |t, s| {
            if !s.is_empty() {
                Ok(Some((s.range(), String::new())))
            } else if s.head == 0 {
                Ok(None)
            } else {
                Ok(Some((backspace_start(t, s.head)?..s.head, String::new())))
            }
        })
    }

    /// Delete（Spec §5.3）。
    pub fn delete_forward(&mut self, view: ViewId) -> Result<Vec<Change>> {
        self.edit(view, Some(OpKind::DeleteForward), |t, s| {
            if !s.is_empty() {
                return Ok(Some((s.range(), String::new())));
            }
            Ok(t.next_grapheme_boundary(s.head)?
                .map(|n| (s.head..n, String::new())))
        })
    }

    /// 空でない選択範囲を削除する（Spec §5.3）。
    pub fn delete_selection(&mut self, view: ViewId) -> Result<Vec<Change>> {
        self.edit(view, None, |_, s| {
            Ok((!s.is_empty()).then(|| (s.range(), String::new())))
        })
    }

    /// 編集コマンドの共通部分（Spec §5、§5.5、§5.6）。
    ///
    /// `op` は各選択範囲に対する範囲置換（`None` は何もしない）を返す。範囲は隣の選択範囲と重ならないように
    /// 切り詰め、後ろの選択範囲から順に1つのトランザクションとして適用する。
    fn edit(
        &mut self,
        view: ViewId,
        kind: Option<OpKind>,
        op: impl Fn(
            &Snapshot,
            Selection,
        ) -> std::result::Result<Option<(Range<usize>, String)>, TextError>,
    ) -> Result<Vec<Change>> {
        self.view(view)?;
        let mut changes: Vec<Change> = self.commit_pending(view).into_iter().collect();
        let st = self.view(view)?;
        let (sels, primary) = (st.selections.clone(), st.primary);
        let t = self.buffer.text();
        let len = t.len_bytes();

        let mut ops = Vec::with_capacity(sels.len());
        let mut prev_end = 0;
        for (i, s) in sels.iter().enumerate() {
            let next_start = sels.get(i + 1).map_or(len, |n| n.start());
            let o = op(t, *s)?.map(|(r, ins)| {
                let start = r.start.max(prev_end);
                (start..r.end.min(next_start).max(start), ins)
            });
            prev_end = o.as_ref().map_or(s.end(), |(r, _)| r.end.max(s.end()));
            ops.push(o);
        }

        let all_empty = sels.iter().all(Selection::is_empty);
        let kind = kind.filter(|_| all_empty);
        let group = match kind {
            Some(k) if self.last_op == Some((view, k)) => UndoGroup::MergeWithPrevious,
            _ => UndoGroup::NewGroup,
        };
        let edits: Vec<Edit> = ops
            .iter()
            .rev()
            .flatten()
            .map(|(r, ins)| Edit::new(r.clone(), ins.clone()))
            .collect();
        let id_before = self.buffer.history().current();
        let Some(change) = self.buffer.apply(edits, group)? else {
            return Ok(changes);
        };

        // 操作したビューの新しい選択範囲。前の選択範囲の置換による位置のずれを積み上げる。
        let mut delta: isize = 0;
        let shift = |x: usize, d: isize| (x as isize + d) as usize;
        let new_sels: Vec<Selection> = sels
            .iter()
            .zip(&ops)
            .map(|(s, o)| match o {
                Some((r, ins)) => {
                    let pos = shift(r.start, delta) + ins.len();
                    delta += ins.len() as isize - r.len() as isize;
                    Selection::cursor(pos)
                }
                None => Selection::new(shift(s.anchor, delta), shift(s.head, delta)),
            })
            .collect();
        self.map_other_views(&change, Some(view));
        self.set_view_selections(view, new_sels, primary);
        self.record(view, id_before, (sels, primary));
        self.last_op = kind.map(|k| (view, k));
        changes.push(change);
        Ok(changes)
    }

    /// 編集後の履歴の項目に、選択範囲を記録する（Spec §5.6）。
    fn record(&mut self, view: ViewId, id_before: HistoryId, before: (Vec<Selection>, usize)) {
        let st = self.view(view).expect("view checked by caller");
        let after = (st.selections.clone(), st.primary);
        let id = self.buffer.history().current();
        match self.records.get_mut(&id) {
            // 同じ項目にまとめた編集: 編集前は最初の編集の前のまま
            Some(rec) if id == id_before => rec.after = after,
            _ => {
                self.records.insert(id, SelectionRecord { before, after });
            }
        }
    }

    /// `except` 以外のビューの選択範囲と変換中の文字列を補正する（Spec §4、§6.2）。
    fn map_other_views(&mut self, change: &Change, except: Option<ViewId>) {
        for (id, st) in self.views.iter_mut() {
            if Some(*id) == except {
                continue;
            }
            let sels = st
                .selections
                .iter()
                .map(|&s| map_selection(s, change, Bias::After))
                .collect();
            let (sels, primary) = normalize(sels, st.primary);
            st.selections = sels;
            st.primary = primary;
            if let Some(c) = &mut st.composition {
                let start = map_offset(c.range.start, Bias::Before, change);
                let end = map_offset(c.range.end, Bias::After, change).max(start);
                c.range = start..end;
            }
        }
    }

    // --- Undo・Redo（Spec §5.6） ---

    pub fn undo(&mut self, view: ViewId) -> Result<Vec<Change>> {
        self.view(view)?;
        let mut changes: Vec<Change> = self.commit_pending(view).into_iter().collect();
        let undone = self.buffer.history().current();
        if let Some(change) = self.buffer.undo() {
            let restore = self.records.get(&undone).map(|r| r.before.clone());
            self.after_history_move(view, &change, restore);
            changes.push(change);
        }
        self.last_op = None;
        Ok(changes)
    }

    pub fn redo(&mut self, view: ViewId) -> Result<Vec<Change>> {
        self.view(view)?;
        let mut changes: Vec<Change> = self.commit_pending(view).into_iter().collect();
        if let Some(change) = self.buffer.redo() {
            let redone = self.buffer.history().current();
            let restore = self.records.get(&redone).map(|r| r.after.clone());
            self.after_history_move(view, &change, restore);
            changes.push(change);
        }
        self.last_op = None;
        Ok(changes)
    }

    fn after_history_move(
        &mut self,
        view: ViewId,
        change: &Change,
        restore: Option<(Vec<Selection>, usize)>,
    ) {
        match restore {
            Some((sels, primary)) => {
                self.map_other_views(change, Some(view));
                self.set_view_selections(view, sels, primary);
            }
            None => self.map_other_views(change, None),
        }
    }

    // --- IME（Spec §6） ---

    /// 変換中の文字列を設定する。変換中でなければ主選択範囲で変換を始める。バッファ・履歴は変えない。
    pub fn set_composition(
        &mut self,
        view: ViewId,
        text: &str,
        selected: Option<Range<usize>>,
    ) -> Result<()> {
        let st = self.view(view)?;
        let selected = selected.unwrap_or(text.len()..text.len());
        let valid = selected.start <= selected.end
            && selected.end <= text.len()
            && text.is_char_boundary(selected.start)
            && text.is_char_boundary(selected.end);
        if !valid {
            return Err(DocumentError::InvalidCompositionSelection {
                selected,
                len: text.len(),
            });
        }
        let range = match &st.composition {
            Some(c) => c.range.clone(),
            None => st.selections[st.primary].range(),
        };
        self.view_mut(view).composition = Some(Composition {
            range,
            text: text.to_string(),
            selected,
        });
        self.last_op = None;
        Ok(())
    }

    /// 確定する。変換中でない場合は主選択範囲を置き換える（Spec §6.2）。
    pub fn commit_composition(&mut self, view: ViewId, text: &str) -> Result<Vec<Change>> {
        self.view(view)?;
        let comp = self.view_mut(view).composition.take();
        Ok(self
            .do_commit(view, comp.map(|c| c.range), text)
            .into_iter()
            .collect())
    }

    /// 変換を取り消す。バッファ・選択範囲は変換の開始前のまま。
    pub fn cancel_composition(&mut self, view: ViewId) -> Result<()> {
        self.view(view)?;
        self.view_mut(view).composition = None;
        Ok(())
    }

    /// 変換中なら現在の文字列で確定する（他の操作の前。Spec §6.2）。
    fn commit_pending(&mut self, view: ViewId) -> Option<Change> {
        let comp = self.view_mut(view).composition.take()?;
        self.do_commit(view, Some(comp.range), &comp.text)
    }

    fn do_commit(
        &mut self,
        view: ViewId,
        range: Option<Range<usize>>,
        text: &str,
    ) -> Option<Change> {
        let st = self.view(view).expect("view checked by caller");
        let (sels, primary) = (st.selections.clone(), st.primary);
        let range = range.unwrap_or_else(|| sels[primary].range());
        let id_before = self.buffer.history().current();
        let change = self
            .buffer
            .apply([Edit::new(range.clone(), text)], UndoGroup::NewGroup)
            .expect("composition range is kept valid by mapping")?;
        let new_sels = sels
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                if i == primary {
                    Selection::cursor(range.start + text.len())
                } else {
                    map_selection(s, &change, Bias::After)
                }
            })
            .collect();
        self.map_other_views(&change, Some(view));
        self.set_view_selections(view, new_sels, primary);
        self.record(view, id_before, (sels, primary));
        self.last_op = None;
        Some(change)
    }

    // --- 入力用の位置（Spec §6.3） ---

    /// 変換中なら (バッファでの開始の UTF-16 位置, `range` の UTF-16 長, `text` の UTF-16 長, 変換中の文字列)。
    fn comp_geometry(&self, view: ViewId) -> Result<Option<(usize, usize, usize, &Composition)>> {
        let Some(c) = self.view(view)?.composition.as_ref() else {
            return Ok(None);
        };
        let t = self.text();
        let s16 = t.byte_to_utf16(c.range.start)?;
        let e16 = t.byte_to_utf16(c.range.end)?;
        Ok(Some((s16, e16 - s16, utf16_len(&c.text), c)))
    }

    /// 入力用の文字列の UTF-16 の長さ。
    pub fn input_len_utf16(&self, view: ViewId) -> Result<usize> {
        let len = self.text().len_utf16();
        Ok(match self.comp_geometry(view)? {
            Some((_, r16, t16, _)) => len - r16 + t16,
            None => len,
        })
    }

    /// 入力用の位置 → バッファのバイト位置。変換中の文字列の内側は `range.start`。
    pub fn input_to_buffer(&self, view: ViewId, offset16: usize) -> Result<usize> {
        let t = self.text();
        match self.comp_geometry(view)? {
            None => Ok(t.utf16_to_byte(offset16)?),
            Some((s16, r16, t16, c)) => {
                if offset16 <= s16 {
                    Ok(t.utf16_to_byte(offset16)?)
                } else if offset16 < s16 + t16 {
                    Ok(c.range.start)
                } else {
                    Ok(t.utf16_to_byte(offset16 - t16 + r16)?)
                }
            }
        }
    }

    /// バッファのバイト位置 → 入力用の位置。`range` の内側は変換中の文字列の先頭。
    pub fn buffer_to_input(&self, view: ViewId, offset: usize) -> Result<usize> {
        let u = self.text().byte_to_utf16(offset)?;
        Ok(match self.comp_geometry(view)? {
            None => u,
            Some((s16, r16, t16, c)) => {
                if offset <= c.range.start {
                    u
                } else if offset < c.range.end {
                    s16
                } else {
                    u - r16 + t16
                }
            }
        })
    }

    /// 変換中の文字列の範囲（入力用の位置）。
    pub fn input_marked_range(&self, view: ViewId) -> Result<Option<Range<usize>>> {
        Ok(self
            .comp_geometry(view)?
            .map(|(s16, _, t16, _)| s16..s16 + t16))
    }

    /// 主選択範囲（入力用の位置）と、`head` が前にあるか。変換中は変換中の文字列の中の選択範囲。
    pub fn input_selected_range(&self, view: ViewId) -> Result<(Range<usize>, bool)> {
        if let Some((s16, _, _, c)) = self.comp_geometry(view)? {
            let a = s16 + utf16_len(&c.text[..c.selected.start]);
            let b = s16 + utf16_len(&c.text[..c.selected.end]);
            return Ok((a..b, false));
        }
        let s = self.primary_selection(view)?;
        let t = self.text();
        Ok((
            t.byte_to_utf16(s.start())?..t.byte_to_utf16(s.end())?,
            s.is_reversed(),
        ))
    }

    /// 入力用の文字列の範囲の文字列。サロゲートペアの途中は外側へ広げ、長さを超える部分は切り詰める。
    pub fn input_text_for_range(&self, view: ViewId, range16: Range<usize>) -> Result<String> {
        let total = self.input_len_utf16(view)?;
        let end = range16.end.min(total);
        let start = range16.start.min(end);
        let t = self.text();
        let buf = |lo: usize, hi: usize| -> Result<String> {
            Ok(t.slice(buf_utf16_to_byte(t, lo, false)?..buf_utf16_to_byte(t, hi, true)?)?)
        };
        let Some((s16, r16, t16, c)) = self.comp_geometry(view)? else {
            return buf(start, end);
        };
        let mut out = String::new();
        // 変換中の文字列の前（バッファ）
        let (lo, hi) = (start.min(s16), end.min(s16));
        if lo < hi {
            out += &buf(lo, hi)?;
        }
        // 変換中の文字列
        let (lo, hi) = (start.clamp(s16, s16 + t16), end.clamp(s16, s16 + t16));
        if lo < hi {
            out += &c.text[str_utf16_to_byte(&c.text, lo - s16, false)
                ..str_utf16_to_byte(&c.text, hi - s16, true)];
        }
        // 変換中の文字列の後ろ（バッファ。位置を range の終わりに合わせる）
        let (lo, hi) = (start.max(s16 + t16), end.max(s16 + t16));
        if lo < hi {
            out += &buf(lo - t16 + r16, hi - t16 + r16)?;
        }
        Ok(out)
    }

    // --- 保存済みの状態（Spec §8） ---

    pub fn is_dirty(&self) -> bool {
        self.buffer.history().current() != self.saved
    }

    /// 保存する内容と、その履歴の項目。書き込みに成功した後に `mark_saved` に渡す。
    pub fn save_point(&self) -> (Snapshot, HistoryId) {
        (self.buffer.snapshot(), self.buffer.history().current())
    }

    pub fn mark_saved(&mut self, id: HistoryId) -> Result<()> {
        if self.buffer.history().node(id).is_none() {
            return Err(TextError::UnknownHistoryId { id }.into());
        }
        self.saved = id;
        Ok(())
    }

    /// 外部での変更を読み込む（Spec §8.2）。Undo 可能な1つの項目として置き換え、保存済みにする。
    pub fn reload(&mut self, text: &str) -> Result<Option<Change>> {
        for st in self.views.values_mut() {
            st.composition = None;
        }
        self.last_op = None;
        let current = self.text().to_string();
        let (a, b) = (current.as_bytes(), text.as_bytes());
        let max = a.len().min(b.len());
        let mut prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        while !(current.is_char_boundary(prefix) && text.is_char_boundary(prefix)) {
            prefix -= 1;
        }
        let mut suffix = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take_while(|(x, y)| x == y)
            .count()
            .min(max - prefix);
        while !(current.is_char_boundary(a.len() - suffix)
            && text.is_char_boundary(b.len() - suffix))
        {
            suffix -= 1;
        }
        let edit = Edit::new(prefix..a.len() - suffix, &text[prefix..b.len() - suffix]);
        let change = self.buffer.apply([edit], UndoGroup::NewGroup)?;
        if let Some(c) = &change {
            self.map_other_views(c, None);
        }
        self.saved = self.buffer.history().current();
        self.line_ending = LineEnding::detect(text);
        Ok(change)
    }
}

fn map_selection(s: Selection, change: &Change, bias: Bias) -> Selection {
    Selection::new(
        map_offset(s.anchor, bias, change),
        map_offset(s.head, bias, change),
    )
}

/// 始まりの位置の順に並べ、重なる選択範囲をまとめる（Spec §3）。主選択の新しい位置も返す。
fn normalize(sels: Vec<Selection>, primary: usize) -> (Vec<Selection>, usize) {
    let mut order: Vec<usize> = (0..sels.len()).collect();
    order.sort_by_key(|&i| (sels[i].start(), sels[i].end()));
    let mut out: Vec<Selection> = Vec::with_capacity(sels.len());
    let mut new_primary = 0;
    for i in order {
        let s = sels[i];
        if let Some(last) = out.last_mut() {
            let overlaps = s.start() < last.end()
                || (s.start() == last.end() && (s.is_empty() || last.is_empty()));
            if overlaps {
                let (start, end) = (last.start(), last.end().max(s.end()));
                let reversed = if i == primary {
                    s.is_reversed()
                } else {
                    last.is_reversed()
                };
                *last = if reversed {
                    Selection::new(end, start)
                } else {
                    Selection::new(start, end)
                };
                if i == primary {
                    new_primary = out.len() - 1;
                }
                continue;
            }
        }
        if i == primary {
            new_primary = out.len();
        }
        out.push(s);
    }
    (out, new_primary)
}

/// 後退削除の開始位置（Spec §5.3.1）。`cursor` は0より大きい文字境界。
fn backspace_start(t: &Snapshot, cursor: usize) -> std::result::Result<usize, TextError> {
    let prev_char = t.prev_char_boundary(cursor)?.unwrap_or(0);
    if !t.is_grapheme_boundary(cursor)? {
        return Ok(prev_char);
    }
    let g = t.prev_grapheme_boundary(cursor)?.unwrap_or(0);
    let cluster = t.slice(g..cursor)?;
    let whole = cluster == "\r\n"
        || cluster.chars().any(|c| {
            matches!(c,
                '\u{200D}'
                | '\u{1F1E6}'..='\u{1F1FF}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{E0100}'..='\u{E01EF}'
                | '\u{1F3FB}'..='\u{1F3FF}'
                | '\u{E0020}'..='\u{E007F}')
        });
    Ok(if whole { g } else { prev_char })
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// 文字列の UTF-16 位置 → バイト位置。サロゲートペアの途中は `round_up` に従って前後の文字境界へ。
fn str_utf16_to_byte(s: &str, index16: usize, round_up: bool) -> usize {
    let mut u = 0;
    for (b, ch) in s.char_indices() {
        if u >= index16 {
            return b;
        }
        let next = u + ch.len_utf16();
        if next > index16 {
            return if round_up { b + ch.len_utf8() } else { b };
        }
        u = next;
    }
    s.len()
}

/// バッファの UTF-16 位置 → バイト位置。サロゲートペアの途中は `round_up` に従って前後の文字境界へ。
fn buf_utf16_to_byte(
    t: &Snapshot,
    index16: usize,
    round_up: bool,
) -> std::result::Result<usize, TextError> {
    match t.utf16_to_byte(index16) {
        Err(TextError::NotUtf16Boundary { .. }) => {
            t.utf16_to_byte(if round_up { index16 + 1 } else { index16 - 1 })
        }
        r => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_merges_overlaps_and_tracks_primary() {
        let sels = vec![
            Selection::new(5, 8),
            Selection::cursor(0),
            Selection::new(7, 10),
            Selection::cursor(0),
        ];
        let (out, p) = normalize(sels, 2);
        assert_eq!(out, vec![Selection::cursor(0), Selection::new(5, 10)]);
        assert_eq!(p, 1);
        // 接する空でない範囲はまとめない。範囲の端のカーソルはまとめる
        let (out, _) = normalize(
            vec![
                Selection::new(0, 2),
                Selection::new(2, 4),
                Selection::cursor(4),
            ],
            0,
        );
        assert_eq!(out, vec![Selection::new(0, 2), Selection::new(2, 4)]);
    }

    #[test]
    fn str_utf16_conversion_rounds_inside_surrogates() {
        let s = "a😀b";
        assert_eq!(str_utf16_to_byte(s, 1, false), 1);
        assert_eq!(str_utf16_to_byte(s, 2, false), 1);
        assert_eq!(str_utf16_to_byte(s, 2, true), 5);
        assert_eq!(str_utf16_to_byte(s, 3, false), 5);
        assert_eq!(str_utf16_to_byte(s, 9, false), 6);
    }
}
