//! What moves every frame without a view running again: the live values of the views, a drag,
//! and the time of the title row, each bound to a Rune live value. A view's live value is a
//! small expression (`sound_typescript::live`) worked out here, from the playhead, the time, the
//! meters and the fields as they play. Kept across builds, so a binding survives a new tree.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use rune::LiveText;
use sound_core::{InstanceId, Ticks};
use sound_typescript::live::{
    Ease, Eased, Leaves, Live, Num, Readout, TickReadout, number, text_of,
};

use crate::app::State;

/// A live value bound to an element, and the instance whose view it is of.
struct Binding {
    instance: InstanceId,
    /// What it shows, as text: another in its place binds again.
    shows: String,
    bound: Bound,
}

#[derive(Clone)]
enum Bound {
    Text {
        shown: LiveText,
        value: Num,
        unit: Option<Readout>,
        digits: Option<usize>,
    },
    /// Where an element is, and how big: [x, y, w, h].
    Rect {
        shown: rune::Live<[f32; 4]>,
        rect: [Num; 4],
    },
    Offset {
        shown: rune::Live<[f32; 2]>,
        at: [Num; 2],
    },
    Opacity {
        shown: rune::Live<f32>,
        value: Num,
    },
}

pub(crate) struct Lives {
    bindings: RefCell<HashMap<String, Binding>>,
    eased: RefCell<HashMap<String, Eased>>,
    /// The loudest of each channel of each meter read this frame, by instance path and name: a
    /// meter keeps its loudest until read.
    peaks: RefCell<BTreeMap<String, [f32; 2]>>,
    frame: Cell<u64>,
    dt: Cell<f64>,
    elapsed: Cell<f64>,
    playhead: Cell<f64>,
    /// Whether an eased value still moves without new sound.
    moving: Cell<bool>,
    drags: RefCell<HashMap<String, rune::Live<[f32; 2]>>>,
    /// Where the piece plays, as the title row shows it.
    pub(crate) time: LiveText,
}

impl Lives {
    pub(crate) fn new(time: &str) -> Self {
        Self {
            bindings: RefCell::default(),
            eased: RefCell::default(),
            peaks: RefCell::default(),
            frame: Cell::new(0),
            dt: Cell::new(0.0),
            elapsed: Cell::new(0.0),
            playhead: Cell::new(0.0),
            moving: Cell::new(false),
            drags: RefCell::default(),
            time: LiveText::new(time.to_string()),
        }
    }

    /// The text of `value` of the element `key` of the view of `instance`, in `unit` or with
    /// `digits`.
    pub(crate) fn text(
        &self,
        (state, instance, key): (&State, &InstanceId, Option<&str>),
        value: &Num,
        unit: Option<Readout>,
        digits: Option<usize>,
    ) -> LiveText {
        let shows = format!("text {value:?} {unit:?} {digits:?}");
        let bound = || Bound::Text {
            shown: LiveText::new(""),
            value: value.clone(),
            unit,
            digits,
        };
        match self.bind((state, instance, key), shows, bound) {
            Bound::Text { shown, .. } => shown,
            _ => LiveText::new(""),
        }
    }

    /// Where an element of the view of `instance` is and how big, `[x, y, w, h]`.
    pub(crate) fn rect(
        &self,
        element: (&State, &InstanceId, Option<&str>),
        rect: [Num; 4],
    ) -> rune::Live<[f32; 4]> {
        let shows = format!("rect {rect:?}");
        match self.bind(element, shows, || Bound::Rect {
            shown: rune::Live::new([0.0; 4]),
            rect,
        }) {
            Bound::Rect { shown, .. } => shown,
            _ => rune::Live::new([0.0; 4]),
        }
    }

    pub(crate) fn offset(
        &self,
        element: (&State, &InstanceId, Option<&str>),
        at: [Num; 2],
    ) -> rune::Live<[f32; 2]> {
        let shows = format!("offset {at:?}");
        match self.bind(element, shows, || Bound::Offset {
            shown: rune::Live::new([0.0; 2]),
            at,
        }) {
            Bound::Offset { shown, .. } => shown,
            _ => rune::Live::new([0.0; 2]),
        }
    }

    pub(crate) fn opacity(
        &self,
        element: (&State, &InstanceId, Option<&str>),
        value: &Num,
    ) -> rune::Live<f32> {
        let shows = format!("opacity {value:?}");
        match self.bind(element, shows, || Bound::Opacity {
            shown: rune::Live::new(1.0),
            value: value.clone(),
        }) {
            Bound::Opacity { shown, .. } => shown,
            _ => rune::Live::new(1.0),
        }
    }

    /// The binding that `shows` of the element `key` of the view of `instance`, made by `bound`
    /// when it showed nothing or something else, showing what it is now. Bound by its element,
    /// or by `shows` for one with no key: a view that makes a new expression in every build, as
    /// `live.playhead.minus(start)` does, keeps one binding.
    fn bind(
        &self,
        (state, instance, key): (&State, &InstanceId, Option<&str>),
        shows: String,
        bound: impl FnOnce() -> Bound,
    ) -> Bound {
        let kind = shows.split(' ').next().unwrap_or_default();
        let name = match key {
            Some(key) => format!("{instance} {kind} key {key}"),
            None => format!("{instance} {shows}"),
        };
        let mut bindings = self.bindings.borrow_mut();
        if let Some(binding) = bindings.get(&name).filter(|binding| binding.shows == shows) {
            return binding.bound.clone();
        }
        let binding = Binding {
            instance: instance.clone(),
            shows,
            bound: bound(),
        };
        self.show(state, &binding);
        let bound = binding.bound.clone();
        bindings.insert(name, binding);
        bound
    }

    /// How far the element `key` is dragged.
    pub(crate) fn dragged(&self, key: &str) -> rune::Live<[f32; 2]> {
        self.drags
            .borrow_mut()
            .entry(key.to_string())
            .or_insert_with(|| rune::Live::new([0.0; 2]))
            .clone()
    }

    /// A new tree came: what a drag moved is where its record says now.
    pub(crate) fn drop_drags(&self) {
        for live in self.drags.borrow().values() {
            live.set([0.0; 2]);
        }
    }

    /// The window shows only the views of `shown` now.
    pub(crate) fn keep(&self, shown: &BTreeSet<InstanceId>) {
        (self.bindings.borrow_mut()).retain(|_, binding| shown.contains(&binding.instance));
        let scoped = |key: &str| shown.iter().any(|id| key.starts_with(&format!("{id} ")));
        self.eased.borrow_mut().retain(|key, _| scoped(key));
    }

    /// One frame `dt` seconds after the last. Whether a value still moves with no new sound,
    /// such as a meter that falls.
    pub(crate) fn update(&self, state: &State, dt: f64) -> bool {
        let (project, playhead) = (&state.project, state.playhead);
        self.frame.set(self.frame.get() + 1);
        self.dt.set(dt);
        self.elapsed.set(self.elapsed.get() + dt);
        self.playhead.set(playhead.0 as f64);
        self.peaks.borrow_mut().clear();
        self.moving.set(false);
        let position = Some(Readout::Tick(TickReadout::Position));
        self.time
            .set(text_of(playhead.0 as f64, position, Some(project.clock())));
        for binding in self.bindings.borrow().values() {
            self.show(state, binding);
        }
        self.moving.get()
    }

    fn show(&self, state: &State, binding: &Binding) {
        let project = &state.project;
        let sources = Sources {
            lives: self,
            state,
            instance: &binding.instance,
        };
        let of = |value: &Num| number(value, &sources) as f32;
        match &binding.bound {
            Bound::Text {
                shown,
                value,
                unit,
                digits,
            } => {
                let value = number(value, &sources);
                let text = match digits {
                    Some(digits) => format!("{value:.digits$}"),
                    None => text_of(value, *unit, Some(project.clock())),
                };
                shown.set(text);
            }
            Bound::Rect { shown, rect } => {
                shown.set(rect.each_ref().map(of));
            }
            Bound::Offset { shown, at } => {
                shown.set(at.each_ref().map(of));
            }
            Bound::Opacity { shown, value } => {
                shown.set(of(value).clamp(0.0, 1.0));
            }
        }
    }

    /// The loudest of each meter the views read this frame, in dB, by instance path, for the
    /// agent's tree.
    pub(crate) fn levels(&self) -> Vec<(String, f64)> {
        let peaks = self.peaks.borrow();
        let decibels =
            |[left, right]: [f32; 2]| (20.0 * f64::from(left.max(right)).log10()).max(-120.0);
        (peaks.iter())
            .map(|(name, peaks)| (name.clone(), (decibels(*peaks) * 10.0).round() / 10.0))
            .collect()
    }
}

/// What the live values of the view of `instance` read.
struct Sources<'a> {
    lives: &'a Lives,
    state: &'a State,
    instance: &'a InstanceId,
}

impl Sources<'_> {
    /// The loudest sample of the meter at `path` inside the instance since the last frame, or
    /// of the peaks it keeps as `path.name`.
    fn peak(&self, path: &str, channel: Option<u8>) -> f64 {
        let (path, name) = path.split_once('.').unwrap_or((path, "level"));
        let Ok(id) = self.instance.child(path) else {
            return 0.0;
        };
        let key = match name {
            "level" => id.to_string(),
            name => format!("{id}.{name}"),
        };
        let mut peaks = self.lives.peaks.borrow_mut();
        let taken = *peaks.entry(key).or_insert_with(|| {
            (self.state.project.peaks(&id, name)).map_or([0.0; 2], |peaks| peaks.take())
        });
        f64::from(match channel {
            Some(0) => taken[0],
            Some(_) => taken[1],
            None => taken[0].max(taken[1]),
        })
    }
}

#[cfg(test)]
impl Lives {
    pub(crate) fn bindings(&self) -> usize {
        self.bindings.borrow().len()
    }

    /// Where each live rect is now.
    pub(crate) fn rects(&self) -> Vec<[f32; 4]> {
        let bindings = self.bindings.borrow();
        (bindings.values())
            .filter_map(|binding| match &binding.bound {
                Bound::Rect { shown, .. } => Some(shown.get()),
                _ => None,
            })
            .collect()
    }

    /// What each live text shows now.
    pub(crate) fn texts(&self) -> Vec<String> {
        let bindings = self.bindings.borrow();
        (bindings.values())
            .filter_map(|binding| match &binding.bound {
                Bound::Text { shown, .. } => Some(shown.get().to_string()),
                _ => None,
            })
            .collect()
    }
}

impl Leaves for Sources<'_> {
    fn leaf(&self, live: &Live) -> f64 {
        let playhead = self.lives.playhead.get();
        match live {
            Live::Tick => playhead,
            Live::Elapsed => self.lives.elapsed.get(),
            Live::Bpm => {
                let tempo_map = self.state.project.clock().tempo_map();
                tempo_map.change_at(Ticks(playhead as u64)).bpm.bpm()
            }
            Live::Peak { name, channel } => self.peak(name, *channel),
            Live::Value { name, child, .. } => (self.state.tools.as_ref()).map_or(0.0, |tools| {
                let project = &self.state.project;
                let tick = Ticks(playhead as u64);
                tools.field_now(project, self.instance, child.as_deref(), name, tick)
            }),
            // The new window has no drags, spaces or pointer of these yet.
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
