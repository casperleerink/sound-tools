//! The agent of the sidebar: a coding agent CLI run as a child process in the project folder.
//!
//! - [`Thread::start`] starts the agent for one thread and gives the [`Thread`] handle, which
//!   sends to it, and the [`Events`] it answers with: text, steps, approvals and the end of
//!   each turn, the same [`AgentEvent`]s whichever [`Provider`] runs.
//! - [`Conversation`] is what the sidebar shows of a thread, built from those events with no
//!   process, and [`Sidebar`] is the view the window shows in its left panel.
//! - [`login_shell_environment`] is the environment to run it in. An app opened from the
//!   Finder has a bare `PATH`, and the agent needs `cargo` and `git`. [`program_on_path`]
//!   finds a CLI the composer installed.
//!
//! No async runtime of its own: the futures run on smol, so gpui's executors or
//! `smol::block_on` drive them. The plan and the reasons are in `docs/plans/agent-sidebar.md`.
//!
//! `cargo run -p sound-agent --example chat -- <folder>` chats with Claude Code in the
//! terminal.

mod environment;
mod provider;
mod conversation;
mod view;

pub use environment::{login_shell_environment, program_on_path};
pub use provider::{
    Account, AgentEvent, ApprovalAnswer, ApprovalId, ApprovalMode, Events, ExitReason, Model,
    Provider, Session, StepId, StepOutcome, Thread, ThreadClosed, ThreadOptions, TurnOutcome,
};
pub use conversation::{Approval, Conversation, Entry, Step, Turn, TurnEnd};
pub use view::{Installed, Sidebar};
