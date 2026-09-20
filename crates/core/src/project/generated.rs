//! Files the runtime writes into the project folder for agents that have only file access.
//!
//! `AGENTS.md` tells an agent that has never seen this repository how to work in the folder:
//! a core section, then the section of every enabled extension. `CLAUDE.md` imports it, so
//! Codex and Claude Code both find it. `problems.txt` lists what [`Project::problems`] holds.
//! It is there the whole time a runtime has the project open, with one plain line when there
//! are no problems, and it goes away when the project closes. So an agent can tell "all is
//! well" from "nobody is watching". All three are written only when their text changes.

use super::{Project, ProjectError};

pub const AGENT_DOC_FILE: &str = "AGENTS.md";
pub const PROBLEMS_FILE: &str = "problems.txt";
const CLAUDE_FILE: &str = "CLAUDE.md";
const CLAUDE_TEXT: &str = "@AGENTS.md\n";
/// The whole of `problems.txt` when every file is live.
pub const NO_PROBLEMS: &str = "No problems. Every file is live.\n";
const CORE_SECTION: &str = include_str!("agent_doc.md");

impl Project {
    /// The text of `AGENTS.md` for this project.
    ///
    /// These placeholders are filled in everywhere, also in the sections of extensions, so
    /// the bar math of every example follows the time signature of the project:
    /// `{{time_signature}}`, `{{ticks_per_beat}}`, `{{ticks_per_bar}}`, `{{bar_5_start}}`,
    /// `{{four_bars}}`, `{{bar_9_start}}` and `{{bar_3_beat_2}}`.
    pub fn agent_doc(&self) -> String {
        let enabled = &self.project_file.extensions;
        let mut tools = String::from("| Tool | Form |\n| --- | --- |\n");
        for (tool, extension, owns_children) in self.registry.tools() {
            if enabled.iter().any(|enabled| enabled == extension) {
                let form = if owns_children {
                    "`<name>/instance.json`, children next to it"
                } else {
                    "`<name>.json`"
                };
                tools.push_str(&format!("| `{tool}` | {form} |\n"));
            }
        }

        let mut doc = CORE_SECTION.replace("{{tools}}", tools.trim_end());
        let sections = enabled
            .iter()
            .filter_map(|extension| self.registry.agent_doc_of(extension));
        for section in sections.chain(self.registry.runtime_agent_doc()) {
            doc.push('\n');
            doc.push_str(section.trim());
            doc.push('\n');
        }

        let time_signature = self.project_file.tempo_map.time_signature();
        let (beat, bar) = (
            time_signature.ticks_per_beat(),
            time_signature.ticks_per_bar(),
        );
        let extensions: Vec<String> = enabled.iter().map(|name| format!("{name:?}")).collect();
        let values = [
            ("{{extensions}}", extensions.join(", ")),
            ("{{time_signature}}", time_signature.to_string()),
            ("{{ticks_per_beat}}", beat.to_string()),
            ("{{ticks_per_bar}}", bar.to_string()),
            ("{{bar_5_start}}", (4 * bar).to_string()),
            ("{{four_bars}}", (4 * bar).to_string()),
            ("{{bar_9_start}}", (8 * bar).to_string()),
            ("{{bar_3_beat_2}}", (2 * bar + beat).to_string()),
        ];
        for (placeholder, value) in values {
            doc = doc.replace(placeholder, &value);
        }
        doc
    }

    /// Brings the generated files up to date. Called when the project opens and from `poll`.
    /// A read-only project writes nothing.
    pub(crate) fn write_generated_files(&mut self) -> Result<(), ProjectError> {
        if self.read_only || !self.generated_are_stale {
            return Ok(());
        }
        // Before the writes: a failed write is reported once, not again on every poll.
        self.generated_are_stale = false;

        let mut problems = String::new();
        for problem in self.problems() {
            problems.push_str(&format!("{}: {}\n", problem.path, problem.message));
        }
        let problems = if problems.is_empty() {
            NO_PROBLEMS
        } else {
            &problems
        };
        self.storage
            .write_generated(PROBLEMS_FILE, Some(problems))?;
        self.storage
            .write_generated(AGENT_DOC_FILE, Some(&self.agent_doc()))?;
        self.storage
            .write_generated(CLAUDE_FILE, Some(CLAUDE_TEXT))?;
        Ok(())
    }
}

/// A clean close takes `problems.txt` away, so its absence means that no runtime watches.
impl Drop for Project {
    fn drop(&mut self) {
        if self.read_only {
            return;
        }
        // There is nobody left to report a failure to. A file that stays is what a crash
        // leaves too, and the agent doc says that such a file can be stale.
        match self.storage.write_generated(PROBLEMS_FILE, None) {
            Ok(()) | Err(_) => {}
        }
    }
}
