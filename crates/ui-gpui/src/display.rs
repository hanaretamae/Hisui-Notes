//! 論理行の表示用の文字列と、バッファの位置との対応（Spec: editor-ui-m2 §4.1）。GPUI に依存しない。

use std::ops::Range;

use app_api::{CompositionState, ViewState};
use app_core::TextSnapshot;

/// 1つの論理行の表示の内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayLine {
    /// 表示する文字列（改行を含まない）。
    pub text: String,
    /// 論理行の先頭のバッファ位置。
    pub line_start: usize,
    /// 変換中の文字列を置いた場合: (表示上の開始位置, 表示上の長さ, 置き換えたバッファの範囲)。
    pub composition: Option<(usize, usize, Range<usize>)>,
    /// 高さ 0 で表示する（変換中の文字列の範囲が複数行にわたる場合の2行目以降）。
    pub hidden: bool,
    /// 選択範囲（表示上の位置）。`bool` は範囲が行末の改行を越えて続くか。
    pub selections: Vec<(Range<usize>, bool)>,
    /// カーソルの表示上の位置。
    pub cursors: Vec<usize>,
}

impl DisplayLine {
    /// 表示上の位置をバッファの位置にする。変換中の文字列の内側は置き換える範囲の始まり。
    pub fn display_to_buffer(&self, d: usize) -> usize {
        match &self.composition {
            Some((cs, len, range)) if d >= *cs => {
                if d < cs + len {
                    range.start
                } else {
                    range.end + (d - cs - len)
                }
            }
            _ => self.line_start + d,
        }
    }

    /// バッファの位置を表示上の位置にする（この行の範囲内の位置について）。
    pub fn buffer_to_display(&self, b: usize) -> usize {
        match &self.composition {
            Some((cs, len, range)) if b >= range.start => {
                if b < range.end {
                    *cs
                } else {
                    cs + len + (b - range.end)
                }
            }
            _ => b.saturating_sub(self.line_start),
        }
    }
}

/// 表示に使う改行以外の文字列にする（バイト長を変えない）。
fn without_breaks(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// 論理行 `line` の表示の内容を作る。
pub fn display_line(
    snapshot: &TextSnapshot,
    state: &ViewState,
    line: usize,
) -> Option<DisplayLine> {
    let range = snapshot.line_range(line).ok()?;
    let with_break = snapshot.line_range_with_break(line).ok()?;
    let comp: Option<&CompositionState> = state.composition.as_ref();
    let comp_lines = comp.and_then(|c| {
        Some((
            snapshot.byte_to_line(c.range.start).ok()?,
            snapshot.byte_to_line(c.range.end).ok()?,
        ))
    });

    let mut out = DisplayLine {
        text: String::new(),
        line_start: range.start,
        composition: None,
        hidden: false,
        selections: Vec::new(),
        cursors: Vec::new(),
    };
    match (comp, comp_lines) {
        (Some(_), Some((first, last))) if line > first && line <= last => {
            out.hidden = true;
            return Some(out);
        }
        (Some(c), Some((first, last))) if line == first => {
            let end_line_end = snapshot.line_range(last).ok()?.end;
            let prefix = snapshot.slice(range.start..c.range.start).ok()?;
            let suffix = snapshot.slice(c.range.end..end_line_end).ok()?;
            out.text = format!("{prefix}{}{suffix}", without_breaks(&c.text));
            let cs = prefix.len();
            out.composition = Some((cs, c.text.len(), c.range.clone()));
            // 変換中は、変換中の文字列の中の選択範囲を表示する
            let sel = cs + c.selected.start..cs + c.selected.end;
            if sel.is_empty() {
                out.cursors.push(sel.start);
            } else {
                out.selections.push((sel.clone(), false));
                out.cursors.push(sel.end);
            }
            return Some(out);
        }
        _ => {}
    }
    out.text = snapshot.slice(range.clone()).ok()?;
    for s in &state.selections {
        let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
        if a < b && a < with_break.end && b > range.start {
            let start = a.max(range.start) - range.start;
            let end = b.min(range.end) - range.start;
            out.selections.push((start..end, b > range.end));
        }
        // カーソルは行末の改行の前まで（次の行の先頭は次の行に描く）
        if s.head >= range.start && s.head <= range.end {
            out.cursors.push(s.head - range.start);
        }
    }
    Some(out)
}

/// 入力用の文字列の長さ（UTF-16）。
pub fn input_len_utf16(snapshot: &TextSnapshot, state: &ViewState) -> usize {
    let total = snapshot.len_utf16();
    match &state.composition {
        Some(c) => {
            let replaced = snapshot
                .slice(c.range.clone())
                .map(|s| s.encode_utf16().count())
                .unwrap_or(0);
            total - replaced + c.text.encode_utf16().count()
        }
        None => total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_api::Selection;

    fn snap(text: &str) -> TextSnapshot {
        document::DocumentSession::new(text).text().clone()
    }

    fn state(
        sels: &[(usize, usize)],
        comp: Option<(Range<usize>, &str, Range<usize>)>,
    ) -> ViewState {
        ViewState {
            selections: sels
                .iter()
                .map(|&(anchor, head)| Selection { anchor, head })
                .collect(),
            primary: 0,
            composition: comp.map(|(range, text, selected)| CompositionState {
                range,
                text: text.into(),
                selected,
            }),
        }
    }

    #[test]
    fn lines_without_composition() {
        let s = snap("ab\r\ncd\n");
        let st = state(&[(1, 5)], None);
        let l0 = display_line(&s, &st, 0).unwrap();
        assert_eq!(l0.text, "ab");
        assert_eq!(l0.selections, vec![(1..2, true)], "改行を越えて続く");
        assert!(l0.cursors.is_empty());
        let l1 = display_line(&s, &st, 1).unwrap();
        assert_eq!((l1.text.as_str(), l1.line_start), ("cd", 4));
        assert_eq!(l1.selections, vec![(0..1, false)]);
        assert_eq!(l1.cursors, vec![1]);
        assert_eq!(display_line(&s, &st, 2).unwrap().text, "", "末尾の空の行");
        assert!(display_line(&s, &st, 3).is_none());
        assert_eq!(l1.display_to_buffer(2), 6);
        assert_eq!(l1.buffer_to_display(6), 2);
    }

    #[test]
    fn cursor_at_line_end_belongs_to_that_line() {
        let s = snap("ab\ncd");
        let st = state(&[(2, 2)], None);
        assert_eq!(display_line(&s, &st, 0).unwrap().cursors, vec![2]);
        assert!(display_line(&s, &st, 1).unwrap().cursors.is_empty());
    }

    #[test]
    fn composition_replaces_range_and_maps_positions() {
        // "前ab後" の "ab" を選択して変換中の文字列 "にほ" に置き換えている
        let s = snap("前ab後\nx");
        let st = state(&[(3, 5)], Some((3..5, "にほ", 6..6)));
        let l = display_line(&s, &st, 0).unwrap();
        assert_eq!(l.text, "前にほ後");
        assert_eq!(l.composition, Some((3, 6, 3..5)));
        assert_eq!(l.cursors, vec![9]);
        assert!(l.selections.is_empty());
        assert_eq!(l.display_to_buffer(0), 0);
        assert_eq!(l.display_to_buffer(6), 3, "変換中の文字列の内側");
        assert_eq!(l.display_to_buffer(9), 5);
        assert_eq!(l.display_to_buffer(12), 8);
        assert_eq!(l.buffer_to_display(8), 12);
        assert_eq!(l.buffer_to_display(4), 3);
        assert_eq!(
            input_len_utf16(&s, &st),
            "前にほ後\nx".encode_utf16().count()
        );
    }

    #[test]
    fn multi_line_composition_hides_following_lines() {
        let s = snap("a1\nb2\nc3");
        let st = state(&[(1, 4)], Some((1..4, "X\nY", 1..2)));
        let l0 = display_line(&s, &st, 0).unwrap();
        assert_eq!(l0.text, "aX Y2", "改行は表示上空白にする（長さは変えない）");
        assert_eq!(l0.selections, vec![(2..3, false)]);
        assert!(display_line(&s, &st, 1).unwrap().hidden);
        let l2 = display_line(&s, &st, 2).unwrap();
        assert!(!l2.hidden);
        assert_eq!(l2.text, "c3");
        assert_eq!(
            l0.display_to_buffer(4),
            4,
            "接尾部はバッファの range.end 以降"
        );
        assert_eq!(l0.display_to_buffer(5), 5);
    }
}
