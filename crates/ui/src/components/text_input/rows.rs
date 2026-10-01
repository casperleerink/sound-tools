//! The rows of a multi-line text input and the caret math over them, apart from GPUI so it can
//! be tested. A row is one visual line: a hard line between newlines, or a part of one that
//! wrapped at the width of the box.

use std::ops::Range;

use gpui::{Pixels, px};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Row {
    /// Byte range of the row in the text, without the newline that ends it.
    pub range: Range<usize>,
    /// The last place the caret may stop on this row. A wrapped row gives its end to the next
    /// row, so every offset lies on exactly one row.
    pub caret_end: usize,
    /// The hard line the row is part of, and where that line starts in the text.
    pub line: usize,
    pub line_start: usize,
}

/// Up or down from the caret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrow {
    Up,
    Down,
}

/// The rows of `text`. `wraps[line]` holds the offsets inside each hard line where it wrapped.
pub(super) fn rows(text: &str, wraps: &[Vec<usize>]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut line_start = 0;
    for (line, line_text) in text.split('\n').enumerate() {
        let line_end = line_start + line_text.len();
        let mut row_start = line_start;
        for wrap in wraps.get(line).into_iter().flatten() {
            let row_end = line_start + wrap;
            if row_end <= row_start || row_end >= line_end {
                continue;
            }
            let caret_end = text
                .get(..row_end)
                .and_then(|before| before.char_indices().next_back())
                .map_or(row_start, |(index, _)| index.max(row_start));
            rows.push(Row {
                range: row_start..row_end,
                caret_end,
                line,
                line_start,
            });
            row_start = row_end;
        }
        rows.push(Row {
            range: row_start..line_end,
            caret_end: line_end,
            line,
            line_start,
        });
        line_start = line_end + '\n'.len_utf8();
    }
    rows
}

/// The row the caret is on at `offset`.
pub(super) fn row_of(rows: &[Row], offset: usize) -> usize {
    rows.iter()
        .position(|row| offset <= row.caret_end)
        .unwrap_or(rows.len().saturating_sub(1))
}

/// The row an arrow key moves to, or `None` past the first or last row.
pub(super) fn row_after(rows: &[Row], row: usize, arrow: Arrow) -> Option<usize> {
    match arrow {
        Arrow::Up => row.checked_sub(1),
        Arrow::Down => (row + 1 < rows.len()).then_some(row + 1),
    }
}

/// The row at `y` from the top of the text, the last row below it, `None` above it.
pub(super) fn row_at(y: Pixels, line_height: Pixels, row_count: usize) -> Option<usize> {
    if y < px(0.) || line_height <= px(0.) {
        return None;
    }
    Some(((y / line_height) as usize).min(row_count.saturating_sub(1)))
}

/// The part of `selection` on each row it touches: the row, its byte range there, and whether
/// the selection goes on past the end of the row (over a newline or a wrap).
pub(super) fn selection_spans(
    rows: &[Row],
    selection: &Range<usize>,
) -> Vec<(usize, Range<usize>, bool)> {
    let first = row_of(rows, selection.start);
    let last = row_of(rows, selection.end);
    (first..=last)
        .filter_map(|index| {
            let row = rows.get(index)?;
            let start = if index == first {
                selection.start
            } else {
                row.range.start
            };
            let end = if index == last {
                selection.end
            } else {
                row.range.end
            };
            let continues = index != last;
            (start < end || continues).then_some((index, start..end, continues))
        })
        .collect()
}

/// The scroll offset that keeps the caret row in view, from the offset of the last frame.
pub(super) fn scroll_to_show(
    scroll_top: Pixels,
    caret_row: usize,
    line_height: Pixels,
    view_height: Pixels,
    content_height: Pixels,
) -> Pixels {
    let caret_top = line_height * caret_row;
    let caret_bottom = caret_top + line_height;
    let top = if caret_top < scroll_top {
        caret_top
    } else if caret_bottom > scroll_top + view_height {
        caret_bottom - view_height
    } else {
        scroll_top
    };
    clamp_scroll(top, view_height, content_height)
}

/// A scroll offset that shows no room above the first row or below the last.
pub(super) fn clamp_scroll(top: Pixels, view_height: Pixels, content_height: Pixels) -> Pixels {
    top.min(content_height - view_height).max(px(0.))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(rows: &[Row]) -> Vec<Range<usize>> {
        rows.iter().map(|row| row.range.clone()).collect()
    }

    #[test]
    fn hard_and_wrapped_lines_make_rows() {
        let text = "hello world foo\nbar\n";
        let rows = rows(text, &[vec![12], vec![], vec![]]);
        assert_eq!(ranges(&rows), [0..12, 12..15, 16..19, 20..20]);
        // A wrapped row ends before its trailing space; a hard line ends at its newline.
        let caret_ends: Vec<_> = rows.iter().map(|row| row.caret_end).collect();
        assert_eq!(caret_ends, [11, 15, 19, 20]);
        let lines: Vec<_> = rows.iter().map(|row| (row.line, row.line_start)).collect();
        assert_eq!(lines, [(0, 0), (0, 0), (1, 16), (2, 20)]);
    }

    #[test]
    fn an_empty_text_is_one_row() {
        assert_eq!(ranges(&rows("", &[])), vec![0..0]);
    }

    #[test]
    fn a_wrap_inside_a_character_or_at_an_end_is_ignored() {
        assert_eq!(ranges(&rows("ab", &[vec![0, 2]])), vec![0..2]);
        // "é" is two bytes, so the caret end of the first row is before it.
        let rows = rows("aébc", &[vec![3]]);
        assert_eq!(ranges(&rows), [0..3, 3..5]);
        assert_eq!(rows[0].caret_end, 1);
    }

    #[test]
    fn every_offset_lies_on_one_row() {
        let rows = rows("hello world foo\nbar", &[vec![12]]);
        let rows_of: Vec<_> = (0..=19).map(|offset| row_of(&rows, offset)).collect();
        assert_eq!(
            rows_of,
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2]
        );
        assert_eq!(row_of(&rows, 99), 2);
    }

    #[test]
    fn arrows_stop_at_the_first_and_last_row() {
        let rows = rows("a\nb\nc", &[]);
        assert_eq!(row_after(&rows, 0, Arrow::Up), None);
        assert_eq!(row_after(&rows, 1, Arrow::Up), Some(0));
        assert_eq!(row_after(&rows, 1, Arrow::Down), Some(2));
        assert_eq!(row_after(&rows, 2, Arrow::Down), None);
    }

    #[test]
    fn a_point_finds_its_row() {
        let height = px(20.);
        assert_eq!(row_at(px(-1.), height, 3), None);
        assert_eq!(row_at(px(0.), height, 3), Some(0));
        assert_eq!(row_at(px(39.), height, 3), Some(1));
        assert_eq!(row_at(px(400.), height, 3), Some(2));
    }

    #[test]
    fn a_selection_spans_rows() {
        let rows = rows("hello world foo\nbar", &[vec![12]]);
        assert_eq!(selection_spans(&rows, &(1..3)), [(0, 1..3, false)]);
        assert_eq!(
            selection_spans(&rows, &(6..18)),
            [(0, 6..12, true), (1, 12..15, true), (2, 16..18, false)]
        );
        // Ending at the start of a row selects nothing on that row, but the newline before it.
        assert_eq!(selection_spans(&rows, &(13..16)), [(1, 13..15, true)]);
    }

    #[test]
    fn an_empty_line_inside_a_selection_shows_its_newline() {
        let rows = rows("a\n\nb", &[]);
        assert_eq!(
            selection_spans(&rows, &(0..4)),
            [(0, 0..1, true), (1, 2..2, true), (2, 3..4, false)]
        );
    }

    #[test]
    fn the_scroll_follows_the_caret_and_stays_inside_the_text() {
        let (height, view, content) = (px(20.), px(60.), px(200.));
        // In view: unchanged.
        assert_eq!(scroll_to_show(px(20.), 2, height, view, content), px(20.));
        // Below: the caret row becomes the last visible row.
        assert_eq!(scroll_to_show(px(0.), 5, height, view, content), px(60.));
        // Above: the caret row becomes the first.
        assert_eq!(scroll_to_show(px(100.), 1, height, view, content), px(20.));
        // Text that got shorter pulls the scroll back.
        assert_eq!(clamp_scroll(px(180.), view, content), px(140.));
        assert_eq!(clamp_scroll(px(40.), view, px(40.)), px(0.));
    }
}
