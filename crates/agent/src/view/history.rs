//! Up and down in the composer recall the earlier messages of the thread, as a shell does.

use sound_ui::components::text_input::Arrow;

/// Where the composer is among the earlier messages. Only an empty composer, or one that still
/// shows a recalled message as it was, moves through them, so a draft is never lost.
#[derive(Debug, Default)]
pub struct History {
    /// How far back from the newest message, which is 0. `None` while none shows.
    position: Option<usize>,
}

impl History {
    /// The text the composer shows next, or `None` to leave it as it is. `messages` are the
    /// thread's, oldest first; `text` is what the composer shows now. Up goes back from the
    /// newest, down goes forward again and past the newest to an empty composer.
    pub fn recall(&mut self, messages: &[&str], text: &str, arrow: Arrow) -> Option<String> {
        let newest_first = |position: usize| {
            messages
                .len()
                .checked_sub(position + 1)
                .and_then(|index| messages.get(index))
        };
        let shown = self.position.and_then(newest_first);
        let position = match shown {
            Some(shown) if *shown == text => self.position,
            _ if text.is_empty() => None,
            _ => return None,
        };
        let next = match (arrow, position) {
            (Arrow::Up, None) => 0,
            (Arrow::Up, Some(position)) => position + 1,
            (Arrow::Down, None) => return None,
            (Arrow::Down, Some(0)) => {
                self.position = None;
                return Some(String::new());
            }
            (Arrow::Down, Some(position)) => position - 1,
        };
        let message = newest_first(next)?;
        self.position = Some(next);
        Some(message.to_string())
    }

    /// After a send, or in a new thread: up starts again from the newest.
    pub fn reset(&mut self) {
        self.position = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESSAGES: [&str; 3] = ["Add a bass", "Make it louder", "Undo that"];

    #[test]
    fn up_goes_back_from_the_newest_and_down_comes_back_to_empty() {
        let mut history = History::default();
        let mut text = String::new();
        let mut press = |arrow| {
            if let Some(next) = history.recall(&MESSAGES, &text, arrow) {
                text = next;
            }
            text.clone()
        };
        assert_eq!(press(Arrow::Down), "");
        assert_eq!(press(Arrow::Up), "Undo that");
        assert_eq!(press(Arrow::Up), "Make it louder");
        assert_eq!(press(Arrow::Up), "Add a bass");
        // The oldest stays.
        assert_eq!(press(Arrow::Up), "Add a bass");
        assert_eq!(press(Arrow::Down), "Make it louder");
        assert_eq!(press(Arrow::Down), "Undo that");
        assert_eq!(press(Arrow::Down), "");
        assert_eq!(press(Arrow::Down), "");
        assert_eq!(press(Arrow::Up), "Undo that");
    }

    #[test]
    fn a_draft_stays() {
        let mut history = History::default();
        assert_eq!(history.recall(&MESSAGES, "Add a drum", Arrow::Up), None);
        assert_eq!(
            history.recall(&MESSAGES, "", Arrow::Up).as_deref(),
            Some("Undo that")
        );
        // A recalled message the composer changed is a draft too.
        assert_eq!(
            history.recall(&MESSAGES, "Undo that twice", Arrow::Up),
            None
        );
        assert_eq!(
            history.recall(&MESSAGES, "Undo that twice", Arrow::Down),
            None
        );
        // Cleared, it starts again from the newest.
        assert_eq!(
            history.recall(&MESSAGES, "", Arrow::Up).as_deref(),
            Some("Undo that")
        );
    }

    #[test]
    fn a_thread_with_no_messages_recalls_nothing() {
        let mut history = History::default();
        assert_eq!(history.recall(&[], "", Arrow::Up), None);
        assert_eq!(history.recall(&[], "", Arrow::Down), None);
    }
}
