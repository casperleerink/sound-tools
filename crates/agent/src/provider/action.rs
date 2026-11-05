//! What a step of the agent does, in the words of its titles, for every provider.

use std::path::Path;

use super::{AgentEvent, StepId, StepKind};

/// The longest pattern a title shows, in characters.
const TITLE_DETAIL_CHARACTERS: usize = 60;

/// What a tool use does, for its title.
#[derive(Debug)]
pub(super) enum Action {
    /// A command, whole, and what the agent says it does, when it says.
    Run(String, Option<String>),
    Read(String),
    Edit(String),
    Write(String),
    Search(String),
    Use(String),
}

impl Action {
    /// The step it starts.
    pub(super) fn started(&self, id: StepId) -> AgentEvent {
        AgentEvent::StepStarted {
            id,
            title: self.done_title(),
            running_title: self.running_title(),
            request_title: self.request_title(),
            command: self.command(),
            kind: self.kind(),
        }
    }

    pub(super) fn command(&self) -> Option<String> {
        match self {
            Action::Run(command, _) => Some(command.clone()),
            _ => None,
        }
    }

    fn kind(&self) -> StepKind {
        match self {
            Action::Run(..) => StepKind::Run,
            Action::Read(path) => StepKind::Read { path: path.clone() },
            Action::Edit(path) | Action::Write(path) => StepKind::Change { path: path.clone() },
            Action::Search(_) => StepKind::Search,
            Action::Use(_) => StepKind::Other,
        }
    }

    fn done_title(&self) -> String {
        match self {
            Action::Run(_, Some(description)) => description.clone(),
            Action::Run(_, None) => "Ran a command".to_string(),
            Action::Read(path) => format!("Read {path}"),
            Action::Edit(path) => format!("Edited {path}"),
            Action::Write(path) => format!("Wrote {path}"),
            Action::Search(pattern) => format!("Searched for {}", code(pattern)),
            Action::Use(tool) => format!("Used {tool}"),
        }
    }

    fn running_title(&self) -> String {
        match self {
            Action::Run(_, Some(description)) => description.clone(),
            Action::Run(_, None) => "Running a command".to_string(),
            Action::Read(path) => format!("Reading {path}"),
            Action::Edit(path) => format!("Editing {path}"),
            Action::Write(path) => format!("Writing {path}"),
            Action::Search(pattern) => format!("Searching for {}", code(pattern)),
            Action::Use(tool) => format!("Using {tool}"),
        }
    }

    pub(super) fn request_title(&self) -> String {
        match self {
            Action::Run(_, Some(description)) => description.clone(),
            Action::Run(_, None) => "Run a command".to_string(),
            Action::Read(path) => format!("Read {path}"),
            Action::Edit(path) => format!("Edit {path}"),
            Action::Write(path) => format!("Write {path}"),
            Action::Search(pattern) => format!("Search for {}", code(pattern)),
            Action::Use(tool) => format!("Use {tool}"),
        }
    }
}

/// A path inside the project as the composer knows it, from the project folder.
pub(super) fn relative(path: &str, folder: &Path) -> String {
    Path::new(path)
        .strip_prefix(folder)
        .map(|inside| inside.display().to_string())
        .unwrap_or_else(|_| path.to_string())
}

/// The first line, cut to fit one line of the sidebar.
pub(super) fn shorten(text: &str) -> String {
    // Trimmed first, so a pattern that starts with a blank line is not shown empty.
    let mut lines = text.trim().lines();
    let line = lines.next().unwrap_or_default().trim();
    let more = lines.next().is_some();
    if line.chars().count() <= TITLE_DETAIL_CHARACTERS && !more {
        return line.to_string();
    }
    let cut: String = line.chars().take(TITLE_DETAIL_CHARACTERS).collect();
    format!("{}…", cut.trim_end())
}

/// A pattern in backticks, which the sidebar shows as code. One that has a backtick of its own
/// is left bare: its backtick would close ours early, and the line would show the wrong part
/// as code.
fn code(text: &str) -> String {
    if text.contains('`') {
        text.to_string()
    } else {
        format!("`{text}`")
    }
}

#[cfg(test)]
mod tests {
    use super::shorten;

    #[test]
    fn a_pattern_that_starts_with_a_blank_line_shows_its_first_line() {
        assert_eq!(shorten("\n  *.json\n"), "*.json");
        assert_eq!(shorten("\n*.json\n*.tsx"), "*.json…");
    }
}
