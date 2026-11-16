//! What moves every frame without a view running again: the playhead, the time, the meters and
//! a drag, each a Rune [`Live`] value an element binds by name. Kept across builds, so a binding
//! survives a new tree.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use sound_core::InstanceId;

use rune::{Live, LiveText};

use crate::tree::LiveX;

/// The quietest a meter shows, in dB: the bottom of its well.
const FLOOR_DB: f32 = -60.0;
/// How fast a meter falls after the sound, in dB a second.
const FALL_DB: f32 = 24.0;

pub(crate) struct Lives {
    offsets: RefCell<HashMap<String, (Live<[f32; 2]>, LiveX)>>,
    levels: RefCell<HashMap<String, (Live<[f32; 4]>, String, [f32; 2])>>,
    drags: RefCell<HashMap<String, Live<[f32; 2]>>>,
    pub(crate) time: LiveText,
    /// What each meter shows, in dB, by its live name: in order, as the agent's tree lists them.
    meters: RefCell<BTreeMap<String, f32>>,
    playhead: RefCell<f32>,
}

impl Lives {
    pub(crate) fn new(time: &str) -> Self {
        Self {
            offsets: RefCell::default(),
            levels: RefCell::default(),
            drags: RefCell::default(),
            time: LiveText::new(time.to_string()),
            meters: RefCell::default(),
            playhead: RefCell::new(0.0),
        }
    }

    /// The offset of the element `key`, moved by `spec` each frame.
    pub(crate) fn offset(&self, key: &str, spec: LiveX) -> Live<[f32; 2]> {
        let mut offsets = self.offsets.borrow_mut();
        let entry = offsets
            .entry(key.to_string())
            .or_insert_with(|| (Live::new([0.0; 2]), spec.clone()));
        entry.1 = spec;
        entry.0.set([
            entry.1.offset + entry.1.scale * self.value(&entry.1.live),
            0.0,
        ]);
        entry.0.clone()
    }

    /// The rectangle of the element `key`, `size` points, filled from the bottom by `name`.
    pub(crate) fn level(&self, key: &str, name: String, size: [f32; 2]) -> Live<[f32; 4]> {
        let filled = self.filled(&name, size);
        let mut levels = self.levels.borrow_mut();
        let entry = levels
            .entry(key.to_string())
            .or_insert_with(|| (Live::new(filled), name.clone(), size));
        (entry.1, entry.2) = (name, size);
        entry.0.set(filled);
        entry.0.clone()
    }

    /// How far the element `key` is dragged.
    pub(crate) fn dragged(&self, key: &str) -> Live<[f32; 2]> {
        self.drags
            .borrow_mut()
            .entry(key.to_string())
            .or_insert_with(|| Live::new([0.0; 2]))
            .clone()
    }

    /// A new tree came: what a drag moved is where its record says now.
    pub(crate) fn drop_drags(&self) {
        for live in self.drags.borrow().values() {
            live.set([0.0; 2]);
        }
    }

    /// One frame `seconds` after the last: the playhead in ticks, the time as text, and the
    /// loudest amplitude of each meter since. Whether a meter still moves.
    pub(crate) fn update(
        &self,
        names: &BTreeSet<String>,
        playhead: f32,
        time: String,
        seconds: f32,
        mut amplitude: impl FnMut(&str) -> f32,
    ) -> bool {
        *self.playhead.borrow_mut() = playhead;
        self.time.set(time);
        let mut moving = false;
        {
            let mut meters = self.meters.borrow_mut();
            meters.retain(|name, _| names.contains(name));
            for name in names {
                let heard = 20.0 * amplitude(&name).max(1e-6).log10();
                let shown = meters.entry(name.clone()).or_insert(FLOOR_DB);
                *shown = heard.max(*shown - FALL_DB * seconds).max(FLOOR_DB);
                moving |= *shown > FLOOR_DB;
            }
        }
        for (live, spec) in self.offsets.borrow().values() {
            live.set([spec.offset + spec.scale * self.value(&spec.live), 0.0]);
        }
        for (live, name, size) in self.levels.borrow().values() {
            live.set(self.filled(name, *size));
        }
        moving
    }

    /// A live value by name: the playhead in ticks, or a meter from 0 to 1.
    fn value(&self, name: &str) -> f32 {
        match name {
            "playhead" => *self.playhead.borrow(),
            meter => self
                .meters
                .borrow()
                .get(meter)
                .map_or(0.0, |db| (db - FLOOR_DB) / -FLOOR_DB)
                .clamp(0.0, 1.0),
        }
    }

    fn filled(&self, name: &str, [w, h]: [f32; 2]) -> [f32; 4] {
        let filled = self.value(name) * h;
        [0.0, h - filled, w, filled]
    }

    /// What each meter shows from 0 to 1, by live name, for the agent's tree.
    pub(crate) fn levels(&self) -> Vec<(String, f32)> {
        let names: Vec<String> = self.meters.borrow().keys().cloned().collect();
        names
            .into_iter()
            .map(|name| {
                let level = self.value(&name);
                (name, level)
            })
            .collect()
    }
}

/// A live name with its path made whole for the view of `instance`: `meter:t01/mix` of the
/// song is `meter:song/t01/mix`.
pub(crate) fn live_name(instance: &InstanceId, name: &str) -> String {
    match name.strip_prefix("meter:").map(|path| instance.child(path)) {
        Some(Ok(id)) => format!("meter:{id}"),
        _ => name.to_string(),
    }
}
