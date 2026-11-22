//! The saved threads: reading the project's last one, **New**, **Open recent**, and writing
//! a thread in order as it goes.

use std::path::PathBuf;

use sound_core::Project;

use super::{Chat, Done};
use crate::conversation::{Conversation, request_label};
use crate::jobs::{self, Job, Outbox};
use crate::store::{RecentThread, SavedThread, ThreadStore, Write};

/// How many threads **Open recent** lists.
const RECENT: usize = 10;

/// The label of **New**, its menu, and the rows of the menu.
pub const NEW: &str = "New";
pub const NEW_MENU: &str = "Recent threads and instructions";
pub const NEW_THREAD: &str = "New thread";
pub const OPEN_RECENT: &str = "Open recent";

/// Reads the current thread of `project` from `threads` in the background.
pub(super) fn load(threads: PathBuf, project: PathBuf, outbox: Outbox<Done>) -> Job {
    jobs::spawn(async move {
        let loaded = smol::unblock(move || {
            let store = ThreadStore::new(&threads, &project);
            let (current, recent) = (store.current(), store.recent(RECENT));
            Done::Loaded {
                store,
                current,
                recent,
            }
        });
        outbox.send(loaded.await);
    })
}

/// Hands each write to the background in turn, so the files get them in order. Detached: it
/// ends after the writes still queued once the chat is gone.
fn write_in_order(store: ThreadStore, outbox: Outbox<Done>) -> smol::channel::Sender<Write> {
    let (sender, receiver) = smol::channel::unbounded::<Write>();
    smol::spawn(async move {
        let mut told = false;
        while let Ok(write) = receiver.recv().await {
            let store = store.clone();
            let written = smol::unblock(move || store.write(&write)).await;
            // Said once: a full disk would otherwise add a line at every event.
            if let Err(error) = written
                && !told
            {
                told = true;
                outbox.send(Done::Notice(format!("This thread is not saved: {error}")));
            }
            outbox.send(Done::Saved);
        }
    })
    .detach();
    sender
}

impl Chat {
    pub(super) fn loaded(
        &mut self,
        store: ThreadStore,
        current: Result<Option<(SavedThread, Conversation)>, String>,
        recent: Result<Vec<RecentThread>, String>,
    ) {
        self.loading = None;
        self.writes = Some(write_in_order(store.clone(), self.inbox.outbox()));
        self.store = Some(store);
        // The index did not read: `current` says so.
        self.recent = recent.unwrap_or_default();
        // **New** while it loaded: the saved thread stays in the store, and the new one is
        // current.
        let current = match std::mem::take(&mut self.new_thread_while_loading) {
            true => {
                if let Ok(Some((saved, conversation))) = &current {
                    self.remember(saved, conversation);
                }
                self.keep([Write::Current(None)]);
                Ok(None)
            }
            false => current,
        };
        match current {
            Ok(Some((saved, conversation))) => {
                self.saved = Some(saved);
                // Only notices came meanwhile: no message goes while it loads.
                let meanwhile = std::mem::replace(&mut self.conversation, conversation);
                self.conversation.append(meanwhile);
            }
            Ok(None) => {}
            Err(error) => self
                .conversation
                .notice(format!("The saved thread could not be read: {error}")),
        }
        self.forget_entries();
    }

    /// The project's other threads for **Open recent**, the last one left first, by their
    /// first message.
    pub fn recent_threads(&self) -> &[RecentThread] {
        &self.recent
    }

    /// Puts a thread first in **Open recent**, by its first message. A thread with none has
    /// nothing to show.
    fn remember(&mut self, saved: &SavedThread, conversation: &Conversation) {
        let Some(message) = conversation.messages().next() else {
            return;
        };
        self.recent.retain(|thread| thread.id != saved.id);
        let title = request_label(message);
        let id = saved.id.clone();
        self.recent.insert(0, RecentThread { id, title });
        self.recent.truncate(RECENT);
    }

    pub(super) fn keep(&mut self, writes: impl IntoIterator<Item = Write>) {
        let Some(sender) = &self.writes else {
            return;
        };
        for write in writes {
            // Fails only once the writer is gone, which ends with the chat.
            if sender.try_send(write).is_ok() {
                self.saving += 1;
            }
        }
    }

    /// **New**: drops the thread and its process, and starts empty. Mid-turn the turn ends,
    /// and its request with it.
    pub fn new_thread(&mut self, project: &mut Project) {
        self.leave_thread(project);
        // The old one stays in the store, and no thread is current until the next message.
        self.keep([Write::Current(None)]);
        // With no writer yet, the thread that loads must not come back.
        self.new_thread_while_loading = self.loading.is_some();
    }

    /// Drops the thread and its process, and shows the recent thread `id` once it is read.
    pub fn open_thread(&mut self, id: &str, project: &mut Project) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.leave_thread(project);
        self.openings += 1;
        let opening = self.openings;
        let outbox = self.inbox.outbox();
        let id = id.to_string();
        let job = jobs::spawn(async move {
            let opened = smol::unblock(move || store.thread(&id)).await;
            outbox.send(Done::Opened(opening, opened));
        });
        self.opening = Some((opening, job));
    }

    pub(super) fn opened(&mut self, opened: Result<Option<(SavedThread, Conversation)>, String>) {
        self.opening = None;
        match opened {
            Ok(Some((saved, conversation))) => {
                self.recent.retain(|thread| thread.id != saved.id);
                self.keep([Write::Current(Some(saved.clone()))]);
                self.saved = Some(saved);
                // Only notices came meanwhile: no message goes while it reads.
                let meanwhile = std::mem::replace(&mut self.conversation, conversation);
                self.conversation.append(meanwhile);
            }
            // Gone from the index meanwhile, such as by a copy of the app on the same project.
            Ok(None) => self
                .conversation
                .notice("That thread is not saved any more."),
            Err(error) => self
                .conversation
                .notice(format!("The thread could not be read: {error}")),
        }
        self.forget_entries();
    }

    /// Ends the thread's process, keeps the thread in **Open recent**, and empties the
    /// chat. A recent thread still being read stays where it is.
    fn leave_thread(&mut self, project: &mut Project) {
        self.close_instructions();
        self.end_agent(project);
        self.opening = None;
        let conversation = std::mem::take(&mut self.conversation);
        if let Some(saved) = self.saved.take() {
            self.remember(&saved, &conversation);
        }
        self.forget_entries();
    }
}
