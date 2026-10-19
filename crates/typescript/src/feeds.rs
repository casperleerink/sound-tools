//! What the live values of a card read from the engine and the clock, taken once a frame: the
//! levels of its meters and of the device, the sound of its meters, the numbers as lanes and
//! cables play them, and what eases from frame to frame.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;
use sound_core::{InstanceId, Project, Scope, Ticks, ValueRange};
use sound_ui::{PeaksReader, Session};

use crate::tree::{Ease, Uses};

/// The name a behaviour keeps the peaks and the scope of a meter under, see `tools.rs`.
pub(crate) fn meter_name(name: &str) -> String {
    format!("meter:{name}")
}

/// The name an owner keeps the watch of what a cable plays into the number of a child under,
/// `child.number`.
pub(crate) fn played_name(child_number: &str) -> String {
    format!("played:{child_number}")
}

/// What live values last read, for tests: `peak:<name>`, the louder channel, and
/// `played:<name>`.
static SHOWN: Mutex<BTreeMap<String, f32>> = Mutex::new(BTreeMap::new());
static FRAMES: AtomicU64 = AtomicU64::new(0);

/// What the live values of a card last read of the peaks `name` (`peak:<name>`, the louder
/// channel) or of a number as it plays (`played:<name>`), for a test that a meter moves.
pub fn shown_by_live_values(name: &str) -> Option<f32> {
    let shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner);
    shown.get(name).copied()
}

/// How many frames the live values of every card worked out, for a measurement.
pub fn live_frames() -> u64 {
    FRAMES.load(Ordering::Relaxed)
}

/// Two reads closer than this take nothing new: the second draw of one frame would otherwise
/// see silence, as a take empties the peaks.
const SAME_FRAME: Duration = Duration::from_millis(4);
/// A frame after a pause eases as one of this long, so a meter does not jump.
const LONGEST_FRAME: f64 = 0.1;

#[derive(Default)]
pub(crate) struct Feeds {
    /// Counts the frames, so what eases moves once a frame, however often it is drawn.
    pub frame: u64,
    /// Seconds since the card opened, and since the frame before.
    pub time: f64,
    pub dt: f64,
    started: Option<Instant>,
    last: Option<Instant>,
    /// The loudest of each channel since the last frame, by the name the tree reads them by.
    pub peaks: HashMap<String, [f32; 2]>,
    taken: Option<Instant>,
    output_reader: Option<PeaksReader>,
    pub output: [f32; 2],
    /// Numbers as they play, by the name the tree reads them by, with how their knob turns.
    pub played: HashMap<String, (f32, Option<ValueRange>)>,
    /// Fields of children, as `child.field`.
    pub children: HashMap<String, (f32, Option<ValueRange>)>,
    /// The last frames of each scope the tree draws, oldest first, and how far it was read.
    pub scopes: HashMap<String, (VecDeque<[f32; 2]>, u64)>,
    pub eased: RefCell<HashMap<String, Eased>>,
    pub histories: RefCell<HashMap<String, (u64, VecDeque<f32>)>>,
}

/// What an easing keeps from frame to frame.
#[derive(Clone, Copy, Default)]
pub(crate) struct Eased {
    frame: u64,
    value: f64,
    /// How long a hold has held.
    held: f64,
}

impl Feeds {
    /// A new frame: reads what `uses` needs of the instance `id`. `state` is its record.
    pub(crate) fn read(
        &mut self,
        session: &Session,
        id: &InstanceId,
        state: &Value,
        uses: &Uses,
        tick: Ticks,
    ) {
        let now = Instant::now();
        let started = *self.started.get_or_insert(now);
        self.dt = (self.last.map_or(0.0, |last| (now - last).as_secs_f64())).min(LONGEST_FRAME);
        self.last = Some(now);
        self.time = (now - started).as_secs_f64();
        self.frame += 1;
        let project = session.project();
        if self.taken.is_none_or(|taken| now - taken >= SAME_FRAME) {
            self.taken = Some(now);
            self.peaks = (uses.peaks.iter())
                .map(|name| (name.clone(), peaks_of(project, id, name)))
                .collect();
            match (uses.output, &self.output_reader) {
                (true, Some(reader)) => self.output = reader.take(),
                (true, None) => {
                    let reader = session.output_peaks();
                    self.output = reader.take();
                    self.output_reader = Some(reader);
                }
                (false, _) => self.output_reader = None,
            }
        }
        self.children = (uses.children.iter())
            .filter_map(|name| Some((name.clone(), child_field(project, id, name)?)))
            .collect();
        self.played = (uses.played.iter())
            .filter_map(|name| Some((name.clone(), played(project, id, state, name, tick)?)))
            .collect();
        self.scopes.retain(|name, _| uses.scopes.contains(name));
        for name in &uses.scopes {
            let Some(scope) = project.scope(id, &meter_name(name)) else {
                continue;
            };
            let (frames, next) = self.scopes.entry(name.clone()).or_default();
            let mut read = Vec::new();
            *next = scope.read(*next, &mut read);
            frames.extend(read);
            let over = frames.len().saturating_sub(Scope::FRAMES);
            frames.drain(..over);
        }
        self.tell_tests();
        // What no tree drew for a while starts again when it shows.
        let frame = self.frame;
        (self.eased.borrow_mut()).retain(|_, eased| frame - eased.frame < 60);
        (self.histories.borrow_mut()).retain(|_, (last, _)| frame - *last < 60);
    }

    fn tell_tests(&self) {
        let mut shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner);
        FRAMES.fetch_add(1, Ordering::Relaxed);
        for (name, [left, right]) in &self.peaks {
            shown.insert(format!("peak:{name}"), left.max(*right));
        }
        for (name, (value, _)) in &self.played {
            shown.insert(format!("played:{name}"), *value);
        }
    }

    /// `value` eased as `ease` says, under `key`: once a frame, the same wherever it is drawn.
    pub(crate) fn ease(&self, key: &str, ease: Ease, value: f64) -> f64 {
        let mut eased = self.eased.borrow_mut();
        let Some(kept) = eased.get_mut(key) else {
            eased.insert(
                key.to_string(),
                Eased {
                    frame: self.frame,
                    value,
                    held: 0.0,
                },
            );
            return value;
        };
        if kept.frame == self.frame {
            return kept.value;
        }
        kept.frame = self.frame;
        let dt = self.dt;
        match ease {
            Ease::Smooth { ms } => {
                let seconds = f64::from(ms.max(1.0)) / 1000.0;
                kept.value += (value - kept.value) * (1.0 - (-dt / seconds).exp());
            }
            Ease::Fall { per_second } => {
                kept.value = value.max(kept.value - f64::from(per_second) * dt);
            }
            Ease::Hold { seconds } => {
                kept.held += dt;
                if value >= kept.value || kept.held > f64::from(seconds) {
                    kept.value = value;
                    kept.held = 0.0;
                }
            }
        }
        if !kept.value.is_finite() {
            kept.value = value;
        }
        kept.value
    }

    /// The last `frames` values of `value` under `key`, one a frame, oldest first.
    pub(crate) fn history(&self, key: &str, frames: u32, value: f64) -> Vec<f32> {
        let mut histories = self.histories.borrow_mut();
        let (last, values) = histories.entry(key.to_string()).or_default();
        if *last != self.frame {
            *last = self.frame;
            values.push_back(value as f32);
            let over = values.len().saturating_sub(frames as usize);
            values.drain(..over);
        }
        values.iter().copied().collect()
    }
}

/// The peaks the tree reads as `name`: of a meter of the instance, or `child.name` of a child.
fn peaks_of(project: &Project, id: &InstanceId, name: &str) -> [f32; 2] {
    let peaks = match name.split_once('.') {
        Some((child, peaks)) => {
            (id.child(child).ok()).and_then(|child| project.peaks(&child, peaks))
        }
        None => project.peaks(id, &meter_name(name)),
    };
    peaks.map_or([0.0; 2], |peaks| peaks.take())
}

/// The field `child.field` of a child's record, with how its knob turns.
fn child_field(
    project: &Project,
    id: &InstanceId,
    name: &str,
) -> Option<(f32, Option<ValueRange>)> {
    let (child, field) = name.split_once('.')?;
    let child = id.child(child).ok()?;
    let range = project.automation(&child, field).map(|number| number.range);
    let record: Value = serde_json::from_str(&project.state_json(&child)?).ok()?;
    let value = record.get(field).and_then(Value::as_f64);
    let value = value.map(|value| value as f32).or_else(|| {
        let number = project.automation(&child, field)?;
        number.record
    })?;
    Some((value, range))
}

/// The number `name` as it plays: of a child, `child.number`, what a cable plays into it, else
/// what a lane plays, else its record; of the instance, what a lane plays, else its record.
fn played(
    project: &Project,
    id: &InstanceId,
    state: &Value,
    name: &str,
    tick: Ticks,
) -> Option<(f32, Option<ValueRange>)> {
    let lane = |instance: &InstanceId, number: &str| {
        let lanes = project.lanes(instance)?;
        let mut values: Vec<(Arc<str>, f32)> = Vec::new();
        lanes.values_at(tick, &mut values);
        values
            .into_iter()
            .find(|(each, _)| **each == *number)
            .map(|(_, value)| value)
    };
    let Some((child, number)) = name.split_once('.') else {
        let range = project.automation(id, name).map(|number| number.range);
        let record = state
            .get(name)
            .and_then(Value::as_f64)
            .map(|value| value as f32);
        return Some((lane(id, name).or(record)?, range));
    };
    let cable = (project.watches(id).into_iter())
        .find(|(each, _)| *each == played_name(name))
        .map(|(_, watch)| watch.get())
        .filter(|value| value.is_finite());
    let child_id = id.child(child).ok()?;
    let (record, range) = child_field(project, id, name)?;
    Some((
        cable.or_else(|| lane(&child_id, number)).unwrap_or(record),
        range,
    ))
}
