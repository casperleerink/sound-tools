use gpui::{
    App, Application, Bounds, Context, Entity, Window, WindowBounds, WindowOptions, prelude::*, px,
    size,
};
struct Root {
    view: Entity<timing_extension::ExperimentView>,
    first: bool,
}
impl Render for Root {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if self.first {
            self.first = false;
            window.on_next_frame(|_, _| println!("FIRST_FRAME {}", timing_extension::REVISION));
        }
        self.view.clone()
    }
}
fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(780.), px(380.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| Root {
                    view: cx.new(timing_extension::ExperimentView::new),
                    first: true,
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
