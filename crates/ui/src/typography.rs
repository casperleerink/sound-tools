//! Font setup. Apply `ui_font()` on the window root; use `tabular()` on numbers, and
//! `tabular_runs()` on text with numbers in it.

use std::sync::Arc;

use gpui::{Font, FontFeatures, FontStyle, FontWeight, Hsla, SharedString, StyledText, TextRun};

const FAMILY: &str = "Inter Display";
pub const MONO: &str = "Menlo";

fn font(weight: FontWeight, features: &[&str]) -> Font {
    Font {
        family: FAMILY.into(),
        features: FontFeatures(Arc::new(
            features.iter().map(|f| (f.to_string(), 1)).collect(),
        )),
        fallbacks: None,
        weight,
        style: FontStyle::Normal,
    }
}

/// InterDisplay with `ss03` and `cv01`, the app's default text font.
pub fn ui_font() -> Font {
    font(FontWeight::NORMAL, &["ss03", "cv01"])
}

/// Same font with tabular lining numbers, for meters, times and values.
pub fn tabular() -> Font {
    font(FontWeight::NORMAL, &["ss03", "cv01", "tnum", "lnum"])
}

/// `run` over `text` split so that its numbers have tabular figures and its words do not:
/// Inter's `tnum` also widens the space, hyphen, stop, comma and colon, which loosens words
/// ("Pre - delay"). A number is its digits, a stop, comma or colon between two digits, and a
/// minus or plus before one.
pub fn tabular_runs(text: &str, run: &TextRun) -> Vec<TextRun> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let digit = |at: Option<&(usize, char)>| at.is_some_and(|(_, c)| c.is_ascii_digit());
    let mut runs: Vec<TextRun> = Vec::new();
    for (index, &(at, c)) in chars.iter().enumerate() {
        let before = index.checked_sub(1).and_then(|before| chars.get(before));
        let after = chars.get(index + 1);
        let number = c.is_ascii_digit()
            || matches!(c, '.' | ',' | ':') && digit(before) && digit(after)
            || matches!(c, '\u{2212}' | '+') && digit(after);
        let font = Font {
            weight: run.font.weight,
            style: run.font.style,
            ..if number { tabular() } else { ui_font() }
        };
        let len = after.map_or(text.len(), |(next, _)| *next) - at;
        match runs.last_mut() {
            Some(last) if last.font == font => last.len += len,
            _ => runs.push(TextRun {
                len,
                font,
                ..run.clone()
            }),
        }
    }
    runs
}

/// `text` in `color`, its numbers with tabular figures: see [`tabular_runs`].
pub fn tabular_text(text: impl Into<SharedString>, color: Hsla) -> StyledText {
    let text = text.into();
    let run = TextRun {
        len: text.len(),
        font: ui_font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs = tabular_runs(&text, &run);
    StyledText::new(text).with_runs(runs)
}

#[cfg(test)]
mod tests {
    use gpui::{TextRun, hsla};

    use super::{tabular, tabular_runs, ui_font};

    #[test]
    fn only_the_numbers_of_a_line_have_tabular_figures() {
        let run = TextRun {
            len: 0,
            font: ui_font(),
            color: hsla(0., 0., 1., 1.),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let text = "Pre-delay \u{2212}20.5 ms · 0:11";
        let mut at = 0;
        let parts: Vec<(&str, bool)> = (tabular_runs(text, &run).into_iter())
            .map(|run| {
                let part = &text[at..at + run.len];
                at += run.len;
                (part, run.font == tabular())
            })
            .collect();
        assert_eq!(
            parts,
            [
                ("Pre-delay ", false),
                ("\u{2212}20.5", true),
                (" ms · ", false),
                ("0:11", true),
            ]
        );
    }
}
