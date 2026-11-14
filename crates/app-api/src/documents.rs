//! 版0.2の Vault・文書の要素（Spec §5A）。

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// 開いている文書の識別子。Core が割り当てる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub u64);

/// 文書のビューの識別子。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ViewId(pub u64);

/// 選択範囲（UTF-8 のバイト位置）。`head` がカーソル。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

/// IME の変換中の文字列（文書セッション §6.1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionState {
    /// 確定時に置き換えるバッファの範囲。
    pub range: Range<usize>,
    pub text: String,
    /// `text` の中の選択範囲（`text` 内のバイト位置）。
    pub selected: Range<usize>,
}

/// ビューの状態。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewState {
    /// 選択範囲（始まりの位置の順）。
    pub selections: Vec<Selection>,
    /// 主選択の位置。
    pub primary: usize,
    pub composition: Option<CompositionState>,
}

/// 内容の変更の1つの範囲置換。内容そのものは含めない（Spec §5A.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEdit {
    /// 変更前の内容での開始位置（バイト）。
    pub start: usize,
    /// 削除した長さ（バイト）。
    pub old_len: usize,
    /// 挿入した長さ（バイト）。
    pub new_len: usize,
}

/// カーソルの移動（文書セッション §5.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Movement {
    Left,
    Right,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
}

/// 文書セッションの編集コマンド（Spec §5A.2.1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EditCommand {
    InsertText {
        text: String,
    },
    InsertNewline,
    DeleteBackward,
    DeleteForward,
    DeleteSelection,
    MoveCursor {
        movement: Movement,
        extend: bool,
    },
    SelectAll,
    SetSelections {
        selections: Vec<Selection>,
        primary: usize,
    },
    Undo,
    Redo,
    SetComposition {
        text: String,
        selected: Option<Range<usize>>,
    },
    CommitComposition {
        text: String,
    },
    CancelComposition,
}

/// 文書の情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentInfo {
    /// Vault 相対パス。
    pub path: String,
    pub version: u64,
    pub dirty: bool,
}

/// Vault 内のファイルの一覧。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultFiles {
    /// Vault 相対パス（昇順）。
    pub files: Vec<String>,
    /// 一覧から除外した要素の数（名前が UTF-8 でない等。Vault §4）。
    pub skipped: usize,
}
