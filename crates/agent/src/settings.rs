//! What the composer chose for the agent: how much it may do without asking, and the model.
//!
//! One small file per machine, `agent/settings.json` in the support folder, for every project.
//! Never in a project: the agent writes there, and must not raise its own access by editing a
//! file.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ApprovalMode;

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
        // Through a file of its own and a rename, so a crash never leaves half a file.
        let temporary = folder.join(format!("settings.{}.tmp", Uuid::new_v4()));
        fs::create_dir_all(folder)
            .and_then(|()| fs::write(&temporary, text))
            .and_then(|()| fs::rename(&temporary, file))
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
