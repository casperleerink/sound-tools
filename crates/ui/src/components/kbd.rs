//! Kbd: one keyboard shortcut drawn as key caps. `Kbd::new("mod+k")` renders `⌘ K`.

use gpui::{App, Div, SharedString, StyleRefinement, Window, div, prelude::*, px};

use crate::theme::ActiveTheme;

#[derive(IntoElement)]
pub struct Kbd {
    base: Div,
    shortcut: SharedString,
}

impl Kbd {
    /// Keys separated by `+`, e.g. `"mod+shift+k"`.
    pub fn new(shortcut: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            shortcut: shortcut.into(),
        }
    }
}

impl Styled for Kbd {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

fn key_label(key: &str) -> String {
    match key.to_lowercase().as_str() {
        "mod" | "cmd" => "⌘".into(),
        "alt" | "option" => "⌥".into(),
        "shift" => "⇧".into(),
        "ctrl" => "⌃".into(),
        "enter" => "↵".into(),
        _ => key.to_uppercase(),
    }
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, border, text) = (
            theme.alpha_at(0.04),
            theme.alpha_at(0.05),
            theme.gray_900,
        );

        self.base
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .children(self.shortcut.split('+').map(|key| {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(16.))
                    .min_w(px(16.))
                    .px(px(3.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .text_size(px(12.))
                    .text_color(text)
                    .child(key_label(key))
            }))
    }
}
