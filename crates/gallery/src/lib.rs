//! Storybook for the UI SDK. Shows every component and variant.

pub mod composed;
pub mod sections;

use gpui::{
    App, Context, FocusHandle, KeyBinding, ScrollHandle, Window, actions, div, prelude::*, px,
};
use sound_ui::{ActiveTheme, typography};

/// `focus` shows one of each control that the keyboard reaches: tab gives each the focus in
/// turn, and one window has one focus, so its snapshot is taken once per tab.
pub const SECTIONS: [&str; 5] = ["foundation", "rack", "focus", "overlays", "composed"];

actions!(gallery, [FocusNext, FocusPrevious]);

const KEY_CONTEXT: &str = "Gallery";

/// Binds tab and shift-tab, as the window of the application does. Call once, after
/// `sound_ui::init`.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(KEY_CONTEXT)),
    ]);
}

pub struct Gallery {
    only: Option<String>,
    scroll: ScrollHandle,
    /// Not a tab stop. Keys need a focus to start from, or tab reaches nothing.
    focus_handle: FocusHandle,
}

impl Gallery {
    /// `only` limits the page to one of [`SECTIONS`].
    pub fn new(only: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            only,
            scroll: ScrollHandle::new(),
            focus_handle,
        }
    }
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let show = |name: &str| self.only.as_deref().is_none_or(|o| o == name);
        div()
            .id("gallery")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
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
            .when(show("rack"), |d| {
                d.child(sections::rack::section(window, cx))
            })
            .when(show("focus"), |d| {
                d.child(sections::rack::focus_section(window, cx))
            })
            .when(show("overlays"), |d| {
                d.child(sections::overlays::section(window, cx))
            })
            .when(show("composed"), |d| {
                d.child(sections::composed::section(window, cx))
            })
    }
}
