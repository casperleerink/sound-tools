//! The agent of the sidebar: a coding agent CLI run as a child process in the project folder.
//!
//! - [`Thread::start`] starts the agent for one thread and gives the [`Thread`] handle, which
//!   sends to it, and the [`Events`] it answers with: text, steps, approvals and the end of
//!   each turn, the same [`AgentEvent`]s whichever [`Provider`] runs.
//! - [`Conversation`] is what the sidebar shows of a thread, built from those events with no
//!   process, and [`Sidebar`] is the view the window shows in its left panel.
//! - [`login_shell_environment`] is the environment to run it in. An app opened from the
//!   Finder has a bare `PATH`, and the agent needs `cargo` and `git`.
//! - [`install`] downloads the provider's pinned program, which the sidebar runs, and
//!   [`Provider::account`] and [`SignInChoice`] run its own sign-in. [`Onboarding`] shows
//!   each [`Setup`] state until the agent is ready.
//!
//! No async runtime of its own: the futures run on smol, so gpui's executors or
//! `smol::block_on` drive them. The plan and the reasons are in `docs/plans/agent-sidebar.md`.
//!
//! `cargo run -p sound-agent --example chat -- <folder>` chats with Claude Code in the
//! terminal, with a `claude` on the `PATH` ([`program_on_path`]).

mod environment;
mod install;
mod provider;
mod thread;
mod view;

pub use environment::{login_shell_environment, program_on_path};
pub use install::{Download, InstallError, install};
pub use provider::{
    Account, AgentEvent, ApprovalAnswer, ApprovalId, ApprovalMode, Events, ExitReason, Installed,
    Model, Provider, Session, SignInChoice, StepId, StepOutcome, Thread, ThreadClosed,
    ThreadOptions, TurnOutcome,
};
pub use thread::{Approval, Conversation, Entry, Step, Turn, TurnEnd};
pub use view::{Onboarding, Setup, SetupAction, Sidebar};
