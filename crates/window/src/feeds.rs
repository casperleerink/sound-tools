//! Feeds as the views bind them: the numbers live values read, each read from
//! `sound_core::feeds` at most once a frame, and only while a shown element binds it.

use std::cell::RefCell;
use std::collections::BTreeMap;

use sound_core::feeds::{self, Feed, FeedRequest, Readings, Sources};
use sound_core::{InstanceId, Project, Ticks};

pub(crate) struct Feeds {
    readings: Readings,
    sources: Box<dyn Sources>,
    /// The numbers read this frame and the last, by instance and name.
    numbers: RefCell<BTreeMap<(InstanceId, String), f32>>,
    last: BTreeMap<(InstanceId, String), f32>,
}

impl Feeds {
    pub(crate) fn new(sources: Box<dyn Sources>) -> Self {
        Self {
            readings: Readings::new(),
            sources,
            numbers: RefCell::default(),
            last: BTreeMap::new(),
        }
    }

    /// Starts a frame `seconds` after the last.
    pub(crate) fn next_frame(&mut self, seconds: f64) {
        self.readings.next_frame(seconds as f32);
        self.last = std::mem::take(self.numbers.get_mut());
    }

    /// The number feed `name` of the instance at `path` inside `instance` (itself when left
    /// out), or `list.N`, at `tick`: 0 for one it does not have.
    pub(crate) fn number(
        &self,
        project: &Project,
        tick: Ticks,
        instance: &InstanceId,
        path: Option<&str>,
        name: &str,
    ) -> f64 {
        let id = match path {
            Some(path) => instance.child(path).ok(),
            None => Some(instance.clone()),
        };
        let Some(id) = id else {
            return 0.0;
        };
        let key = (id, name.to_string());
        if let Some(number) = self.numbers.borrow().get(&key) {
            return f64::from(*number);
        }
        let request = FeedRequest {
            tick,
            points: 0,
            range: None,
        };
        let read = feeds::read(
            project,
            &self.readings,
            &*self.sources,
            &key.0,
            name,
            request,
        );
        let number = match read {
            Some(Feed::Number(number)) => number,
            _ => 0.0,
        };
        self.numbers.borrow_mut().insert(key, number);
        f64::from(number)
    }

    /// Whether a number read this frame differs from the last frame, or was not read then.
    pub(crate) fn moved(&self) -> bool {
        let numbers = self.numbers.borrow();
        (numbers.iter()).any(|(key, number)| self.last.get(key) != Some(number))
    }

    /// The numbers read this frame, as `<instance> <name>`, for the agent's tree.
    pub(crate) fn numbers(&self) -> Vec<(String, f32)> {
        let numbers = self.numbers.borrow();
        (numbers.iter())
            .map(|((id, name), number)| (format!("{id} {name}"), *number))
            .collect()
    }
}
