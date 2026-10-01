//! The stream-json messages of the `claude` CLI, as far as the driver uses them.
//!
//! Undocumented and checked against recordings of the pinned version (`tests/fixtures/claude/`).
//! Every enum ends in a catch-all, so a message kind a newer CLI adds is skipped, never an
//! error. Fields the driver does not read are not declared.

use std::path::PathBuf;

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A line the CLI writes.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Incoming {
    System(System),
    /// A piece of the answer while it streams (`--include-partial-messages`).
    StreamEvent {
        event: StreamEvent,
    },
    /// One finished content block of the answer.
    Assistant {
        message: Message,
    },
    /// Tool results, and the CLI's own notes such as "[Request interrupted by user]".
    User {
        message: Message,
    },
    /// The end of a turn, the one message every turn ends with.
    Result(TurnResult),
    /// The CLI asks us, such as whether a tool may run.
    ControlRequest {
        request_id: String,
        request: CliRequest,
    },
    /// The answer to one of our [`Request`]s.
    ControlResponse {
        response: ControlResponse<Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum System {
    /// Sent at the start of each turn.
    Init { cwd: PathBuf },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    ContentBlockDelta {
        delta: Delta,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    TextDelta {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub content: Content,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Blocks(Vec<Block>),
    /// Plain text, which only the composer's own messages use.
    Text(IgnoredAny),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct TurnResult {
    pub subtype: ResultKind,
    #[serde(default)]
    pub is_error: bool,
    pub terminal_reason: Option<TerminalReason>,
    /// What went wrong. Lines that start with `[ede_diagnostic]` are the CLI's own notes.
    #[serde(default)]
    pub errors: Vec<String>,
    /// The last text of the answer, or the error of a failed turn.
    pub result: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    Success,
    #[serde(other)]
    Error,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalReason {
    /// Interrupted while the answer streamed.
    AbortedStreaming,
    /// Interrupted while a tool ran.
    AbortedTools,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum CliRequest {
    /// Under `--permission-prompt-tool stdio`, every tool the permission mode does not allow.
    CanUseTool {
        tool_name: String,
        input: Value,
        /// Permission updates that would allow it from now on, such as a rule or a mode.
        #[serde(default)]
        permission_suggestions: Vec<Value>,
        tool_use_id: Option<String>,
    },
    #[serde(other)]
    Other,
}

/// Both directions: the CLI answers our requests, and we answer its requests.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum ControlResponse<Payload> {
    Success {
        request_id: String,
        response: Option<Payload>,
    },
    Error {
        request_id: String,
        error: String,
    },
}

/// The part of the `initialize` response the driver reads.
#[derive(Debug, Deserialize)]
pub struct Initialized {
    pub account: Option<InitializedAccount>,
    #[serde(default)]
    pub models: Vec<InitializedModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializedAccount {
    pub email: Option<String>,
    pub subscription_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializedModel {
    pub value: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
}

/// A line we write. `Deserialize` too, so the tests replay recorded runs.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outgoing {
    User {
        message: UserMessage,
        parent_tool_use_id: Option<String>,
        /// Empty: the CLI fills in its own.
        session_id: String,
    },
    ControlRequest {
        request_id: String,
        request: Request,
    },
    ControlResponse {
        response: ControlResponse<PermissionResult>,
    },
}

impl Outgoing {
    pub fn user(text: String) -> Self {
        Outgoing::User {
            message: UserMessage {
                role: Role::User,
                // The text stays the last block when images join it later: the CLI reads the
                // last block for the prompt.
                content: vec![UserBlock::Text { text }],
            },
            parent_tool_use_id: None,
            session_id: String::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserMessage {
    pub role: Role,
    pub content: Vec<UserBlock>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserBlock {
    Text { text: String },
}

/// Our requests to the CLI.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum Request {
    /// Answers with the account and the models.
    Initialize,
    Interrupt,
    SetPermissionMode {
        mode: PermissionMode,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    BypassPermissions,
}

impl PermissionMode {
    /// The value of `--permission-mode`, the same as the JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::Default => "default",
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::BypassPermissions => "bypassPermissions",
        }
    }
}

/// Our answer to [`CliRequest::CanUseTool`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum PermissionResult {
    Allow {
        /// The tool's input as it was asked for; the CLI requires it.
        #[serde(rename = "updatedInput")]
        updated_input: Value,
        #[serde(
            rename = "updatedPermissions",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        updated_permissions: Option<Vec<Value>>,
    },
    Deny {
        message: String,
    },
}
