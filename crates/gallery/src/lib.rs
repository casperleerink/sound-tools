//! Storybook for the UI SDK. Shows every component and variant.

pub mod composed;
pub mod sections;

use gpui::{Context, ScrollHandle, Window, div, prelude::*, px};
use sound_ui::{ActiveTheme, typography};

pub const SECTIONS: [&str; 4] = ["foundation", "inputs", "overlays", "composed"];

pub struct Gallery {
    only: Option<String>,
    scroll: ScrollHandle,
}

impl Gallery {
    /// `only` limits the page to one of [`SECTIONS`].
    pub fn new(only: Option<String>) -> Self {
        Self {
            only,
            scroll: ScrollHandle::new(),
        }
    }
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let show = |name: &str| self.only.as_deref().is_none_or(|o| o == name);
        div()
            .id("gallery")
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .size_full()
            .bg(theme.gray_100)
            .text_color(theme.gray_950)
            .font(typography::ui_font())
            .text_size(px(14.))
            .p(px(40.))
            .flex()
            .flex_col()
            .gap(px(48.))
            .when(show("foundation"), |d| {
                d.child(sections::foundation::section(window, cx))
            })
            .when(show("inputs"), |d| {
                d.child(sections::inputs::section(window, cx))
            })
            .when(show("overlays"), |d| {
                d.child(sections::overlays::section(window, cx))
            })
            .when(show("composed"), |d| {
                d.child(sections::composed::section(window, cx))
            })
    }
}
