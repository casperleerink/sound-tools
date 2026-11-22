//! The agent of the sidebar: a coding agent CLI run as a child process in the project folder,
//! with no toolkit. A window drives it and draws what it reads.
//!
//! - [`Thread::start`] starts the agent for one thread and gives the [`Thread`] handle, which
//!   sends to it, and the [`Events`] it answers with: text, steps, approvals and the end of
//!   each turn, the same [`AgentEvent`]s whichever [`Provider`] runs.
//! - [`Chat`] is the sidebar without its drawing: the thread ([`Conversation`], built from
//!   those events with no process), the composer's rules, the saved threads (`store`), the
//!   instructions, and the naming of versions with the provider's small model
//!   ([`Provider::ask_small_model`], `naming`).
//! - [`Agents`] finds each provider's program once for the app, downloads Claude Code when
//!   there is none ([`install`]), runs the provider's own sign-in ([`Provider::status`],
//!   [`SignInChoice`]), and holds the composer's [`AgentSettings`]. [`Agents::sections`] is
//!   what the Agents section of Settings shows.
//! - [`login_shell_environment`] is the environment to run it in. An app opened from the
//!   Finder has a bare `PATH`, and the agent needs `cargo` and `git`.
//!
//! Background work runs on smol's threads. It wakes the window through the [`Waker`] the
//! window passes in, and the window takes what came with [`Chat::poll`] on its next frame.

mod agents;
mod chat;
mod conversation;
mod environment;
mod install;
mod jobs;
mod names;
mod naming;
mod process;
mod provider;
mod settings;
mod setup;
mod store;
mod text;

pub use agents::{Agents, Polled, Program};
pub use chat::{
    APPROVAL_ANSWERS, AccessChoice, Arrow, Attached, Below, CANNOT_CONTINUE, COMPOSER_PLACEHOLDER,
    Chat, FIX_NOT_LIVE, FIX_THEM, Footer, INSTRUCTIONS, Instructions, Mark, Meta, ModelGroup,
    ModelRow, NEW, NEW_MENU, NEW_THREAD, NO_AGENT, OPEN_RECENT, OPEN_SETTINGS, PASTED_IMAGE,
    SEARCH_MODELS, Scope, Shows, StepShown, TRY_AGAIN, TurnShown, Working,
};
pub use conversation::{Approval, Conversation, Entry, Step, Turn, TurnEnd};
pub use environment::{login_shell_environment, program_on_path};
pub use install::{Download, InstallError, install};
pub use jobs::Waker;
pub use names::file_name;
pub use provider::{
    Account, AgentEvent, Answer, ApprovalAnswer, ApprovalId, ApprovalMode, Attachment, Command,
    Events, ExitReason, Image, ImageFormat, Installed, Message, Model, Pick, Provider, Service,
    SignInChoice, SignInQuestion, SignInStep, SignedIn, Status, StepId, StepKind, StepOutcome,
    Thread, ThreadClosed, ThreadOptions, TurnOutcome,
};
pub use settings::{AgentSettings, Settings};
pub use setup::{
    ADD_A_SERVICE, AGENTS_INTRO, Body, Button, COPIED, COPY_DETAILS, Control, Emphasis,
    LOOKING_FOR_AGENTS, Panel, PanelBody, Reason, SEARCH_SERVICES, SIGNED_IN_GROUP, Section,
    ServiceRow, Services, Setup, SetupAction, SigningIn, Steps,
};
pub use store::RecentThread;
pub use text::{Span, code_spans, link_opens};
