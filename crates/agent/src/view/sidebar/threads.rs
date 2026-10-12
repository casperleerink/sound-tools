//! The saved threads: reading the project's last one, **New**, **Open recent**, and writing
//! a thread in order as it goes.

use std::path::PathBuf;

use gpui::{Context, Focusable, Task, Window, prelude::*};

use super::Sidebar;
use crate::conversation::{Conversation, request_label};
use crate::store::{RecentThread, SavedThread, ThreadStore, Write};

/// How many threads **Open recent** lists.
const RECENT: usize = 10;

impl Sidebar {
    /// Reads the current thread of `project` from `threads` in the background.
    pub(super) fn load(threads: PathBuf, project: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |sidebar, cx| {
            let (store, current, recent) = cx
                .background_spawn(async move {
                    let store = ThreadStore::new(&threads, &project);
                    let (current, recent) = (store.current(), store.recent(RECENT));
                    (store, current, recent)
                })
                .await;
            // A sidebar that went in the meantime shows nothing.
            sidebar
                .update(cx, |sidebar, cx| sidebar.loaded(store, current, recent, cx))
                .ok();
        })
    }

    fn loaded(
        &mut self,
        store: ThreadStore,
        current: Result<Option<(SavedThread, Conversation)>, String>,
        recent: Result<Vec<RecentThread>, String>,
        cx: &mut Context<Self>,
    ) {
        self.loading = None;
        self.writes = Some(write_in_order(store.clone(), cx));
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
        self.list.reset(self.conversation.entries().len());
        self.update_new_menu(cx);
        cx.notify();
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

    pub(super) fn keep(&self, writes: impl IntoIterator<Item = Write>) {
        let Some(sender) = &self.writes else {
            return;
        };
        for write in writes {
            // The writer ends only with the sidebar, which owns the sender.
            sender.try_send(write).ok();
        }
    }

    /// Drops the thread and its process, and starts empty.
    pub(super) fn new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_thread(window, cx);
        // The old one stays in the store, and no thread is current until the next message.
        self.keep([Write::Current(None)]);
        // With no writer yet, the thread that loads must not come back.
        self.new_thread_while_loading = self.loading.is_some();
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    /// Drops the thread and its process, and shows the recent thread `id` once it is read.
    pub(super) fn open_thread(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.leave_thread(window, cx);
        self.opening = Some(cx.spawn(async move |sidebar, cx| {
            let opened = cx.background_spawn(async move { store.thread(&id) }).await;
            // A sidebar that went in the meantime shows nothing.
            sidebar
                .update(cx, |sidebar, cx| sidebar.opened(opened, cx))
                .ok();
        }));
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    fn opened(
        &mut self,
        opened: Result<Option<(SavedThread, Conversation)>, String>,
        cx: &mut Context<Self>,
    ) {
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
        self.list.reset(self.conversation.entries().len());
        self.update_new_menu(cx);
        cx.notify();
    }

    /// Ends the thread's process, keeps the thread in **Open recent**, and empties the
    /// sidebar. A recent thread still being read stays where it is.
    fn leave_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_instructions(window, cx);
        self.end_agent(cx);
        self.opening = None;
        let conversation = std::mem::take(&mut self.conversation);
        if let Some(saved) = self.saved.take() {
            self.remember(&saved, &conversation);
        }
        self.forget_entries();
        self.list.reset(0);
        self.update_new_menu(cx);
    }
}

/// Hands each write to the background in turn, so the files get them in order. Detached: it
/// ends when the sidebar does, after the writes still queued.
fn write_in_order(store: ThreadStore, cx: &mut Context<Sidebar>) -> smol::channel::Sender<Write> {
    let (sender, receiver) = smol::channel::unbounded::<Write>();
    cx.spawn(async move |sidebar, cx| {
        let mut told = false;
        while let Ok(write) = receiver.recv().await {
            let store = store.clone();
            let written = cx
                .background_spawn(async move { store.write(&write) })
                .await;
            // Said once: a full disk would otherwise add a line at every event.
            if let Err(error) = written
                && !told
            {
                told = true;
                // A sidebar that went has nobody to tell.
                sidebar
                    .update(cx, |sidebar, cx| {
                        let notice = format!("This thread is not saved: {error}");
                        sidebar.conversation.notice(notice);
                        sidebar.show(None, cx);
                    })
                    .ok();
            }
        }
    })
    .detach();
    sender
}
