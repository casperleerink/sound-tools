//! The composer's instructions to the agent, edited in the sidebar in place of the thread.
//!
//! Two files of plain text. The project's is `instructions.md` in the project folder, where
//! every agent reads it, and where the agent itself adds a line when the composer asks it to
//! remember something. The one for every project is `agent/instructions.md` in the support
//! folder: the agent gets it in its system prompt, and it stays out of the project, as it is
//! the composer's and not the piece's.
//!
//! The text saves itself as the composer types, through one writer, so the last text typed is
//! the one on disk, and once more when it closes in case a save failed. An empty text removes
//! the file, so a project without instructions has no file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sound_core::INSTRUCTIONS_FILE;

use super::{Chat, Done};
use crate::jobs::{self, Outbox};
use crate::store::write_whole;

/// The title over the instructions, and the group of **New**'s menu that opens them.
pub const INSTRUCTIONS: &str = "Instructions";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Project,
    AllProjects,
}

impl Scope {
    /// Its row in **New**'s menu.
    pub fn label(self) -> &'static str {
        match self {
            Scope::Project => "This project",
            Scope::AllProjects => "All projects",
        }
    }

    /// What the editor says above the text.
    pub fn explanation(self) -> &'static str {
        match self {
            Scope::Project => {
                "For this project. The agent follows these, and adds to them when you ask it to \
                 remember something. Changes apply from the next thread."
            }
            Scope::AllProjects => {
                "For all your projects. The agent follows these everywhere. Changes apply from \
                 the next thread."
            }
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Scope::Project => "Keep the strings out of the intro.",
            Scope::AllProjects => "Never automate the volume of a track directly. Use a utility.",
        }
    }
}

/// A text for the file at a path.
pub(super) type Save = (PathBuf, String);

/// The instructions in one scope, as the sidebar shows them.
#[derive(Debug)]
pub struct Instructions {
    scope: Scope,
    file: PathBuf,
    /// What the composer typed, once the file is read. Nothing saves before, so a slow read
    /// never empties the file.
    text: Option<String>,
    /// The text last handed to the writer, so only a change is written.
    saved: String,
    /// The text as it was read. A close writes any other text again, in case a save failed.
    read: String,
}

impl Instructions {
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// The text to edit, or `None` while the file is read. Once it is in, the caret goes to
    /// its end.
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    fn typed(&self) -> Option<String> {
        Some(self.text.as_deref()?.trim_end().to_string())
    }
}

/// Writes the instructions in turn, so the last text typed is the one on disk. Detached: it
/// ends after the writes still queued once the chat is gone. A failed one shows in the thread.
pub(super) fn writer(outbox: Outbox<Done>) -> smol::channel::Sender<Save> {
    let (sender, receiver) = smol::channel::unbounded::<Save>();
    let saving = outbox.clone();
    let save = move |(file, text): Save| {
        let outbox = saving.clone();
        async move {
            if let Err(error) = smol::unblock(move || write(&file, &text)).await {
                outbox.send(Done::Notice(error));
            }
        }
    };
    smol::spawn(async move {
        while let Ok(mut next) = receiver.recv().await {
            let mut taken = 1;
            // Of a run of saves to one file, only the last needs writing.
            while let Ok(later) = receiver.try_recv() {
                taken += 1;
                if later.0 != next.0 {
                    save(std::mem::replace(&mut next, later)).await;
                } else {
                    next = later;
                }
            }
            save(next).await;
            for _ in 0..taken {
                outbox.send(Done::Saved);
            }
        }
    })
    .detach();
    sender
}

impl Chat {
    /// Whether **New**'s menu can open the instructions in `scope`: those for all projects
    /// need a support folder to be kept in.
    pub fn can_open_instructions(&self, scope: Scope) -> bool {
        self.instructions_file(scope).is_some()
    }

    fn instructions_file(&self, scope: Scope) -> Option<PathBuf> {
        match scope {
            Scope::Project => Some(self.folder.join(INSTRUCTIONS_FILE)),
            Scope::AllProjects => self.agents.settings().instructions_file(),
        }
    }

    /// Shows the instructions in `scope` in place of the thread, once read.
    pub fn open_instructions(&mut self, scope: Scope) {
        let Some(file) = self.instructions_file(scope) else {
            return;
        };
        self.close_instructions();
        let outbox = self.inbox.outbox();
        let reading = file.clone();
        jobs::spawn(async move {
            let text = smol::unblock({
                let file = reading.clone();
                move || read(&file)
            });
            outbox.send(Done::InstructionsRead(reading, text.await));
        })
        .detach();
        self.instructions = Some(Instructions {
            scope,
            file,
            text: None,
            saved: String::new(),
            read: String::new(),
        });
    }

    pub fn instructions(&self) -> Option<&Instructions> {
        self.instructions.as_ref()
    }

    pub(super) fn instructions_read(&mut self, file: PathBuf, text: io::Result<String>) {
        let Some(instructions) = self.instructions.as_mut() else {
            return;
        };
        if instructions.file != file || instructions.text.is_some() {
            return;
        }
        match text {
            Ok(text) => {
                instructions.saved = text.clone();
                instructions.read = text.clone();
                instructions.text = Some(text);
            }
            // Nothing is written over a file that did not read.
            Err(error) => {
                let message = format!("{} could not be read: {error}", file.display());
                self.instructions = None;
                self.conversation.notice(message);
            }
        }
    }

    /// What the composer typed. Saves it if it changed.
    pub fn edit_instructions(&mut self, text: &str) {
        let Some(instructions) = self.instructions.as_mut() else {
            return;
        };
        if instructions.text.is_none() {
            return;
        }
        instructions.text = Some(text.to_string());
        if let Some(typed) = instructions
            .typed()
            .filter(|typed| *typed != instructions.saved)
        {
            instructions.saved = typed.clone();
            let save = (instructions.file.clone(), typed);
            self.hand_instructions(save);
        }
    }

    /// The back arrow and escape: shows the thread again, after one more save when the text
    /// is not the one read, in case a save failed while the composer typed.
    pub fn close_instructions(&mut self) {
        let Some(instructions) = self.instructions.take() else {
            return;
        };
        let Some(typed) = instructions.typed() else {
            return;
        };
        if typed != instructions.saved || typed != instructions.read {
            self.hand_instructions((instructions.file, typed));
        }
    }

    fn hand_instructions(&mut self, save: Save) {
        // Fails only once the writer is gone, which ends with the chat.
        if self.instruction_saves.try_send(save).is_ok() {
            self.saving += 1;
        }
    }
}

/// The text of the file, empty when there is none yet.
fn read(file: &Path) -> io::Result<String> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(text.trim_end().to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// Writes `text` as the whole file, or removes the file for an empty text.
fn write(file: &Path, text: &str) -> Result<(), String> {
    let text = text.trim_end();
    let failed = |error: io::Error| format!("{} was not saved: {error}", file.display());
    if text.is_empty() {
        return match fs::remove_file(file) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(failed(error)),
            Ok(()) | Err(_) => Ok(()),
        };
    }
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).map_err(failed)?;
    }
    write_whole(file, &format!("{text}\n")).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_text_removes_the_file_and_a_new_one_makes_its_folder() {
        let machine = tempfile::tempdir().unwrap();
        let file = machine.path().join("agent/instructions.md");
        write(&file, "Use a utility for volume.\n\n").unwrap();
        assert_eq!(read(&file).unwrap(), "Use a utility for volume.");
        write(&file, "  \n").unwrap();
        assert!(!file.exists());
        // Already gone.
        write(&file, "").unwrap();
        assert_eq!(read(&file).unwrap(), "");
    }
}
