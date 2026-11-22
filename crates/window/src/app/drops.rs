//! Files dropped on an element of a view: brought into the project off the main thread, then
//! given to its `onDrop` all at once, in the order they came.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use rune::{Color, Live};
use runtime::input::{Brought, Importing};
use serde_json::json;
use sound_core::InstanceId;

use super::State;

#[derive(Default)]
pub(crate) struct Drops {
    /// The key in the window of the element files or a dragged element are over now.
    over: Option<String>,
    /// The look of each element with a `hover` look, by its key in the window: live, so a memo
    /// Rune does not build again shows it too.
    looks: RefCell<HashMap<String, Look>>,
    waiting: Vec<Dropping>,
}

/// The fill of an element: at rest, and while something is over it.
#[derive(Clone)]
struct Look {
    live: Live<Color>,
    rest: Color,
    over: Color,
}

struct Dropping {
    instance: InstanceId,
    element: String,
    at: [f32; 2],
    files: Vec<(Importing, Option<Result<Brought, String>>)>,
}

impl Drops {
    pub(crate) fn over(&self, key: &str) -> bool {
        self.over.as_deref() == Some(key)
    }

    pub(crate) fn hover(&mut self, key: &str, on: bool) {
        if on {
            self.set_over(Some(key.to_string()));
        } else if self.over(key) {
            self.set_over(None);
        }
    }

    /// What files or a dragged element are over now, by its key in the window: its `hover`
    /// look shows, as the pointer's does.
    pub(crate) fn set_over(&mut self, over: Option<String>) {
        if over == self.over {
            return;
        }
        let looks = self.looks.borrow();
        if let Some(look) = self.over.as_ref().and_then(|key| looks.get(key)) {
            look.live.set(look.rest);
        }
        if let Some(look) = over.as_ref().and_then(|key| looks.get(key)) {
            look.live.set(look.over);
        }
        drop(looks);
        self.over = over;
    }

    /// The fill of the element `key`: `rest`, or `over` while something is over it.
    pub(crate) fn look(&self, key: &str, rest: Color, over: Color) -> Live<Color> {
        let live = Live::new(if self.over(key) { over } else { rest });
        let look = Look {
            live: live.clone(),
            rest,
            over,
        };
        self.looks.borrow_mut().insert(key.to_string(), look);
        live
    }
}

impl Dropping {
    /// Whether every file came in; a run without a window waits for them.
    fn came(&mut self, headless: bool) -> bool {
        for (importing, brought) in &mut self.files {
            while brought.is_none() {
                *brought = importing.poll();
                if brought.is_none() {
                    if !headless {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
        true
    }
}

impl State {
    /// `files` dropped at `at` on the element `element` of the view of `instance`.
    pub(crate) fn drop_files(
        &mut self,
        instance: &InstanceId,
        element: &str,
        files: Vec<PathBuf>,
        at: [f32; 2],
    ) {
        let waker = self.waker.clone();
        let wake: runtime::input::Waker = Arc::new(move || {
            if let Some(wake) = waker
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                wake();
            }
        });
        let assets = self.project().assets().clone();
        let files = (files.into_iter())
            .map(|file| (runtime::input::import(&assets, file, wake.clone()), None))
            .collect();
        self.drops.waiting.push(Dropping {
            instance: instance.clone(),
            element: element.to_string(),
            at,
            files,
        });
        self.handled();
    }

    /// Gives code the drops whose files all came in. A file that did not is said in the corner.
    pub(super) fn dropped(&mut self) {
        let headless = self.headless();
        let mut came = Vec::new();
        for mut dropping in std::mem::take(&mut self.drops.waiting) {
            match dropping.came(headless) {
                true => came.push(dropping),
                false => self.drops.waiting.push(dropping),
            }
        }
        for dropping in came {
            let (mut audio, mut parts) = (Vec::new(), Vec::new());
            for (_, brought) in dropping.files {
                match brought {
                    Some(Ok(Brought::Sound(asset))) => audio.push(asset),
                    Some(Ok(Brought::Parts(each))) => parts.extend(each),
                    Some(Err(error)) => self.session.report(error),
                    None => {}
                }
            }
            if audio.is_empty() && parts.is_empty() {
                continue;
            }
            let [x, y] = dropping.at;
            let value = json!({ "x": x, "y": y, "audio": audio, "parts": parts });
            self.pointed(&dropping.instance, &dropping.element, "drop", Some(value));
        }
    }
}
