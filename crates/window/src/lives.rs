//! What moves every frame without a view running again: the live values of the views and a
//! drag, each bound to a Rune live value. A view's live value is a
//! small expression (`sound_typescript::live`) worked out here, from the playhead, the time, the
//! feeds and the fields as they play, a drag and a plane. Kept while a tree that is shown binds
//! it, and read only in a frame where what it reads moved: a binding to a drag or a plane rests
//! with them.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rune::LiveText;
use sound_core::{InstanceId, Ticks};
use sound_typescript::live::{
    DragPart, Ease, Eased, Leaves, Live, Num, PlanePart, Readout, number, text_of,
};

use crate::app::State;
use crate::tree::{Listed, Place};

/// What a live value of a view shows: also its name, so the same shown twice binds once, and a
/// new expression in its place, such as `live.playhead.minus(start)` after a drag, is another.
#[derive(Debug, Clone)]
pub(crate) enum Shows {
    Text {
        value: Num,
        unit: Option<Readout>,
        digits: Option<usize>,
    },
    /// Where an element is, and how big: [x, y, w, h].
    Rect([Num; 4]),
    Offset([Num; 2]),
    Opacity(Num),
    /// The part of a shape's lines that draws: [from, to], 0 to 1.
    Trim([Num; 2]),
    /// Where an element `place` puts in the plane `plane` (its key in the window) is, in points
    /// from its parent, which is placed at `parent` or is the plane's content. A mark is
    /// `size` points, centred on its place.
    Placed {
        plane: String,
        place: Place,
        size: [Option<Num>; 2],
        parent: Option<[Num; 2]>,
    },
    Lists(Listed),
}

/// What Rune draws of a [`Shows`] every frame.
#[derive(Clone)]
pub(crate) enum Shown {
    /// What it shows, and the widest it showed, which sizes it: Rune never resizes live text.
    Text(LiveText, Rc<RefCell<String>>),
    Rect(rune::Live<[f32; 4]>),
    Offset(rune::Live<[f32; 2]>),
    Opacity(rune::Live<f32>),
    Trim(rune::Live<[f32; 2]>),
    /// The lists it drew last, and in a plane where the part that shows is.
    Pixels(
        rune::Pixels,
        Rc<RefCell<Vec<Vec<f32>>>>,
        Option<rune::Live<[f32; 4]>>,
    ),
}

impl Shown {
    /// Binds `e` to it: a live place takes the element out of the flow.
    pub(crate) fn apply(&self, e: rune::El) -> rune::El {
        match self {
            Self::Text(text, _) => e.live_text(text),
            Self::Rect(rect) => e.absolute(0.0, 0.0).live_rect(rect),
            Self::Offset(offset) => e.absolute(0.0, 0.0).live_offset(offset),
            Self::Opacity(opacity) => e.live_opacity(opacity),
            Self::Trim(trim) => e.live_trim(trim),
            Self::Pixels(pixels, _, None) => e.pixels(pixels),
            Self::Pixels(pixels, _, Some(rect)) => e.live_rect(rect).pixels(pixels),
        }
    }
}

/// A live value bound to an element, the instance whose view it is of, and what it reads.
struct Binding {
    instance: InstanceId,
    shows: Shows,
    shown: Shown,
    reads: Reads,
}

/// What a binding reads: something that may move in any frame, or only drags, planes, and the
/// points per unit of planes (a place), by their keys in the window.
#[derive(Default)]
struct Reads {
    always: bool,
    drags: Vec<String>,
    planes: Vec<String>,
    scales: Vec<String>,
}

/// The drags and planes that moved since their bindings were last read.
#[derive(Default)]
struct Moved {
    drags: HashSet<String>,
    planes: HashSet<String>,
    scales: HashSet<String>,
}

/// A drag of `live.drag(id)`, in the units of the plane around what it moves.
#[derive(Clone, Copy, Default)]
pub(crate) struct Hand {
    /// How far it moved: on the grid and within its bounds, as it ends.
    pub(crate) moved: [f32; 2],
    /// Where it began.
    pub(crate) start: [f32; 2],
    pub(crate) active: bool,
    pub(crate) alt: bool,
}

pub(crate) struct Lives {
    bindings: RefCell<HashMap<String, Binding>>,
    eased: RefCell<HashMap<String, Eased>>,
    frame: Cell<u64>,
    dt: Cell<f64>,
    elapsed: Cell<f64>,
    playhead: Cell<f64>,
    /// Whether an eased value still moves without new sound.
    moving: Cell<bool>,
    /// Whether a live text showed wider than it was built for, or a live accessibility value
    /// moved, since the last look.
    wider: Cell<bool>,
    /// The drags by id in the window, kept until a new tree says where the records put it.
    hands: RefCell<HashMap<String, Hand>>,
    moved: RefCell<Moved>,
    /// The live accessibility values of the views, by element key.
    spoken: RefCell<HashMap<String, Spoken>>,
    /// When the values of meters were last looked at, in seconds of `elapsed`.
    metered: Cell<f64>,
}

/// A live accessibility value of a view: its instance, its value, and what it said when the view
/// was built. A meter's moves every frame its sound plays, and building the window again that
/// often for it cost more than the frame: it is looked at [`METERED`] times a second.
struct Spoken {
    instance: InstanceId,
    value: Num,
    said: String,
    meter: bool,
}

const METERED: f64 = 4.0;

impl Lives {
    pub(crate) fn new() -> Self {
        Self {
            bindings: RefCell::default(),
            eased: RefCell::default(),
            frame: Cell::new(0),
            dt: Cell::new(0.0),
            elapsed: Cell::new(0.0),
            playhead: Cell::new(0.0),
            moving: Cell::new(false),
            wider: Cell::new(false),
            hands: RefCell::default(),
            moved: RefCell::default(),
            spoken: RefCell::default(),
            metered: Cell::new(0.0),
        }
    }

    /// The name of the binding of `shows` in the view of `instance`.
    pub(crate) fn name(instance: &InstanceId, shows: &Shows) -> String {
        format!("{instance} {shows:?}")
    }

    /// What Rune draws of `shows` in the view of `instance`, showing what it is now.
    pub(crate) fn bind(&self, state: &State, instance: &InstanceId, shows: Shows) -> Shown {
        let name = Self::name(instance, &shows);
        let mut bindings = self.bindings.borrow_mut();
        if let Some(binding) = bindings.get(&name) {
            return binding.shown.clone();
        }
        let shown = match &shows {
            Shows::Text { .. } => Shown::Text(LiveText::new(""), Rc::default()),
            Shows::Rect(_) => Shown::Rect(rune::Live::new([0.0; 4])),
            Shows::Offset(_) => Shown::Offset(rune::Live::new([0.0; 2])),
            Shows::Opacity(_) => Shown::Opacity(rune::Live::new(1.0)),
            Shows::Trim(_) => Shown::Trim(rune::Live::new([0.0, 1.0])),
            Shows::Placed { .. } => Shown::Rect(rune::Live::new([0.0; 4])),
            Shows::Lists(listed) => Shown::Pixels(
                rune::Pixels::new(),
                Rc::default(),
                (listed.plane()).map(|_| rune::Live::new([0.0; 4])),
            ),
        };
        let binding = Binding {
            reads: self.reads(state, instance, &shows),
            instance: instance.clone(),
            shows,
            shown: shown.clone(),
        };
        self.show(state, &binding);
        bindings.insert(name, binding);
        shown
    }

    /// What `shows` in the view of `instance` reads.
    fn reads(&self, state: &State, instance: &InstanceId, shows: &Shows) -> Reads {
        let recorder = Recorder {
            prefix: state.prefix_of(instance),
            reads: RefCell::default(),
        };
        let nums: Vec<&Num> = match shows {
            Shows::Text { value, .. } | Shows::Opacity(value) => vec![value],
            Shows::Rect(rect) => rect.iter().collect(),
            Shows::Offset(at) | Shows::Trim(at) => at.iter().collect(),
            Shows::Placed {
                plane,
                place,
                size,
                parent,
            } => {
                recorder.reads.borrow_mut().scales.push(plane.clone());
                (place.numbers().iter())
                    .chain(size.iter().flatten())
                    .chain(parent.iter().flatten())
                    .collect()
            }
            Shows::Lists(listed) => {
                let mut reads = recorder.reads.borrow_mut();
                reads.always = true;
                // What shows of it moves with its plane at once.
                reads.planes.extend(listed.plane().map(str::to_string));
                Vec::new()
            }
        };
        for num in nums {
            number(num, &recorder);
        }
        recorder.reads.into_inner()
    }

    /// The drag `id`, its key in the window, as it is now.
    pub(crate) fn hand(&self, id: &str) -> Hand {
        self.hands.borrow().get(id).copied().unwrap_or_default()
    }

    /// The drag `id` moved: what binds to it shows it at once, not a frame later.
    pub(crate) fn move_hand(&self, state: &State, id: &str, hand: Hand) {
        self.hands.borrow_mut().insert(id.to_string(), hand);
        self.moved.borrow_mut().drags.insert(id.to_string());
        self.refresh(state, false);
    }

    /// The planes `keys` moved, and with `scaled` their points per unit changed too: what binds
    /// to them shows it at once.
    pub(crate) fn move_planes(&self, state: &State, keys: &[String], scaled: bool) {
        let mut moved = self.moved.borrow_mut();
        moved.planes.extend(keys.iter().cloned());
        if scaled {
            moved.scales.extend(keys.iter().cloned());
        }
        drop(moved);
        self.refresh(state, false);
    }

    /// The number `value` of the view of `instance` is now.
    pub(crate) fn value(&self, state: &State, instance: &InstanceId, value: &Num) -> f32 {
        let sources = Sources {
            lives: self,
            state,
            instance,
        };
        number(value, &sources) as f32
    }

    /// The accessibility value of the element `key` of the view of `instance`, `value` as it is
    /// now; while it is live it is followed each frame, and the view is built again when what it
    /// says changes, as Rune's accessibility values do not move by themselves.
    pub(crate) fn said_live(
        &self,
        state: &State,
        instance: &InstanceId,
        key: &str,
        value: &serde_json::Value,
        meter: bool,
    ) -> String {
        let said = self.said(state, instance, value);
        let text = said
            .as_str()
            .map_or_else(|| said.to_string(), str::to_string);
        if let Ok(value @ Num::Live(_)) = serde_json::from_value::<Num>(value.clone()) {
            let spoken = Spoken {
                instance: instance.clone(),
                value,
                said: text.clone(),
                meter,
            };
            self.spoken.borrow_mut().insert(key.to_string(), spoken);
        }
        text
    }

    /// What `value` of an element of the view of `instance` says now: a live value as it is.
    pub(crate) fn said(
        &self,
        state: &State,
        instance: &InstanceId,
        value: &serde_json::Value,
    ) -> serde_json::Value {
        match serde_json::from_value::<Num>(value.clone()) {
            Ok(num @ Num::Live(_)) => {
                let sources = Sources {
                    lives: self,
                    state,
                    instance,
                };
                serde_json::json!(number(&num, &sources))
            }
            _ => value.clone(),
        }
    }

    /// Shows the bindings that read what moved since they were last read, and in a `frame` those
    /// that may move in any: a list feed draws once a frame.
    fn refresh(&self, state: &State, frame: bool) {
        let moved = self.moved.take();
        for binding in self.bindings.borrow().values() {
            let reads = &binding.reads;
            let moved = (frame && reads.always)
                || reads.drags.iter().any(|id| moved.drags.contains(id))
                || reads.planes.iter().any(|key| moved.planes.contains(key))
                || reads.scales.iter().any(|key| moved.scales.contains(key));
            if moved {
                self.show(state, binding);
            }
        }
    }

    /// A new tree came: what a drag moved is where its record says now, but the drag `held`,
    /// which goes on.
    pub(crate) fn drop_drags(&self, state: &State, held: Option<&str>) {
        let mut hands = self.hands.borrow_mut();
        let ended: Vec<String> = (hands.keys())
            .filter(|id| Some(id.as_str()) != held)
            .cloned()
            .collect();
        for id in &ended {
            hands.remove(id);
        }
        drop(hands);
        self.moved.borrow_mut().drags.extend(ended);
        self.refresh(state, false);
    }

    /// The views show only the bindings `named` now, see [`Lives::name`]: the others go, also
    /// those of a part of a view that Rune kept.
    pub(crate) fn keep(&self, named: &HashSet<String>) {
        (self.bindings.borrow_mut()).retain(|name, _| named.contains(name));
    }

    /// One frame `dt` seconds after the last. Whether a value still moves with no new sound,
    /// such as a meter that falls.
    pub(crate) fn update(&self, state: &State, dt: f64) -> bool {
        let playhead = state.playhead;
        self.frame.set(self.frame.get() + 1);
        self.dt.set(dt);
        self.elapsed.set(self.elapsed.get() + dt);
        self.playhead.set(playhead.0 as f64);
        self.moving.set(false);
        self.refresh(state, true);
        let mut spoken = self.spoken.borrow_mut();
        spoken.retain(|_, spoken| state.shown.contains(&spoken.instance));
        let metered = self.elapsed.get() - self.metered.get() >= 1.0 / METERED;
        if metered {
            self.metered.set(self.elapsed.get());
        }
        for spoken in spoken.values().filter(|spoken| metered || !spoken.meter) {
            let sources = Sources {
                lives: self,
                state,
                instance: &spoken.instance,
            };
            let now = serde_json::json!(number(&spoken.value, &sources)).to_string();
            if now != spoken.said {
                self.wider.set(true);
            }
        }
        drop(spoken);
        // What no binding read this frame eases nothing any more.
        let frame = self.frame.get();
        self.eased
            .borrow_mut()
            .retain(|_, eased| eased.frame == frame);
        self.moving.get() || state.feeds.moved()
    }

    fn show(&self, state: &State, binding: &Binding) {
        let project = state.project();
        let sources = Sources {
            lives: self,
            state,
            instance: &binding.instance,
        };
        let of = |value: &Num| number(value, &sources) as f32;
        match (&binding.shows, &binding.shown) {
            (
                Shows::Text {
                    value,
                    unit,
                    digits,
                },
                Shown::Text(shown, widest),
            ) => {
                let value = number(value, &sources);
                let text = match digits {
                    Some(digits) => format!("{value:.digits$}"),
                    None => text_of(value, *unit, Some(project.clock())),
                };
                let mut widest = widest.borrow_mut();
                if text.chars().count() > widest.chars().count() {
                    // The first text sizes it as it is built; a wider one builds it again.
                    self.wider.set(self.wider.get() || !widest.is_empty());
                    widest.clone_from(&text);
                }
                shown.set(text);
            }
            (Shows::Rect(rect), Shown::Rect(shown)) => {
                shown.set(rect.each_ref().map(of));
            }
            (Shows::Offset(at), Shown::Offset(shown)) => {
                shown.set(at.each_ref().map(of));
            }
            (Shows::Opacity(value), Shown::Opacity(shown)) => {
                shown.set(of(value).clamp(0.0, 1.0));
            }
            (Shows::Trim(trim), Shown::Trim(shown)) => {
                shown.set(trim.each_ref().map(|end| of(end).clamp(0.0, 1.0)));
            }
            (
                Shows::Placed {
                    plane,
                    place,
                    size,
                    parent,
                },
                Shown::Rect(shown),
            ) => {
                let size = size.each_ref().map(|size| size.as_ref().map(of));
                let parent = parent.as_ref().map(|at| at.each_ref().map(of));
                let place = place.numbers().iter().map(of).collect::<Vec<_>>();
                shown.set(state.planes.points(plane, &place, size, parent));
            }
            (Shows::Lists(listed), Shown::Pixels(pixels, last, rect)) => {
                let drawn = (pixels, rect.as_ref());
                (state.feeds).draw(state, &binding.instance, listed, drawn, last);
            }
            _ => {}
        }
    }

    /// Whether a live text showed wider than it was built for, or a live accessibility value
    /// moved, since the last call: the view is built again.
    pub(crate) fn take_wider(&self) -> bool {
        self.wider.replace(false)
    }
}

/// What the live values of the view of `instance` read.
struct Sources<'a> {
    lives: &'a Lives,
    state: &'a State,
    instance: &'a InstanceId,
}

#[cfg(test)]
impl Lives {
    pub(crate) fn bindings(&self) -> usize {
        self.bindings.borrow().len()
    }

    pub(crate) fn eased(&self) -> usize {
        self.eased.borrow().len()
    }

    /// The size of each drawing of list feeds now, in pixels.
    pub(crate) fn pixels(&self) -> Vec<(u32, u32)> {
        let bindings = self.bindings.borrow();
        (bindings.values())
            .filter_map(|binding| match &binding.shown {
                Shown::Pixels(pixels, ..) => Some(pixels.size()),
                _ => None,
            })
            .collect()
    }

    /// The lists each drawing of list feeds drew last, and where it is in a plane.
    pub(crate) fn lists(&self) -> Vec<(Vec<Vec<f32>>, Option<[f32; 4]>)> {
        let bindings = self.bindings.borrow();
        (bindings.values())
            .filter_map(|binding| match &binding.shown {
                Shown::Pixels(_, last, rect) => {
                    Some((last.borrow().clone(), rect.as_ref().map(rune::Live::get)))
                }
                _ => None,
            })
            .collect()
    }

    /// Where each live rect is now.
    pub(crate) fn rects(&self) -> Vec<[f32; 4]> {
        let bindings = self.bindings.borrow();
        (bindings.values())
            .filter_map(|binding| match &binding.shown {
                Shown::Rect(shown) => Some(shown.get()),
                _ => None,
            })
            .collect()
    }

    /// What the live accessibility value of the element `key` said when its view was built.
    pub(crate) fn spoken(&self, key: &str) -> Option<String> {
        self.spoken
            .borrow()
            .get(key)
            .map(|spoken| spoken.said.clone())
    }

    /// What each live text shows now, after what it shows written out.
    pub(crate) fn texts(&self) -> Vec<(String, String)> {
        let bindings = self.bindings.borrow();
        (bindings.values())
            .filter_map(|binding| match &binding.shown {
                Shown::Text(shown, _) => {
                    Some((format!("{:?}", binding.shows), shown.get().to_string()))
                }
                _ => None,
            })
            .collect()
    }
}

/// What a live value reads, found by working it out once.
struct Recorder {
    prefix: String,
    reads: RefCell<Reads>,
}

impl Leaves for Recorder {
    fn leaf(&self, live: &Live) -> f64 {
        let mut reads = self.reads.borrow_mut();
        match live {
            Live::Drag { id, .. } => reads.drags.push(format!("{}{id}", self.prefix)),
            Live::Plane { key, .. } => reads.planes.push(format!("{}{key}", self.prefix)),
            _ => reads.always = true,
        }
        0.0
    }

    fn ease(&self, _: &str, _: Ease, value: f64) -> f64 {
        self.reads.borrow_mut().always = true;
        value
    }
}

impl Leaves for Sources<'_> {
    fn leaf(&self, live: &Live) -> f64 {
        let playhead = self.lives.playhead.get();
        match live {
            Live::Tick => playhead,
            Live::Playing => f64::from(u8::from(self.state.playing)),
            Live::Elapsed => self.lives.elapsed.get(),
            Live::Bpm => {
                let tempo_map = self.state.project().clock().tempo_map();
                tempo_map.change_at(Ticks(playhead as u64)).bpm.bpm()
            }
            Live::Feed { name, path } => {
                (self.state.feeds).number(self.state, self.instance, path.as_deref(), name)
            }
            Live::Value { name, child, .. } => (self.state.tools.as_ref()).map_or(0.0, |tools| {
                let project = self.state.project();
                let tick = Ticks(playhead as u64);
                tools.field_now(project, self.instance, child.as_deref(), name, tick)
            }),
            Live::Drag { id, part } => {
                let key = format!("{}{id}", self.state.prefix_of(self.instance));
                let hand = self.lives.hand(&key);
                f64::from(match part {
                    DragPart::Dx => hand.moved[0],
                    DragPart::Dy => hand.moved[1],
                    DragPart::StartX => hand.start[0],
                    DragPart::StartY => hand.start[1],
                    DragPart::X => hand.start[0] + hand.moved[0],
                    DragPart::Y => hand.start[1] + hand.moved[1],
                    DragPart::Active => f32::from(u8::from(hand.active)),
                    DragPart::Alt => f32::from(u8::from(hand.active && hand.alt)),
                })
            }
            Live::Plane { key, part } => {
                let key = format!("{}{key}", self.state.prefix_of(self.instance));
                let Some(view) = self.state.planes.view(&key) else {
                    return 0.0;
                };
                f64::from(match part {
                    PlanePart::Left => view.left,
                    PlanePart::Top => view.top,
                    PlanePart::Right => view.right,
                    PlanePart::Bottom => view.bottom,
                    PlanePart::Zoom => view.zoom,
                })
            }
            // The new window has no pointer of these yet.
            _ => 0.0,
        }
    }

    fn ease(&self, key: &str, ease: Ease, value: f64) -> f64 {
        let frame = self.lives.frame.get();
        let mut eased = self.lives.eased.borrow_mut();
        let kept = (eased.entry(format!("{} {key}", self.instance)))
            .or_insert_with(|| Eased::new(frame, value));
        if kept.frame != frame {
            kept.frame = frame;
            if kept.step(ease, value, self.lives.dt.get()) {
                self.lives.moving.set(true);
            }
        }
        kept.value
    }
}
