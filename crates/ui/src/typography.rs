//! Font setup. Apply `ui_font()` on the window root; use `tabular()` on numbers.

use std::sync::Arc;

use gpui::{Font, FontFeatures, FontStyle, FontWeight};

pub const FAMILY: &str = "Inter Display";
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
