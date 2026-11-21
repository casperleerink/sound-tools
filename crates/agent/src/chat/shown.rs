//! What a turn of the thread shows, in the composer's words: the line once it ended, the steps
//! behind it, the answer, the working line while it works, and what goes under it. A message
//! and a notice show as they are in [`crate::Entry`].

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use sound_core::Project;

use super::Chat;
use crate::conversation::{Approval, Entry, Step};
use crate::names::file_name;
use crate::text::{Span, code_spans};
use crate::{StepKind, StepOutcome, TurnOutcome};

/// Under the last turn when it failed: its message again.
pub const TRY_AGAIN: (&str, &str) = ("redo-2", "Try again");
/// Under the problems the last turn left: sends [`super::FIX_NOT_LIVE`].
pub const FIX_THEM: (&str, &str) = ("wrench", "Ask the agent to fix them");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnShown<'a> {
    /// Once it ended: "Worked for …", which opens the steps when it took any.
    pub meta: Option<Meta>,
    /// While they are open.
    pub steps: Option<Vec<StepShown>>,
    /// The blocks of the answer, as markdown, the one that streams last.
    pub answer: Vec<&'a str>,
    /// While the agent works on it: a new block fades in. A thread that opens shows its
    /// answers at once.
    pub streaming: bool,
    pub working: Option<Working>,
    pub below: Option<Below<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// "Worked for 12 s", "Stopped after 2 min 5 s", or the failure, in red, which may wrap.
    pub line: String,
    /// "· Changed 1 file and read 1 file", cut with an ellipsis.
    pub summary: Option<String>,
    pub failed: bool,
    /// Whether it has steps to open, with a chevron.
    pub opens: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepShown {
    pub icon: &'static str,
    pub words: String,
    pub mark: Option<Mark>,
    /// The first line of its command, in mono under its words.
    pub command: Option<String>,
}

/// A step that did not do what it says says so in words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// In red.
    Failed,
    Denied,
}

impl Mark {
    pub fn words(self) -> &'static str {
        match self {
            Mark::Failed => "· failed",
            Mark::Denied => "· denied",
        }
    }
}

/// While it works: the running step's icon and words, or a pulsing dot and "Working", with a
/// light band sweeping across, and the time beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Working {
    /// `None`: the pulsing dot.
    pub icon: Option<&'static str>,
    pub words: String,
    pub elapsed: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Below<'a> {
    /// The question the agent waits on, in a lavender card: its words, with only `code`
    /// styled, the command whole, every line, and the answers of
    /// [`super::menus::APPROVAL_ANSWERS`].
    Approval {
        words: Vec<Span>,
        command: Option<&'a str>,
    },
    /// The files the turn left not live, in peach: one line that opens to a line each.
    Problems {
        line: String,
        /// While open: "name: message".
        lines: Option<Vec<String>>,
        /// "Ask the agent to fix them", under the last turn.
        fix: bool,
    },
    TryAgain,
}

impl Chat {
    /// What the turn at `index` shows at `now`, or `None` for an entry that is no turn.
    pub fn turn(&self, index: usize, project: &Project, now: SystemTime) -> Option<TurnShown<'_>> {
        let Some(Entry::Turn(turn)) = self.conversation.entries().get(index) else {
            return None;
        };
        let meta = turn.end.as_ref().map(|end| {
            let (line, summary, failed) = match &end.outcome {
                TurnOutcome::Completed => (
                    format!("Worked for {}", duration(end.worked)),
                    turn.summary(),
                    false,
                ),
                TurnOutcome::Interrupted => (
                    format!("Stopped after {}", duration(end.worked)),
                    turn.summary(),
                    false,
                ),
                TurnOutcome::Failed { message } if message.is_empty() => {
                    ("The turn failed".to_string(), None, true)
                }
                TurnOutcome::Failed { message } => (message.clone(), None, true),
            };
            Meta {
                line,
                summary: summary.map(|summary| format!("· {summary}")),
                failed,
                opens: !turn.steps.is_empty(),
            }
        });
        let steps = self.steps_open.contains(&index).then(|| {
            (turn.steps.iter())
                .map(|step| step_shown(step, project))
                .collect()
        });
        let working = turn.end.is_none();
        let answer = (turn.blocks.iter())
            .chain((!turn.streaming.is_empty()).then_some(&turn.streaming))
            .map(String::as_str)
            .collect();
        // While a question waits, the question is what the agent does.
        let working_line = (working && turn.approvals.is_empty()).then(|| {
            // A step's icon and words while it runs; between steps the agent thinks.
            let step = turn.current_step().filter(|step| step.outcome.is_none());
            Working {
                icon: step.map(|step| icon(&step.kind)),
                words: plain(step.map_or("Working", doing)),
                elapsed: duration(turn.working_for(now)),
            }
        });
        let below = match (turn.approval(), self.problems_left.get(&index)) {
            (Some(approval), _) => Some(question(approval)),
            (None, Some(problems)) => {
                let files = (problems.iter())
                    .map(|problem| problem.path.as_str())
                    .collect::<BTreeSet<_>>()
                    .len();
                let line = match files {
                    1 => "1 file is not live".to_string(),
                    files => format!("{files} files are not live"),
                };
                let lines = self.problems_open.contains(&index).then(|| {
                    (problems.iter())
                        .map(|problem| {
                            format!("{}: {}", file_name(project, &problem.path), problem.message)
                        })
                        .collect()
                });
                Some(Below::Problems {
                    line,
                    lines,
                    fix: self.goes_on_after(index),
                })
            }
            (None, None) => self.retry_message(index).map(|_| Below::TryAgain),
        };
        Some(TurnShown {
            meta,
            steps,
            answer,
            streaming: working,
            working: working_line,
            below,
        })
    }
}

fn question(approval: &Approval) -> Below<'_> {
    Below::Approval {
        words: code_spans(&approval.title),
        command: approval.command.as_deref(),
    }
}

fn step_shown(step: &Step, project: &Project) -> StepShown {
    let mark = match step.outcome {
        Some(StepOutcome::Failed) => Some(Mark::Failed),
        Some(StepOutcome::Denied) => Some(Mark::Denied),
        Some(StepOutcome::Done) | None => None,
    };
    let command = step.command.as_deref().map(|command| {
        let mut lines = command.lines();
        let first = lines.next().unwrap_or_default();
        let more = if lines.next().is_some() { " …" } else { "" };
        format!("{first}{more}")
    });
    StepShown {
        icon: icon(&step.kind),
        words: plain(&title(step, project)),
        mark,
        command,
    }
}

/// A step's line: what it read or changed by the name of that part, the others as the
/// provider says them. A denied step says what it asked, since it never ran.
fn title(step: &Step, project: &Project) -> String {
    match (&step.kind, &step.outcome) {
        (StepKind::Read { path }, _) => format!("Read {}", file_name(project, path)),
        (StepKind::Change { path }, Some(StepOutcome::Denied)) => {
            format!("Change {}", file_name(project, path))
        }
        (StepKind::Change { path }, _) => format!("Changed {}", file_name(project, path)),
        _ => step.finished_title().to_string(),
    }
}

/// What a step does, in the composer's words: a command as the agent says it does (or
/// "Running a command", as the provider writes it), the others by their kind. Their own
/// titles name a file or a tool of the agent; their lines in the opened steps show that.
fn doing(step: &Step) -> &str {
    match &step.kind {
        StepKind::Run => &step.running_title,
        StepKind::Read { .. } => "Reading the project",
        StepKind::Change { .. } => "Changing the project",
        StepKind::Search => "Searching the project",
        StepKind::Other => "Working",
    }
}

/// The icon of a step's line.
fn icon(kind: &StepKind) -> &'static str {
    match kind {
        StepKind::Read { .. } => "eye",
        StepKind::Change { .. } => "pencil",
        StepKind::Run => "terminal",
        StepKind::Search => "search",
        StepKind::Other => "wrench",
    }
}

/// A title with its commands as plain text, for a line that is cut with an ellipsis.
fn plain(title: &str) -> String {
    title.replace('`', "")
}

/// "12 s", or "2 min 5 s" from a minute on.
fn duration(worked: Duration) -> String {
    let seconds = worked.as_secs();
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min {} s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_in_seconds_then_minutes() {
        assert_eq!(duration(Duration::from_millis(12_400)), "12 s");
        assert_eq!(duration(Duration::from_secs(125)), "2 min 5 s");
    }
}
