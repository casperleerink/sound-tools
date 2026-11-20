//! Planes: boxes with units of their own (`plane`), which the window scrolls and zooms by
//! itself, and what `place` puts in them at any depth. A scroll moves one live offset; a zoom
//! moves the live place of each placed element, and builds what shows again in Rust only once it
//! rests, as does a scroll to a stretch not built yet. Bun hears neither.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use rune::{El, Key, KeyEvent, el, memo};
use serde::Serialize;
use sound_core::{InstanceId, Problem};
use sound_typescript::live::Num;

use crate::app::State;
use crate::lives::Shows;

use super::build::{build, holds_plane, is_overlay};
use super::{AxisSpec, BoxNode, Node, Place, PlaneSpec, Scope};

/// The cameras of the planes, kept by key across the views' runs, and what is built in them.
#[derive(Default)]
pub(crate) struct Planes {
    cameras: RefCell<HashMap<String, Camera>>,
    /// The keyed elements built in a plane, by key in the window.
    found: RefCell<HashMap<String, Found>>,
    /// The element with the keys, as the last frame said.
    focused: RefCell<Option<String>>,
    /// What was said of places outside a plane, last.
    misplaced: RefCell<Vec<Problem>>,
    /// The ranges code asked a plane to show before it was laid out, across and down.
    asked: RefCell<HashMap<String, [Option<[f32; 2]>; 2]>>,
    /// How many times the content of a plane was built, for a test that a scroll builds none.
    #[cfg(test)]
    pub(crate) builds: Cell<u64>,
}

/// Where an element built in a plane is.
struct Found {
    plane: String,
    /// The key of the placed element in the plane's content it is, or is in.
    top: String,
    /// Where it is in the plane's units: [x, y, w, h].
    rect: [f32; 4],
}

/// What a plane shows: where it is in its units, and how far it is zoomed across.
struct Camera {
    axes: [Axis; 2],
    zoom_range: Option<[f32; 2]>,
    /// Its left and top, in its units.
    at: [f32; 2],
    zoom: f32,
    /// Its size in points, once laid out.
    size: [f32; 2],
    /// Where it was drawn in the window: [x, y, w, h].
    drawn: rune::Live<[f32; 4]>,
    /// What is placed in it spans, in its units: [left, top, right, bottom].
    extent: Option<[f32; 4]>,
    /// How far what is placed in it is moved, in points.
    offset: rune::Live<[f32; 2]>,
    /// Its scroll bars' thumbs, across and down: [x, y, w, h] in its points.
    thumbs: [rune::Live<[f32; 4]>; 2],
    opened: bool,
    /// What its content was last built for.
    built: Cell<Option<Stretch>>,
    /// How long ago it last zoomed, in seconds.
    zoomed: f64,
    follow: Option<Follow>,
}

/// A place across a plane, of the view of `instance`, that it keeps in view while it moves.
struct Follow {
    instance: InstanceId,
    at: Num,
    last: Option<f32>,
    /// How long it has not moved, in seconds.
    rested: f64,
    /// A scroll or zoom by hand stopped the plane following it.
    paused: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Axis {
    Points,
    Scale(f32),
    Range(f32, f32),
    /// The plane with this key in the window, whose camera it shares on this axis; placed in
    /// that plane, where it starts in its units, as it moves with it already.
    Shared(String, Option<f32>),
}

/// What a plane's content is built for: the points per unit its elements are built at, and the
/// part of each axis built, in units. That part begins where a stretch as long as the plane
/// begins and reaches to the end of the next, so a scroll inside a stretch shows only what is
/// built.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Stretch {
    per: [f32; 2],
    range: [[f32; 2]; 2],
}

/// What a plane shows, in its units, as `live.plane` and `--tree` tell it.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    pub(crate) left: f32,
    pub(crate) right: f32,
    pub(crate) top: f32,
    pub(crate) bottom: f32,
    pub(crate) zoom: f32,
}

/// How long a plane is taken to be before it is laid out, in points.
const UNLAID: f32 = 1000.0;
/// How long a zoom rests before what is placed is built again at its scale, in seconds: until
/// then each placed element only moves and sizes.
const ZOOM_RESTS: f64 = 0.15;
/// How much a step of Cmd-= or Cmd-- zooms.
const ZOOM_STEP: f32 = 1.25;
/// How long what a plane follows rests before its moving again is a new start, in seconds: as
/// when the piece plays again.
const RESTED: f64 = 0.25;
/// How thick a scroll bar's thumb is, and how far from the edge.
const THUMB: f32 = 4.0;
const THUMB_EDGE: f32 = 2.0;

impl Planes {
    /// The points per unit of the plane `key`, across and down.
    pub(crate) fn per(&self, key: &str) -> [f32; 2] {
        let cameras = self.cameras.borrow();
        [per(&cameras, key, 0, 0), per(&cameras, key, 1, 0)]
    }

    /// The window point `window` in the units of the plane `key`.
    pub(crate) fn units(&self, key: &str, window: [f32; 2]) -> [f32; 2] {
        let cameras = self.cameras.borrow();
        let Some(camera) = cameras.get(key) else {
            return window;
        };
        let drawn = camera.drawn.get();
        std::array::from_fn(|i| {
            at(&cameras, key, i, 0) + (window[i] - drawn[i]) / per(&cameras, key, i, 0)
        })
    }

    /// What the plane `key` shows now.
    pub(crate) fn view(&self, key: &str) -> Option<View> {
        let cameras = self.cameras.borrow();
        let camera = cameras.get(key)?;
        let [left, top] = [at(&cameras, key, 0, 0), at(&cameras, key, 1, 0)];
        let size = laid(camera.size);
        let zoom = owner(&cameras, key, 0).map_or(1.0, |owner| cameras[owner].zoom);
        Some(View {
            left,
            right: left + size[0] / per(&cameras, key, 0, 0),
            top,
            bottom: top + size[1] / per(&cameras, key, 1, 0),
            zoom,
        })
    }

    /// What each plane shows, by key, for `--tree`.
    pub(crate) fn views(&self) -> BTreeMap<String, View> {
        let keys: Vec<String> = self.cameras.borrow().keys().cloned().collect();
        (keys.into_iter())
            .filter_map(|key| Some((key.clone(), self.view(&key)?)))
            .collect()
    }

    /// Where an element `place` puts in a plane of `per` points per unit is, in points from its
    /// parent at `parent`: `place` [x, y, w, h], or a mark [x, y] of `size` points centred there.
    pub(crate) fn points(
        [px, py]: [f32; 2],
        place: &[f32],
        size: [Option<f32>; 2],
        parent: Option<[f32; 2]>,
    ) -> [f32; 4] {
        let [ox, oy] = parent.unwrap_or_default();
        match *place {
            [x, y, w, h] => {
                // An axis that runs backwards, such as a range [1, 0] down, has fewer points
                // per unit than none: the place reaches back from its start.
                let along = |at: f32, long: f32| [at.min(at + long), long.abs()];
                let ([x, w], [y, h]) = (along((x - ox) * px, w * px), along((y - oy) * py, h * py));
                [x, y, w, h]
            }
            [x, y] => {
                let [w, h] = size.map(Option::unwrap_or_default);
                [(x - ox) * px - w / 2.0, (y - oy) * py - h / 2.0, w, h]
            }
            _ => [0.0; 4],
        }
    }

    /// Takes the plane `key` as its view now gives it, keeping where it is.
    /// `given` is its size in points where its view says it, which layout confirms later.
    /// `within` is the plane it is placed in and where it starts there.
    fn ensure(
        &self,
        key: &str,
        (spec, instance): (&PlaneSpec, &InstanceId),
        prefix: &str,
        given: [Option<f32>; 2],
        within: Option<(&str, [f32; 2])>,
    ) {
        let axis = |spec: &Option<AxisSpec>, i: usize| match spec {
            None => Axis::Points,
            Some(AxisSpec::Scale(scale)) => Axis::Scale(scale.max(f32::EPSILON)),
            Some(AxisSpec::Range([from, to])) if to != from => Axis::Range(*from, *to),
            Some(AxisSpec::Range(_)) => Axis::Points,
            Some(AxisSpec::Shared(key)) => {
                let leader = format!("{prefix}{key}");
                let starts = within.filter(|(outer, _)| *outer == leader);
                Axis::Shared(leader, starts.map(|(_, start)| start[i]))
            }
        };
        let axes = [axis(&spec.x, 0), axis(&spec.y, 1)];
        let mut cameras = self.cameras.borrow_mut();
        let camera = cameras.entry(key.to_string()).or_insert_with(|| Camera {
            axes: axes.clone(),
            zoom_range: None,
            at: [0.0; 2],
            zoom: 1.0,
            size: [0.0; 2],
            drawn: rune::Live::new([0.0; 4]),
            extent: None,
            offset: rune::Live::new([0.0; 2]),
            thumbs: [rune::Live::new([0.0; 4]), rune::Live::new([0.0; 4])],
            opened: false,
            built: Cell::new(None),
            zoomed: f64::INFINITY,
            follow: None,
        });
        camera.axes = axes;
        let kept = camera.follow.take();
        camera.follow = spec.follow.clone().map(|at| match kept {
            Some(kept) if kept.instance == *instance => Follow { at, ..kept },
            _ => Follow {
                instance: instance.clone(),
                at,
                last: None,
                rested: 0.0,
                paused: false,
            },
        });
        camera.zoom_range = spec.zoom.map(|[a, b]| [a.min(b), a.max(b)]);
        if let Some([low, high]) = camera.zoom_range {
            camera.zoom = camera.zoom.clamp(low, high);
        }
        for (size, given) in camera.size.iter_mut().zip(given) {
            if *size == 0.0 {
                *size = given.unwrap_or_default();
            }
        }
        camera.open();
    }

    /// The plane `key` is `size` points: a range shows where it opens, once.
    fn laid_out(&self, key: &str, size: [f32; 2]) {
        let mut cameras = self.cameras.borrow_mut();
        let Some(camera) = cameras.get_mut(key) else {
            return;
        };
        camera.size = size;
        camera.open();
        drop(cameras);
        if let Some([x, y]) = self.asked.borrow_mut().remove(key) {
            self.show(key, x, y);
        }
        self.sync();
    }

    /// Scrolls the plane `key` by `by` points of its content, or the planes it moves with:
    /// whether any moved.
    fn scroll(&self, key: &str, by: [f32; 2]) -> bool {
        let units = {
            let cameras = self.cameras.borrow();
            std::array::from_fn(|i| by[i] / per(&cameras, key, i, 0))
        };
        let moved = self.scroll_units(key, units);
        if moved {
            self.by_hand(key);
        }
        moved
    }

    fn scroll_units(&self, key: &str, by: [f32; 2]) -> bool {
        let mut moved = false;
        for (i, by) in by.into_iter().enumerate() {
            if by == 0.0 {
                continue;
            }
            let mut cameras = self.cameras.borrow_mut();
            let Some(owner) = owner(&cameras, key, i).map(str::to_string) else {
                continue;
            };
            let (low, high) = bounds(&cameras, &owner, i);
            let camera = cameras.get_mut(&owner).expect("an owner is a camera");
            let to = (camera.at[i] + by).clamp(low, high);
            moved |= to != camera.at[i];
            camera.at[i] = to;
        }
        if moved {
            self.sync();
        }
        moved
    }

    /// Zooms the plane `key` across by `factor`, about `x` points from its left, within its
    /// zoom: whether it zoomed.
    fn zoom(&self, key: &str, factor: f32, x: f32) -> bool {
        let mut cameras = self.cameras.borrow_mut();
        let Some(owner) = owner(&cameras, key, 0).map(str::to_string) else {
            return false;
        };
        let Some([low, high]) = cameras[&owner].zoom_range else {
            return false;
        };
        let before = per(&cameras, &owner, 0, 0);
        let pointed = cameras[&owner].at[0] + x / before;
        let camera = cameras.get_mut(&owner).expect("an owner is a camera");
        let zoom = (camera.zoom * factor).clamp(low, high);
        if zoom == camera.zoom || !zoom.is_finite() {
            return false;
        }
        camera.zoom = zoom;
        camera.zoomed = 0.0;
        let after = per(&cameras, &owner, 0, 0);
        let (low, high) = bounds(&cameras, &owner, 0);
        let camera = cameras.get_mut(&owner).expect("an owner is a camera");
        camera.at[0] = (pointed - x / after).clamp(low, high);
        drop(cameras);
        self.sync();
        self.by_hand(key);
        true
    }

    /// Why the window has no plane `key`, for an agent.
    pub(crate) fn find_plane(&self, key: &str) -> Result<(), String> {
        let cameras = self.cameras.borrow();
        match cameras.contains_key(key) {
            true => Ok(()),
            false => Err(format!(
                "no plane is called {key}; the window has: {}",
                cameras.keys().cloned().collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// Shows a range of the plane `key`, as code or an agent asks: zoomed across so `x` fills
    /// it, as far as it zooms, and scrolled to the middle of each range, or to its start when it
    /// is longer than what shows. A plane not laid out yet shows it once it is. Whether it moved.
    pub(crate) fn show(&self, key: &str, x: Option<[f32; 2]>, y: Option<[f32; 2]>) -> bool {
        let opened = (self.cameras.borrow().get(key)).is_some_and(|camera| camera.opened);
        if !opened {
            self.asked.borrow_mut().insert(key.to_string(), [x, y]);
            return false;
        }
        let ordered = |range: Option<[f32; 2]>| range.map(|[a, b]| [a.min(b), a.max(b)]);
        let (x, y) = (ordered(x), ordered(y));
        let Some(view) = self.view(key) else {
            return false;
        };
        let zoomed = x.is_some_and(|[from, to]| {
            to > from && self.zoom(key, (view.right - view.left) / (to - from), 0.0)
        });
        let Some(view) = self.view(key) else {
            return zoomed;
        };
        let shown = [[view.left, view.right], [view.top, view.bottom]];
        let by: [f32; 2] = std::array::from_fn(|i| match [x, y][i] {
            Some([from, to]) => {
                let long = shown[i][1] - shown[i][0];
                let start = match to - from <= long {
                    true => (from + to - long) / 2.0,
                    false => from,
                };
                start - shown[i][0]
            }
            None => 0.0,
        });
        self.by_hand(key);
        self.scroll_units(key, by) || zoomed
    }

    /// The plane `key` moved by hand, by code or by an agent: the planes that follow a place
    /// along its x stop until that place shows again, or rests and moves again.
    fn by_hand(&self, key: &str) {
        let mut cameras = self.cameras.borrow_mut();
        let Some(moved) = owner(&cameras, key, 0).map(str::to_string) else {
            return;
        };
        let along: Vec<String> = (cameras.keys())
            .filter(|each| owner(&cameras, each, 0) == Some(moved.as_str()))
            .cloned()
            .collect();
        for each in along {
            if let Some(follow) = cameras
                .get_mut(&each)
                .and_then(|camera| camera.follow.as_mut())
            {
                follow.paused = true;
            }
        }
    }

    /// One frame `dt` seconds on: a plane whose followed place moved out of what it shows pages
    /// on, so it starts there. Whether one moved.
    fn follow(&self, state: &State, dt: f64) -> bool {
        let followed: Vec<(String, InstanceId, Num)> = (self.cameras.borrow().iter())
            .filter_map(|(key, camera)| {
                let follow = camera.follow.as_ref()?;
                Some((key.clone(), follow.instance.clone(), follow.at.clone()))
            })
            .collect();
        let mut moved = false;
        for (key, instance, at) in followed {
            let value = state.lives.value(state, &instance, &at);
            let Some(view) = self.view(&key) else {
                continue;
            };
            let page = {
                let mut cameras = self.cameras.borrow_mut();
                let Some(follow) = cameras
                    .get_mut(&key)
                    .and_then(|camera| camera.follow.as_mut())
                else {
                    continue;
                };
                let moving = follow.last.is_some_and(|last| last != value);
                follow.last = Some(value);
                if !moving {
                    follow.rested += dt;
                    continue;
                }
                if follow.rested >= RESTED {
                    follow.paused = false;
                }
                follow.rested = 0.0;
                let shown = (view.left..=view.right).contains(&value);
                if shown {
                    follow.paused = false;
                }
                !shown && !follow.paused
            };
            if page {
                moved |= self.scroll_units(&key, [value - view.left, 0.0]);
            }
        }
        moved
    }

    /// Moves the live values of every plane to where its camera is: what is placed, and the
    /// thumbs of its scroll bars.
    fn sync(&self) {
        let cameras = self.cameras.borrow();
        for (key, camera) in cameras.iter() {
            let per = [per(&cameras, key, 0, 0), per(&cameras, key, 1, 0)];
            let at = [at(&cameras, key, 0, 0), at(&cameras, key, 1, 0)];
            camera.offset.set([-at[0] * per[0], -at[1] * per[1]]);
            let size = camera.size;
            for i in 0..2 {
                let shared = matches!(camera.axes[i], Axis::Shared(..));
                let (low, high) = bounds(&cameras, key, i);
                let view = (size[i] / per[i]).abs();
                let whole = high - low + view;
                let thumb = match shared || high - low <= f32::EPSILON || whole <= 0.0 {
                    true => [0.0; 4],
                    false => {
                        let (from, long) =
                            ((at[i] - low) / whole * size[i], view / whole * size[i]);
                        let edge = size[1 - i] - THUMB - THUMB_EDGE;
                        match i {
                            0 => [from, edge, long, THUMB],
                            _ => [edge, from, THUMB, long],
                        }
                    }
                };
                camera.thumbs[i].set(thumb);
            }
        }
    }

    /// Whether a plane shows more than its content was built for, or its scale changed and does
    /// not zoom now: the view builds again.
    fn stale(&self) -> bool {
        let cameras = self.cameras.borrow();
        (cameras.iter()).any(|(key, camera)| {
            let Some(built) = camera.built.get() else {
                return false;
            };
            let now = [per(&cameras, key, 0, 0), per(&cameras, key, 1, 0)];
            let size = laid(camera.size);
            let outside = (0..2).any(|i| {
                let begins = at(&cameras, key, i, 0);
                let [low, high] = ordered([begins, begins + size[i] / now[i]]);
                low < built.range[i][0] || high > built.range[i][1]
            });
            outside || (built.per != now && !zooming(&cameras, key))
        })
    }

    /// One frame `dt` seconds on: whether a zoom rested in it.
    fn rest(&self, dt: f64) -> bool {
        let mut cameras = self.cameras.borrow_mut();
        let mut rested = false;
        for camera in cameras.values_mut() {
            let was = camera.zoomed;
            camera.zoomed += dt;
            rested |= was < ZOOM_RESTS && camera.zoomed >= ZOOM_RESTS;
        }
        rested
    }

    /// Whether a plane zoomed and has not rested yet: the window draws on until it has.
    pub(crate) fn zooming(&self) -> bool {
        (self.cameras.borrow().values()).any(|camera| camera.zoomed < ZOOM_RESTS)
    }

    /// How many times the points per unit of the plane `key` across are of those its content
    /// was built at: 1 but while it zooms.
    pub(crate) fn stretched(&self, key: &str) -> f32 {
        let cameras = self.cameras.borrow();
        let built = cameras.get(key).and_then(|camera| camera.built.get());
        built.map_or(1.0, |built| per(&cameras, key, 0, 0) / built.per[0])
    }

    /// Every plane's key: the planes a camera moves.
    pub(crate) fn keys(&self) -> Vec<String> {
        self.cameras.borrow().keys().cloned().collect()
    }

    /// The element `focused` has the keys now: when it is in a plane, the plane scrolls to
    /// show it. Whether one scrolled.
    pub(crate) fn focus(&self, focused: Option<&str>) -> bool {
        if self.focused.borrow().as_deref() == focused {
            return false;
        }
        *self.focused.borrow_mut() = focused.map(str::to_string);
        let found = self.found.borrow();
        let Some(Found { plane, rect, .. }) = focused.and_then(|key| found.get(key)) else {
            return false;
        };
        let Some(view) = self.view(plane) else {
            return false;
        };
        let shown = [[view.left, view.right], [view.top, view.bottom]];
        let by: [f32; 2] = std::array::from_fn(|i| {
            let [from, to] = [rect[i], rect[i] + rect[i + 2]];
            match (from < shown[i][0], to > shown[i][1]) {
                (true, _) => from - shown[i][0],
                (false, true) => (to - shown[i][1]).min(from - shown[i][0]),
                _ => 0.0,
            }
        });
        let plane = plane.clone();
        drop(found);
        self.scroll_units(&plane, by)
    }

    /// The key of the keyed element of `plane` at `point` in its units, the smallest there,
    /// but `left_out` and what is in it.
    pub(crate) fn over(&self, plane: &str, point: [f32; 2], left_out: &str) -> Option<String> {
        let found = self.found.borrow();
        let inside = |rect: &[f32; 4]| {
            (0..2).all(|i| point[i] >= rect[i] && point[i] <= rect[i] + rect[i + 2])
        };
        (found.iter())
            .filter(|(key, found)| {
                found.plane == plane && *key != left_out && found.top != left_out
            })
            .filter(|(_, found)| inside(&found.rect))
            .min_by(|a, b| (a.1.rect[2] * a.1.rect[3]).total_cmp(&(b.1.rect[2] * b.1.rect[3])))
            .map(|(key, _)| key.clone())
    }
}

impl Camera {
    /// Once its size is known: a range shows where it opens.
    fn open(&mut self) {
        if self.opened || self.size[0] <= 0.0 {
            return;
        }
        self.opened = true;
        for i in 0..2 {
            if let Axis::Range(from, _) = self.axes[i] {
                self.at[i] = from;
            }
        }
    }
}

/// The plane `outer` the plane `b` is placed in, and where it starts there.
fn within<'a>(scope: &Scope, outer: Option<&'a str>, b: &BoxNode) -> Option<(&'a str, [f32; 2])> {
    let origin = b.place.as_ref()?.origin();
    let start = origin.each_ref().map(|num| scope.state.lives.value(scope.state, scope.instance, num));
    Some((outer?, start))
}

/// The size in points the view gives `b`, where it gives one.
fn given(b: &BoxNode) -> [Option<f32>; 2] {
    let (w, h) = b.size();
    [w, h].map(|size| size.and_then(Num::fixed))
}

/// The plane whose camera `key` has on axis `i`: itself, or the one it shares that axis with.
fn owner<'a>(cameras: &'a HashMap<String, Camera>, key: &'a str, i: usize) -> Option<&'a str> {
    let mut key = key;
    for _ in 0..8 {
        match &cameras.get(key)?.axes[i] {
            Axis::Shared(leader, _) => key = leader,
            _ => return Some(key),
        }
    }
    None
}

/// Points per unit of the plane `key` on axis `i`.
fn per(cameras: &HashMap<String, Camera>, key: &str, i: usize, depth: u8) -> f32 {
    let Some(camera) = cameras.get(key).filter(|_| depth < 8) else {
        return 1.0;
    };
    let zoom = if i == 0 { camera.zoom } else { 1.0 };
    match &camera.axes[i] {
        Axis::Points => 1.0,
        Axis::Scale(scale) => scale * zoom,
        Axis::Range(from, to) => laid(camera.size)[i] / (to - from) * zoom,
        Axis::Shared(leader, _) => per(cameras, leader, i, depth + 1),
    }
}

/// Where the plane `key` begins on axis `i`, in its units.
fn at(cameras: &HashMap<String, Camera>, key: &str, i: usize, depth: u8) -> f32 {
    match cameras.get(key).filter(|_| depth < 8) {
        Some(Camera {
            axes, at: begins, ..
        }) => match &axes[i] {
            Axis::Shared(_, Some(start)) => *start,
            Axis::Shared(leader, None) => at(cameras, leader, i, depth + 1),
            _ => begins[i],
        },
        None => 0.0,
    }
}

/// Where the plane `key` may begin on axis `i`: over what is placed in it and its range, and
/// past the end a view more of room when its units are points per unit.
fn bounds(cameras: &HashMap<String, Camera>, key: &str, i: usize) -> (f32, f32) {
    let camera = &cameras[key];
    let view = laid(camera.size)[i] / per(cameras, key, i, 0);
    // A range that runs backwards begins at its high end and shows down from there.
    let (backwards, view) = (view < 0.0, view.abs());
    let (mut low, mut high) = match camera.axes[i] {
        Axis::Range(from, to) => (from.min(to), from.max(to)),
        _ => (0.0, 0.0),
    };
    if let Some(extent) = camera.extent {
        (low, high) = (low.min(extent[i]), high.max(extent[i + 2]));
    }
    if matches!(camera.axes[i], Axis::Scale(_)) {
        high += view;
    }
    match backwards {
        true => (high.min(low + view), high),
        false => (low, low.max(high - view)),
    }
}

/// What the plane `key` builds now: the stretch it shows, at the points per unit it has, or
/// while it zooms at those it was built at, so what was built is kept.
fn stretch(cameras: &HashMap<String, Camera>, key: &str) -> Stretch {
    let camera = cameras.get(key);
    let size = camera.map_or([UNLAID; 2], |camera| laid(camera.size));
    let now = [per(cameras, key, 0, 0), per(cameras, key, 1, 0)];
    let built = camera.and_then(|camera| camera.built.get());
    Stretch {
        per: match (zooming(cameras, key), built) {
            (true, Some(built)) => built.per,
            _ => now,
        },
        range: std::array::from_fn(|i| {
            let long = size[i] / now[i];
            let from = (at(cameras, key, i, 0) / long).floor() * long;
            ordered([from, from + 2.0 * long])
        }),
    }
}

fn ordered([a, b]: [f32; 2]) -> [f32; 2] {
    [a.min(b), a.max(b)]
}

/// Whether the plane `key` zooms across now, and has not rested.
fn zooming(cameras: &HashMap<String, Camera>, key: &str) -> bool {
    owner(cameras, key, 0).is_some_and(|owner| cameras[owner].zoomed < ZOOM_RESTS)
}

fn laid(size: [f32; 2]) -> [f32; 2] {
    size.map(|size| if size > 0.0 { size } else { UNLAID })
}

impl Scope<'_> {
    /// Places `e` at `place` in the plane around it, from its parent: `size` are the points of
    /// a mark. It moves every frame its place or its plane's zoom moves.
    pub(super) fn put(&self, e: El, place: &Place, size: (Option<&Num>, Option<&Num>)) -> El {
        let Some(shows) = self.placed_shows(place, size) else {
            return e;
        };
        // Sized as it is now as well: Rune lays out and focuses only what has a size.
        let [.., w, h] = self.rect(place, size);
        let per = (self.plane.as_deref()).map_or([1.0; 2], |plane| self.state.planes.per(plane));
        let e = match place {
            Place::Box(_) => e.w(w * per[0]).h(h * per[1]),
            Place::Mark(_) => {
                let e = if size.0.is_some() { e.w(w * per[0]) } else { e };
                if size.1.is_some() { e.h(h * per[1]) } else { e }
            }
        };
        self.bind(e, shows)
    }

    /// What a place in the plane around it binds.
    pub(super) fn placed_shows(
        &self,
        place: &Place,
        size: (Option<&Num>, Option<&Num>),
    ) -> Option<Shows> {
        let plane = self.plane.as_ref()?;
        let size = match place {
            Place::Mark(_) => [size.0.cloned(), size.1.cloned()],
            Place::Box(_) => [None, None],
        };
        Some(Shows::Placed {
            plane: plane.clone(),
            place: place.clone(),
            size,
            parent: self.parent.clone(),
        })
    }

    /// Where `place` is in the plane's units now: [x, y, w, h], a mark `size` points.
    fn rect(&self, place: &Place, size: (Option<&Num>, Option<&Num>)) -> [f32; 4] {
        let value = |num: &Num| self.state.lives.value(self.state, self.instance, num);
        match place {
            Place::Box(rect) => rect.each_ref().map(value),
            Place::Mark([x, y]) => {
                let per =
                    (self.plane.as_deref()).map_or([1.0; 2], |plane| self.state.planes.per(plane));
                let [w, h] = [size.0, size.1].map(|size| size.map_or(0.0, value));
                let [w, h] = [w / per[0].abs(), h / per[1].abs()];
                [value(x) - w / 2.0, value(y) - h / 2.0, w, h]
            }
        }
    }

    /// The units a placed `shapes` draws in: the size of its place.
    pub(super) fn place_units(&self, place: Option<&Place>) -> Option<[f32; 2]> {
        match place? {
            Place::Box([_, _, w, h]) => {
                let value = |num: &Num| self.state.lives.value(self.state, self.instance, num);
                Some([value(w), value(h)])
            }
            Place::Mark(_) => None,
        }
    }
}

/// The box `b`, a plane keyed `key` in the window, as `e`: what is placed in it moves with its
/// camera, and only what shows, and a stretch about it, is built.
/// `outer` is the plane it is in.
pub(super) fn plane(
    scope: &Scope,
    e: El,
    b: &BoxNode,
    (key, spec): (&str, &PlaneSpec),
    outer: Option<&str>,
) -> El {
    let planes = &scope.state.planes;
    #[cfg(test)]
    planes.builds.set(planes.builds.get() + 1);
    let within = within(scope, outer, b);
    planes.ensure(key, (spec, scope.instance), scope.prefix, given(b), within);
    planes
        .found
        .borrow_mut()
        .retain(|_, found| found.plane != key);
    let inside = Scope {
        plane: Some(key.to_string()),
        parent: None,
        placed: None,
        ..scope.clone()
    };
    let built = {
        let cameras = planes.cameras.borrow();
        let built = stretch(&cameras, key);
        if let Some(camera) = cameras.get(key) {
            camera.built.set(Some(built));
        }
        built
    };
    let mut gathered = Gathered {
        shows: Some(built.range),
        per: built.per,
        ..Gathered::default()
    };
    for child in b.children.iter().filter(|child| !is_overlay(child)) {
        gather(&inside, child, &mut gathered);
    }
    let (drawn, offset, thumbs) = {
        let mut cameras = planes.cameras.borrow_mut();
        let camera = cameras.get_mut(key).expect("ensured");
        camera.extent = gathered.extent;
        (
            camera.drawn.clone(),
            camera.offset.clone(),
            camera.thumbs.clone(),
        )
    };
    planes.sync();
    let theme = &scope.state.theme;
    let bars = thumbs.into_iter().enumerate().map(|(i, thumb)| {
        let key = key.to_string();
        el().absolute(0.0, 0.0)
            .live_rect(&thumb)
            .radius(THUMB / 2.0)
            .fill(theme.color("text-muted"))
            .on_drag(
                move |s: &mut State, d: &mut rune::Drag, tx: &mut rune::Tx| {
                    let moved = [d.dx, d.dy][i] - d.data;
                    d.data = [d.dx, d.dy][i];
                    let whole = s.planes.whole(&key, i);
                    let mut by = [0.0; 2];
                    by[i] = moved * whole;
                    let moved = s.planes.scroll_units(&key, by);
                    if moved {
                        s.planes.by_hand(&key);
                    }
                    s.camera_moved(tx, moved);
                },
            )
    });
    let (laid_key, wheel_key, pinch_key, keys_key) = (
        key.to_string(),
        key.to_string(),
        key.to_string(),
        key.to_string(),
    );
    e.clip_soft(0.0)
        .drawn_rect_into(&drawn)
        .on_layout_tx(move |s: &mut State, rect: [f32; 4], tx: &mut rune::Tx| {
            s.planes.laid_out(&laid_key, [rect[2], rect[3]]);
            s.camera_moved(tx, true);
        })
        .on_wheel(
            move |s: &mut State, wheel: rune::WheelEvent, tx: &mut rune::Tx| {
                let moved = match wheel.mods.cmd {
                    true => s.planes.zoom(&wheel_key, (wheel.dy / 200.0).exp(), wheel.x),
                    false => {
                        let by = match wheel.mods.shift && wheel.dx == 0.0 {
                            true => [wheel.dy, 0.0],
                            false => [wheel.dx, wheel.dy],
                        };
                        s.planes.scroll(&wheel_key, by.map(|by| -by))
                    }
                };
                s.camera_moved(tx, moved)
            },
        )
        .on_magnify(
            move |s: &mut State, pinch: rune::MagnifyEvent, tx: &mut rune::Tx| {
                let moved = s.planes.zoom(&pinch_key, 1.0 + pinch.delta, pinch.x);
                s.camera_moved(tx, moved)
            },
        )
        .on_key(move |s: &mut State, key: &KeyEvent, tx: &mut rune::Tx| {
            let moved = s.planes.keyed(&keys_key, key);
            s.camera_moved(tx, moved)
        })
        .children(gathered.flow)
        .child(
            el().absolute(0.0, 0.0)
                .live_offset(&offset)
                .children(gathered.placed),
        )
        .children(bars)
}

impl Planes {
    /// How many units of the plane `key` a point of its scroll bar on axis `i` is.
    fn whole(&self, key: &str, i: usize) -> f32 {
        let cameras = self.cameras.borrow();
        let Some(camera) = cameras.get(key) else {
            return 0.0;
        };
        let (low, high) = bounds(&cameras, key, i);
        let size = laid(camera.size)[i];
        (high - low + (size / per(&cameras, key, i, 0)).abs()) / size
    }

    /// A key while the plane `key` has the keys: Page and Home keys scroll it, Cmd-= and Cmd--
    /// zoom it. Whether it moved.
    fn keyed(&self, key: &str, pressed: &KeyEvent) -> bool {
        let Some(view) = self.view(key) else {
            return false;
        };
        let [across, down] = [view.right - view.left, view.bottom - view.top];
        let down_scrolls = {
            let cameras = self.cameras.borrow();
            owner(&cameras, key, 1).is_some_and(|owner| {
                let (low, high) = bounds(&cameras, owner, 1);
                high > low
            })
        };
        let page = |by: f32| match down_scrolls {
            true => [0.0, by * down],
            false => [by * across, 0.0],
        };
        let text = |want: &[&str]| matches!(&pressed.key, Key::Text(text) if want.contains(&text.as_str()));
        let scrolled = |by: [f32; 2]| {
            let moved = self.scroll_units(key, by);
            if moved {
                self.by_hand(key);
            }
            moved
        };
        match &pressed.key {
            Key::PageDown => scrolled(page(1.0)),
            Key::PageUp => scrolled(page(-1.0)),
            Key::Home => scrolled([-f32::MAX / 4.0, 0.0]),
            Key::End => scrolled([f32::MAX / 4.0, 0.0]),
            _ if pressed.mods.cmd && text(&["=", "+"]) => {
                self.zoom(key, ZOOM_STEP, self.middle(key))
            }
            _ if pressed.mods.cmd && text(&["-"]) => {
                self.zoom(key, 1.0 / ZOOM_STEP, self.middle(key))
            }
            _ => false,
        }
    }

    /// The middle of the plane `key` across, in its points.
    fn middle(&self, key: &str) -> f32 {
        (self.cameras.borrow().get(key)).map_or(0.0, |camera| laid(camera.size)[0] / 2.0)
    }

    /// Takes the planes the views shown now draw, before they are built: a plane that moves
    /// with another finds its camera there.
    pub(crate) fn discover(&self, state: &State) {
        for id in &state.shown {
            if let Some(root) = state.root(id) {
                self.find(state, id, &state.prefix_of(id), &root, None);
            }
        }
    }

    fn find(&self, state: &State, id: &InstanceId, prefix: &str, node: &Node, outer: Option<&str>) {
        match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
                let key = (b.plane.as_ref()).zip(b.key.as_ref().map(|key| format!("{prefix}{key}")));
                if let Some((spec, key)) = &key {
                    let within = within(&Scope::new(state, id, prefix), outer, b);
                    self.ensure(key, (spec, id), prefix, given(b), within);
                    let scope = Scope {
                        plane: Some(key.clone()),
                        ..Scope::new(state, id, prefix)
                    };
                    let mut extent = None;
                    b.children
                        .iter()
                        .for_each(|child| span(&scope, child, &mut extent));
                    if let Some(camera) = self.cameras.borrow_mut().get_mut(key) {
                        camera.extent = extent;
                    }
                }
                let inner = key.as_ref().map_or(outer, |(_, key)| Some(key.as_str()));
                for child in &b.children {
                    self.find(state, id, prefix, child, inner);
                }
            }
            Node::Memo { key } => {
                if let Some((_, inner)) = state.memo_of(id, key) {
                    self.find(state, id, prefix, &inner, outer);
                }
            }
            _ => {}
        }
    }

    /// What a view places outside a plane, as problems of the project.
    pub(crate) fn misplaced(&self, state: &State) -> Option<Vec<Problem>> {
        let mut problems = Vec::new();
        for id in &state.shown {
            let Some(root) = state.root(id) else {
                continue;
            };
            let mut keys = Vec::new();
            outside(state, id, &root, false, &mut keys);
            if !keys.is_empty() {
                problems.push(Problem {
                    path: format!("{MISPLACED}{id}"),
                    message: format!(
                        "{} {} placed outside a plane: put {} in a box with `plane`",
                        keys.join(", "),
                        if keys.len() == 1 { "is" } else { "are" },
                        if keys.len() == 1 { "it" } else { "them" },
                    ),
                });
            }
        }
        let mut said = self.misplaced.borrow_mut();
        (*said != problems).then(|| {
            said.clone_from(&problems);
            problems
        })
    }
}

/// Where the problems of places outside a plane are, in the project's problems.
pub(crate) const MISPLACED: &str = "views/";

/// The keys of what `node` places outside a plane, `in_plane` or not.
fn outside(state: &State, id: &InstanceId, node: &Node, in_plane: bool, keys: &mut Vec<String>) {
    let (place, key, children) = match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            (&b.place, b.key.as_deref(), &b.children[..])
        }
        Node::Shapes(d) => (&d.place, d.key.as_deref(), &[][..]),
        Node::Memo { key } => {
            if let Some((_, inner)) = state.memo_of(id, key) {
                outside(state, id, &inner, in_plane, keys);
            }
            return;
        }
        _ => return,
    };
    if place.is_some() && !in_plane {
        keys.push(key.unwrap_or("a shape without a key").to_string());
    }
    let in_plane = in_plane
        || matches!(node, Node::Row(b) | Node::Col(b) | Node::Plain(b) if b.plane.is_some());
    for child in children {
        outside(state, id, child, in_plane, keys);
    }
}

/// What a plane, or a placed element in one, holds: what is placed (in the stretch `shows` of
/// a plane's content), what flows, and what the placed span.
#[derive(Default)]
pub(super) struct Gathered {
    shows: Option<[[f32; 2]; 2]>,
    /// The points per unit what is placed is built at.
    per: [f32; 2],
    pub(super) flow: Vec<El>,
    pub(super) placed: Vec<El>,
    extent: Option<[f32; 4]>,
}

/// The children of a box in a plane: what is placed is put in place, also through boxes that
/// only hold placed ones; the rest flows.
pub(super) fn children(scope: &Scope, nodes: &[Node]) -> Vec<El> {
    let mut gathered = Gathered::default();
    for node in nodes.iter().filter(|node| !is_overlay(node)) {
        gather(scope, node, &mut gathered);
    }
    gathered.flow.into_iter().chain(gathered.placed).collect()
}

fn gather(scope: &Scope, node: &Node, into: &mut Gathered) {
    let (place, size, key) = match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            (b.place.as_ref(), b.size(), b.key.as_deref())
        }
        Node::Shapes(d) => (d.place.as_ref(), (None, None), d.key.as_deref()),
        Node::Memo { key } => {
            if let Some(inner) = scope.memo(key) {
                gather(scope, &inner, into);
            }
            return;
        }
        _ => (None, (None, None), None),
    };
    let Some(place) = place else {
        match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b)
                if b.overlay.is_none() && holds_place(scope, node) =>
            {
                for child in b.children.iter().filter(|child| !is_overlay(child)) {
                    gather(scope, child, into);
                }
            }
            _ => into.flow.push(build(scope, node)),
        }
        return;
    };
    let Some(shows) = into.shows else {
        into.placed.push(build(scope, node));
        return;
    };
    let rect = scope.rect(place, size);
    spanned(&mut into.extent, place, rect);
    let plane = scope.plane.clone().unwrap_or_default();
    let key = key.map(|key| scope.key(key));
    let planes = &scope.state.planes;
    let focused = planes.focused.borrow().clone();
    let holds_focus = |key: &str| {
        focused.as_deref() == Some(key)
            || (focused.as_ref()).is_some_and(|focused| {
                planes
                    .found
                    .borrow()
                    .get(focused)
                    .is_some_and(|found| found.top == key)
            })
    };
    let shown = (0..2).all(|i| rect[i] + rect[i + 2] >= shows[i][0] && rect[i] <= shows[i][1]);
    if !shown && !key.as_deref().is_some_and(holds_focus) {
        return;
    }
    let Some(key) = key else {
        into.placed.push(build(scope, node));
        return;
    };
    record(
        scope,
        node,
        &plane,
        &key,
        rect,
        &mut planes.found.borrow_mut(),
    );
    // Built again when its tree or the scale it is built at changes; one with a field or a
    // plane each time, as what the field says and whether it has the keys are the window's, and
    // what a plane shows.
    if (scope.state.shows_a_field() && holds_field(scope, node)) || holds_plane(scope, node) {
        into.placed.push(build(scope, node));
        return;
    }
    let version = (version_of(scope, node), into.per.map(f32::to_bits));
    into.placed.push(memo(key, version, || build(scope, node)));
}

fn holds_field(scope: &Scope, node: &Node) -> bool {
    match node {
        Node::Field(_) => true,
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            b.children.iter().any(|child| holds_field(scope, child))
        }
        Node::Memo { key } => scope
            .memo(key)
            .is_some_and(|inner| holds_field(scope, &inner)),
        _ => false,
    }
}

/// What tells a placed element's tree from the one before: the memo it came in, or the tree.
fn version_of(scope: &Scope, node: &Node) -> usize {
    std::ptr::from_ref(node) as usize
        ^ scope
            .state
            .tools
            .as_ref()
            .map_or(0, |tools| tools.view_runs() as usize)
}

/// What a plane spans with `place` at `rect` too. A mark spans its point: a playhead at the
/// start lets the plane begin there.
fn spanned(extent: &mut Option<[f32; 4]>, place: &Place, rect: [f32; 4]) {
    let [x, y, w, h] = match place {
        Place::Box(_) => rect,
        Place::Mark(_) => [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0, 0.0, 0.0],
    };
    *extent = Some(match *extent {
        Some([l, t, r, b]) => [l.min(x), t.min(y), r.max(x + w), b.max(y + h)],
        None => [x, y, x + w, y + h],
    });
}

/// What the placed elements of `node` span in the plane of `scope`.
fn span(scope: &Scope, node: &Node, extent: &mut Option<[f32; 4]>) {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) if b.overlay.is_none() => match &b.place {
            Some(place) => spanned(extent, place, scope.rect(place, b.size())),
            None => b
                .children
                .iter()
                .for_each(|child| span(scope, child, extent)),
        },
        Node::Shapes(d) => {
            if let Some(place) = &d.place {
                spanned(extent, place, scope.rect(place, (None, None)));
            }
        }
        Node::Memo { key } => {
            if let Some(inner) = scope.memo(key) {
                span(scope, &inner, extent);
            }
        }
        _ => {}
    }
}

/// Notes where the keyed elements of `node`, placed at `rect` in `plane`, are.
fn record(
    scope: &Scope,
    node: &Node,
    plane: &str,
    top: &str,
    rect: [f32; 4],
    found: &mut HashMap<String, Found>,
) {
    let (key, children, rect) = match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            let rect = match &b.place {
                Some(place) => scope.rect(place, b.size()),
                None => rect,
            };
            (b.key.as_deref(), &b.children[..], rect)
        }
        Node::Shapes(d) => (d.key.as_deref(), &[][..], rect),
        Node::Memo { key } => {
            if let Some(inner) = scope.memo(key) {
                record(scope, &inner, plane, top, rect, found);
            }
            return;
        }
        _ => return,
    };
    if let Some(key) = key {
        let found_at = Found {
            plane: plane.to_string(),
            top: top.to_string(),
            rect,
        };
        found.insert(scope.key(key), found_at);
    }
    for child in children.iter().filter(|child| !is_overlay(child)) {
        record(scope, child, plane, top, rect, found);
    }
}

/// Whether `node` is placed or holds what is.
fn holds_place(scope: &Scope, node: &Node) -> bool {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            b.place.is_some()
                || (b.children.iter()).any(|child| !is_overlay(child) && holds_place(scope, child))
        }
        Node::Shapes(d) => d.place.is_some(),
        Node::Memo { key } => scope
            .memo(key)
            .is_some_and(|inner| holds_place(scope, &inner)),
        _ => false,
    }
}

impl State {
    /// One frame `dt` seconds on: the planes that follow a place page on. Whether the views
    /// build again, as a plane shows a stretch not built.
    pub(crate) fn follow_planes(&self, dt: f64) -> bool {
        let rested = self.planes.rest(dt);
        let followed = self.planes.follow(self, dt);
        if followed {
            self.lives.move_planes(self);
        }
        (followed || rested) && self.planes.stale()
    }

    /// A camera moved, or did not: what is placed and what binds to the planes shows it at
    /// once, and the view builds again in Rust only when a plane shows a stretch not built.
    /// Whether it moved, which takes the wheel or the pinch that moved it.
    pub(crate) fn camera_moved(&mut self, tx: &mut rune::Tx, moved: bool) -> bool {
        if !moved {
            tx.live_only();
            return false;
        }
        self.lives.move_planes(self);
        match self.planes.stale() {
            // What is placed lands where the camera says, not by a glide.
            true => tx.instant(),
            false => tx.live_only(),
        }
        true
    }
}
