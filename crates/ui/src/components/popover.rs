//! The anchoring, surface and trigger helpers of the overlays: the dropdown menu, which is also
//! the select. Ported from the source design system's `popover.tsx`.

use gpui::{
    Anchor, App, BoxShadow, Div, FontWeight, Stateful, anchored, deferred, div, hsla, point,
    prelude::*, px,
};

use crate::theme::ActiveTheme;

/// Which side of the trigger the content opens on, like the React `side` prop.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Side {
    #[default]
    Bottom,
    Top,
}

/// Which edge the content lines up with, like the React `align` prop.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Align {
    #[default]
    Start,
    End,
}

/// Standard height of the small trigger buttons in this file.
pub(crate) const TRIGGER_HEIGHT: f32 = 32.;

/// The shared overlay surface: `gray_200` with an `alpha/10` border and the
/// dropdown shadow from `globals.css` (`0 4px 24px -8px rgba(0,0,0,0.2)`).
pub(crate) fn surface(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .occlude()
        .bg(theme.gray_200)
        .border_1()
        .border_color(theme.alpha_at(0.10))
        .rounded(px(10.))
        .text_color(theme.gray_950)
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.2),
            offset: point(px(0.), px(4.)),
            blur_radius: px(24.),
            spread_radius: px(-8.),
            inset: false,
        }])
}

/// Zero-size box pinned to the chosen trigger edge; the deferred `anchored`
/// content hangs off it, so alignment does not depend on the trigger's size.
pub(crate) fn anchor(side: Side, align: Align, content: impl IntoElement) -> Div {
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
        .h(px(TRIGGER_HEIGHT))
        .px(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.alpha_at(0.10))
        .bg(theme.alpha_at(0.05))
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.gray_950)
        .cursor_pointer()
        .hover(|s| s.bg(theme.alpha_at(0.10)))
}
