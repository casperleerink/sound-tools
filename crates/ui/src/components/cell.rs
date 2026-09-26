//! Cell: the place of one control on a device card, 56 x 72 pt. The control in the top 36 pt,
//! centred, so a 36 pt knob fills it and a 24 pt toggle sits 6 pt from the top, on the line of a
//! knob's centre. Under it the label line at 38 and the value line at 54, 14 pt each, in 12 pt
//! type. A control with no value leaves its value line empty, so every row of a card lines up.
//!
//! The knob is a cell of its own. `Cell` puts any other control into the same frame.

use gpui::{
    AnyElement, App, Div, Hsla, SharedString, StyleRefinement, Window, div, prelude::*, px,
};

use crate::theme::ActiveTheme;
use crate::typography;

pub const CELL_WIDTH: f32 = 56.;
/// A row of cells. A card body has two.
pub const ROW_HEIGHT: f32 = 72.;
/// The room of the control at the top of a cell: the size of a knob.
pub const CONTROL_HEIGHT: f32 = 36.;
const LINE_TOP: f32 = 38.;
const LINE_HEIGHT: f32 = 14.;

#[derive(IntoElement)]
pub struct Cell {
    base: Div,
    control: Option<AnyElement>,
    label: Option<SharedString>,
    value: Option<SharedString>,
}

impl Cell {
    pub fn new(control: impl IntoElement) -> Self {
        Self {
            base: div(),
            control: Some(control.into_any_element()),
            label: None,
            value: None,
        }
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
        frame(self.base, self.control, self.label, self.value, cx)
    }
}

/// The frame of a cell around its control. The knob uses it too.
pub(crate) fn frame(
    base: Div,
    control: Option<AnyElement>,
    label: Option<SharedString>,
    value: Option<SharedString>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let (label_color, value_color) = (theme.gray_800, theme.gray_950);
    base.relative()
        .flex_none()
        .w(px(CELL_WIDTH))
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
        .children(label.map(|label| line(LINE_TOP, label_color, false).child(label)))
        .children(
            value.map(|value| line(LINE_TOP + LINE_HEIGHT + 2., value_color, true).child(value)),
        )
}

/// One line of 12 pt text, centred on the cell. It may be wider than the cell: a long label
/// such as `Resonance` runs into the air next to it rather than being cut.
fn line(top: f32, color: Hsla, tabular: bool) -> Div {
    div()
        .absolute()
        .top(px(top))
        .left(px(-12.))
        .w(px(CELL_WIDTH + 24.))
        .flex()
        .justify_center()
        .h(px(LINE_HEIGHT))
        .text_size(px(12.))
        .line_height(px(LINE_HEIGHT))
        .text_color(color)
        .whitespace_nowrap()
        .when(tabular, |line| line.font(typography::tabular()))
}
