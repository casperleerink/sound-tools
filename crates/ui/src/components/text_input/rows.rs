//! The rows of a text input and the caret and scroll math over them, apart from GPUI so it can
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

/// The row at `y` from the top of the text: the first row above it and the last below it, as
/// a drag past the edge selects.
pub(super) fn row_at(y: Pixels, line_height: Pixels, row_count: usize) -> usize {
    if line_height <= px(0.) {
        return 0;
    }
    ((y.max(px(0.)) / line_height) as usize).min(row_count.saturating_sub(1))
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

/// The scroll offset along one axis that keeps the caret, from `caret_start` to `caret_end`, in
/// view, from the offset of the last frame. A caret bigger than the view shows its start.
pub(super) fn scroll_to_show(
    scroll: Pixels,
    caret_start: Pixels,
    caret_end: Pixels,
    view: Pixels,
    content: Pixels,
) -> Pixels {
    let scroll = match caret_end > scroll + view {
        true => caret_end - view,
        false => scroll,
    };
    clamp_scroll(scroll.min(caret_start), view, content)
}

/// A scroll offset that shows no room before the start of the text or past its end.
pub(super) fn clamp_scroll(scroll: Pixels, view: Pixels, content: Pixels) -> Pixels {
    scroll.min(content - view).max(px(0.))
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
    fn rows_of_text_with_more_bytes_to_a_character() {
        // "ü" is 2 bytes, "😀" 4, and each CJK character 3. CJK wraps between characters.
        let text = "ü😀\n日本語日本";
        let rows = rows(text, &[vec![], vec![9]]);
        assert_eq!(ranges(&rows), [0..6, 7..16, 16..22]);
        let caret_ends: Vec<_> = rows.iter().map(|row| row.caret_end).collect();
        assert_eq!(caret_ends, [6, 13, 22]);
        assert!(rows.iter().all(|row| text.is_char_boundary(row.caret_end)));
        assert_eq!(row_of(&rows, 2), 0);
        assert_eq!(row_of(&rows, 13), 1);
        assert_eq!(row_of(&rows, 16), 2);
    }

    #[test]
    fn a_point_finds_its_row() {
        let height = px(20.);
        assert_eq!(row_at(px(-1.), height, 3), 0);
        assert_eq!(row_at(px(0.), height, 3), 0);
        assert_eq!(row_at(px(39.), height, 3), 1);
        assert_eq!(row_at(px(400.), height, 3), 2);
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
        let (view, content) = (px(60.), px(200.));
        let row = |index: f32| (px(20. * index), px(20. * index + 20.));
        let show = |scroll, (start, end)| scroll_to_show(scroll, start, end, view, content);
        // In view: unchanged.
        assert_eq!(show(px(20.), row(2.)), px(20.));
        // Below: the caret row becomes the last visible row.
        assert_eq!(show(px(0.), row(5.)), px(60.));
        // Above: the caret row becomes the first.
        assert_eq!(show(px(100.), row(1.)), px(20.));
        // A caret taller than the view shows its top, from either side: a row of a box
        // shorter than the row used to move 2 px at every key.
        for scroll in [px(0.), px(2.)] {
            assert_eq!(
                scroll_to_show(scroll, px(0.), px(20.), px(18.), px(20.)),
                px(0.)
            );
        }
        // Text that got shorter pulls the scroll back.
        assert_eq!(clamp_scroll(px(180.), view, content), px(140.));
        assert_eq!(clamp_scroll(px(40.), view, px(40.)), px(0.));
    }
}
