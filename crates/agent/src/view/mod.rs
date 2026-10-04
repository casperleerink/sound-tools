//! The agent sidebar: the onboarding, the thread, its entries and the composer.

mod entry;
mod history;
mod markdown;
mod menu;
mod onboarding;
mod sidebar;

pub use markdown::{Markdown, MarkdownText};
pub use onboarding::{Onboarding, Setup, SetupAction};
pub use sidebar::Sidebar;
