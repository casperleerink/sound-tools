//! Sound Tools UI SDK: design tokens, shared components and the bridge from a live project to
//! GPUI views. `README.md` in this crate is the guide for writing a view.

pub mod assets;
pub mod components;
pub mod devices;
pub mod focus;
pub mod session;
pub mod theme;
pub mod typography;
pub mod views;

pub use assets::Assets;
pub use devices::{
    DeviceLabel, DeviceOffer, Devices, Slot, enable_extension, extension_is_enabled,
};
pub use focus::KeyboardFocus;
pub use session::{POLL_INTERVAL, Playhead, Session};
pub use theme::{ActiveTheme, Theme};
pub use views::Views;

/// Register the theme and fonts. Call inside `Application::run`, before opening windows.
pub fn init(cx: &mut gpui::App) {
    theme::install(cx);
    assets::load_fonts(cx);
}
