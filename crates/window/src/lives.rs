//! What moves every frame without a view running again: the live values of the views, a drag,
//! and the time of the title row, each bound to a Rune live value. A view's live value is a
//! small expression (`sound_typescript::live`) worked out here, from the playhead, the time, the
//! meters and the fields as they play. Kept while a tree that is shown binds it.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use rune::LiveText;
use sound_core::{InstanceId, Ticks};
use sound_typescript::live::{
    Ease, Eased, Leaves, Live, Num, Readout, TickReadout, number, text_of,
};

use crate::app::State;

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
}

/// What Rune draws of a [`Shows`] every frame.
#[derive(Clone)]
pub(crate) enum Shown {
    /// What it shows, and the widest it showed, which sizes it: Rune never resizes live text.
    Text(LiveText, Rc<RefCell<String>>),
    Rect(rune::Live<[f32; 4]>),
    Offset(rune::Live<[f32; 2]>),
    Opacity(rune::Live<f32>),
}

impl Shown {
    /// Binds `e` to it: a live place takes the element out of the flow.
    pub(crate) fn apply(&self, e: rune::El) -> rune::El {
        match self {
            Self::Text(text, _) => e.live_text(text),
            Self::Rect(rect) => e.absolute(0.0, 0.0).live_rect(rect),
            Self::Offset(offset) => e.absolute(0.0, 0.0).live_offset(offset),
            Self::Opacity(opacity) => e.live_opacity(opacity),
        }
    }
}

/// A live value bound to an element, and the instance whose view it is of.
struct Binding {
    instance: InstanceId,
    shows: Shows,
    shown: Shown,
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
    /// Whether a live text showed wider than it was built for since the last look.
    wider: Cell<bool>,
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
            wider: Cell::new(false),
            drags: RefCell::default(),
            time: LiveText::new(time.to_string()),
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
        };
        let binding = Binding {
            instance: instance.clone(),
            shows,
            shown: shown.clone(),
        };
        self.show(state, &binding);
        bindings.insert(name, binding);
        shown
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

    /// The views show only the bindings `named` now, see [`Lives::name`]: the others go, also
    /// those of a part of a view that Rune kept.
    pub(crate) fn keep(&self, named: &HashSet<String>) {
        (self.bindings.borrow_mut()).retain(|name, _| named.contains(name));
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
        // What no binding read this frame eases nothing any more.
        let frame = self.frame.get();
        self.eased
            .borrow_mut()
            .retain(|_, eased| eased.frame == frame);
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
            _ => {}
        }
    }

    /// Whether a live text showed wider than it was built for, since the last call: the view
    /// is built again to make room.
    pub(crate) fn take_wider(&self) -> bool {
        self.wider.replace(false)
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

    pub(crate) fn eased(&self) -> usize {
        self.eased.borrow().len()
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

    /// What each live text shows now.
    pub(crate) fn texts(&self) -> Vec<String> {
        let bindings = self.bindings.borrow();
        (bindings.values())
            .filter_map(|binding| match &binding.shown {
                Shown::Text(shown, _) => Some(shown.get().to_string()),
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
