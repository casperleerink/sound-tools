//! How often each part of the window renders, and how long its own `render` takes, by
//! instance. The render budgets of the window tests read it, and so does the measurement of
//! `tooling/perf`. It is always on: one map lookup per render.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

/// A part of the window that renders on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    /// The tree of a card or a page of a tool of the project.
    Body,
    /// The layer over a page, which draws the spaces that move every frame.
    Over,
    /// What a page shows in the window's title bar.
    TitleBar,
    /// The card of a device in a rack. Counted, not timed: its view is the device's own.
    Card,
    Shell,
    Sidebar,
}

impl Part {
    pub const ALL: [Part; 6] = [
        Part::Body,
        Part::Over,
        Part::TitleBar,
        Part::Card,
        Part::Shell,
        Part::Sidebar,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Part::Body => "body",
            Part::Over => "over",
            Part::TitleBar => "title bar",
            Part::Card => "card",
            Part::Shell => "shell",
            Part::Sidebar => "sidebar",
        }
    }
}

/// The renders of one part of one instance, and the time its `render` took in all.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Count {
    pub renders: u64,
    pub time: Duration,
}

/// Every count so far, by part and instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Renders(pub BTreeMap<(Part, String), Count>);

impl Renders {
    /// What rendered after `earlier`, without what rendered none.
    pub fn since(&self, earlier: &Renders) -> Renders {
        let counts = (self.0.iter())
            .filter_map(|(key, now)| {
                let before = earlier.0.get(key).copied().unwrap_or_default();
                let renders = now.renders - before.renders;
                let time = now.time.saturating_sub(before.time);
                (renders > 0).then(|| (key.clone(), Count { renders, time }))
            })
            .collect();
        Renders(counts)
    }

    /// The renders of `part` of `instance`.
    pub fn of(&self, part: Part, instance: &str) -> u64 {
        (self.0.get(&(part, instance.to_string()))).map_or(0, |count| count.renders)
    }

    /// The renders of `part`, of every instance.
    pub fn all_of(&self, part: Part) -> u64 {
        (self.0.iter())
            .filter(|((each, _), _)| *each == part)
            .map(|(_, count)| count.renders)
            .sum()
    }
}

#[derive(Default)]
struct Counts {
    /// The place of each instance in `counts`, by part. Nested, so a lookup takes a `&str` and a
    /// render allocates nothing after its first.
    slots: HashMap<Part, HashMap<String, usize>>,
    counts: Vec<((Part, String), Count)>,
}

impl Counts {
    fn count(&mut self, part: Part, instance: &str) -> usize {
        let slots = self.slots.entry(part).or_default();
        let slot = match slots.get(instance) {
            Some(slot) => *slot,
            None => {
                self.counts
                    .push(((part, instance.to_string()), Count::default()));
                slots.insert(instance.to_string(), self.counts.len() - 1);
                self.counts.len() - 1
            }
        };
        if let Some((_, count)) = self.counts.get_mut(slot) {
            count.renders += 1;
        }
        slot
    }
}

thread_local! {
    static COUNTS: RefCell<Counts> = RefCell::default();
}

/// Counts a render of `part` of `instance` and times it until the result drops: keep it for
/// the whole `render`.
pub fn start(part: Part, instance: &str) -> Rendering {
    let slot = COUNTS.with_borrow_mut(|counts| counts.count(part, instance));
    Rendering {
        slot,
        started: Instant::now(),
    }
}

/// Counts a render of `part` of `instance`, untimed.
pub fn count(part: Part, instance: &str) {
    COUNTS.with_borrow_mut(|counts| counts.count(part, instance));
}

/// Every count on this thread so far.
pub fn renders() -> Renders {
    COUNTS.with_borrow(|counts| Renders(counts.counts.iter().cloned().collect()))
}

#[must_use = "a render is timed until this drops"]
pub struct Rendering {
    slot: usize,
    started: Instant,
}

impl Drop for Rendering {
    fn drop(&mut self) {
        let time = self.started.elapsed();
        COUNTS.with_borrow_mut(|counts| {
            if let Some((_, count)) = counts.counts.get_mut(self.slot) {
                count.time += time;
            }
        });
    }
}
