//! What the composer chose for the agent: how much it may do without asking, and the model.
//!
//! One small file per machine, `agent/settings.json` in the support folder, for every project.
//! Never in a project: the agent writes there, and must not raise its own access by editing a
//! file.
//!
//! [`AgentSettings`] holds them once for the app, so every sidebar shows and changes the same.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gpui::{AppContext as _, Context, EventEmitter, Task};
use serde::{Deserialize, Serialize};

use crate::store::write_whole;
use crate::{ApprovalMode, Model};

/// The settings of the app, read once and shared by every sidebar. A sidebar observes it to
/// show it, and hears an [`AgentSettingsEvent`] to tell its agent.
pub struct AgentSettings {
    settings: Settings,
    /// Where they are kept, or nowhere with `None`.
    file: Option<PathBuf>,
    /// Reads the file. No message goes until it is in.
    reading: Option<Task<()>>,
    /// Why the file did not read, so a sidebar made later can say so too.
    unreadable: Option<String>,
    /// Writes the last change. A later change waits for it, so the last one is on disk.
    saving: Option<Task<()>>,
    /// What the provider offers, from the first agent that started. Empty until then.
    models: Vec<Model>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSettingsEvent {
    /// The file is read. [`AgentSettings::unreadable`] says whether it did not read.
    Read,
    /// The composer picked another mode. It applies at once, also to a running agent.
    ApprovalModeChanged(ApprovalMode),
    ModelChanged(String),
    /// A change was not written, and says why.
    NotSaved(String),
}

impl EventEmitter<AgentSettingsEvent> for AgentSettings {}

impl AgentSettings {
    /// Reads `file` in the background. With `None` the defaults hold and nothing is saved.
    pub fn new(file: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let reading = file.clone().map(|file| {
            cx.spawn(async move |settings, cx| {
                let read = cx
                    .background_spawn(async move { Settings::read(&file) })
                    .await;
                settings
                    .update(cx, |settings, cx| settings.read(read, cx))
                    .ok();
            })
        });
        Self {
            settings: Settings::default(),
            file,
            reading,
            unreadable: None,
            saving: None,
            models: Vec::new(),
        }
    }

    fn read(&mut self, read: Result<Settings, String>, cx: &mut Context<Self>) {
        self.reading = None;
        match read {
            Ok(settings) => self.settings = settings,
            Err(error) => {
                self.unreadable = Some(format!("{error}. The agent uses its default settings."));
            }
        }
        cx.emit(AgentSettingsEvent::Read);
        cx.notify();
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Whether the file is read, or there is none to read.
    pub fn is_read(&self) -> bool {
        self.reading.is_none()
    }

    /// Why the file did not read, if it did not.
    pub fn unreadable(&self) -> Option<&str> {
        self.unreadable.as_deref()
    }

    pub fn models(&self) -> &[Model] {
        &self.models
    }

    pub fn set_models(&mut self, models: Vec<Model>, cx: &mut Context<Self>) {
        if self.models != models {
            self.models = models;
            cx.notify();
        }
    }

    pub fn set_approval_mode(&mut self, approval_mode: ApprovalMode, cx: &mut Context<Self>) {
        if self.settings.approval_mode != approval_mode {
            self.settings.approval_mode = approval_mode;
            cx.emit(AgentSettingsEvent::ApprovalModeChanged(approval_mode));
            self.save(cx);
        }
    }

    /// `None` is the provider's default, which a running agent cannot be told: only the
    /// provider's list names it, so it applies to the next agent.
    pub fn set_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        if self.settings.model != model {
            self.settings.model = model.clone();
            if let Some(model) = model {
                cx.emit(AgentSettingsEvent::ModelChanged(model));
            }
            self.save(cx);
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        let Some(file) = self.file.clone() else {
            return;
        };
        let settings = self.settings.clone();
        let earlier = self.saving.take();
        self.saving = Some(cx.spawn(async move |this, cx| {
            if let Some(earlier) = earlier {
                earlier.await;
            }
            let written = cx
                .background_spawn(async move { settings.write(&file) })
                .await;
            if let Err(error) = written {
                // With no settings left there is nobody to tell.
                this.update(cx, |_, cx| cx.emit(AgentSettingsEvent::NotSaved(error)))
                    .ok();
            }
        }));
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub approval_mode: ApprovalMode,
    /// One of the ids in [`crate::AgentEvent::Started`], or `None` for the provider's
    /// default.
    pub model: Option<String>,
}

impl Settings {
    /// The settings in `file`, or the defaults when there is none yet. A file that does not
    /// read is an error, which says so; it is written over at the next change.
    pub fn read(file: &Path) -> Result<Settings, String> {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Settings::default());
            }
            Err(error) => return Err(format!("{} could not be read: {error}", file.display())),
        };
        serde_json::from_str(&text)
            .map_err(|error| format!("{} could not be read: {error}", file.display()))
    }

    pub fn write(&self, file: &Path) -> Result<(), String> {
        let failed = |error: String| format!("{} was not written: {error}", file.display());
        let mut text =
            serde_json::to_string_pretty(self).map_err(|error| failed(error.to_string()))?;
        text.push('\n');
        let folder = file
            .parent()
            .ok_or_else(|| failed("it has no folder".to_string()))?;
        fs::create_dir_all(folder)
            .and_then(|()| write_whole(file, &text))
            .map_err(|error| failed(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_come_back_as_written() {
        let machine = tempfile::tempdir().unwrap();
        let file = machine.path().join("agent/settings.json");
        let settings = Settings {
            approval_mode: ApprovalMode::NeverAsk,
            model: Some("sonnet".to_string()),
        };
        settings.write(&file).unwrap();
        assert_eq!(Settings::read(&file), Ok(settings));
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains(r#""approval_mode": "never_ask""#), "{text}");
    }

    #[test]
    fn no_file_is_the_defaults() {
        let machine = tempfile::tempdir().unwrap();
        let settings = Settings::read(&machine.path().join("settings.json"));
        assert_eq!(settings, Ok(Settings::default()));
        assert_eq!(
            Settings::default().approval_mode,
            ApprovalMode::AskBeforeCommands
        );
    }

    /// The caller shows the error and goes on with the defaults.
    #[test]
    fn a_broken_file_says_why() {
        let machine = tempfile::tempdir().unwrap();
        let file = machine.path().join("settings.json");
        for broken in ["{", r#"{"approval_mode": "ask_sometimes"}"#] {
            fs::write(&file, broken).unwrap();
            let error = Settings::read(&file).unwrap_err();
            assert!(error.contains("could not be read"), "{error}");
        }
        // What a later version adds is left out, and what is missing is the default.
        fs::write(&file, r#"{"model": "haiku", "effort": "high"}"#).unwrap();
        let settings = Settings {
            model: Some("haiku".to_string()),
            ..Settings::default()
        };
        assert_eq!(Settings::read(&file), Ok(settings));
    }
}
