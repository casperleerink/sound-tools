//! Popover: a trigger plus floating content. Stateful view, because it owns
//! `open`. Also holds the anchoring and surface helpers the other overlays use
//! (dropdown menu, select, dialog). Ported from Hooman Studio `popover.tsx`.

use std::rc::Rc;

use gpui::{
    AnyElement, App, BoxShadow, Context, Corner, Div, FocusHandle, FontWeight, KeyDownEvent,
    MouseDownEvent, Render, SharedString, Stateful, Window, anchored, deferred, div, hsla,
    point, prelude::*, px,
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
        }])
}

/// Zero-size box pinned to the chosen trigger edge; the deferred `anchored`
/// content hangs off it, so alignment does not depend on the trigger's size.
pub(crate) fn anchor(side: Side, align: Align, content: impl IntoElement) -> Div {
    let corner = match (side, align) {
        (Side::Bottom, Align::Start) => Corner::TopLeft,
        (Side::Bottom, Align::End) => Corner::TopRight,
        (Side::Top, Align::Start) => Corner::BottomLeft,
        (Side::Top, Align::End) => Corner::BottomRight,
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

type ContentFn = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

pub struct Popover {
    focus_handle: FocusHandle,
    label: SharedString,
    content: ContentFn,
    open: bool,
    side: Side,
    align: Align,
    width: f32,
}

impl Popover {
    pub fn new(
        label: impl Into<SharedString>,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            label: label.into(),
            content: Rc::new(content),
            open: false,
            side: Side::default(),
            align: Align::default(),
            width: 224.,
        }
    }

    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }
}

impl Render for Popover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.content.clone();
        let width = self.width;
        let (side, align) = (self.side, self.align);

        div()
            .relative()
            .flex()
            .flex_none()
            .child(
                trigger("popover-trigger", cx)
                    .child(self.label.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.open {
                            this.close(cx);
                        } else {
                            this.open(window, cx);
                        }
                    })),
            )
            .when(self.open, |d| {
                d.child(anchor(
                    side,
                    align,
                    surface(cx)
                        .track_focus(&self.focus_handle)
                        .w(px(width))
                        .p(px(12.))
                        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.close(cx)
                        }))
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                            if ev.keystroke.key == "escape" {
                                this.close(cx);
                            }
                        }))
                        .child(content(_window, cx)),
                ))
            })
    }
}
