//! Sound Tools UI SDK: design tokens, shared components and the bridge from a live project to
//! GPUI views. `README.md` in this crate is the guide for writing a view.

pub mod assets;
pub mod components;
pub mod control_edit;
pub mod devices;
pub mod focus;
pub mod import;
pub mod lanes;
pub mod metering;
pub mod recording;
pub mod session;
pub mod theme;
pub mod typography;
pub mod views;
pub mod waveforms;

pub use assets::Assets;
pub use control_edit::{ControlEdit, weak_action, weak_callback};
pub use devices::{
    DeviceLabel, DeviceOffer, Devices, Needs, OfferGroup, Slot, extension_is_enabled,
};
pub use focus::KeyboardFocus;
pub use lanes::Lanes;
pub use metering::{Metering, every_poll};
pub use recording::{InputLevels, LiveSound, LiveTake, Recording};
pub use session::{POLL_INTERVAL, Playhead, Session};
pub use theme::{ActiveTheme, Theme};
pub use views::Views;
pub use waveforms::Waveforms;

/// Register the theme and fonts. Call inside `Application::run`, before opening windows.
pub fn init(cx: &mut gpui::App) {
    theme::install(cx);
    assets::load_fonts(cx);
}
