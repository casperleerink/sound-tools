//! Storybook for the UI SDK. Shows every component and variant.
//! `GALLERY_SECTION=inputs cargo run -p gallery` shows one section only.

mod composed;
mod sections;

use gpui::{
    App, Application, Bounds, Context, ScrollHandle, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, size,
};
use sound_ui::{ActiveTheme, Assets, typography};

struct Gallery {
    only: Option<String>,
    scroll: ScrollHandle,
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
            .child(div().mt(px(-scroll_offset())))
            .bg(theme.gray_100)
            .text_color(theme.gray_950)
            .font(typography::ui_font())
            .text_size(px(14.))
            .p(px(40.))
            .flex()
            .flex_col()
            .gap(px(48.))
            .when(show("foundation"), |d| d.child(sections::foundation::section(window, cx)))
            .when(show("inputs"), |d| d.child(sections::inputs::section(window, cx)))
            .when(show("overlays"), |d| d.child(sections::overlays::section(window, cx)))
            .when(show("composed"), |d| d.child(sections::composed::section(window, cx)))
    }
}

fn main() {
    let only = std::env::var("GALLERY_SECTION").ok();
    Application::new().with_assets(Assets).run(move |cx: &mut App| {
        sound_ui::init(cx);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1440.), px(900.)),
                    cx,
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Gallery {
                    only,
                    scroll: ScrollHandle::new(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

/// `GALLERY_SCROLL=<px>` lifts the page so lower blocks fit in a screenshot.
fn scroll_offset() -> f32 {
    std::env::var("GALLERY_SCROLL")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.)
}
