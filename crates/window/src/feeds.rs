//! Feeds as the views bind them: numbers live values read and lists a `shapes` draws, each read
//! from `sound_core::feeds` at most once a frame, and only while a shown element binds it.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use runtime::files::Files;
use runtime::input::{Input, TakeNotes};
use serde::Deserialize;
use serde_json::{Value, json};
use sound_core::feeds::{self, Feed, FeedRequest, Readings, Sources};
use sound_core::{AssetName, Assets, InstanceId, Problem, Project, Ticks};
use sound_typescript::RecorderState;
use sound_typescript::next::Drawn;

use crate::app::State;
use crate::tree::{Kept, Listed};

/// A list feed in the `points` or `rects` of a `shapes`: `live.feed(name, path)`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) enum ListFeed {
    Feed { name: String, path: Option<String> },
}

pub(crate) struct Feeds {
    readings: Readings,
    files: Files,
    /// The numbers read this frame and the last, by instance and name.
    numbers: RefCell<BTreeMap<(InstanceId, String), f32>>,
    last: BTreeMap<(InstanceId, String), f32>,
    /// How many numbers each list read this frame gave.
    lists: RefCell<BTreeMap<(InstanceId, String), usize>>,
    /// How many drawings of lists drew anew this frame.
    drew: Cell<usize>,
    /// What was said of what the views read that is not there, last.
    unread: RefCell<Vec<Problem>>,
}

/// Where the problems of what the views read that is not there are, in the project's problems.
pub(crate) const UNREAD: &str = "live/";

impl Feeds {
    pub(crate) fn new() -> Self {
        Self {
            readings: Readings::new(),
            files: Files::new(),
            numbers: RefCell::default(),
            last: BTreeMap::new(),
            lists: RefCell::default(),
            drew: Cell::new(0),
            unread: RefCell::default(),
        }
    }

    /// What the views shown read that is not there, as problems of the project, when that
    /// changed: a feed the device at its path does not give, or a live value the window does
    /// not know. Each reads 0, and the rest of its view draws.
    pub(crate) fn unread(&self, state: &State) -> Option<Vec<Problem>> {
        let mut problems = Vec::new();
        for id in &state.shown {
            let Some(Drawn::View { root, memos }) = state.drawn(id) else {
                continue;
            };
            let trees = std::iter::once(root).chain(memos.values().map(|(_, memo)| memo));
            let unread = trees.filter_map(Kept::of).flat_map(|read| &read.unread);
            let mut said: Vec<String> = unread
                .filter_map(|unread| match unread {
                    Unread::Feed { key, name, path } => {
                        // Nothing there yet gives nothing, as while a view runs again after a
                        // device went.
                        let there = inside(id, path.as_deref());
                        let gives = there.and_then(|at| feeds::gives(state.project(), &at, name));
                        (gives == Some(false)).then(|| {
                            let of = path
                                .as_ref()
                                .map_or(String::new(), |path| format!(" of {path}"));
                            format!("{key} reads the feed \"{name}\"{of}, which is not there")
                        })
                    }
                    Unread::Op { key, op } => {
                        Some(format!("{key} reads \"{op}\", which is no live value"))
                    }
                })
                .collect();
            if !said.is_empty() {
                said.sort();
                said.dedup();
                problems.push(Problem {
                    path: format!("{UNREAD}{id}"),
                    message: format!("{}; each reads 0", said.join("; ")),
                });
            }
        }
        let mut said = self.unread.borrow_mut();
        (*said != problems).then(|| {
            said.clone_from(&problems);
            problems
        })
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
        state: &State,
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
            tick: state.playhead,
            points: 0,
            range: None,
        };
        let number = match self.read(state, &key, request) {
            Some(Feed::Number(number)) => number,
            _ => 0.0,
        };
        self.numbers.borrow_mut().insert(key, number);
        f64::from(number)
    }

    /// Draws the lists of `listed` in the view of `instance` into `pixels`, when they are not
    /// those it drew `last`; in a plane, only the part that shows, which `rect` moves to. A list
    /// that is empty, such as the wave of a file still read, is read again next frame.
    pub(crate) fn draw(
        &self,
        state: &State,
        instance: &InstanceId,
        listed: &Listed,
        (pixels, rect): (&rune::Pixels, Option<&rune::Live<[f32; 4]>>),
        last: &RefCell<Vec<Vec<f32>>>,
    ) {
        let seen = listed.seen(state, instance);
        if let Some(rect) = rect {
            rect.set(seen.and_then(|seen| seen.rect()).unwrap_or_default());
        }
        let Some(seen) = seen else {
            return;
        };
        let lists: Vec<Vec<f32>> = (listed.feeds())
            .map(|ListFeed::Feed { name, path }| {
                let Some(id) = inside(instance, path.as_deref()) else {
                    return Vec::new();
                };
                let key = (id, name.clone());
                let request = FeedRequest {
                    tick: state.playhead,
                    points: seen.points(),
                    range: seen.range(),
                };
                let list = match self.read(state, &key, request) {
                    Some(Feed::List(list)) => list,
                    _ => Vec::new(),
                };
                self.lists.borrow_mut().insert(key, list.len());
                list
            })
            .collect();
        let mut last = last.borrow_mut();
        let size = Listed::pixels(&seen, &lists);
        if *last == lists && pixels.size() == size && size != (0, 0) {
            return;
        }
        let (w, h, rgba) = listed.raster(&state.theme, &seen, &lists);
        pixels.set(w, h, rgba);
        *last = lists;
        self.drew.set(self.drew.get() + 1);
    }

    fn read(
        &self,
        state: &State,
        (id, name): &(InstanceId, String),
        request: FeedRequest,
    ) -> Option<Feed> {
        let (project, sources) = (state.project(), state.sources());
        feeds::read(project, &self.readings, &sources, id, name, request)
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

impl State {
    /// What feeds and facts read now that the project does not hold.
    pub(crate) fn sources(&self) -> Read<'_> {
        Read {
            files: &self.feeds.files,
            recording: Some(&self.incoming.live),
            project: self.project(),
            now: self.playhead,
        }
    }
}

/// What the recorders' feeds and facts read of the input that records.
pub(crate) trait Recording {
    /// The level now of each channel of the audio input, 0 to 1; none while it is closed.
    fn levels(&self) -> &[f32];
    /// The notes of the take of `recorder` that runs.
    fn notes(&self, recorder: &InstanceId) -> Option<&TakeNotes>;
    /// Where the sound of the take of `recorder` that runs began, and the second of its file
    /// heard then.
    fn began(&self, recorder: &InstanceId) -> Option<(Ticks, f64)>;
    /// The loudest sample between each two of `edges`, seconds into the file of that take.
    fn peaks(&self, recorder: &InstanceId, edges: &[f64]) -> Option<Vec<f32>>;
}

impl Recording for Input {
    fn levels(&self) -> &[f32] {
        self.input_levels()
    }

    fn notes(&self, recorder: &InstanceId) -> Option<&TakeNotes> {
        self.take_notes(recorder)
    }

    fn began(&self, recorder: &InstanceId) -> Option<(Ticks, f64)> {
        let sound = self.take_sound(recorder)?;
        Some((sound.start, sound.start_seconds))
    }

    fn peaks(&self, recorder: &InstanceId, edges: &[f64]) -> Option<Vec<f32>> {
        let overview = &self.take_sound(recorder)?.overview;
        Some(overview.read(|overview| {
            let rate = f64::from(overview.sample_rate());
            let frames: Vec<u64> = (edges.iter())
                .map(|seconds| (seconds * rate).max(0.0) as u64)
                .collect();
            overview.peaks(&frames)
        }))
    }
}

/// The window's [`Sources`]: the waves of files, and what the input records now.
pub(crate) struct Read<'a> {
    files: &'a Files,
    recording: Option<&'a dyn Recording>,
    project: &'a Project,
    now: Ticks,
}

impl Sources for Read<'_> {
    fn wave(&self, assets: &Assets, asset: &AssetName, edges: &[f64]) -> Option<Vec<f32>> {
        self.files.wave(assets, asset, edges)
    }

    fn input_level(&self, recorder: &InstanceId) -> Option<f32> {
        let levels = self.recording?.levels();
        let record = serde_json::from_str::<RecorderState>(&self.project.state_json(recorder)?);
        let channels = record.ok()?.channels;
        let heard = channels.iter().filter_map(|channel| levels.get(*channel));
        heard.copied().reduce(f32::max)
    }

    fn taking(&self, recorder: &InstanceId) -> Option<Ticks> {
        let recording = self.recording?;
        (recording.began(recorder).map(|(start, _)| start))
            .or_else(|| recording.notes(recorder).map(|notes| notes.start))
    }

    fn take_wave(
        &self,
        recorder: &InstanceId,
        from: f64,
        to: f64,
        points: usize,
    ) -> Option<Vec<f32>> {
        let recording = self.recording?;
        let (start, heard) = recording.began(recorder)?;
        let clock = self.project.clock();
        let began = clock.seconds_of(start);
        let step = (to - from) / points.max(1) as f64;
        let edges: Vec<f64> = (0..=points.max(1))
            .map(|edge| {
                let tick = Ticks(start.0 + (from + step * edge as f64).max(0.0) as u64);
                heard + clock.seconds_of(tick) - began
            })
            .collect();
        recording.peaks(recorder, &edges)
    }

    fn take_notes(&self, recorder: &InstanceId) -> Option<Vec<f32>> {
        let take = self.recording?.notes(recorder)?;
        let after = |tick: Ticks| tick.0.saturating_sub(take.start.0) as f32;
        let notes = (take.notes.iter()).flat_map(|note| {
            let end = note.end.unwrap_or(self.now);
            let length = end.0.saturating_sub(note.start.0) as f32;
            [after(note.start), f32::from(note.pitch), length, 1.0]
        });
        Some(notes.collect())
    }
}

#[cfg(test)]
impl State {
    /// [`State::sources`] with `recording` as the input.
    pub(crate) fn recorded<'a>(&'a self, recording: &'a dyn Recording) -> Read<'a> {
        Read {
            recording: Some(recording),
            ..self.sources()
        }
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

/// What a tree of a view reads that may not be there, by the key of the element that reads it.
pub(crate) enum Unread {
    /// A feed, which the device at `path` may not give.
    Feed {
        key: String,
        name: String,
        path: Option<String>,
    },
    /// A live value the window does not know.
    Op { key: String, op: String },
}

/// What the tree `value` reads that may not be there.
pub(crate) fn unread_in(value: &Value) -> Vec<Unread> {
    let mut unread = Vec::new();
    walk(value, "the view", &mut HashMap::new(), &mut unread);
    unread
}

/// What `value`, in the element `key` or its own, reads that may not be there; `known` says of
/// each op seen whether it is a live value.
fn walk(value: &Value, key: &str, known: &mut HashMap<String, bool>, unread: &mut Vec<Unread>) {
    match value {
        Value::Object(map) => {
            // An element's key, not that of a live value, such as a plane's.
            let element = map.contains_key("t").then(|| map.get("key")).flatten();
            let key = element.and_then(Value::as_str).unwrap_or(key);
            match map.get("op").and_then(Value::as_str) {
                Some("feed") => unread.push(Unread::Feed {
                    key: key.to_string(),
                    name: (map.get("name").and_then(Value::as_str))
                        .unwrap_or_default()
                        .to_string(),
                    path: map.get("path").and_then(Value::as_str).map(str::to_string),
                }),
                Some(op) => {
                    let knows = *(known.entry(op.to_string()))
                        .or_insert_with(|| sound_typescript::live::is_known(op));
                    if !knows {
                        unread.push(Unread::Op {
                            key: key.to_string(),
                            op: op.to_string(),
                        });
                    }
                }
                None => {}
            }
            for inside in map.values() {
                walk(inside, key, known, unread);
            }
        }
        Value::Array(items) => {
            for inside in items {
                walk(inside, key, known, unread);
            }
        }
        _ => {}
    }
}
