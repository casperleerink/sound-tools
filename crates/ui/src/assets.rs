//! Embedded assets: fonts and icons. Register with `Application::new().with_assets(Assets)`.

use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*
        ];
    };
}

icons!(
    "arrow-left", "arrow-right", "arrow-up", "bot", "check", "chevron-down", "chevron-left",
    "chevron-right", "chevron-up", "circle", "circle-alert", "circle-check", "copy", "ellipsis",
    "eye", "eye-off", "folder", "info", "loader-circle", "lock", "mic", "minus", "music", "pause",
    "pencil", "piano", "play", "plus", "redo-2", "search", "send", "settings", "skip-back",
    "skip-forward", "sliders-horizontal", "sparkles", "square", "trash-2", "triangle-alert",
    "undo-2", "volume-2", "volume-x", "x", "zap",
);

pub const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/InterDisplay-Regular.ttf");
pub const FONT_MEDIUM: &[u8] = include_bytes!("../assets/fonts/InterDisplay-Medium.ttf");
pub const FONT_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/InterDisplay-SemiBold.ttf");

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let name = path.strip_prefix("icons/").and_then(|p| p.strip_suffix(".svg"));
        Ok(name
            .and_then(|n| ICONS.iter().find(|(k, _)| *k == n))
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(if path == "icons" {
            ICONS.iter().map(|(k, _)| format!("{k}.svg").into()).collect()
        } else {
            vec![]
        })
    }
}

/// Load the InterDisplay fonts. Call once at startup, before opening a window.
pub fn load_fonts(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(FONT_REGULAR),
            Cow::Borrowed(FONT_MEDIUM),
            Cow::Borrowed(FONT_SEMIBOLD),
        ])
        .expect("load InterDisplay");
}
