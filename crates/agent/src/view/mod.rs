//! The agent sidebar: the onboarding, the thread, its entries and the composer.

mod agents;
mod agents_page;
mod entry;
mod history;
mod instructions;
mod markdown;
mod menu;
mod model_picker;
mod onboarding;
mod sidebar;

pub use agents::{Agents, AgentsEvent, Program};
pub use agents_page::AgentsPage;
pub use markdown::{Markdown, MarkdownText};
pub use onboarding::{Onboarding, Reason, Setup, SetupAction, SigningIn};
pub use sidebar::{FIX_NOT_LIVE, Sidebar};
