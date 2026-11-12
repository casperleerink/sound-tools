//! The parts of a device card a view names: its display, its cells, a row of steps and a grid
//! of them.

use serde::Deserialize;

use super::{Num, changed};

props! {
    /// The well at the left of a card, as wide as a built-in one or `wide`, with a quiet line
    /// under it. The space among its children draws in it; the others sit above that.
    DisplayProps / DisplayChange {
        wide: bool,
        caption: String,
        scale: Scale,
        /// The line of no change, at a place down its drawing from 0 to 1.
        zero: f32,
    }
}

/// The marks of the scale of a display, at places from 0 to 1 of its drawing, `y` down.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scale {
    #[serde(default)]
    pub x: Vec<Mark>,
    #[serde(default)]
    pub y: Vec<f32>,
}

/// A line of a scale at a place across, alone or with its word.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Mark {
    Line(f32),
    Worded(f32, String),
}

props! {
    /// A control of a card with its label line and value line under it.
    CellProps / CellChange { label: String, value: String }
}

props! {
    /// A row of steps; a press or a key on one tells its code its place.
    StepsProps / StepsChange {
        key: String,
        /// How strong each step is: 0 off, 1 full, and a softer hit between.
        levels: Vec<f32>,
        /// The place of the step that plays now, or less than 0.
        playing: Num,
        label: String,
        /// A row that changes another row, such as accents: quieter than a row of notes.
        secondary: bool,
        on_change: bool,
    }
}

props! {
    /// Rows of steps one under the other, then the controls of the whole of it beside them.
    GridProps / GridChange {
        /// What each column of the controls of its rows is, said once over the first row.
        columns: Vec<String>,
    }
}
