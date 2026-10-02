//! What a control shows while an automation lane of the track moves its value: a 4 pt dot at
//! its top right, and a tooltip that says why it does not drag. The knob and the volume use it.
//!
//! The dot is in the grey of a label and not in the colour of the track: no control takes a
//! track colour (DESIGN.md, "Colour").

use gpui::{App, Div, Stateful, div, prelude::*, px};

use crate::components::tooltip::Tooltip;
use crate::theme::ActiveTheme;

/// What the tooltip of an automated control says.
const TOOLTIP: &str = "Follows the automation of the track";
/// The size of the dot.
pub(crate) const MARK: f32 = 4.;

/// The dot, with its top left corner at `left` and `top` in the control.
pub(crate) fn mark(left: f32, top: f32, cx: &App) -> Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .size(px(MARK))
        .rounded_full()
        .bg(cx.theme().gray_800)
}

/// The control with the tooltip of an automated value.
pub(crate) fn tooltip(control: Stateful<Div>) -> Stateful<Div> {
    control.tooltip(|_, cx| Tooltip::new(TOOLTIP).view(cx))
}
