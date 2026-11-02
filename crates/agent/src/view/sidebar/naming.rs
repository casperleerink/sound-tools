//! Naming the versions of the history with the agent's small model, one at a time, once the
//! history has been still for a moment. Only while the agent is set up: without it a version
//! is "Version N".

use std::collections::HashSet;
use std::io;
use std::time::Duration;

use gpui::{Context, Task, prelude::*};
use smol::future;
use sound_core::{History, PointId};

use super::Sidebar;
use crate::naming::{self, Naming};
use crate::view::onboarding::Setup;

/// A program that takes longer is ended, so naming goes on.
const ASK_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct Namer {
    /// The generation of the history the quiet wait started at.
    seen: Option<u64>,
    quiet: Option<Task<()>>,
    asking: Option<Task<()>>,
    /// Versions, by start and tip, the model gave no name for.
    failed: HashSet<(PointId, PointId)>,
}

impl Sidebar {
    /// After any change of the session: the history changed when its generation did.
    pub(super) fn naming_heard(&mut self, cx: &mut Context<Self>) {
        let generation = self.history_generation(cx);
        if generation.is_some() && generation != self.namer.seen {
            self.wait_for_quiet(cx);
        }
    }

    fn history_generation(&self, cx: &Context<Self>) -> Option<u64> {
        let project = self.session.read(cx).project();
        project.history().map(History::generation)
    }

    /// Names the next version once nothing changed for [`naming::QUIET`].
    pub(super) fn wait_for_quiet(&mut self, cx: &mut Context<Self>) {
        self.namer.seen = self.history_generation(cx);
        self.namer.quiet = Some(cx.spawn(async move |sidebar, cx| {
            cx.background_executor().timer(naming::QUIET).await;
            sidebar.update(cx, |sidebar, cx| sidebar.name_next(cx)).ok();
        }));
    }

    fn name_next(&mut self, cx: &mut Context<Self>) {
        if self.namer.asking.is_some() {
            return;
        }
        let (Setup::Ready { .. }, Some(installed)) = (&self.setup, &self.installed) else {
            return;
        };
        let Some(history) = self.session.read(cx).project().history() else {
            return;
        };
        let Naming {
            start,
            tip,
            description,
        } = match naming::next(history, &self.namer.failed) {
            Ok(Some(naming)) => naming,
            Ok(None) => return,
            Err(error) => {
                eprintln!("the versions to name are unreadable: {error}");
                return;
            }
        };
        let (provider, installed) = (self.provider, installed.clone());
        let asking = cx.background_spawn(async move {
            (provider.ask_small_model(&installed, naming::INSTRUCTIONS, &description)).await
        });
        let timeout = cx.background_executor().timer(ASK_TIMEOUT);
        self.namer.asking = Some(cx.spawn(async move |sidebar, cx| {
            let timeout = async {
                timeout.await;
                Err(io::Error::new(io::ErrorKind::TimedOut, "it took over 60 s"))
            };
            let answer = future::or(asking, timeout).await;
            (sidebar.update(cx, |sidebar, cx| sidebar.named(start, tip, answer, cx))).ok();
        }));
    }

    fn named(
        &mut self,
        start: PointId,
        tip: PointId,
        answer: io::Result<String>,
        cx: &mut Context<Self>,
    ) {
        self.namer.asking = None;
        let name = answer.and_then(|answer| {
            naming::name_in(&answer).ok_or_else(|| io::Error::other("its answer was empty"))
        });
        let stored = name.and_then(|name| {
            self.session.update(cx, |session, cx| {
                let named =
                    session.background(cx, |project| project.name_version(start, name, tip));
                // The history's views read it again.
                cx.notify();
                named.map_err(io::Error::other)
            })
        });
        if let Err(error) = stored {
            eprintln!("a version was not named: {error}");
            self.namer.failed.insert((start, tip));
        }
        self.wait_for_quiet(cx);
    }
}
