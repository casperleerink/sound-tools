//! Feeds: what a view draws of an instance that moves while it plays, worked out in Rust by the
//! code that makes the sound, so a curve on a card is what is heard. A tool registers each by
//! name, [`crate::ToolRegistration::feed`]; the window reads one per shown binding and frame
//! with [`read`], never on the audio thread.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use serde::Serialize;

use super::Project;
use super::assets::{AssetName, Assets};
use super::instance::{InstanceId, JsonState, State};
use crate::clock::{Clock, Ticks};
use crate::event_tap::{EventTap, Tappable};
use crate::scope::Scope;

/// What a level feed in dB gives for silence, which has no level in dB.
pub const SILENT_DB: f32 = -120.0;

/// An amplitude in dBFS, [`SILENT_DB`] or more.
pub fn db(amplitude: f32) -> f32 {
    (20.0 * amplitude.log10()).max(SILENT_DB)
}

/// What a response feed spans across: octaves above 20 Hz, up to 20 kHz.
pub const AUDIBLE_OCTAVES: [f64; 2] = [0.0, 9.965_784];

/// The frequency `octaves` above 20 Hz: where a point of a response feed is.
pub fn hz_at(octaves: f32) -> f32 {
    20.0 * octaves.exp2()
}

/// What a feed gives for one frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Feed {
    Number(f32),
    List(Vec<f32>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedKind {
    Number,
    List,
}

/// What a feed function returns: its kind is known from its type, so a list feed cannot give a
/// number.
pub trait FeedValue {
    const KIND: FeedKind;
    fn into_feed(self) -> Feed;
}

impl FeedValue for f32 {
    const KIND: FeedKind = FeedKind::Number;
    fn into_feed(self) -> Feed {
        Feed::Number(self)
    }
}

impl FeedValue for Vec<f32> {
    const KIND: FeedKind = FeedKind::List;
    fn into_feed(self) -> Feed {
        Feed::List(self)
    }
}

/// Where a binding reads a feed: where the piece plays, and for a list how many points across
/// and which part of its units it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeedRequest {
    /// Lanes play into the record here: a feed works from the record as it plays.
    pub tick: Ticks,
    /// Points across a list, such as the width in device pixels. 0: the feed's own default.
    pub points: usize,
    /// The part of a list's units that shows, such as the seconds of a file a zoomed view
    /// shows. `None`: all of it.
    pub range: Option<[f64; 2]>,
}

/// What feeds and facts read that the engine does not hold: files and takes. The window gives
/// one; every method reads nothing by default.
pub trait Sources {
    /// The loudest sample, 0 to 1, between each two of `edges`, seconds into the audio file
    /// `asset` from left to right. `None` while it is read, or when it does not play.
    fn wave(&self, _assets: &Assets, _asset: &AssetName, _edges: &[f64]) -> Option<Vec<f32>> {
        None
    }

    /// The level now, 0 to 1, of the audio input channels `recorder` records.
    fn input_level(&self, _recorder: &InstanceId) -> Option<f32> {
        None
    }

    /// Where the take `recorder` records now began, while one runs.
    fn taking(&self, _recorder: &InstanceId) -> Option<Ticks> {
        None
    }

    /// The loudest sample of each of `points` equal stretches of the running take of
    /// `recorder`, from `from` to `to` ticks after it began.
    fn take_wave(
        &self,
        _recorder: &InstanceId,
        _from: f64,
        _to: f64,
        _points: usize,
    ) -> Option<Vec<f32>> {
        None
    }

    /// The notes of the running take of `recorder`, four numbers each: ticks after the take
    /// began, pitch, length in ticks (up to now for a held one), 1.
    fn take_notes(&self, _recorder: &InstanceId) -> Option<Vec<f32>> {
        None
    }
}

/// Sources that read nothing, for a caller without files or takes.
pub struct NoSources;

impl Sources for NoSources {}

/// What feeds keep from one frame to the next: the peaks each took this frame, which every
/// feed of the frame shares, as a take empties them, and the state a feed keeps, such as an
/// analysis that hears the sound since its last look. One per window; [`Self::next_frame`]
/// between frames.
#[derive(Default)]
pub struct Readings {
    frame: u64,
    seconds: f32,
    taken: RefCell<HashMap<(InstanceId, String), [f32; 2]>>,
    kept: RefCell<HashMap<(InstanceId, &'static str), Kept>>,
}

struct Kept {
    frame: u64,
    value: Box<dyn Any>,
}

impl Readings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a frame `seconds` after the last one. What no feed read in the last frame goes:
    /// a feed shown again starts afresh.
    pub fn next_frame(&mut self, seconds: f32) {
        let last = self.frame;
        self.frame += 1;
        self.seconds = seconds;
        self.taken.get_mut().clear();
        self.kept.get_mut().retain(|_, kept| kept.frame == last);
    }
}

/// What a feed function reads besides the record: the engine's readings of its instance, and
/// what the binding asks for.
pub struct FeedContext<'a> {
    pub sample_rate: u32,
    /// Points across a list. 0: the feed's own default, see [`Self::points_or`].
    pub points: usize,
    /// See [`FeedRequest::range`].
    pub range: Option<[f64; 2]>,
    pub tick: Ticks,
    project: &'a Project,
    instance: &'a InstanceId,
    readings: &'a Readings,
    sources: &'a dyn Sources,
}

impl<'a> FeedContext<'a> {
    pub fn instance(&self) -> &InstanceId {
        self.instance
    }

    /// What the feed `name` of each instance right inside this one gives this frame, for a feed
    /// that gathers its children's, such as the keys down on what a track plays.
    pub fn of_children(&self, name: &str) -> Vec<Feed> {
        let request = FeedRequest {
            tick: self.tick,
            points: self.points,
            range: self.range,
        };
        let children = (self.project.instances.keys())
            .filter(|id| id.parent().as_ref() == Some(self.instance));
        children
            .filter_map(|child| {
                read(
                    self.project,
                    self.readings,
                    self.sources,
                    child,
                    name,
                    request,
                )
            })
            .collect()
    }

    /// The largest value of each channel of the peaks the instance keeps as `name` since the
    /// last frame. Every feed of a frame gets the same values.
    pub fn peaks(&self, name: &str) -> [f32; 2] {
        let key = (self.instance.clone(), name.to_string());
        *(self.readings.taken.borrow_mut().entry(key)).or_insert_with(|| {
            (self.project.peaks(self.instance, name)).map_or([0.0; 2], |peaks| peaks.take())
        })
    }

    /// The loudest of both channels of [`Self::peaks`] `name`, in dBFS, [`SILENT_DB`] for
    /// silence.
    pub fn peak_db(&self, name: &str) -> f32 {
        let [left, right] = self.peaks(name);
        db(left.max(right))
    }

    /// The watch the instance keeps as `name`, 0 without one.
    pub fn watched(&self, name: &str) -> f32 {
        self.project.watched(self.instance, name).unwrap_or(0.0)
    }

    pub fn scope(&self, name: &str) -> Option<Scope> {
        self.project.scope(self.instance, name)
    }

    pub fn tap<T: Tappable + 'static>(&self, name: &str) -> Option<EventTap<T>> {
        self.project.tap(self.instance, name)
    }

    /// Seconds since the last frame.
    pub fn seconds(&self) -> f32 {
        self.readings.seconds
    }

    /// Counts frames, so state kept by [`Self::kept`] can tell a second read in a frame.
    pub fn frame(&self) -> u64 {
        self.readings.frame
    }

    /// State this instance keeps under `key` from one frame to the next while a feed reads it,
    /// such as an analysis that hears the sound since its last look.
    pub fn kept<T: Default + 'static, R>(
        &self,
        key: &'static str,
        read: impl FnOnce(&mut T) -> R,
    ) -> R {
        let mut kept = self.readings.kept.borrow_mut();
        let entry = kept
            .entry((self.instance.clone(), key))
            .or_insert_with(|| Kept {
                frame: self.readings.frame,
                value: Box::new(T::default()),
            });
        entry.frame = self.readings.frame;
        if !entry.value.is::<T>() {
            entry.value = Box::new(T::default());
        }
        match entry.value.downcast_mut::<T>() {
            Some(value) => read(value),
            None => read(&mut T::default()),
        }
    }

    pub fn assets(&self) -> &Assets {
        self.project.assets()
    }

    pub fn clock(&self) -> &Clock {
        self.project.clock()
    }

    pub fn sources(&self) -> &dyn Sources {
        self.sources
    }

    /// See [`Sources::wave`].
    pub fn wave(&self, asset: &AssetName, edges: &[f64]) -> Option<Vec<f32>> {
        self.sources.wave(self.project.assets(), asset, edges)
    }

    /// [`Self::points`], or `default` where the binding left it to the feed.
    pub fn points_or(&self, default: usize) -> usize {
        match self.points {
            0 => default,
            points => points,
        }
    }

    /// [`Self::range`], or `default` where the binding shows all of it.
    pub fn range_or(&self, default: [f64; 2]) -> [f64; 2] {
        self.range.unwrap_or(default)
    }

    /// `count` values evenly from one end of the range, `default` where the binding shows all
    /// of it, to the other, both ends included: the edges of `count - 1` columns.
    pub fn across(&self, default: [f64; 2], count: usize) -> impl Iterator<Item = f64> + use<> {
        let [from, to] = self.range_or(default);
        let points = count.max(2);
        let step = (to - from) / (points - 1) as f64;
        (0..points).map(move |point| from + step * point as f64)
    }

    /// A curve as a line of `shapes` draws a list: `y` of each of [`Self::points`] places of
    /// [`Self::across`], `points` where the binding leaves it.
    pub fn curve(&self, default: [f64; 2], points: usize, y: impl Fn(f32) -> f32) -> Vec<f32> {
        (self.across(default, self.points_or(points)))
            .map(|x| y(x as f32))
            .collect()
    }
}

/// A registered feed of a tool, or of every instance.
pub(crate) struct RegisteredFeed {
    pub name: &'static str,
    pub doc: &'static str,
    pub kind: FeedKind,
    pub read: ErasedFeed,
}

/// Gets the state as it plays, `None` when it is not the state the feed reads.
pub(crate) type ErasedFeed = Box<dyn Fn(&dyn Any, &FeedContext<'_>) -> Option<Feed>>;

/// Gives a copy of a state with the values of lanes, by the names of its numbers.
pub(crate) type ErasedPlay = Box<dyn Fn(&dyn Any, &[(Arc<str>, f32)]) -> Option<Box<dyn Any>>>;

/// The state `S` of a record: typed, or read from the JSON of a tool of the project.
pub(crate) fn typed<S: State>(state: &dyn Any, read: impl FnOnce(&S) -> Feed) -> Option<Feed> {
    match state.downcast_ref::<S>() {
        Some(state) => Some(read(state)),
        None => {
            let json = state.downcast_ref::<JsonState>()?;
            Some(read(&serde_json::from_value::<S>(json.value.clone()).ok()?))
        }
    }
}

/// See [`crate::ToolRegistration::feed`].
pub(crate) fn erase<S: State, T: FeedValue>(
    name: &'static str,
    doc: &'static str,
    read: impl Fn(&S, &FeedContext<'_>) -> T + 'static,
) -> RegisteredFeed {
    RegisteredFeed {
        name,
        doc,
        kind: T::KIND,
        read: Box::new(move |state, context| {
            typed::<S>(state, |state| read(state, context).into_feed())
        }),
    }
}

/// See [`crate::ToolRegistration::schema`]: lanes play into these numbers by their paths.
pub(crate) fn player<S: State>(setters: Vec<(&'static str, fn(&mut S, f32))>) -> ErasedPlay {
    Box::new(move |state, values| {
        let mut played = state.downcast_ref::<S>()?.clone();
        for (name, value) in values {
            let setter = setters.iter().find(|(field, _)| **field == **name);
            if let Some((_, set)) = setter {
                set(&mut played, *value);
            }
        }
        Some(Box::new(played))
    })
}

/// The feeds every instance has, whatever its tool.
pub(crate) fn every_instance() -> Vec<RegisteredFeed> {
    const LEVEL: &str = "level";
    let feed = |name, doc, read: fn(&FeedContext<'_>) -> f32| RegisteredFeed {
        name,
        doc,
        kind: FeedKind::Number,
        read: Box::new(move |_, context| Some(Feed::Number(read(context)))),
    };
    vec![
        feed(
            "peak",
            "The loudest sample of its level since the last frame, 0 to 1 (1 is 0 dBFS).",
            |context| {
                let [left, right] = context.peaks(LEVEL);
                left.max(right)
            },
        ),
        feed(
            "peak_left",
            "The loudest sample of the left channel of its level since the last frame, 0 to 1.",
            |context| context.peaks(LEVEL)[0],
        ),
        feed(
            "peak_right",
            "The loudest sample of the right channel of its level since the last frame, 0 to 1.",
            |context| context.peaks(LEVEL)[1],
        ),
    ]
}

/// Registers feeds and facts for the tool of `S` where no [`crate::ToolRegistration`] is at
/// hand: a tool of the project whose records are JSON, such as the region.
pub struct Shows<'a, S> {
    pub(crate) shown: &'a mut super::registry::Shown,
    pub(crate) state: PhantomData<fn(S)>,
}

impl<S: State> Shows<'_, S> {
    /// See [`crate::ToolRegistration::feed`].
    pub fn feed<T: FeedValue>(
        self,
        name: &'static str,
        doc: &'static str,
        read: impl Fn(&S, &FeedContext<'_>) -> T + 'static,
    ) -> Self {
        self.shown.add_feed(erase(name, doc, read));
        self
    }

    /// See [`crate::ToolRegistration::facts`].
    pub fn facts<F: Serialize + schemars::JsonSchema>(
        self,
        facts: impl Fn(&S, &super::facts::FactsContext<'_>) -> F + 'static,
    ) -> Self {
        self.shown.facts = Some(super::facts::erase(facts));
        self
    }
}

/// Heights, 0 to 1, of the stretches between `edges` as rects, x y w h each, centred on 0.5 of
/// 0 to 1 down: a `rects` drawing with `units` [across, 1] shows a wave mirrored about its middle.
pub fn band(edges: &[f64], tall: impl IntoIterator<Item = f32>) -> Vec<f32> {
    (edges.windows(2).zip(tall))
        .flat_map(|(edge, tall)| {
            let tall = tall.clamp(0.0, 1.0);
            [
                edge[0] as f32,
                (1.0 - tall) / 2.0,
                (edge[1] - edge[0]) as f32,
                tall,
            ]
        })
        .collect()
}

/// What `name` of the instance `instance` gives this frame, from its record as it plays at the
/// request's tick. `name.N` is the number at N of a list feed, as `sounding.60`. `None` for a
/// feed its tool does not have.
pub fn read(
    project: &Project,
    readings: &Readings,
    sources: &dyn Sources,
    instance: &InstanceId,
    name: &str,
    request: FeedRequest,
) -> Option<Feed> {
    let indexed = name
        .rsplit_once('.')
        .and_then(|(list, index)| Some((list, index.parse::<usize>().ok()?)));
    if let Some((list, index)) = indexed {
        return match read(project, readings, sources, instance, list, request)? {
            Feed::List(values) => Some(Feed::Number(values.get(index).copied().unwrap_or(0.0))),
            Feed::Number(_) => None,
        };
    }
    let record = project.instances.get(instance)?;
    let registry = &project.registry;
    let shown = registry.shown(record.tool);
    let feed = (shown.iter().flat_map(|shown| &shown.feeds))
        .chain(&registry.every_instance)
        .find(|feed| feed.name == name)?;
    let context = FeedContext {
        sample_rate: project.sample_rate(),
        points: request.points,
        range: request.range,
        tick: request.tick,
        project,
        instance,
        readings,
        sources,
    };
    let state = record.state.as_any();
    let played = (shown.and_then(|shown| shown.play.as_ref())).and_then(|play| {
        let mut values = Vec::new();
        project
            .lanes(instance)?
            .values_at(request.tick, &mut values);
        (!values.is_empty()).then(|| play(state, &values)).flatten()
    });
    (feed.read)(played.as_deref().unwrap_or(state), &context)
}

/// Whether the instance `instance` gives the feed `name`, or `name.N` of a list feed; `None`
/// when it is not there.
pub fn gives(project: &Project, instance: &InstanceId, name: &str) -> Option<bool> {
    let record = project.instances.get(instance)?;
    let indexed = (name.rsplit_once('.')).filter(|(_, index)| index.parse::<usize>().is_ok());
    let wanted = indexed.map_or(name, |(list, _)| list);
    let registry = &project.registry;
    let shown = registry.shown(record.tool);
    let feed = (shown.iter().flat_map(|shown| &shown.feeds))
        .chain(&registry.every_instance)
        .find(|feed| feed.name == wanted);
    Some(feed.is_some_and(|feed| indexed.is_none() || feed.kind == FeedKind::List))
}

/// A feed as the types of the code of a project name it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FeedName {
    /// `*` for a feed of every instance.
    pub tool: &'static str,
    pub name: &'static str,
    pub kind: FeedKind,
    /// One line that says its units.
    pub doc: &'static str,
}

/// Every feed of every tool.
pub fn names(registry: &super::Registry) -> Vec<FeedName> {
    let every = (registry.every_instance.iter()).map(|feed| ("*", feed));
    let tools = (registry.shown.iter())
        .flat_map(|(tool, shown)| shown.feeds.iter().map(|feed| (*tool, feed)));
    (every.chain(tools))
        .map(|(tool, feed)| FeedName {
            tool,
            name: feed.name,
            kind: feed.kind,
            doc: feed.doc,
        })
        .collect()
}
