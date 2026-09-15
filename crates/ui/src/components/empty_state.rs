//! EmptyState: centred title, one muted line of body text and an optional action element.

use gpui::{AnyElement, App, Div, FontWeight, SharedString, StyleRefinement, Window, div, prelude::*, px};

use crate::theme::ActiveTheme;

#[derive(IntoElement)]
pub struct EmptyState {
    base: Div,
    title: SharedString,
    body: SharedString,
    action: Option<AnyElement>,
}

impl EmptyState {
    pub fn new(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            title: title.into(),
            body: body.into(),
            action: None,
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl Styled for EmptyState {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (title_color, body_color) = (theme.gray_950, theme.gray_700);

        self.base
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(24.))
            .py(px(64.))
            .text_center()
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(title_color)
                    .child(self.title),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(14.))
                    .text_color(body_color)
                    .child(self.body),
            )
            .when_some(self.action, |state, action| {
                state.child(div().mt(px(16.)).child(action))
            })
    }
}
