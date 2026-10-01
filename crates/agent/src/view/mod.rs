//! The agent sidebar: the onboarding, the thread, its entries and the composer.

mod entry;
mod history;
mod menu;
mod onboarding;
mod sidebar;

pub use onboarding::{Onboarding, Setup, SetupAction};
pub use sidebar::Sidebar;
