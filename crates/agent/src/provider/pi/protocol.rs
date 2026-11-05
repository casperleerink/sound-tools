//! The RPC records of `pi --mode rpc`, as far as the driver uses them. Every enum ends in a
//! catch-all, so a record a newer Pi adds is skipped, never an error. Fields the driver does
//! not read are not declared.

use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provider::{self, Attachment, ImageFormat};

/// A line Pi writes.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Incoming {
    /// The answer to one of our commands.
    Response {
        id: Option<String>,
        #[serde(default)]
        success: bool,
        error: Option<String>,
    },
    MessageUpdate {
        #[serde(rename = "assistantMessageEvent")]
        event: Streamed,
    },
    MessageEnd {
        message: Ended,
    },
    #[serde(rename_all = "camelCase")]
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        #[serde(default)]
        args: Value,
    },
    #[serde(rename_all = "camelCase")]
    ToolExecutionEnd {
        tool_call_id: String,
        #[serde(default)]
        is_error: bool,
    },
    /// Pi will do nothing more for the prompt: the end of our turn.
    AgentSettled {
        #[serde(default)]
        aborted: bool,
    },
    /// A question or a note of an extension, ours.
    ExtensionUiRequest(UiRequest),
    ExtensionError {
        error: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Streamed {
    TextDelta {
        delta: String,
    },
    /// The whole text of the block.
    TextEnd {
        content: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Ended {
    pub role: String,
    pub stop_reason: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub(crate) enum UiRequest {
    /// An approval: the title is the id of the tool call.
    Select { id: String, title: String },
    /// A question of a sign-in: the title is the question as JSON.
    Input { id: String, title: String },
    /// A record of ours as JSON, or an extension's note.
    Notify {
        message: String,
        #[serde(rename = "notifyType")]
        kind: Option<String>,
    },
    #[serde(other)]
    Other,
}

/// A line we write.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Outgoing {
    Prompt {
        id: String,
        message: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageContent>,
        /// The composer's message, which starts a turn, and not a command of ours.
        #[serde(skip)]
        starts_turn: bool,
    },
    Abort {
        id: String,
    },
    SetModel {
        id: String,
        provider: String,
        #[serde(rename = "modelId")]
        model_id: String,
    },
    ExtensionUiResponse {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        cancelled: bool,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageContent {
    #[serde(rename = "type")]
    kind: &'static str,
    data: String,
    mime_type: &'static str,
}

impl Outgoing {
    /// The composer's message: the note first, files as their paths after the text.
    pub(crate) fn message(id: String, message: provider::Message) -> Self {
        let provider::Message {
            text,
            attachments,
            note,
        } = message;
        let mut images = Vec::new();
        let mut files = Vec::new();
        for attachment in attachments {
            match attachment {
                Attachment::Image(image) => images.push(ImageContent {
                    kind: "image",
                    data: BASE64_STANDARD.encode(&image.bytes),
                    mime_type: media_type(image.format),
                }),
                Attachment::File(path) => files.push(path),
            }
        }
        let mut message = note.map(|note| note + "\n\n").unwrap_or_default();
        message.push_str(&text);
        if !files.is_empty() {
            message.push_str("\n\nAttached files:");
            for path in files {
                message.push('\n');
                message.push_str(&path.to_string_lossy());
            }
        }
        Outgoing::Prompt {
            id,
            message,
            images,
            starts_turn: true,
        }
    }

    /// One of the commands of our extension, which never starts a turn.
    pub(crate) fn command(id: String, command: String) -> Self {
        Outgoing::Prompt {
            id,
            message: command,
            images: Vec::new(),
            starts_turn: false,
        }
    }
}

fn media_type(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Webp => "image/webp",
    }
}

/// What `/sound-tools-models` says.
#[derive(Debug, Deserialize)]
pub(crate) struct Models {
    pub models: Vec<ListedModel>,
    /// The model a thread starts on, as `provider/id`.
    pub current: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListedModel {
    pub id: String,
    pub name: String,
    /// The provider's name, such as "OpenRouter".
    pub source: String,
}

/// What `/sound-tools-providers` says.
#[derive(Debug, Deserialize)]
pub(crate) struct Providers {
    pub providers: Vec<ListedProvider>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListedProvider {
    pub id: String,
    pub name: String,
    /// The label of its sign-in in the browser, when it has one.
    pub sign_in: Option<String>,
    /// Whether it takes an API key.
    pub api_key: bool,
    pub signed_in: bool,
}

/// One record of `/sound-tools-sign-in`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SignInRecord {
    Step(Step),
    SignedIn(IgnoredAny),
    Failed(String),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Step {
    /// Its instructions are left out: they speak of callbacks and redirect URLs.
    AuthUrl {
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    DeviceCode {
        user_code: String,
        verification_uri: String,
    },
    Info {
        message: String,
    },
    Progress {
        message: String,
    },
    #[serde(other)]
    Other,
}

/// A question of a sign-in, from the title of its `input`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Question {
    Text {
        message: String,
        placeholder: Option<String>,
    },
    Secret {
        message: String,
        placeholder: Option<String>,
    },
    /// The address the browser ended on, in case its way back to Pi does not arrive.
    ManualCode {},
    Select {
        message: String,
        options: Vec<SelectOption>,
    },
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelectOption {
    pub id: String,
    pub label: String,
}
