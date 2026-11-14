//! 編集と履歴の操作（Spec §5、§6）。

use crate::{
    AppliedEdit, Change, Edit, History, HistoryId, Snapshot, TextError, UndoGroup, Version,
};

/// 1つの文書の編集可能なテキスト。
#[derive(Debug, Clone)]
pub struct TextBuffer {
    text: Snapshot,
    history: History,
}

impl TextBuffer {
    /// `text` を内容とするバッファを作る。版番号は0。内容を正規化・変換しない（Spec §2）。
    pub fn new(text: &str) -> Self {
        Self {
            text: Snapshot::new(text),
            history: History::default(),
        }
    }

    /// 現在の内容（読み出し専用。複製しない）。
    pub fn text(&self) -> &Snapshot {
        &self.text
    }

    /// 現在の内容の不変のスナップショット（O(1)。Spec §7）。
    pub fn snapshot(&self) -> Snapshot {
        self.text.clone()
    }

    pub fn version(&self) -> Version {
        self.text.version
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// 範囲置換の列を1つのトランザクションとして適用する（Spec §5.2）。
    ///
    /// 各範囲は、それより前の編集を適用した後の内容に対する位置。いずれかが不正な場合は、内容・版・履歴を
    /// 変えずにエラーを返す。すべて「変化なし」の場合は `Ok(None)`。
    pub fn apply(
        &mut self,
        edits: impl IntoIterator<Item = Edit>,
        group: UndoGroup,
    ) -> Result<Option<Change>, TextError> {
        let backup = self.text.rope.clone();
        let mut applied = Vec::new();
        for edit in edits {
            if let Err(e) = self.text.check_range(&edit.range) {
                self.text.rope = backup;
                return Err(e);
            }
            if !edit.is_noop() {
                applied.push(self.text.replace(edit.range, edit.insert));
            }
        }
        if applied.is_empty() {
            return Ok(None);
        }
        self.history.record(applied.clone(), group);
        Ok(Some(self.commit(applied)))
    }

    /// 現在の項目を取り消す（Spec §6.3）。取り消す項目が無い場合は `None`。
    pub fn undo(&mut self) -> Option<Change> {
        let edits = self.history.undo()?;
        Some(self.replay(edits))
    }

    /// 最後に通った子へ進む（Spec §6.3）。進む項目が無い場合は `None`。
    pub fn redo(&mut self) -> Option<Change> {
        let edits = self.history.redo()?;
        Some(self.replay(edits))
    }

    /// 現在の項目の子 `child` へ進む（Spec §6.3）。
    pub fn redo_to(&mut self, child: HistoryId) -> Result<Change, TextError> {
        let edits = self.history.redo_to(child)?;
        Ok(self.replay(edits))
    }

    /// 任意の項目へ移る（Spec §6.3）。現在の項目と同じ場合は `Ok(None)`。
    pub fn goto(&mut self, id: HistoryId) -> Result<Option<Change>, TextError> {
        Ok(self.history.goto(id)?.map(|edits| self.replay(edits)))
    }

    /// 履歴に記録済みの範囲置換を適用する。履歴の不変条件（Spec §6.1）により常に有効な範囲である。
    fn replay(&mut self, edits: Vec<AppliedEdit>) -> Change {
        let applied = edits
            .into_iter()
            .map(|e| {
                let range = e.start..e.end();
                debug_assert_eq!(
                    self.text.slice(range.clone()).as_deref(),
                    Ok(e.deleted.as_str())
                );
                self.text.replace(range, e.inserted)
            })
            .collect();
        self.commit(applied)
    }

    fn commit(&mut self, edits: Vec<AppliedEdit>) -> Change {
        self.text.version += 1;
        Change {
            version: self.text.version,
            edits,
        }
    }
}
