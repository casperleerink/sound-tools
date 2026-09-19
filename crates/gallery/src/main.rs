//! Opens the gallery in a window.
//! `GALLERY_SECTION=inputs cargo run -p gallery` shows one section only.
//! `cargo test -p gallery --test snapshots` renders every section to PNG without opening a window.

use gallery::Gallery;
use gpui::{App, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use sound_ui::Assets;

fn main() {
    let only = std::env::var("GALLERY_SECTION").ok();
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            sound_ui::init(cx);
            cx.on_window_closed(|cx, _window_id| {
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
                |_, cx| cx.new(|_| Gallery::new(only)),
            )
            .unwrap();
            cx.activate(true);
        });
}
