//! What the composer chose for the agent: how much it may do without asking, the provider,
//! and a model for each provider.
//!
//! One small file per machine, `agent/settings.json` in the support folder, for every project.
//! Never in a project: the agent writes there, and must not raise its own access by editing a
//! file. Another process of the app reads it when it starts.
//!
//! [`AgentSettings`] also knows where the composer's instructions for every project are:
//! `instructions.md` beside the settings, read when an agent starts.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use smol::channel::Sender;

use crate::jobs::{self, Inbox, Job, Waker};
use crate::store::write_whole;
use crate::{ApprovalMode, Model, Provider};

const INSTRUCTIONS_FILE: &str = "instructions.md";

/// The settings of the app, read once in the background, and the models each provider offers.
pub struct AgentSettings {
    settings: Settings,
    /// Where they are kept, or nowhere with `None`.
    file: Option<PathBuf>,
    /// Reads the file. No message goes until it is in.
    reading: Option<Job>,
    /// Why the file did not read.
    unreadable: Option<String>,
    /// Hands each change to the writer, in order, so the last one is on disk.
    writes: Option<Sender<Settings>>,
    /// What each provider offers, asked once signed in. None until it answers.
    models: HashMap<Provider, Vec<Model>>,
    inbox: Inbox<Done>,
}

/// Reads `file` in the background.
fn read(file: PathBuf, inbox: &Inbox<Done>) -> Job {
    let outbox = inbox.outbox();
    jobs::spawn(async move {
        let read = smol::unblock(move || Settings::read(&file)).await;
        outbox.send(Done::Read(read));
    })
}

enum Done {
    Read(Result<Settings, String>),
    NotSaved(String),
}

impl AgentSettings {
    /// Reads `file` in the background. With `None` the defaults hold and nothing is saved.
    pub fn new(file: Option<PathBuf>, waker: Waker) -> Self {
        let inbox = Inbox::new(waker);
        let reading = file.clone().map(|file| read(file, &inbox));
        let writes = file.clone().map(|file| {
            let (sender, receiver) = smol::channel::unbounded::<Settings>();
            let outbox = inbox.outbox();
            // Detached: the changes still queued are written after the settings went.
            smol::spawn(async move {
                while let Ok(mut settings) = receiver.recv().await {
                    // Only the last of what queued meanwhile needs writing.
                    while let Ok(later) = receiver.try_recv() {
                        settings = later;
                    }
                    let file = file.clone();
                    let written = smol::unblock(move || settings.write(&file)).await;
                    if let Err(error) = written {
                        outbox.send(Done::NotSaved(error));
                    }
                }
            })
            .detach();
            sender
        });
        Self {
            settings: Settings::default(),
            file,
            reading,
            unreadable: None,
            writes,
            models: HashMap::new(),
            inbox,
        }
    }

    /// Reads the file again, for what another window of the app changed in it meanwhile.
    pub fn read_again(&mut self) {
        if let (Some(file), None) = (&self.file, &self.reading) {
            self.reading = Some(read(file.clone(), &self.inbox));
        }
    }

    /// What came in: the file once read, or a change not written. Gives what the thread says
    /// of it.
    pub(crate) fn poll(&mut self) -> Vec<String> {
        let mut notices = Vec::new();
        while let Some(done) = self.inbox.take() {
            match done {
                Done::Read(read) => {
                    self.reading = None;
                    match read {
                        Ok(settings) => {
                            self.settings = settings;
                            self.unreadable = None;
                        }
                        Err(error) => {
                            let unreadable =
                                format!("{error}. The agent uses its default settings.");
                            // Said once, not at every reading again.
                            if self.unreadable.as_ref() != Some(&unreadable) {
                                notices.push(unreadable.clone());
                            }
                            self.unreadable = Some(unreadable);
                        }
                    }
                }
                Done::NotSaved(error) => notices.push(error),
            }
        }
        notices
    }

    /// The composer's instructions for every project, beside the settings file.
    pub fn instructions_file(&self) -> Option<PathBuf> {
        let file = self.file.as_ref()?;
        Some(file.with_file_name(INSTRUCTIONS_FILE))
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

    pub fn models(&self, provider: Provider) -> &[Model] {
        self.models.get(&provider).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn set_models(&mut self, provider: Provider, models: Vec<Model>) {
        self.models.insert(provider, models);
    }

    /// Whether it changed.
    pub(crate) fn set_approval_mode(&mut self, approval_mode: ApprovalMode) -> bool {
        let changed = self.settings.approval_mode != approval_mode;
        if changed {
            self.settings.approval_mode = approval_mode;
            self.save();
        }
        changed
    }

    /// Picks `provider` for new threads, with the model picked for it before.
    pub(crate) fn set_provider(&mut self, provider: Provider) {
        if self.settings.provider != provider {
            self.settings.provider = provider;
            self.save();
        }
    }

    /// Picks `provider` for new threads, and its model. `None` is the provider's default,
    /// which a running agent cannot be told: only the provider's list names it, so it applies
    /// to the next agent. Whether it changed.
    pub(crate) fn set_model(&mut self, provider: Provider, model: Option<String>) -> bool {
        let unchanged =
            self.settings.provider == provider && self.settings.model(provider) == model.as_deref();
        if unchanged {
            return false;
        }
        self.settings.provider = provider;
        match model {
            Some(model) => self.settings.models.insert(provider, model),
            None => self.settings.models.remove(&provider),
        };
        self.save();
        true
    }

    fn save(&self) {
        if let Some(writes) = &self.writes {
            // The writer ends only once this sender is dropped.
            writes.try_send(self.settings.clone()).ok();
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub approval_mode: ApprovalMode,
    /// The provider of a new thread, while its program is set up.
    pub provider: Provider,
    /// One of the ids in [`crate::AgentEvent::Started`] for each provider, or none for its
    /// default.
    pub models: BTreeMap<Provider, String>,
}

/// The file as it reads, also as written before Pi came, when it kept one model: Claude Code's.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Stored {
    #[serde(flatten)]
    settings: Settings,
    model: Option<String>,
}

impl Settings {
    pub fn model(&self, provider: Provider) -> Option<&str> {
        self.models.get(&provider).map(String::as_str)
    }

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
        let Stored {
            mut settings,
            model,
        } = serde_json::from_str(&text)
            .map_err(|error| format!("{} could not be read: {error}", file.display()))?;
        if let Some(model) = model {
            settings.models.entry(Provider::Claude).or_insert(model);
        }
        Ok(settings)
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
            provider: Provider::Pi,
            models: BTreeMap::from([(Provider::Claude, "sonnet".to_string())]),
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

    /// Another window of the app changed the file: read again, it holds; a broken file is
    /// said once.
    #[test]
    fn a_file_read_again_brings_what_another_window_wrote() {
        let machine = tempfile::tempdir().unwrap();
        let file = machine.path().join("settings.json");
        let (waker, woken) = jobs::testing::waker();
        let mut settings = AgentSettings::new(Some(file.clone()), waker);
        let mut said = Vec::new();
        let mut read = |settings: &mut AgentSettings| loop {
            said.extend(settings.poll());
            if settings.is_read() {
                return said.len();
            }
            jobs::testing::wait(&woken);
        };
        read(&mut settings);
        let elsewhere = Settings {
            approval_mode: ApprovalMode::NeverAsk,
            ..Settings::default()
        };
        elsewhere.write(&file).unwrap();
        settings.read_again();
        read(&mut settings);
        assert_eq!(settings.settings(), &elsewhere);

        fs::write(&file, "{").unwrap();
        settings.read_again();
        assert_eq!(read(&mut settings), 1);
        settings.read_again();
        assert_eq!(read(&mut settings), 1);
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
        // A file of before Pi kept one model, Claude Code's.
        fs::write(&file, r#"{"model": "haiku", "effort": "high"}"#).unwrap();
        let settings = Settings {
            models: BTreeMap::from([(Provider::Claude, "haiku".to_string())]),
            ..Settings::default()
        };
        assert_eq!(Settings::read(&file), Ok(settings));
    }
}
