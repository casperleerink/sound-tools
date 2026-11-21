//! After the composer went back in the history: a line above the composer that says where to,
//! and a note the agent reads with the next message, so it knows the files changed under it.

use sound_core::{Line, PointId, Project};

use super::Chat;

pub(super) struct WentBack {
    title: String,
    note: String,
}

impl Chat {
    /// A newer one replaces the last.
    pub(super) fn went_back(&mut self, point: PointId, project: &Project) {
        let Some(history) = project.history() else {
            return;
        };
        let title = match history.line_of(point) {
            Ok(line) => line.map(Line::title),
            Err(error) => {
                eprintln!("the version of point {point:?} is unreadable: {error}");
                None
            }
        };
        let Some(title) = title else {
            return;
        };
        let folder = history.folder().display();
        let note = format!(
            "Before this message the composer went back in the project history to point \
             {short} of the version \"{title}\". The project files hold that state now, not \
             what you last saw. The history is a git repository at {folder}; \
             `git --git-dir \"{folder}\" log --all --graph` shows every point.",
            short = point.short(),
        );
        self.went_back = Some(WentBack { title, note });
    }

    /// The muted line above the composer after going back in the history, with a × that
    /// drops it. The next message tells the agent, and the line goes.
    pub fn went_back_line(&self) -> Option<String> {
        let went_back = self.went_back.as_ref()?;
        Some(format!("Went back to “{}”", went_back.title))
    }

    /// The × of the line: the agent is told nothing.
    pub fn dismiss_went_back(&mut self) {
        self.went_back = None;
    }

    /// The note for the message that goes now. The line goes with it.
    pub(super) fn take_went_back_note(&mut self) -> Option<String> {
        Some(self.went_back.take()?.note)
    }
}
