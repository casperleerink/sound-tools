//! Alert: a confirm block with title, description and two buttons. Variants `primary` and
//! `danger` (danger colours the icon, title and confirm button red).

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, FontWeight, SharedString, StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::icon::Icon;
use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AlertVariant {
    #[default]
    Primary,
    Danger,
}

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Alert {
    base: Div,
    title: SharedString,
    description: SharedString,
    variant: AlertVariant,
    action_label: SharedString,
    cancel_label: SharedString,
    on_action: Option<Handler>,
    on_cancel: Option<Handler>,
}

impl Alert {
    pub fn new(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            title: title.into(),
            description: description.into(),
            variant: AlertVariant::default(),
            action_label: "Confirm".into(),
            cancel_label: "Cancel".into(),
            on_action: None,
            on_cancel: None,
        }
    }

    pub fn variant(mut self, variant: AlertVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn action_label(mut self, label: impl Into<SharedString>) -> Self {
        self.action_label = label.into();
        self
    }

    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = label.into();
        self
    }

    pub fn on_action(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    pub fn on_cancel(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(f));
        self
    }
}

impl Styled for Alert {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Alert {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let danger = self.variant == AlertVariant::Danger;
        let accent = if danger { theme.red } else { theme.gray_950 };
        let (bg, border, body) = (theme.gray_100, theme.alpha_at(0.10), theme.gray_700);
        let icon = if danger { "triangle-alert" } else { "info" };
        let action_variant = if danger {
            ButtonVariant::Solid(theme.red)
        } else {
            ButtonVariant::Primary
        };
        let on_action = self.on_action;
        let on_cancel = self.on_cancel;

        self.base
            .flex()
            .flex_col()
            .w(px(360.))
            .rounded(px(16.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(24.))
                    .text_center()
                    .child(Icon::new(icon).size(20.).color(accent))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(accent)
                            .child(self.title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(body)
                            .child(self.description),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .p(px(20.))
                    .border_t_1()
                    .border_color(theme.alpha_at(0.05))
                    .child(
                        div().flex_1().child(
                            Button::new("alert-cancel", self.cancel_label)
                                .variant(ButtonVariant::Subtle)
                                .size(ButtonSize::Lg)
                                .w_full()
                                .when_some(on_cancel, |b, f| {
                                    b.on_click(move |event, window, cx| f(event, window, cx))
                                }),
                        ),
                    )
                    .child(
                        div().flex_1().child(
                            Button::new("alert-action", self.action_label)
                                .variant(action_variant)
                                .size(ButtonSize::Lg)
                                .w_full()
                                .when_some(on_action, |b, f| {
                                    b.on_click(move |event, window, cx| f(event, window, cx))
                                }),
                        ),
                    ),
            )
    }
}
