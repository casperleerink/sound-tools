//! The anchoring, surface and trigger helpers of the overlays: the dropdown menu, the select and
//! the split button.

use gpui::{
    Anchor, App, BoxShadow, Div, Stateful, anchored, deferred, div, hsla, point, prelude::*, px,
};

use crate::scale::ControlSize;
use crate::scale::radius;
use crate::theme::ActiveTheme;
use crate::typography::{TextVariant, Typography};

/// Which side of the trigger the content opens on.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Side {
    #[default]
    Bottom,
    Top,
}

/// Which edge the content lines up with.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Align {
    #[default]
    Start,
    End,
}

/// The size of the trigger buttons of the overlays.
pub(crate) const TRIGGER_SIZE: ControlSize = ControlSize::Lg;

pub fn shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hsla(0., 0., 0., 0.2),
        offset: point(px(0.), px(4.)),
        blur_radius: px(24.),
        spread_radius: px(-8.),
        inset: false,
    }]
}

/// The shared overlay surface, with the dropdown [`shadow`].
pub fn surface(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .occlude()
        .bg(theme.surface())
        .border_1()
        .border_color(theme.border_strong())
        .rounded(radius::LG)
        .text_color(theme.text())
        .shadow(shadow())
}

/// Zero-size box pinned to the chosen trigger edge; the deferred `anchored`
/// content hangs off it, so alignment does not depend on the trigger's size.
pub fn anchor(side: Side, align: Align, content: impl IntoElement) -> Div {
    let corner = match (side, align) {
        (Side::Bottom, Align::Start) => Anchor::TopLeft,
        (Side::Bottom, Align::End) => Anchor::TopRight,
        (Side::Top, Align::Start) => Anchor::BottomLeft,
        (Side::Top, Align::End) => Anchor::BottomRight,
    };
    let offset = match side {
        Side::Bottom => point(px(0.), px(6.)),
        Side::Top => point(px(0.), px(-6.)),
    };
    div()
        .absolute()
        .map(|d| match side {
            Side::Bottom => d.top_full(),
            Side::Top => d.bottom_full(),
        })
        .map(|d| match align {
            Align::Start => d.left_0(),
            Align::End => d.right_0(),
        })
        .child(
            deferred(
                anchored()
                    .anchor(corner)
                    .offset(offset)
                    .snap_to_window_with_margin(px(8.))
                    .child(content),
            )
            .with_priority(1),
        )
}

/// Outline trigger button shared by the overlay components.
pub(crate) fn trigger(id: &'static str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(TRIGGER_SIZE.height()))
        .px(px(12.))
        .rounded(TRIGGER_SIZE.radius())
        .border_1()
        .border_color(theme.border_strong())
        .bg(theme.fill())
        .text_variant(TextVariant::Heading)
        .text_color(theme.text())
        .cursor_pointer()
        .hover(|s| s.bg(theme.fill_strong()))
}
