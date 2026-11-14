//! 読み出し・位置変換・書記素境界（Spec §3、§4、§7）。

use std::fmt;
use std::ops::Range;

use ropey::Rope;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

use crate::{AppliedEdit, LineCol, TextError, Version};

/// ある版の内容の不変のスナップショット（Spec §7）。複製は O(1) で、別スレッドへ渡せる。
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub(crate) rope: Rope,
    pub(crate) version: Version,
}

impl Snapshot {
    pub(crate) fn new(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            version: 0,
        }
    }

    /// 版番号。
    pub fn version(&self) -> Version {
        self.version
    }

    /// UTF-8 のバイト数。
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// コードポイント数。
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// UTF-16 の符号単位の数。
    pub fn len_utf16(&self) -> usize {
        self.rope.len_utf16_cu()
    }

    /// 行数（行区切りの数 + 1。Spec §3.2）。
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    /// `offset` が内容の範囲内の文字境界か。
    pub fn is_char_boundary(&self, offset: usize) -> bool {
        let len = self.len_bytes();
        if offset >= len {
            return offset == len;
        }
        let (chunk, start, _, _) = self.rope.chunk_at_byte(offset);
        chunk.is_char_boundary(offset - start)
    }

    pub(crate) fn check_offset(&self, offset: usize) -> Result<(), TextError> {
        let len = self.len_bytes();
        if offset > len {
            Err(TextError::OutOfBounds { offset, len })
        } else if !self.is_char_boundary(offset) {
            Err(TextError::NotCharBoundary { offset })
        } else {
            Ok(())
        }
    }

    pub(crate) fn check_range(&self, range: &Range<usize>) -> Result<(), TextError> {
        if range.start > range.end {
            return Err(TextError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }
        self.check_offset(range.start)?;
        self.check_offset(range.end)
    }

    /// 検査済みのバイト位置をコードポイント位置にする。
    fn char_of(&self, offset: usize) -> usize {
        self.rope.byte_to_char(offset)
    }

    // --- 読み出し（Spec §4） ---

    /// 範囲の文字列。
    pub fn slice(&self, range: Range<usize>) -> Result<String, TextError> {
        Ok(self.chunks(range)?.collect())
    }

    /// 範囲の文字列をチャンクの列として返す（複製しない）。
    pub fn chunks(
        &self,
        range: Range<usize>,
    ) -> Result<impl Iterator<Item = &str> + '_, TextError> {
        self.check_range(&range)?;
        Ok(self
            .rope
            .slice(self.char_of(range.start)..self.char_of(range.end))
            .chunks())
    }

    // --- 位置変換（Spec §3.3） ---

    pub fn byte_to_char(&self, offset: usize) -> Result<usize, TextError> {
        self.check_offset(offset)?;
        Ok(self.char_of(offset))
    }

    pub fn char_to_byte(&self, index: usize) -> Result<usize, TextError> {
        let len = self.len_chars();
        if index > len {
            return Err(TextError::CharIndexOutOfBounds { index, len });
        }
        Ok(self.rope.char_to_byte(index))
    }

    pub fn byte_to_utf16(&self, offset: usize) -> Result<usize, TextError> {
        self.check_offset(offset)?;
        Ok(self.rope.char_to_utf16_cu(self.char_of(offset)))
    }

    pub fn utf16_to_byte(&self, index: usize) -> Result<usize, TextError> {
        let len = self.len_utf16();
        if index > len {
            return Err(TextError::Utf16OutOfBounds { index, len });
        }
        // ropey はサロゲートペアの途中の位置をその文字の位置に丸めるため、往復で検出する
        let ch = self.rope.utf16_cu_to_char(index);
        if self.rope.char_to_utf16_cu(ch) != index {
            return Err(TextError::NotUtf16Boundary { index });
        }
        Ok(self.rope.char_to_byte(ch))
    }

    /// バイト位置の行。CRLF の間の位置は、CRLF の前の行に属する（Spec §3.2）。
    pub fn byte_to_line(&self, offset: usize) -> Result<usize, TextError> {
        self.check_offset(offset)?;
        // ropey（cr_lines）は CRLF を1つの行区切りとして数えるため、CRLF の間の位置は前の行になる
        Ok(self.rope.byte_to_line(offset))
    }

    fn check_line(&self, line: usize) -> Result<(), TextError> {
        let line_count = self.line_count();
        if line >= line_count {
            Err(TextError::LineOutOfBounds { line, line_count })
        } else {
            Ok(())
        }
    }

    /// 行区切りを含む行の範囲（検査済みの行）。
    fn full_line_range(&self, line: usize) -> Range<usize> {
        let start = self.rope.line_to_byte(line);
        let end = if line + 1 < self.line_count() {
            self.rope.line_to_byte(line + 1)
        } else {
            self.len_bytes()
        };
        start..end
    }

    /// 行区切りのバイト数（0、1、2）。
    fn line_break_len(&self, full: &Range<usize>) -> usize {
        if full.is_empty() {
            return 0;
        }
        match self.rope.byte(full.end - 1) {
            b'\n' if full.end - full.start >= 2 && self.rope.byte(full.end - 2) == b'\r' => 2,
            b'\n' | b'\r' => 1,
            _ => 0,
        }
    }

    /// 行の範囲（行区切りを含まない）。
    pub fn line_range(&self, line: usize) -> Result<Range<usize>, TextError> {
        self.check_line(line)?;
        let full = self.full_line_range(line);
        let brk = self.line_break_len(&full);
        Ok(full.start..full.end - brk)
    }

    /// 行区切りを含む行の範囲。
    pub fn line_range_with_break(&self, line: usize) -> Result<Range<usize>, TextError> {
        self.check_line(line)?;
        Ok(self.full_line_range(line))
    }

    /// 行内の位置の有効な最大値（Spec §3.3）。`full_len` は行区切りを含む行の長さ（単位は呼び出し側）。
    fn max_column(full_len: usize, has_break: bool) -> usize {
        if has_break { full_len - 1 } else { full_len }
    }

    /// バイト位置 → (行, 行内のバイト位置)。
    pub fn byte_to_line_col(&self, offset: usize) -> Result<LineCol, TextError> {
        let line = self.byte_to_line(offset)?;
        Ok(LineCol::new(line, offset - self.rope.line_to_byte(line)))
    }

    /// (行, 行内のバイト位置) → バイト位置。
    pub fn line_col_to_byte(&self, pos: LineCol) -> Result<usize, TextError> {
        self.check_line(pos.line)?;
        let full = self.full_line_range(pos.line);
        let has_break = self.line_break_len(&full) > 0;
        let max = Self::max_column(full.len(), has_break);
        if pos.column > max {
            return Err(TextError::ColumnOutOfBounds {
                line: pos.line,
                column: pos.column,
                max,
            });
        }
        let offset = full.start + pos.column;
        self.check_offset(offset)?;
        Ok(offset)
    }

    /// バイト位置 → (行, 行内の UTF-16 位置)。
    pub fn byte_to_line_utf16(&self, offset: usize) -> Result<LineCol, TextError> {
        let line = self.byte_to_line(offset)?;
        let start = self.rope.line_to_byte(line);
        let col = self.byte_to_utf16(offset)? - self.byte_to_utf16(start)?;
        Ok(LineCol::new(line, col))
    }

    /// (行, 行内の UTF-16 位置) → バイト位置。
    pub fn line_utf16_to_byte(&self, pos: LineCol) -> Result<usize, TextError> {
        self.check_line(pos.line)?;
        let full = self.full_line_range(pos.line);
        let has_break = self.line_break_len(&full) > 0;
        let start16 = self.byte_to_utf16(full.start)?;
        let full16 = self.byte_to_utf16(full.end)? - start16;
        let max = Self::max_column(full16, has_break);
        if pos.column > max {
            return Err(TextError::ColumnOutOfBounds {
                line: pos.line,
                column: pos.column,
                max,
            });
        }
        self.utf16_to_byte(start16 + pos.column)
    }

    // --- 書記素クラスタ・文字境界（Spec §3.4） ---

    /// `offset` が書記素クラスタの境界か。
    pub fn is_grapheme_boundary(&self, offset: usize) -> Result<bool, TextError> {
        self.check_offset(offset)?;
        Ok(self.drive(offset, GraphemeCursor::is_boundary))
    }

    /// `offset` より後ろの最初の書記素クラスタの境界。内容の末尾では `None`。
    pub fn next_grapheme_boundary(&self, offset: usize) -> Result<Option<usize>, TextError> {
        self.check_offset(offset)?;
        if offset == self.len_bytes() {
            return Ok(None);
        }
        Ok(self.drive(offset, GraphemeCursor::next_boundary))
    }

    /// `offset` より前の最初の書記素クラスタの境界。内容の先頭では `None`。
    pub fn prev_grapheme_boundary(&self, offset: usize) -> Result<Option<usize>, TextError> {
        self.check_offset(offset)?;
        if offset == 0 {
            return Ok(None);
        }
        Ok(self.drive(offset, GraphemeCursor::prev_boundary))
    }

    /// `GraphemeCursor` の操作 `op` を、Rope のチャンクを必要に応じて渡しながら実行する
    /// （P0-09: チャンク単位で使う）。カーソルが前後のチャンク・前方の文脈を要求したら、それを渡して繰り返す。
    /// `offset` は呼び出し側で検査済み（内容の範囲内の文字境界）であること。
    fn drive<T>(
        &self,
        offset: usize,
        op: fn(&mut GraphemeCursor, &str, usize) -> Result<T, GraphemeIncomplete>,
    ) -> T {
        let mut cursor = GraphemeCursor::new(offset, self.len_bytes(), true);
        let (mut chunk, mut chunk_start, _, _) = self.rope.chunk_at_byte(offset);
        loop {
            match op(&mut cursor, chunk, chunk_start) {
                Ok(v) => return v,
                Err(GraphemeIncomplete::NextChunk) => {
                    chunk_start += chunk.len();
                    chunk = self.rope.chunk_at_byte(chunk_start).0;
                }
                Err(GraphemeIncomplete::PrevChunk) => {
                    let (c, s, _, _) = self.rope.chunk_at_byte(chunk_start - 1);
                    chunk = c;
                    chunk_start = s;
                }
                Err(GraphemeIncomplete::PreContext(n)) => {
                    let (c, s, _, _) = self.rope.chunk_at_byte(n - 1);
                    cursor.provide_context(c, s);
                }
                // 検査済みの位置と、カーソルの要求どおりのチャンクを渡しているため起きない
                Err(GraphemeIncomplete::InvalidOffset) => {
                    unreachable!("grapheme cursor got an invalid chunk")
                }
            }
        }
    }

    /// `offset` より後ろの次の文字（コードポイント）の境界。内容の末尾では `None`。
    pub fn next_char_boundary(&self, offset: usize) -> Result<Option<usize>, TextError> {
        self.check_offset(offset)?;
        if offset == self.len_bytes() {
            return Ok(None);
        }
        Ok(Some(self.rope.char_to_byte(self.char_of(offset) + 1)))
    }

    /// `offset` より前の文字（コードポイント）の境界。内容の先頭では `None`。
    pub fn prev_char_boundary(&self, offset: usize) -> Result<Option<usize>, TextError> {
        self.check_offset(offset)?;
        if offset == 0 {
            return Ok(None);
        }
        Ok(Some(self.rope.char_to_byte(self.char_of(offset) - 1)))
    }

    // --- 編集（buffer から使う） ---

    /// 検査済みの範囲を置換する。版番号は変えない。
    pub(crate) fn replace(&mut self, range: Range<usize>, insert: String) -> AppliedEdit {
        let (cs, ce) = (self.char_of(range.start), self.char_of(range.end));
        let deleted: String = self.rope.slice(cs..ce).chunks().collect();
        self.rope.remove(cs..ce);
        self.rope.insert(cs, &insert);
        AppliedEdit {
            start: range.start,
            deleted,
            inserted: insert,
        }
    }
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.rope.chunks().try_for_each(|c| f.write_str(c))
    }
}
