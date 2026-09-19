//! Dialog: centred panel over a dimmed backdrop, with a title, a body and a
//! footer of actions. Ported from Hooman Studio `dialog.tsx` (desktop form only).

use std::rc::Rc;

use gpui::{
    App, BoxShadow, ClickEvent, Context, Anchor, FocusHandle, FontWeight, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, Render, SharedString, Window, anchored, deferred,
    div, hsla, point, prelude::*, px,
};

use crate::theme::ActiveTheme;

type ActionFn = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct DialogAction {
    label: SharedString,
    primary: bool,
    on_click: Option<ActionFn>,
}

impl DialogAction {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            primary: false,
            on_click: None,
        }
    }

    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }

    /// Runs, then closes the dialog.
    pub fn on_click(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

pub struct Dialog {
    focus_handle: FocusHandle,
    title: SharedString,
    body: SharedString,
    actions: Vec<DialogAction>,
    open: bool,
    width: f32,
}

impl Dialog {
    pub fn new(
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
            open: false,
            width: 420.,
        }
    }

    pub fn action(mut self, action: DialogAction) -> Self {
        self.actions.push(action);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }
}

impl Render for Dialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div();
        }

        let viewport = window.viewport_size();
        let theme = cx.theme();
        let (surface, border, text, muted, line, accent, accent_text) = (
            theme.gray_200,
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.gray_700,
            theme.alpha_at(0.10),
            theme.lavender,
            theme.gray_200,
        );
        let width = self.width;

        let actions: Vec<_> = self
            .actions
            .iter()
            .enumerate()
            .map(|(ix, action)| {
                let handler = action.on_click.clone();
                let primary = action.primary;
                div()
                    .id(("dialog-action", ix))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .h(px(32.))
                    .px(px(14.))
                    .rounded(px(8.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(primary, |d| {
                        d.bg(accent)
                            .text_color(accent_text)
                            .hover(|s| s.bg(accent.opacity(0.85)))
                    })
                    .when(!primary, |d| {
                        d.border_1()
                            .border_color(border)
                            .text_color(text)
                            .hover(|s| s.bg(line))
                    })
                    .child(action.label.clone())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        if let Some(handler) = handler.clone() {
                            handler(window, cx);
                        }
                        this.close(cx);
                    }))
            })
            .collect();

        div().child(
            deferred(
                anchored()
                    .position(point(px(0.), px(0.)))
                    .anchor(Anchor::TopLeft)
                    .child(
                        div()
                            .occlude()
                            .w(viewport.width)
                            .h(viewport.height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(hsla(0., 0., 0., 0.5))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| this.close(cx)),
                            )
                            .child(
                                div()
                                    .occlude()
                                    .track_focus(&self.focus_handle)
                                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                                        if ev.keystroke.key == "escape" {
                                            this.close(cx);
                                        }
                                    }))
                                    .w(px(width))
                                    .flex()
                                    .flex_col()
                                    .rounded(px(16.))
                                    .bg(surface)
                                    .border_1()
                                    .border_color(border)
                                    .text_color(text)
                                    .shadow(vec![BoxShadow {
                                        color: hsla(0., 0., 0., 0.2),
                                        offset: point(px(0.), px(8.)),
                                        blur_radius: px(24.),
                                        spread_radius: px(-8.),
                                        inset: false,
                                    }])
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .p(px(16.))
                                            .child(
                                                div()
                                                    .text_size(px(16.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(self.title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .text_color(muted)
                                                    .child(self.body.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .justify_end()
                                            .gap(px(8.))
                                            .p(px(16.))
                                            .border_t_1()
                                            .border_color(line)
                                            .children(actions),
                                    ),
                            ),
                    ),
            )
            .with_priority(2),
        )
    }
}
