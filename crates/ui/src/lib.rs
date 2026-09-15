//! Sound Tools UI SDK: design tokens and shared components built on GPUI.

pub mod assets;
pub mod components;
pub mod theme;
pub mod typography;

pub use assets::Assets;
pub use theme::{ActiveTheme, Theme};

/// Register the theme and fonts. Call inside `Application::run`, before opening windows.
pub fn init(cx: &mut gpui::App) {
    theme::install(cx);
    assets::load_fonts(cx);
}
