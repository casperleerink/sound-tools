//! What the sidebar shows of one thread, built from [`AgentEvent`]s.
//!
//! Plain state with no process and no gpui, so a test feeds it events by hand or from a
//! recording. The view renders it, and the driver only produces the events.
//!
//! Times are the wall clock, not `Instant`: a saved thread replays its events with the times
//! they came at, and gets the same conversation back, "Worked for" included.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use crate::{AgentEvent, ApprovalId, ExitReason, StepId, StepOutcome, TurnOutcome};

/// The longest undo label a message gives, in characters. The project menu says "Undo …" with
/// it, so a long message must not make the menu wide.
const LABEL_LENGTH: usize = 40;

/// The entries of one thread, oldest first.
#[derive(Debug, Default, PartialEq)]
pub struct Conversation {
    entries: Vec<Entry>,
    /// The last event ended a turn as failed. The process usually exits right after with the
    /// same message, which the turn already shows.
    turn_just_failed: bool,
    /// The agent no longer has the session of this thread, so no message can follow.
    session_lost: bool,
}

#[derive(Debug, PartialEq)]
pub enum Entry {
    /// What the composer sent.
    Message(String),
    /// What the agent did for one message.
    Turn(Turn),
    /// A quiet line that is no turn: an error, or why the agent stopped.
    Notice(String),
}

#[derive(Debug, PartialEq)]
pub struct Turn {
    /// The finished blocks of the answer.
    pub blocks: Vec<String>,
    /// The block that streams, until its whole text comes.
    pub streaming: String,
    pub steps: Vec<Step>,
    /// The questions the agent waits on, oldest first. The oldest is shown, and each waits
    /// for its own answer. They are void when the turn ends.
    pub approvals: VecDeque<Approval>,
    started: SystemTime,
    /// `None` while the agent works.
    pub end: Option<TurnEnd>,
}

#[derive(Debug, PartialEq)]
pub struct Step {
    pub id: StepId,
    /// In the past tense, for the steps of a finished turn.
    pub title: String,
    /// In the present tense, for the working line while it runs.
    pub running_title: String,
    /// As it was asked for, for a step the composer denied. Empty in an older saved thread.
    pub request_title: String,
    /// `None` while it runs.
    pub outcome: Option<StepOutcome>,
}

impl Step {
    /// What the fold of a finished turn says: what it did, or for a denied step what it
    /// asked to do, since it never ran.
    pub fn finished_title(&self) -> &str {
        match self.outcome {
            Some(StepOutcome::Denied) if !self.request_title.is_empty() => &self.request_title,
            _ => &self.title,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Approval {
    pub id: ApprovalId,
    pub title: String,
}

#[derive(Debug, PartialEq)]
pub struct TurnEnd {
    pub outcome: TurnOutcome,
    pub worked: Duration,
}

impl Turn {
    fn new(started: SystemTime) -> Self {
        Self {
            blocks: Vec::new(),
            streaming: String::new(),
            steps: Vec::new(),
            approvals: VecDeque::new(),
            started,
            end: None,
        }
    }

    /// The question shown: the oldest one still waiting.
    pub fn approval(&self) -> Option<&Approval> {
        self.approvals.front()
    }

    /// The newest step, which the working line shows.
    pub fn current_step(&self) -> Option<&Step> {
        self.steps.last()
    }

    fn finish(&mut self, outcome: TurnOutcome, now: SystemTime) {
        // An interrupted answer never gets its whole text, so what streamed is what it said.
        if !self.streaming.is_empty() {
            self.blocks.push(std::mem::take(&mut self.streaming));
        }
        self.approvals.clear();
        self.end = Some(TurnEnd {
            outcome,
            // A clock set back during the turn gives nothing rather than nonsense.
            worked: now.duration_since(self.started).unwrap_or_default(),
        });
    }
}

impl Conversation {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// What the composer sent, oldest first.
    pub fn messages(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Message(message) => Some(message.as_str()),
            Entry::Turn(_) | Entry::Notice(_) => None,
        })
    }

    /// Whether a message can follow. Not once the agent lost the session of the thread: only a
    /// new thread can go on.
    pub fn can_continue(&self) -> bool {
        !self.session_lost
    }

    /// Whether a turn runs, which includes one that waits on an approval.
    pub fn is_working(&self) -> bool {
        self.open_turn_index().is_some()
    }

    pub fn approval(&self) -> Option<&Approval> {
        let index = self.open_turn_index()?;
        match self.entries.get(index) {
            Some(Entry::Turn(turn)) => turn.approval(),
            _ => None,
        }
    }

    /// The composer's message, and the turn that answers it, working from now.
    pub fn send(&mut self, message: &str, now: SystemTime) {
        self.entries.push(Entry::Message(message.to_string()));
        self.entries.push(Entry::Turn(Turn::new(now)));
    }

    /// The question shown was answered: its row goes, and the next question or the turn goes
    /// on. Gives the question's id and the index of its turn.
    pub fn answered(&mut self) -> Option<(ApprovalId, usize)> {
        let index = self.open_turn_index()?;
        let turn = self.open_turn()?;
        let approval = turn.approvals.pop_front()?;
        Some((approval.id, index))
    }

    pub fn notice(&mut self, message: impl Into<String>) {
        self.entries.push(Entry::Notice(message.into()));
    }

    /// The entries of `later` after these, for what came while a saved thread was read.
    pub fn append(&mut self, later: Conversation) {
        self.entries.extend(later.entries);
    }

    /// Applies one event. Gives the index of the entry it changed or added, if any, so a view
    /// measures only that one again.
    pub fn apply(&mut self, event: AgentEvent, now: SystemTime) -> Option<usize> {
        let turn_just_failed = std::mem::replace(
            &mut self.turn_just_failed,
            matches!(
                event,
                AgentEvent::TurnEnded {
                    outcome: TurnOutcome::Failed { .. }
                }
            ),
        );
        match event {
            // Nothing to show: the sidebar saves the session id with the thread.
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
            AgentEvent::StepStarted {
                id,
                title,
                running_title,
                request_title,
            } => self.update_open_turn(|turn| {
                turn.steps.push(Step {
                    id,
                    title,
                    running_title,
                    request_title,
                    outcome: None,
                });
            }),
            AgentEvent::StepDone { id, outcome } => self.update_open_turn(|turn| {
                if let Some(step) = turn.steps.iter_mut().rev().find(|step| step.id == id) {
                    step.outcome = Some(outcome);
                }
            }),
            AgentEvent::ApprovalRequested { id, title } => {
                self.update_open_turn(|turn| turn.approvals.push_back(Approval { id, title }))
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
                    // The composer's place says so, see [`Self::can_continue`].
                    ExitReason::SessionNotFound => {
                        self.session_lost = true;
                        "This thread can't continue.".to_string()
                    }
                    // The driver's own sentence, which names the provider.
                    ExitReason::Failed { message } => message,
                };
                // The driver ends every turn before it exits. Should one still be open, it
                // must not look as if the agent still works, and it says why itself.
                let failed = TurnOutcome::Failed {
                    message: message.clone(),
                };
                if let Some(index) = self.update_open_turn(|turn| turn.finish(failed, now)) {
                    return Some(index);
                }
                if turn_just_failed || self.session_lost {
                    return None;
                }
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
        turn_at(conversation, conversation.entries().len() - 1)
    }

    fn turn_at(conversation: &Conversation, index: usize) -> &Turn {
        match conversation.entries().get(index) {
            Some(Entry::Turn(turn)) => turn,
            other => panic!("not a turn: {other:?}"),
        }
    }

    #[test]
    fn a_turn_streams_its_text_and_ends_with_how_long_it_worked() {
        let start = SystemTime::now();
        let mut conversation = Conversation::default();
        conversation.send("Add a clip", start);
        assert!(conversation.is_working());
        let events = [
            AgentEvent::TurnStarted,
            AgentEvent::StepStarted {
                id: step("one"),
                title: "Wrote state/arrangement/track-1/clip.json".to_string(),
                request_title: "Write state/arrangement/track-1/clip.json".to_string(),
                running_title: "Writing state/arrangement/track-1/clip.json".to_string(),
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
        let now = SystemTime::now();
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
            Some((ApprovalId("question".to_string()), 1))
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

    /// The agent can ask a second question before the first is answered. Each waits for its
    /// own answer, the oldest first.
    #[test]
    fn a_second_approval_waits_behind_the_first() {
        let now = SystemTime::now();
        let mut conversation = Conversation::default();
        conversation.send("Build it", now);
        for (id, title) in [
            ("first", "Run `cargo build`"),
            ("second", "Run `cargo test`"),
        ] {
            let asked = AgentEvent::ApprovalRequested {
                id: ApprovalId(id.to_string()),
                title: title.to_string(),
            };
            conversation.apply(asked, now);
        }
        let shown = |conversation: &Conversation| {
            let approval = conversation.approval();
            approval.map(|approval| approval.title.clone())
        };
        assert_eq!(shown(&conversation).as_deref(), Some("Run `cargo build`"));
        let answered = conversation.answered();
        assert_eq!(answered, Some((ApprovalId("first".to_string()), 1)));
        assert_eq!(shown(&conversation).as_deref(), Some("Run `cargo test`"));
        let answered = conversation.answered();
        assert_eq!(answered, Some((ApprovalId("second".to_string()), 1)));
        assert_eq!(shown(&conversation), None);
    }

    fn notices(conversation: &Conversation) -> Vec<&str> {
        conversation
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Notice(line) => Some(line.as_str()),
                _ => None,
            })
            .collect()
    }

    fn exited_with(message: &str) -> AgentEvent {
        AgentEvent::Exited {
            reason: ExitReason::Failed {
                message: message.to_string(),
            },
        }
    }

    /// An exit that finds a turn still open ends it with the reason, on the turn itself.
    #[test]
    fn an_error_is_a_quiet_line_and_an_exit_ends_an_open_turn() {
        let now = SystemTime::now();
        let mut conversation = Conversation::default();
        conversation.send("Hello", now);
        let error = AgentEvent::Error {
            message: "Could not stop".to_string(),
        };
        assert_eq!(conversation.apply(error, now), Some(2));
        let message = "Claude Code stopped: out of memory";
        assert_eq!(conversation.apply(exited_with(message), now), Some(1));
        assert!(!conversation.is_working());
        assert_eq!(notices(&conversation), ["Could not stop"]);
        let outcome = turn_at(&conversation, 1)
            .end
            .as_ref()
            .map(|end| &end.outcome);
        let message = message.to_string();
        assert_eq!(outcome, Some(&TurnOutcome::Failed { message }));

        let finished = AgentEvent::Exited {
            reason: ExitReason::Finished,
        };
        assert_eq!(conversation.apply(finished, now), None);
        // Between turns an exit is a line of its own: the driver's sentence as it is.
        let gone = "Claude Code stopped unexpectedly.";
        assert_eq!(conversation.apply(exited_with(gone), now), Some(3));
        assert_eq!(notices(&conversation), ["Could not stop", gone]);
    }

    /// A crash ends the turn as failed and then exits with the same message: it shows once.
    #[test]
    fn a_crash_mid_turn_says_so_once() {
        let now = SystemTime::now();
        let mut conversation = Conversation::default();
        conversation.send("Hello", now);
        let failed = AgentEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                message: "Killed".to_string(),
            },
        };
        assert_eq!(conversation.apply(failed, now), Some(1));
        assert_eq!(conversation.apply(exited_with("Killed"), now), None);
        assert_eq!(notices(&conversation), Vec::<&str>::new());
    }

    /// A resume of a session the agent no longer has: the turn says what the CLI said, and
    /// the thread takes no more messages. The sidebar says so in place of the composer.
    #[test]
    fn a_lost_session_ends_the_thread() {
        let now = SystemTime::now();
        let mut conversation = Conversation::default();
        conversation.send("Hello", now);
        assert!(conversation.can_continue());
        let failed = AgentEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                message: "No conversation found".to_string(),
            },
        };
        conversation.apply(failed, now);
        let lost = AgentEvent::Exited {
            reason: ExitReason::SessionNotFound,
        };
        assert_eq!(conversation.apply(lost, now), None);
        assert!(!conversation.can_continue());
        assert_eq!(notices(&conversation), Vec::<&str>::new());
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
