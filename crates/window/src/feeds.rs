//! Feeds as the views bind them: numbers live values read and lists a `shapes` draws, each read
//! from `sound_core::feeds` at most once a frame, and only while a shown element binds it.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use sound_core::feeds::{self, Feed, FeedRequest, Readings, Sources};
use sound_core::{InstanceId, Project, Ticks};

use crate::app::State;
use crate::tree::Listed;

/// A list feed in the `points` or `rects` of a `shapes`: `live.feed(name, path)`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) enum ListFeed {
    Feed { name: String, path: Option<String> },
}

pub(crate) struct Feeds {
    readings: Readings,
    sources: Box<dyn Sources>,
    /// The numbers read this frame and the last, by instance and name.
    numbers: RefCell<BTreeMap<(InstanceId, String), f32>>,
    last: BTreeMap<(InstanceId, String), f32>,
    /// How many numbers each list read this frame gave.
    lists: RefCell<BTreeMap<(InstanceId, String), usize>>,
    /// How many drawings of lists drew anew this frame.
    drew: Cell<usize>,
}

impl Feeds {
    pub(crate) fn new(sources: Box<dyn Sources>) -> Self {
        Self {
            readings: Readings::new(),
            sources,
            numbers: RefCell::default(),
            last: BTreeMap::new(),
            lists: RefCell::default(),
            drew: Cell::new(0),
        }
    }

    /// What feeds and facts read that the project does not hold.
    pub(crate) fn sources(&self) -> &dyn Sources {
        &*self.sources
    }

    /// Starts a frame `seconds` after the last.
    pub(crate) fn next_frame(&mut self, seconds: f64) {
        self.readings.next_frame(seconds as f32);
        self.last = std::mem::take(self.numbers.get_mut());
        self.lists.get_mut().clear();
        self.drew.set(0);
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
        let Some(id) = inside(instance, path) else {
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
        let number = match self.read(project, &key, request) {
            Some(Feed::Number(number)) => number,
            _ => 0.0,
        };
        self.numbers.borrow_mut().insert(key, number);
        f64::from(number)
    }

    /// Draws the lists of `listed` in the view of `instance` into `pixels`, when they are not
    /// those it drew `last`. A list that is empty, such as the wave of a file still read, is
    /// read again next frame.
    pub(crate) fn draw(
        &self,
        state: &State,
        instance: &InstanceId,
        listed: &Listed,
        pixels: &rune::Pixels,
        last: &RefCell<Vec<Vec<f32>>>,
    ) {
        let lists: Vec<Vec<f32>> = (listed.feeds())
            .map(|ListFeed::Feed { name, path }| {
                let Some(id) = inside(instance, path.as_deref()) else {
                    return Vec::new();
                };
                let key = (id, name.clone());
                let request = FeedRequest {
                    tick: state.playhead,
                    points: listed.points(),
                    range: listed.range(),
                };
                let list = match self.read(state.project(), &key, request) {
                    Some(Feed::List(list)) => list,
                    _ => Vec::new(),
                };
                self.lists.borrow_mut().insert(key, list.len());
                list
            })
            .collect();
        let mut last = last.borrow_mut();
        if *last == lists && pixels.size() != (0, 0) {
            return;
        }
        let (w, h, rgba) = listed.raster(&state.theme, &lists);
        pixels.set(w, h, rgba);
        *last = lists;
        self.drew.set(self.drew.get() + 1);
    }

    fn read(
        &self,
        project: &Project,
        (id, name): &(InstanceId, String),
        request: FeedRequest,
    ) -> Option<Feed> {
        feeds::read(project, &self.readings, &*self.sources, id, name, request)
    }

    /// Whether a number read this frame differs from the last frame, or was not read then, or a
    /// list drew anew.
    pub(crate) fn moved(&self) -> bool {
        let numbers = self.numbers.borrow();
        self.drew.get() > 0
            || (numbers.iter()).any(|(key, number)| self.last.get(key) != Some(number))
    }

    /// What the views read this frame, as `<instance> <name>`, for the agent's tree: each number,
    /// and how many numbers each list gave.
    pub(crate) fn read_now(&self) -> serde_json::Map<String, Value> {
        let name = |(id, name): &(InstanceId, String)| format!("{id} {name}");
        let numbers = self.numbers.borrow();
        let numbers = numbers
            .iter()
            .map(|(key, number)| (name(key), json!(number)));
        let lists = self.lists.borrow();
        let lists = (lists.iter()).map(|(key, count)| (name(key), json!({ "points": count })));
        numbers.chain(lists).collect()
    }
}

#[cfg(test)]
impl Feeds {
    /// How many drawings of lists drew anew in the last frame.
    pub(crate) fn drew(&self) -> usize {
        self.drew.get()
    }
}

/// The instance at `path` inside `instance`, or `instance` itself.
fn inside(instance: &InstanceId, path: Option<&str>) -> Option<InstanceId> {
    match path {
        Some(path) => instance.child(path).ok(),
        None => Some(instance.clone()),
    }
}
