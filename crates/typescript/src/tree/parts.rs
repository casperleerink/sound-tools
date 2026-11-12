//! The parts of a device card a view names: its display, its cells and a row of steps.

use serde::Deserialize;

use super::{Num, changed};

props! {
    /// The well at the left of a card, as wide as a built-in one or `wide`, with a quiet line
    /// under it. The space among its children draws in it; the others sit above that.
    DisplayProps / DisplayChange { wide: bool, caption: String }
}

props! {
    /// A control of a card with its label line and value line under it.
    CellProps / CellChange { label: String, value: String }
}

props! {
    /// A row of steps, each on or off; a press or a key on one tells its code its place.
    StepsProps / StepsChange {
        key: String,
        on: Vec<bool>,
        /// The place of the step that plays now, or less than 0.
        playing: Num,
        label: String,
        on_change: bool,
    }
}
