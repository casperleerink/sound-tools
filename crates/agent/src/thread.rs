//! What the sidebar shows of one thread, built from [`AgentEvent`]s.
//!
//! Plain state with no process and no gpui, so a test feeds it events by hand or from a
//! recording. The view renders it, and the driver only produces the events.

use std::time::{Duration, Instant};

use crate::{AgentEvent, ApprovalId, ExitReason, StepId, StepOutcome, TurnOutcome};

/// The longest undo label a message gives, in characters. The project menu says "Undo …" with
/// it, so a long message must not make the menu wide.
const LABEL_LENGTH: usize = 40;

/// The entries of one thread, oldest first.
#[derive(Debug, Default)]
pub struct Conversation {
    entries: Vec<Entry>,
}

#[derive(Debug)]
pub enum Entry {
    /// What the composer sent.
    Message(String),
    /// What the agent did for one message.
    Turn(Turn),
    /// A quiet line that is no turn: an error, or why the agent stopped.
    Notice(String),
}

#[derive(Debug)]
pub struct Turn {
    /// The finished blocks of the answer.
    pub blocks: Vec<String>,
    /// The block that streams, until its whole text comes.
    pub streaming: String,
    pub steps: Vec<Step>,
    /// The question the agent waits on. It is void when the turn ends.
    pub approval: Option<Approval>,
    started: Instant,
    /// `None` while the agent works.
    pub end: Option<TurnEnd>,
}

#[derive(Debug)]
pub struct Step {
    pub id: StepId,
    pub title: String,
    /// `None` while it runs.
    pub outcome: Option<StepOutcome>,
}

#[derive(Debug)]
pub struct Approval {
    pub id: ApprovalId,
    pub title: String,
}

#[derive(Debug)]
pub struct TurnEnd {
    pub outcome: TurnOutcome,
    pub worked: Duration,
}

impl Turn {
    fn new(started: Instant) -> Self {
        Self {
            blocks: Vec::new(),
            streaming: String::new(),
            steps: Vec::new(),
            approval: None,
            started,
            end: None,
        }
    }

    /// The newest step, which the working line shows.
    pub fn current_step(&self) -> Option<&Step> {
        self.steps.last()
    }

    fn finish(&mut self, outcome: TurnOutcome, now: Instant) {
        // An interrupted answer never gets its whole text, so what streamed is what it said.
        if !self.streaming.is_empty() {
            self.blocks.push(std::mem::take(&mut self.streaming));
        }
        self.approval = None;
        self.end = Some(TurnEnd {
            outcome,
            worked: now.saturating_duration_since(self.started),
        });
    }
}

impl Conversation {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether a turn runs, which includes one that waits on an approval.
    pub fn is_working(&self) -> bool {
        self.open_turn_index().is_some()
    }

    pub fn approval(&self) -> Option<&Approval> {
        let index = self.open_turn_index()?;
        match self.entries.get(index) {
            Some(Entry::Turn(turn)) => turn.approval.as_ref(),
            _ => None,
        }
    }

    /// The composer's message, and the turn that answers it, working from now.
    pub fn send(&mut self, message: &str, now: Instant) {
        self.entries.push(Entry::Message(message.to_string()));
        self.entries.push(Entry::Turn(Turn::new(now)));
    }

    /// Answered: the row goes, and the turn goes on.
    pub fn answered(&mut self) -> Option<ApprovalId> {
        let turn = self.open_turn()?;
        turn.approval.take().map(|approval| approval.id)
    }

    pub fn notice(&mut self, message: impl Into<String>) {
        self.entries.push(Entry::Notice(message.into()));
    }

    /// Applies one event. Gives the index of the entry it changed or added, if any, so a view
    /// measures only that one again.
    pub fn apply(&mut self, event: AgentEvent, now: Instant) -> Option<usize> {
        match event {
            // Nothing to show yet. The thread is not saved before milestone 7.
            AgentEvent::Started { .. } => None,
            AgentEvent::TurnStarted => match self.open_turn_index() {
                Some(index) => Some(index),
                // [`Self::send`] opens the turn the moment the composer sends, so this only
                // happens for a turn the composer did not start here.
                None => {
                    self.entries.push(Entry::Turn(Turn::new(now)));
                    Some(self.entries.len() - 1)
                }
            },
            AgentEvent::TextDelta { text } => {
                self.update_open_turn(|turn| turn.streaming.push_str(&text))
            }
            AgentEvent::TextDone { text } => self.update_open_turn(|turn| {
                turn.streaming.clear();
                turn.blocks.push(text);
            }),
            AgentEvent::StepStarted { id, title } => self.update_open_turn(|turn| {
                turn.steps.push(Step {
                    id,
                    title,
                    outcome: None,
                });
            }),
            AgentEvent::StepDone { id, outcome } => self.update_open_turn(|turn| {
                if let Some(step) = turn.steps.iter_mut().rev().find(|step| step.id == id) {
                    step.outcome = Some(outcome);
                }
            }),
            AgentEvent::ApprovalRequested { id, title } => {
                self.update_open_turn(|turn| turn.approval = Some(Approval { id, title }))
            }
            AgentEvent::TurnEnded { outcome } => {
                self.update_open_turn(|turn| turn.finish(outcome, now))
            }
            AgentEvent::Error { message } => {
                self.notice(message);
                Some(self.entries.len() - 1)
            }
            AgentEvent::Exited { reason } => {
                let message = match reason {
                    ExitReason::Finished => return None,
                    ExitReason::SessionNotFound => {
                        "This thread can't continue. Start a new one with +.".to_string()
                    }
                    ExitReason::Failed { message } if message.is_empty() => {
                        "Claude Code stopped.".to_string()
                    }
                    ExitReason::Failed { message } => format!("Claude Code stopped: {message}"),
                };
                // The driver ends every turn before it exits. Should one still be open, it
                // must not look as if the agent still works.
                self.update_open_turn(|turn| {
                    let outcome = TurnOutcome::Failed {
                        message: message.clone(),
                    };
                    turn.finish(outcome, now);
                });
                self.notice(message);
                Some(self.entries.len() - 1)
            }
        }
    }

    fn open_turn_index(&self) -> Option<usize> {
        self.entries
            .iter()
            .rposition(|entry| matches!(entry, Entry::Turn(turn) if turn.end.is_none()))
    }

    fn open_turn(&mut self) -> Option<&mut Turn> {
        let index = self.open_turn_index()?;
        match self.entries.get_mut(index) {
            Some(Entry::Turn(turn)) => Some(turn),
            _ => None,
        }
    }

    fn update_open_turn(&mut self, update: impl FnOnce(&mut Turn)) -> Option<usize> {
        let index = self.open_turn_index()?;
        if let Some(Entry::Turn(turn)) = self.entries.get_mut(index) {
            update(turn);
        }
        Some(index)
    }
}

/// The undo label of a request: the message on one line, cut at a word to about 40
/// characters.
pub fn request_label(message: &str) -> String {
    let line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= LABEL_LENGTH {
        return line;
    }
    let cut: String = line.chars().take(LABEL_LENGTH).collect();
    let cut = cut
        .rsplit_once(' ')
        .map_or(cut.as_str(), |(words, _)| words);
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str) -> StepId {
        StepId(id.to_string())
    }

    fn turn(conversation: &Conversation) -> &Turn {
        match conversation.entries().last() {
            Some(Entry::Turn(turn)) => turn,
            other => panic!("not a turn: {other:?}"),
        }
    }

    #[test]
    fn a_turn_streams_its_text_and_ends_with_how_long_it_worked() {
        let start = Instant::now();
        let mut conversation = Conversation::default();
        conversation.send("Add a clip", start);
        assert!(conversation.is_working());
        let events = [
            AgentEvent::TurnStarted,
            AgentEvent::StepStarted {
                id: step("one"),
                title: "Wrote state/arrangement/track-1/clip.json".to_string(),
            },
            AgentEvent::StepDone {
                id: step("one"),
                outcome: StepOutcome::Done,
            },
            AgentEvent::TextDelta {
                text: "Do".to_string(),
            },
            AgentEvent::TextDelta {
                text: "ne".to_string(),
            },
        ];
        for event in events {
            assert_eq!(conversation.apply(event, start), Some(1));
        }
        assert_eq!(turn(&conversation).streaming, "Done");
        assert_eq!(
            turn(&conversation).current_step().map(|step| &step.outcome),
            Some(&Some(StepOutcome::Done))
        );

        let text = AgentEvent::TextDone {
            text: "Done.".to_string(),
        };
        conversation.apply(text, start);
        let end = AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        };
        conversation.apply(end, start + Duration::from_secs(12));
        let turn = turn(&conversation);
        assert_eq!(turn.blocks, ["Done."]);
        assert!(turn.streaming.is_empty());
        let end = turn.end.as_ref().unwrap();
        assert_eq!(end.outcome, TurnOutcome::Completed);
        assert_eq!(end.worked, Duration::from_secs(12));
        assert!(!conversation.is_working());
    }

    #[test]
    fn an_approval_waits_until_answered_and_is_void_when_the_turn_ends() {
        let now = Instant::now();
        let mut conversation = Conversation::default();
        conversation.send("Build it", now);
        let asked = AgentEvent::ApprovalRequested {
            id: ApprovalId("question".to_string()),
            title: "Run `cargo build`".to_string(),
        };
        conversation.apply(asked.clone(), now);
        assert_eq!(
            conversation
                .approval()
                .map(|approval| approval.title.as_str()),
            Some("Run `cargo build`")
        );
        assert_eq!(
            conversation.answered(),
            Some(ApprovalId("question".to_string()))
        );
        assert!(conversation.approval().is_none());

        conversation.apply(asked, now);
        let text = AgentEvent::TextDelta {
            text: "Half".to_string(),
        };
        conversation.apply(text, now);
        let end = AgentEvent::TurnEnded {
            outcome: TurnOutcome::Interrupted,
        };
        conversation.apply(end, now);
        assert!(conversation.approval().is_none());
        // The text of an interrupted answer stays.
        assert_eq!(turn(&conversation).blocks, ["Half"]);
    }

    #[test]
    fn errors_and_a_failed_exit_are_quiet_lines_and_end_an_open_turn() {
        let now = Instant::now();
        let mut conversation = Conversation::default();
        conversation.send("Hello", now);
        let error = AgentEvent::Error {
            message: "Could not stop".to_string(),
        };
        assert_eq!(conversation.apply(error, now), Some(2));
        let exited = AgentEvent::Exited {
            reason: ExitReason::Failed {
                message: "out of memory".to_string(),
            },
        };
        conversation.apply(exited, now);
        assert!(!conversation.is_working());
        let lines: Vec<_> = conversation
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Notice(line) => Some(line.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            lines,
            ["Could not stop", "Claude Code stopped: out of memory"]
        );
        let finished = AgentEvent::Exited {
            reason: ExitReason::Finished,
        };
        assert_eq!(conversation.apply(finished, now), None);
    }

    #[test]
    fn the_label_is_the_message_on_one_line_cut_at_a_word() {
        assert_eq!(request_label("Add a clip"), "Add a clip");
        assert_eq!(
            request_label("Add a bass line in bars 5 to 8 that follows the piano"),
            "Add a bass line in bars 5 to 8 that…"
        );
        assert_eq!(request_label("Two\nlines"), "Two lines");
        let word = "a".repeat(50);
        assert_eq!(request_label(&word), format!("{}…", "a".repeat(40)));
    }
}
