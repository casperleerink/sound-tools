//! Cell: the place of one control on a device card. The control at the top, centred on the line
//! of a knob's centre, then a label line and a value line, so every row of a card lines up.
//!
//! A cell says each thing once. The value line reads what the control does not say: `Low shelf`
//! under a select that shows a picture. A control that says its value, such as a select on
//! `10 ms`, has only its label, and one that says what it is has no line at all: a segmented
//! control on `Clean`, or a switch, which carries its name ([`Cell::switch`]). A unit is never
//! on a line alone.
//!
//! The knob is a cell of its own. `Cell` puts any other control into the same frame. A cell may
//! span two columns for a control that needs the room; its lines are centred on the whole width.

use gpui::{
    AnyElement, App, Div, Hsla, SharedString, StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::toggle::Toggle;
use crate::theme::ActiveTheme;
use crate::typography;
use crate::typography::{TextVariant, Typography};

pub const CELL_WIDTH: f32 = 64.;
/// A row of cells. A card body has two.
pub const ROW_HEIGHT: f32 = 72.;
/// The room of the control at the top of a cell: the size of a knob.
pub const CONTROL_HEIGHT: f32 = 36.;
const LINE_TOP: f32 = 38.;
pub(crate) const LINE_HEIGHT: f32 = 14.;
/// The value line of a cell, from its top.
pub const VALUE_LINE: f32 = LINE_TOP + LINE_HEIGHT + 2.;

/// How wide a control with words on it is in a cell of `columns`, such as a select: the cell
/// less a little air on each side, so its width does not follow its words.
pub const fn control_width(columns: usize) -> f32 {
    columns as f32 * CELL_WIDTH - 8.
}

enum Control {
    Any(AnyElement),
    Switch(Box<Toggle>),
}

#[derive(IntoElement)]
pub struct Cell {
    base: Div,
    control: Control,
    label: Option<SharedString>,
    value: Option<SharedString>,
    columns: usize,
}

impl Cell {
    pub fn new(control: impl IntoElement) -> Self {
        Self::of(Control::Any(control.into_any_element()))
    }

    /// A switch of the device: a toggle whose word is its name, as wide as the control of its
    /// cell whatever it says, so the switches of a card line up. A name that does not fit one
    /// column takes two ([`Cell::span`]).
    pub fn switch(toggle: Toggle) -> Self {
        Self::of(Control::Switch(Box::new(toggle)))
    }

    fn of(control: Control) -> Self {
        Self {
            base: div(),
            control,
            label: None,
            value: None,
            columns: 1,
        }
    }

    /// How many columns of cells it takes, from 1.
    pub fn span(mut self, columns: usize) -> Self {
        self.columns = columns.max(1);
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = Some(value.into());
        self
    }
}

impl Styled for Cell {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Cell {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let width = CELL_WIDTH * self.columns as f32;
        let control = match self.control {
            Control::Any(control) => control,
            Control::Switch(toggle) => toggle.width(control_width(self.columns)).into_any_element(),
        };
        frame(self.base, Some(control), self.label, self.value, width, cx)
    }
}

/// The frame of a cell around its control. The knob uses it too.
pub(crate) fn frame(
    base: Div,
    control: Option<AnyElement>,
    label: Option<SharedString>,
    value: Option<SharedString>,
    width: f32,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let (label_color, value_color) = (theme.text_secondary(), theme.text());
    base.relative()
        .flex_none()
        .w(px(width))
        .h(px(ROW_HEIGHT))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(CONTROL_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .children(control),
        )
        .children(label.map(|label| line(LINE_TOP, width, label_color).child(label)))
        .children(value.map(|value| {
            line(VALUE_LINE, width, value_color).child(typography::tabular_text(value, value_color))
        }))
}

/// One line of 12 pt text, centred on the cell. It may be wider than the cell: a long label
/// such as `Resonance` runs into the air next to it rather than being cut.
fn line(top: f32, width: f32, color: Hsla) -> Div {
    div()
        .absolute()
        .top(px(top))
        .left(px(-12.))
        .w(px(width + 24.))
        .flex()
        .justify_center()
        .h(px(LINE_HEIGHT))
        .text_variant(TextVariant::Label)
        .line_height(px(LINE_HEIGHT))
        .text_color(color)
        .whitespace_nowrap()
}
