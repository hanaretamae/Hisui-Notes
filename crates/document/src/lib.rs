//! 文書モデル、文書セッション、文書形式拡張API、parser-api（ADR-0002）。
//!
//! 現在は文書セッション（`docs/spec/document-session.md`）のみ。文書モデルと parser-api は M3 で追加する。

mod session;

use std::fmt;
use std::ops::Range;

pub use session::DocumentSession;
pub use text_buffer::{Change, HistoryId, Snapshot, TextError};

/// ビューの識別子（Spec §2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewId(u64);

impl ViewId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// 選択範囲（Spec §3）。`head` がカーソルの位置、`anchor` が選択の起点（バイト位置）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    /// 空の選択（カーソル）。
    pub fn cursor(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn range(&self) -> Range<usize> {
        self.start()..self.end()
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// `head` が `anchor` より前か。
    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }
}

/// カーソルの移動（Spec §5.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Movement {
    Left,
    Right,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
}

/// 文書の改行コード（Spec §7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    /// 内容の最初の行区切りで決める。行区切りが無い場合は LF。
    pub fn detect(text: &str) -> Self {
        match text.find(['\n', '\r']) {
            Some(i) if text.as_bytes()[i] == b'\n' => Self::Lf,
            Some(i) if text.as_bytes().get(i + 1) == Some(&b'\n') => Self::CrLf,
            Some(_) => Self::Cr,
            None => Self::Lf,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// IME の変換中の文字列（Spec §6.1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    /// 確定時に置き換えるバッファの範囲。
    pub range: Range<usize>,
    /// 変換中の文字列。
    pub text: String,
    /// `text` の中の選択範囲（`text` 内のバイト位置）。
    pub selected: Range<usize>,
}

/// 文書セッションの操作の誤り。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentError {
    /// 存在しないビュー。
    UnknownView(ViewId),
    /// 選択範囲の列が空。
    EmptySelections,
    /// 主選択の位置が列の範囲外。
    PrimaryOutOfRange { primary: usize, len: usize },
    /// 変換中の文字列の中の選択範囲が不正（範囲外、文字境界でない、逆転）。
    InvalidCompositionSelection { selected: Range<usize>, len: usize },
    /// 位置・範囲・履歴の誤り。
    Text(TextError),
}

impl From<TextError> for DocumentError {
    fn from(e: TextError) -> Self {
        Self::Text(e)
    }
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownView(v) => write!(f, "unknown view {}", v.get()),
            Self::EmptySelections => write!(f, "selections must not be empty"),
            Self::PrimaryOutOfRange { primary, len } => {
                write!(f, "primary selection {primary} is out of range (len {len})")
            }
            Self::InvalidCompositionSelection { selected, len } => {
                write!(
                    f,
                    "invalid composition selection {}..{} (len {len})",
                    selected.start, selected.end
                )
            }
            Self::Text(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for DocumentError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_ending_is_detected_from_first_break() {
        assert_eq!(LineEnding::detect("a\r\nb\nc"), LineEnding::CrLf);
        assert_eq!(LineEnding::detect("a\nb\r\n"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\rb"), LineEnding::Cr);
        assert_eq!(LineEnding::detect("a\r"), LineEnding::Cr);
        assert_eq!(LineEnding::detect("abc"), LineEnding::Lf);
        assert_eq!(LineEnding::detect(""), LineEnding::Lf);
    }
}
