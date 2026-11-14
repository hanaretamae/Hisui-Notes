//! テキストバッファ。仕様は `docs/spec/text-buffer.md`。
//!
//! 1つの文書の原文を Rope（ropey 1.6、ADR-0003）で保持し、編集・編集履歴（Undo tree）・位置変換・
//! 書記素境界・スナップショットを提供する。公開APIの位置は UTF-8 のバイト位置で、ropey の型と
//! 文字位置は公開しない。文書形式・ファイル・UI・時刻に依存しない。

mod buffer;
mod history;
mod snapshot;

use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};

pub use buffer::TextBuffer;
pub use history::{History, HistoryId, HistoryNode};
pub use snapshot::Snapshot;

/// 版番号（Spec §5.3）。作成時に0で、内容を変えた操作ごとに1増える。
pub type Version = u64;

/// 範囲置換（Spec §5.1）。`range` のバイト範囲を削除し、`range.start` に `insert` を挿入する。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub range: Range<usize>,
    pub insert: String,
}

impl Edit {
    pub fn new(range: Range<usize>, insert: impl Into<String>) -> Self {
        Self {
            range,
            insert: insert.into(),
        }
    }

    pub fn insert(offset: usize, text: impl Into<String>) -> Self {
        Self::new(offset..offset, text)
    }

    pub fn delete(range: Range<usize>) -> Self {
        Self::new(range, String::new())
    }

    /// 「変化なし」の編集か（範囲も挿入も空）。
    pub fn is_noop(&self) -> bool {
        self.range.is_empty() && self.insert.is_empty()
    }
}

/// 適用した範囲置換（Spec §5.4）。`start` と `deleted` は適用前の内容に対する値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedEdit {
    pub start: usize,
    pub deleted: String,
    pub inserted: String,
}

impl AppliedEdit {
    /// 適用前の内容での削除範囲の終わり。
    pub fn end(&self) -> usize {
        self.start + self.deleted.len()
    }

    /// 逆の範囲置換（削除と挿入を入れ替えたもの）。
    pub fn inverse(&self) -> Self {
        Self {
            start: self.start,
            deleted: self.inserted.clone(),
            inserted: self.deleted.clone(),
        }
    }

    /// この範囲置換の前の位置を、後の位置へ補正する（Spec §5.5）。
    pub fn map_offset(&self, offset: usize, bias: Bias) -> usize {
        let (start, end, n) = (self.start, self.end(), self.inserted.len());
        if offset < start {
            offset
        } else if offset > end || (start < end && offset == end) {
            offset - (end - start) + n
        } else {
            match bias {
                Bias::Before => start,
                Bias::After => start + n,
            }
        }
    }
}

/// 内容を変えた操作の適用結果（Spec §5.4）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// 適用後の版番号。
    pub version: Version,
    /// 適用した範囲置換の列。前から順に適用すると、適用前の内容が適用後の内容になる。
    pub edits: Vec<AppliedEdit>,
}

impl Change {
    /// 逆の範囲置換の列。適用後の内容に順に適用すると、適用前の内容に戻る。
    pub fn inverse_edits(&self) -> impl Iterator<Item = AppliedEdit> + '_ {
        self.edits.iter().rev().map(AppliedEdit::inverse)
    }
}

/// 挿入位置ちょうどの位置の補正の向き（Spec §5.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Bias {
    /// 挿入の前に留まる。
    Before,
    /// 挿入の後ろへ移る。
    After,
}

/// 古い版の位置を、`change` の適用後の位置へ補正する（Spec §5.5）。
pub fn map_offset(offset: usize, bias: Bias, change: &Change) -> usize {
    change
        .edits
        .iter()
        .fold(offset, |o, e| e.map_offset(o, bias))
}

/// 編集の履歴への入れ方（Spec §6.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UndoGroup {
    /// 現在の項目の子として新しい項目を作る。
    NewGroup,
    /// 現在の項目に追加する（追加できない場合は `NewGroup` と同じ）。
    MergeWithPrevious,
}

/// 行と行内の位置。位置の単位（バイト・UTF-16）は操作ごとに決まる。行・位置とも0始まり。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LineCol {
    pub line: usize,
    pub column: usize,
}

impl LineCol {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// 位置・範囲・履歴の指定の誤り（Spec §3.1、§6.3）。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextError {
    /// バイト位置が内容の長さを超える。
    OutOfBounds { offset: usize, len: usize },
    /// バイト位置が文字境界でない。
    NotCharBoundary { offset: usize },
    /// 範囲の始まりが終わりより後ろ。
    InvalidRange { start: usize, end: usize },
    /// コードポイント位置が長さを超える。
    CharIndexOutOfBounds { index: usize, len: usize },
    /// UTF-16 位置が長さを超える。
    Utf16OutOfBounds { index: usize, len: usize },
    /// UTF-16 位置がサロゲートペアの途中。
    NotUtf16Boundary { index: usize },
    /// 行番号が行数以上。
    LineOutOfBounds { line: usize, line_count: usize },
    /// 行内の位置が有効な範囲の外（Spec §3.3）。`max` は有効な位置の最大値。
    ColumnOutOfBounds {
        line: usize,
        column: usize,
        max: usize,
    },
    /// 履歴の項目が存在しない。
    UnknownHistoryId { id: HistoryId },
    /// 履歴の項目が現在の項目の子でない。
    NotAChild { id: HistoryId },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { offset, len } => {
                write!(f, "byte offset {offset} is out of bounds (len {len})")
            }
            Self::NotCharBoundary { offset } => {
                write!(f, "byte offset {offset} is not a char boundary")
            }
            Self::InvalidRange { start, end } => write!(f, "invalid range {start}..{end}"),
            Self::CharIndexOutOfBounds { index, len } => {
                write!(f, "char index {index} is out of bounds (len {len})")
            }
            Self::Utf16OutOfBounds { index, len } => {
                write!(f, "UTF-16 index {index} is out of bounds (len {len})")
            }
            Self::NotUtf16Boundary { index } => {
                write!(f, "UTF-16 index {index} is inside a surrogate pair")
            }
            Self::LineOutOfBounds { line, line_count } => {
                write!(f, "line {line} is out of bounds (line count {line_count})")
            }
            Self::ColumnOutOfBounds { line, column, max } => {
                write!(
                    f,
                    "column {column} is out of bounds on line {line} (max {max})"
                )
            }
            Self::UnknownHistoryId { id } => write!(f, "unknown history item {}", id.get()),
            Self::NotAChild { id } => write!(
                f,
                "history item {} is not a child of the current item",
                id.get()
            ),
        }
    }
}

impl std::error::Error for TextError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(start: usize, deleted: &str, inserted: &str) -> AppliedEdit {
        AppliedEdit {
            start,
            deleted: deleted.into(),
            inserted: inserted.into(),
        }
    }

    #[test]
    fn map_offset_follows_spec_rules() {
        // "abcdef" の 2..4 ("cd") を "XYZ" に置換 → "abXYZef"
        let e = edit(2, "cd", "XYZ");
        assert_eq!(e.map_offset(1, Bias::After), 1, "前は変わらない");
        assert_eq!(e.map_offset(2, Bias::Before), 2, "先頭は bias に従う");
        assert_eq!(e.map_offset(2, Bias::After), 5);
        assert_eq!(
            e.map_offset(3, Bias::Before),
            2,
            "削除範囲の内側は bias に従う"
        );
        assert_eq!(e.map_offset(3, Bias::After), 5);
        assert_eq!(
            e.map_offset(4, Bias::Before),
            5,
            "削除範囲の直後は置換の後ろ"
        );
        assert_eq!(e.map_offset(6, Bias::Before), 7, "後ろはずれる");
    }

    #[test]
    fn map_offset_at_pure_insertion_point_uses_bias() {
        let e = edit(3, "", "ab");
        assert_eq!(e.map_offset(3, Bias::Before), 3);
        assert_eq!(e.map_offset(3, Bias::After), 5);
        assert_eq!(e.map_offset(4, Bias::Before), 6);
    }

    #[test]
    fn map_offset_applies_edits_in_order() {
        let change = Change {
            version: 1,
            edits: vec![edit(0, "", "aa"), edit(4, "x", "")],
        };
        // 3 → 挿入で5 → 削除範囲 4..5 の直後なので4
        assert_eq!(map_offset(3, Bias::Before, &change), 4);
        assert_eq!(map_offset(5, Bias::Before, &change), 6);
    }

    #[test]
    fn change_round_trips_through_serde() {
        let change = Change {
            version: 7,
            edits: vec![edit(0, "日本", "👩‍💻\r\n")],
        };
        let json = serde_json::to_string(&change).unwrap();
        assert_eq!(serde_json::from_str::<Change>(&json).unwrap(), change);
        let e = Edit::new(1..3, "x");
        assert_eq!(
            serde_json::from_str::<Edit>(&serde_json::to_string(&e).unwrap()).unwrap(),
            e
        );
    }
}
