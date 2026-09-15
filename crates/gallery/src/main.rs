use gpui::{
    App, Application, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    size,
};
use sound_ui::theme::ActiveTheme;

struct Gallery;

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .size_full()
            .bg(theme.gray_100)
            .text_color(theme.gray_950)
            .p(px(24.))
            .child("Sound Tools UI")
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        sound_ui::theme::install(cx);
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
            |_, cx| cx.new(|_| Gallery),
        )
        .unwrap();
        cx.activate(true);
    });
}
