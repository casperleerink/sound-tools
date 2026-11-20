//! What moves every frame without a view running again: the live values of the views and a
//! drag, each bound to a Rune live value. A view's live value is a
//! small expression (`sound_typescript::live`) worked out here, from the playhead, the time, the
//! feeds and the fields as they play. Kept while a tree that is shown binds it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rune::LiveText;
use sound_core::{InstanceId, Ticks};
use sound_typescript::live::{DragPart, Ease, Eased, Leaves, Live, Num, Readout, number, text_of};

use crate::app::State;
use crate::tree::Listed;

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
    /// The lists it drew last.
    Pixels(rune::Pixels, Rc<RefCell<Vec<Vec<f32>>>>),
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
            Self::Pixels(pixels, _) => e.pixels(pixels),
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
    frame: Cell<u64>,
    dt: Cell<f64>,
    elapsed: Cell<f64>,
    playhead: Cell<f64>,
    /// Whether an eased value still moves without new sound.
    moving: Cell<bool>,
    /// Whether a live text showed wider than it was built for, or a live accessibility value
    /// moved, since the last look.
    wider: Cell<bool>,
    drags: RefCell<HashMap<String, rune::Live<[f32; 2]>>>,
    /// The keys of the elements dragged now.
    held: RefCell<HashSet<String>>,
    /// The live accessibility values of the views, by element key: their instance, their value,
    /// and what they said when the view was built.
    spoken: RefCell<HashMap<String, (InstanceId, Num, String)>>,
}

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
            drags: RefCell::default(),
            held: RefCell::default(),
            spoken: RefCell::default(),
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
            Shows::Lists(_) => Shown::Pixels(rune::Pixels::new(), Rc::default()),
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

    /// The accessibility value of the element `key` of the view of `instance`, `value` as it is
    /// now; while it is live it is followed each frame, and the view is built again when what it
    /// says changes, as Rune's accessibility values do not move by themselves.
    pub(crate) fn said_live(
        &self,
        state: &State,
        instance: &InstanceId,
        key: &str,
        value: &serde_json::Value,
    ) -> String {
        let said = self.said(state, instance, value);
        let text = said
            .as_str()
            .map_or_else(|| said.to_string(), str::to_string);
        if let Ok(num @ Num::Live(_)) = serde_json::from_value::<Num>(value.clone()) {
            let entry = (instance.clone(), num, text.clone());
            self.spoken.borrow_mut().insert(key.to_string(), entry);
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

    /// Whether the element `key` is dragged now.
    pub(crate) fn held(&self, key: &str, held: bool) {
        let mut keys = self.held.borrow_mut();
        match held {
            true => keys.insert(key.to_string()),
            false => keys.remove(key),
        };
    }

    /// Shows what a drag moved at once, not a frame later.
    pub(crate) fn refresh(&self, state: &State) {
        for binding in self.bindings.borrow().values() {
            self.show(state, binding);
        }
    }

    /// A new tree came: what a drag moved is where its record says now, but the drag of
    /// `held`, which goes on.
    pub(crate) fn drop_drags(&self, held: Option<&str>) {
        for (key, live) in self.drags.borrow().iter() {
            if Some(key.as_str()) != held {
                live.set([0.0; 2]);
            }
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
        let playhead = state.playhead;
        self.frame.set(self.frame.get() + 1);
        self.dt.set(dt);
        self.elapsed.set(self.elapsed.get() + dt);
        self.playhead.set(playhead.0 as f64);
        self.moving.set(false);
        for binding in self.bindings.borrow().values() {
            self.show(state, binding);
        }
        let mut spoken = self.spoken.borrow_mut();
        spoken.retain(|_, (instance, ..)| state.shown.contains(instance));
        for (instance, value, said) in spoken.values() {
            let sources = Sources {
                lives: self,
                state,
                instance,
            };
            let now = serde_json::json!(number(value, &sources)).to_string();
            if now != *said {
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
            (Shows::Lists(listed), Shown::Pixels(pixels, last)) => {
                (state.feeds).draw(state, &binding.instance, listed, pixels, last);
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
                Shown::Pixels(pixels, _) => Some(pixels.size()),
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
            .map(|(_, _, said)| said.clone())
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
                let (project, tick) = (self.state.project(), self.state.playhead);
                (self.state.feeds).number(project, tick, self.instance, path.as_deref(), name)
            }
            Live::Value { name, child, .. } => (self.state.tools.as_ref()).map_or(0.0, |tools| {
                let project = self.state.project();
                let tick = Ticks(playhead as u64);
                tools.field_now(project, self.instance, child.as_deref(), name, tick)
            }),
            Live::Drag { id, part } => {
                let key = format!("{}{id}", self.state.prefix_of(self.instance));
                let moved = (self.lives.drags.borrow().get(&key)).map_or([0.0; 2], rune::Live::get);
                match part {
                    DragPart::Dx => f64::from(moved[0]),
                    DragPart::Dy => f64::from(moved[1]),
                    DragPart::Active => {
                        f64::from(u8::from(self.lives.held.borrow().contains(&key)))
                    }
                    _ => 0.0,
                }
            }
            // The new window has no spaces or pointer of these yet.
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
