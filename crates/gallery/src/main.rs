use gpui::{
    App, Application, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    size,
};
use sound_ui::{ActiveTheme, Assets, components::icon::Icon, typography};

struct Gallery;

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .size_full()
            .bg(theme.gray_100)
            .text_color(theme.gray_950)
            .font(typography::ui_font())
            .text_size(px(14.))
            .p(px(24.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(Icon::new("music"))
            .child("Sound Tools UI 0123456789")
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
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
            |_, cx| cx.new(|_| Gallery),
        )
        .unwrap();
        cx.activate(true);
    });
}
