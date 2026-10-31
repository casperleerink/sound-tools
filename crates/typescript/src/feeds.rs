//! What the live values of a card read from the engine and the clock, taken once a frame: the
//! levels of its meters and of the device, the sound of its meters, the numbers as lanes and
//! cables play them, and what eases from frame to frame.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use sound_core::{Clock, InstanceId, Peaks, Project, RecordStamp, Scope, Ticks, ValueRange};
use sound_media::Overview;
use sound_ui::{LiveNotes, PeaksReader, Session, SharedPeaks};

use crate::meter;
use crate::tree::{Ease, Uses};

/// The name an owner keeps the watch of what a cable plays into the number of a child under,
/// `child.number`.
pub(crate) fn played_name(child_number: &str) -> String {
    format!("played:{child_number}")
}

/// What live values last read, for tests: `peak:<name>`, the louder channel, `played:<name>`,
/// and `input:<channel>`.
#[cfg(feature = "test-support")]
static SHOWN: std::sync::Mutex<std::collections::BTreeMap<String, f32>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// What the live values of a card last read of the peaks `name` (`peak:<name>`, the louder
/// channel), of a number as it plays (`played:<name>`) or of a channel of the audio input
/// (`input:<channel>`), for a test that a meter moves.
#[cfg(feature = "test-support")]
pub fn shown_by_live_values(name: &str) -> Option<f32> {
    let shown = SHOWN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    shown.get(name).copied()
}

/// How many stretches a waveform of a file holds: more than a wave draws columns, so each
/// column holds a stretch's `+peak` and `-peak`.
const WAVE_COLUMNS: usize = 4096;

/// Two reads closer than this take nothing new: the second draw of one frame would otherwise
/// see silence, as a take empties the peaks.
const SAME_FRAME: Duration = Duration::from_millis(4);
/// A frame after a pause eases as one of this long, so a meter does not jump.
const LONGEST_FRAME: f64 = 0.1;
/// An eased number this close to where it goes is there.
const SETTLED: f64 = 1e-6;

/// A part of a card that draws on its own, from the same feeds.
#[derive(Clone, Copy)]
pub(crate) enum Drawn {
    Body,
    TitleBar,
    /// The layer over a page.
    Over,
}

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
    /// What it reads those from, by the same name.
    meters: HashMap<String, PeaksReader>,
    taken: Option<Instant>,
    output_reader: Option<PeaksReader>,
    pub output: [f32; 2],
    /// The loudest sample of each channel of the audio input at the last poll.
    pub input: Vec<f32>,
    /// Numbers as they play, by the name the tree reads them by, with how their knob turns.
    pub played: HashMap<String, (f32, Option<ValueRange>)>,
    /// Fields of instances inside it, as `path.field`.
    pub inside: HashMap<String, (f32, Option<ValueRange>)>,
    /// The last frames of each scope the tree draws, oldest first, and how far it was read.
    pub scopes: HashMap<String, (VecDeque<[f32; 2]>, u64)>,
    pub eased: RefCell<HashMap<String, Eased>>,
    /// What the draw now eased still moves with no new sound: a meter that falls or a number
    /// that eases.
    pub moving: Cell<bool>,
    /// Whether what each part drew since the last poll still moves.
    moved: Cell<[bool; 3]>,
    /// The frame the last poll saw, and what had moved until then.
    polled: (u64, [bool; 3]),
    pub histories: RefCell<HashMap<String, (u64, VecDeque<f32>)>>,
    /// The clock of the project, for the tempo and the places in the piece a tree shows.
    pub clock: Option<Clock>,
    /// The overviews of the audio files it draws, by file name, once they are made.
    pub files: HashMap<String, Arc<Overview>>,
    /// The takes of recorders as they grow, by the path the tree names the recorder by.
    pub takes: HashMap<String, Rc<[f32]>>,
    /// The notes of the takes of recorders as they grow, by the same path.
    pub take_notes: HashMap<String, Rc<LiveNotes>>,
    /// Waveforms as drawn, by file name and the seconds of it, which stay until it changes.
    pub waves: RefCell<HashMap<(String, u64, u64), (u64, Rc<[f32]>)>>,
    records: Records,
}

/// The records of the instances a card reads fields of, each read once for every change of it,
/// not every frame: those of the last frame, and those read in this one.
#[derive(Default)]
struct Records {
    last: HashMap<InstanceId, (RecordStamp, Value)>,
    now: HashMap<InstanceId, (RecordStamp, Value)>,
}

impl Records {
    fn next_frame(&mut self) {
        self.last = std::mem::take(&mut self.now);
    }

    fn of(&mut self, project: &Project, id: &InstanceId) -> Option<&Value> {
        let stamp = project.stamp(id)?;
        if !self.now.contains_key(id) {
            let kept = (self.last.remove(id)).filter(|(kept, _)| *kept == stamp);
            let record = match kept {
                Some(kept) => kept,
                None => (stamp, serde_json::from_str(&project.state_json(id)?).ok()?),
            };
            self.now.insert(id.clone(), record);
        }
        self.now.get(id).map(|(_, record)| record)
    }
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
    /// A new frame at `now` of the executor's clock: reads what `uses` needs of the instance
    /// `id`. `state` is its record.
    pub(crate) fn read(
        &mut self,
        session: &Session,
        id: &InstanceId,
        state: &Value,
        uses: &Uses,
        tick: Ticks,
        now: Instant,
    ) {
        let started = *self.started.get_or_insert(now);
        // A second read of one frame, as when the title bar draws alone and then the body, is
        // the same frame: what eases moves once.
        if self.last.is_none_or(|last| now - last >= SAME_FRAME) {
            self.dt = (self.last.map_or(0.0, |last| (now - last).as_secs_f64())).min(LONGEST_FRAME);
            self.last = Some(now);
            self.time = (now - started).as_secs_f64();
            self.frame += 1;
        }
        let project = session.project();
        let clock = project.clock();
        if (self.clock.as_ref()).is_none_or(|kept| kept.tempo_map() != clock.tempo_map()) {
            self.clock = Some(clock.clone());
        }
        if self.taken.is_none_or(|taken| now - taken >= SAME_FRAME) {
            self.taken = Some(now);
            self.meters.retain(|name, _| uses.peaks.contains(name));
            let meters = &mut self.meters;
            self.peaks = (uses.peaks.iter())
                .map(|name| {
                    let Some((key, peaks)) = peaks_at(project, id, name) else {
                        meters.remove(name);
                        return (name.clone(), [0.0; 2]);
                    };
                    let reader =
                        (meters.entry(name.clone())).or_insert_with(|| shared_reader(&key, &peaks));
                    if !reader.reads(&peaks) {
                        *reader = shared_reader(&key, &peaks);
                    }
                    (name.clone(), reader.take())
                })
                .collect();
            match (uses.output, &self.output_reader) {
                (true, Some(reader)) => self.output = reader.take(),
                (true, None) => {
                    let reader = session.output_peaks();
                    self.output = reader.take();
                    self.output_reader = Some(reader);
                }
                (false, _) => (self.output_reader, self.output) = (None, [0.0; 2]),
            }
        }
        self.records.next_frame();
        let records = &mut self.records;
        self.inside = (uses.inside.iter())
            .filter_map(|name| Some((name.clone(), child_field(project, id, name, records)?)))
            .collect();
        self.played = (uses.played.iter())
            .filter_map(|name| {
                let value = played(project, (id, state), name, tick, records)?;
                Some((name.clone(), value))
            })
            .collect();
        self.scopes.retain(|name, _| uses.scopes.contains(name));
        for name in &uses.scopes {
            let Some(scope) = scope_of(project, id, name) else {
                continue;
            };
            let (frames, next) = self.scopes.entry(name.clone()).or_default();
            let mut read = Vec::new();
            *next = scope.read(*next, &mut read);
            frames.extend(read);
            let over = frames.len().saturating_sub(Scope::FRAMES);
            frames.drain(..over);
        }
        #[cfg(feature = "test-support")]
        self.tell_tests();
        // What no tree drew for a while starts again when it shows.
        let frame = self.frame;
        (self.eased.borrow_mut()).retain(|_, eased| frame - eased.frame < 60);
        (self.histories.borrow_mut()).retain(|_, (last, _)| frame - *last < 60);
        (self.waves.borrow_mut()).retain(|_, (last, _)| frame - *last < 60);
    }

    /// Whether a frame now would draw something else than the last, of what reads `uses`, the
    /// title bar or the rest: what it reads of the engine, of the audio input or of what plays
    /// changed, or the last frame still moves. Takes nothing, so the frame sees all of it.
    pub(crate) fn changed(
        &mut self,
        session: &Session,
        (id, state): (&InstanceId, &Value),
        (uses, part): (&Uses, Drawn),
        tick: Ticks,
        input: &[f32],
    ) -> bool {
        if self.polled.0 != self.frame {
            self.polled = (self.frame, self.moved.take());
        }
        let moving = self.polled.1[part as usize];
        let loud = |peaks: &[f32; 2]| peaks.iter().any(|peak| *peak > 0.0);
        let peaks = (uses.peaks.iter()).filter_map(|name| self.peaks.get(name));
        if moving || peaks.into_iter().any(loud) || (uses.output && loud(&self.output)) {
            return true;
        }
        let project = session.project();
        let new_peaks = (uses.peaks.iter()).any(|name| {
            let Some((_, peaks)) = peaks_at(project, id, name) else {
                return false;
            };
            match self.meters.get(name) {
                Some(reader) if reader.reads(&peaks) => !reader.is_silent(),
                _ => !peaks.is_silent(),
            }
        });
        let new_output =
            uses.output && (self.output_reader.as_ref()).is_none_or(|reader| !reader.is_silent());
        let records = &mut self.records;
        let new_played = (uses.played.iter()).any(|name| {
            let now = played(project, (id, state), name, tick, records);
            now.map(|(value, _)| value) != self.played.get(name).map(|(value, _)| *value)
        });
        new_peaks || new_output || (uses.input && self.input != input) || new_played
    }

    /// `part` drew: whether what it eased still moves counts for it alone.
    pub(crate) fn drawn(&self, part: Drawn) {
        let mut moved = self.moved.get();
        moved[part as usize] |= self.moving.take();
        self.moved.set(moved);
    }

    /// The waveform of a take as it is now, in the form of [`Self::wave`].
    pub(crate) fn take(overview: &Overview) -> Rc<[f32]> {
        let peaks = overview.columns(0.0, overview.seconds(), WAVE_COLUMNS);
        peaks.iter().flat_map(|peak| [*peak, -*peak]).collect()
    }

    /// The waveform of `file` from `from` to `to` seconds: the peak of each of
    /// [`WAVE_COLUMNS`] stretches, as `+peak` then `-peak`. Empty until its overview is made.
    pub(crate) fn wave(&self, file: &str, from: Option<f64>, to: Option<f64>) -> Rc<[f32]> {
        let Some(overview) = self.files.get(file) else {
            return Rc::from([]);
        };
        let (from, to) = (from.unwrap_or(0.0), to.unwrap_or(overview.seconds()));
        let key = (file.to_string(), from.to_bits(), to.to_bits());
        let mut waves = self.waves.borrow_mut();
        let (last, wave) = waves.entry(key).or_insert_with(|| {
            let peaks = overview.columns(from, to, WAVE_COLUMNS);
            let both = peaks.iter().flat_map(|peak| [*peak, -*peak]);
            (0, both.collect())
        });
        *last = self.frame;
        wave.clone()
    }

    #[cfg(feature = "test-support")]
    fn tell_tests(&self) {
        let mut shown = SHOWN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (name, [left, right]) in &self.peaks {
            shown.insert(format!("peak:{name}"), left.max(*right));
        }
        for (name, (value, _)) in &self.played {
            shown.insert(format!("played:{name}"), *value);
        }
        for (channel, level) in self.input.iter().enumerate() {
            shown.insert(format!("input:{channel}"), *level);
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
        let (dt, before) = (self.dt, kept.value);
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
        // Close enough to stop, or a smooth ease would draw for a minute.
        if !kept.value.is_finite() || (value - kept.value).abs() < SETTLED {
            kept.value = value;
        }
        if kept.value != before || kept.value != value {
            self.moving.set(true);
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

/// The peaks the tree reads as `name`: of the `meter` at that path inside the instance, or
/// `path.name`, peaks an instance inside it keeps, such as a built-in device. With the name
/// every card shares them by.
fn peaks_at(project: &Project, id: &InstanceId, name: &str) -> Option<(String, Peaks)> {
    let (path, peaks) = name.rsplit_once('.').unwrap_or((name, meter::LEVEL));
    let inner = id.child(path).ok()?;
    Some((format!("{inner}.{peaks}"), project.peaks(&inner, peaks)?))
}

thread_local! {
    /// The peaks of every meter a card shows, by instance and name.
    static SHARED: RefCell<HashMap<String, SharedPeaks>> = RefCell::default();
}

/// A reader of the peaks `key` of a meter, which sees every peak however the other cards that
/// show it draw: a take empties the peaks.
fn shared_reader(key: &str, peaks: &Peaks) -> PeaksReader {
    SHARED.with_borrow_mut(|shared| {
        shared.retain(|_, each| each.is_read());
        match shared.get(key) {
            Some(each) if each.shares(peaks) => each.reader(),
            _ => {
                let each = SharedPeaks::new(peaks.clone());
                let reader = each.reader();
                shared.insert(key.to_string(), each);
                reader
            }
        }
    })
}

/// The last sound of the `meter` at the path `name` inside the instance.
fn scope_of(project: &Project, id: &InstanceId, name: &str) -> Option<Scope> {
    project.scope(&id.child(name).ok()?, meter::LEVEL)
}

/// The field `path.field` of the record of an instance inside `id`, with how its knob turns.
fn child_field(
    project: &Project,
    id: &InstanceId,
    name: &str,
    records: &mut Records,
) -> Option<(f32, Option<ValueRange>)> {
    let (child, field) = name.split_once('.')?;
    let child = id.child(child).ok()?;
    let range = project.automation(&child, field).map(|number| number.range);
    let value = records
        .of(project, &child)?
        .get(field)
        .and_then(Value::as_f64);
    let value = value.map(|value| value as f32).or_else(|| {
        let number = project.automation(&child, field)?;
        number.record
    })?;
    Some((value, range))
}

/// The number `name` as it plays: of an instance inside it, `path.number`, what a cable plays
/// into it, else what a lane plays, else its record; of the instance, what a lane plays, else its
/// record.
fn played(
    project: &Project,
    (id, state): (&InstanceId, &Value),
    name: &str,
    tick: Ticks,
    records: &mut Records,
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
    let Some((path, number)) = name.split_once('.') else {
        let range = project.automation(id, name).map(|number| number.range);
        let record = state
            .get(name)
            .and_then(Value::as_f64)
            .map(|value| value as f32);
        return Some((lane(id, name).or(record)?, range));
    };
    let inner = id.child(path).ok()?;
    // The owner whose behaviour wires the cable keeps what it plays, by its child's name.
    let owner = inner.parent()?;
    let cable = (project.watched(&owner, &played_name(&format!("{}.{number}", inner.name()))))
        .filter(|value| value.is_finite());
    let (record, range) = child_field(project, id, name, records)?;
    Some((
        cable.or_else(|| lane(&inner, number)).unwrap_or(record),
        range,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_card_that_shows_a_meter_sees_its_peak_however_their_draws_interleave() {
        let peaks = Peaks::new();
        let first = shared_reader("song/meter.level", &peaks);
        let second = shared_reader("song/meter.level", &peaks);
        peaks.record_block([&[0.5], &[0.25]]);
        assert_eq!(first.take(), [0.5, 0.25]);
        assert!(!second.is_silent());
        peaks.record_block([&[0.75], &[0.0]]);
        assert_eq!(second.take(), [0.75, 0.25]);
        assert_eq!(first.take(), [0.75, 0.0]);
    }
}
