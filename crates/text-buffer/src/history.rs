//! 編集履歴（Undo tree。Spec §6）。

use serde::{Deserialize, Serialize};

use crate::{AppliedEdit, TextError, UndoGroup};

/// 履歴の項目の識別子。作成順に振られ、根は0（Spec §6.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HistoryId(u64);

impl HistoryId {
    /// 根（バッファ作成時の内容）。
    pub const ROOT: HistoryId = HistoryId(0);

    pub fn get(self) -> u64 {
        self.0
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

type Transaction = Vec<AppliedEdit>;

#[derive(Debug, Clone)]
struct Node {
    parent: Option<HistoryId>,
    children: Vec<HistoryId>,
    last_child: Option<HistoryId>,
    transactions: Vec<Transaction>,
}

/// 履歴の項目の情報（Spec §6.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryNode<'a> {
    pub id: HistoryId,
    /// 親。根では `None`。
    pub parent: Option<HistoryId>,
    /// 子（作成順）。
    pub children: &'a [HistoryId],
    /// 最後に通った子。
    pub last_child: Option<HistoryId>,
    /// 含むトランザクションの数（根では0）。
    pub transaction_count: usize,
}

/// 編集履歴の木。
#[derive(Debug, Clone)]
pub struct History {
    nodes: Vec<Node>,
    current: HistoryId,
    /// 直前の履歴の操作が編集で、現在の項目へ追加できるか（Spec §6.2）。
    mergeable: bool,
}

impl Default for History {
    fn default() -> Self {
        Self {
            nodes: vec![Node {
                parent: None,
                children: Vec::new(),
                last_child: None,
                transactions: Vec::new(),
            }],
            current: HistoryId::ROOT,
            mergeable: false,
        }
    }
}

impl History {
    /// 現在の項目。
    pub fn current(&self) -> HistoryId {
        self.current
    }

    /// 項目の数（根を含む）。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 根しかないか。
    pub fn is_empty(&self) -> bool {
        self.nodes.len() == 1
    }

    /// 項目の情報。存在しない場合は `None`。
    pub fn node(&self, id: HistoryId) -> Option<HistoryNode<'_>> {
        self.nodes.get(id.index()).map(|n| HistoryNode {
            id,
            parent: n.parent,
            children: &n.children,
            last_child: n.last_child,
            transaction_count: n.transactions.len(),
        })
    }

    fn get(&self, id: HistoryId) -> &Node {
        &self.nodes[id.index()]
    }

    fn get_mut(&mut self, id: HistoryId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    /// 内容を変えたトランザクションを記録する（Spec §6.2）。
    pub(crate) fn record(&mut self, tx: Transaction, group: UndoGroup) {
        let current = self.current;
        let merge = group == UndoGroup::MergeWithPrevious
            && self.mergeable
            && current != HistoryId::ROOT
            && self.get(current).children.is_empty();
        if merge {
            self.get_mut(current).transactions.push(tx);
        } else {
            let id = HistoryId(self.nodes.len() as u64);
            self.nodes.push(Node {
                parent: Some(current),
                children: Vec::new(),
                last_child: None,
                transactions: vec![tx],
            });
            let parent = self.get_mut(current);
            parent.children.push(id);
            parent.last_child = Some(id);
            self.current = id;
        }
        self.mergeable = true;
    }

    /// 項目を取り消す範囲置換の列（トランザクションを逆順に、各範囲置換も逆順に）。
    fn inverse_edits(&self, id: HistoryId) -> impl Iterator<Item = AppliedEdit> + '_ {
        self.get(id)
            .transactions
            .iter()
            .rev()
            .flat_map(|tx| tx.iter().rev().map(AppliedEdit::inverse))
    }

    /// 項目を適用する範囲置換の列。
    fn forward_edits(&self, id: HistoryId) -> impl Iterator<Item = AppliedEdit> + '_ {
        self.get(id).transactions.iter().flatten().cloned()
    }

    /// 現在の項目を取り消す。適用すべき範囲置換の列を返し、親へ移る（Spec §6.3）。
    pub(crate) fn undo(&mut self) -> Option<Vec<AppliedEdit>> {
        let id = self.current;
        let parent = self.get(id).parent?;
        let edits = self.inverse_edits(id).collect();
        self.get_mut(parent).last_child = Some(id);
        self.current = parent;
        self.mergeable = false;
        Some(edits)
    }

    /// 最後に通った子へ進む。
    pub(crate) fn redo(&mut self) -> Option<Vec<AppliedEdit>> {
        let child = self.get(self.current).last_child?;
        Some(self.enter_child(child))
    }

    /// 指定した子へ進む。
    pub(crate) fn redo_to(&mut self, child: HistoryId) -> Result<Vec<AppliedEdit>, TextError> {
        let node = self
            .nodes
            .get(child.index())
            .ok_or(TextError::UnknownHistoryId { id: child })?;
        if node.parent != Some(self.current) {
            return Err(TextError::NotAChild { id: child });
        }
        Ok(self.enter_child(child))
    }

    fn enter_child(&mut self, child: HistoryId) -> Vec<AppliedEdit> {
        let edits = self.forward_edits(child).collect();
        self.get_mut(self.current).last_child = Some(child);
        self.current = child;
        self.mergeable = false;
        edits
    }

    /// 任意の項目へ移る。共通の祖先まで取り消し、そこから進む（Spec §6.3）。
    /// 現在の項目と同じ場合は `None`。
    pub(crate) fn goto(
        &mut self,
        target: HistoryId,
    ) -> Result<Option<Vec<AppliedEdit>>, TextError> {
        if target.index() >= self.nodes.len() {
            return Err(TextError::UnknownHistoryId { id: target });
        }
        if target == self.current {
            return Ok(None);
        }
        // target から根までの経路
        let mut down = vec![target];
        while let Some(p) = self.get(*down.last().unwrap()).parent {
            down.push(p);
        }
        let mut edits = Vec::new();
        // 共通の祖先まで上る
        while !down.contains(&self.current) {
            edits.extend(self.undo().expect("non-root node has a parent"));
        }
        // 共通の祖先から target まで下る
        let lca = down
            .iter()
            .position(|&n| n == self.current)
            .expect("current is on the path");
        for &child in down[..lca].iter().rev() {
            edits.extend(self.enter_child(child));
        }
        Ok(Some(edits))
    }
}
