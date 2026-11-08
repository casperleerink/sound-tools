//! Standard MIDI files: each part of one read as a clip, and a clip written as one.
//!
//! A file counts in its own ticks to a quarter note; they are scaled to ours, so its notes land
//! on the same beats whatever tempo it has. Its tempo and time signatures are left out: the
//! project owns them.

use std::collections::BTreeMap;

use midly::num::{u4, u7, u15, u24, u28};
use midly::{
    Format, Header, MetaMessage, MidiMessage, PitchBend, Smf, Timing, TrackEvent, TrackEventKind,
};
use sound_core::Ticks;

use crate::{
    Amount, Bend, Clip, Controller, ExpressionValue, Length, Note, Pedal, PedalChange, Pitch,
    Point, Velocity, thinned, value_at,
};

/// Ticks to a quarter note in a clip, and in a file this writes.
const QUARTER: u64 = 960;

/// Controllers whose meaning is in the order of their messages: bank select, and the parameter
/// numbers with their data. A lane sends one value a block, so they are not imported.
const IN_ORDER: [u8; 10] = [0, 32, 6, 38, 96, 97, 98, 99, 100, 101];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MidiFileError {
    #[error("It could not be read as a MIDI file.")]
    Unreadable,
    #[error("It holds separate songs one after another, which cannot be imported.")]
    Sequential,
    #[error("It counts time in frames, not in beats, which cannot be imported.")]
    Frames,
    #[error("It has no notes.")]
    NoNotes,
}

/// Whether `bytes` start as a MIDI file does.
pub fn is_midi_file(bytes: &[u8]) -> bool {
    bytes.starts_with(b"MThd")
}

/// Each part of the file, a channel of a track that plays a note, as a clip at tick 0 with its
/// title: the track's name, else `name`, with a number when parts share it.
pub fn read(bytes: &[u8], name: &str) -> Result<Vec<(String, Clip)>, MidiFileError> {
    let smf = Smf::parse(bytes).map_err(|_| MidiFileError::Unreadable)?;
    let per_quarter = match smf.header.timing {
        Timing::Metrical(ticks) if ticks.as_int() > 0 => u64::from(ticks.as_int()),
        Timing::Metrical(_) => return Err(MidiFileError::Unreadable),
        Timing::Timecode(..) => return Err(MidiFileError::Frames),
    };
    if smf.header.format == Format::Sequential {
        return Err(MidiFileError::Sequential);
    }
    let scale = |ticks: u64| {
        let scaled = (u128::from(ticks) * u128::from(QUARTER) + u128::from(per_quarter) / 2)
            / u128::from(per_quarter);
        Ticks(u64::try_from(scaled).unwrap_or(u64::MAX))
    };
    let mut parts = Vec::new();
    for track in &smf.tracks {
        let mut title = None;
        let mut channels: BTreeMap<u8, Part> = BTreeMap::new();
        let mut at = 0u64;
        for event in track {
            at = at.saturating_add(u64::from(event.delta.as_int()));
            let tick = scale(at);
            match event.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(text)) if title.is_none() => {
                    let text = String::from_utf8_lossy(text).trim().to_string();
                    title = (!text.is_empty()).then_some(text);
                }
                TrackEventKind::Midi { channel, message } => {
                    let part = channels.entry(channel.as_int()).or_default();
                    part.hear(tick, message);
                }
                _ => {}
            }
        }
        let end = scale(at);
        parts.extend(
            channels
                .into_values()
                .filter_map(|part| Some((title.clone(), part.clip(end)?))),
        );
    }
    if parts.is_empty() {
        return Err(MidiFileError::NoNotes);
    }
    let fallback = |title: &Option<String>| title.clone().unwrap_or_else(|| name.to_string());
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (title, _) in &parts {
        *counts.entry(fallback(title)).or_default() += 1;
    }
    let mut numbered: BTreeMap<String, usize> = BTreeMap::new();
    let titled = parts.into_iter().map(|(title, clip)| {
        let title = fallback(&title);
        if counts.get(&title).is_some_and(|count| *count > 1) {
            let number = numbered.entry(title.clone()).or_default();
            *number += 1;
            (format!("{title} {number}"), clip)
        } else {
            (title, clip)
        }
    });
    Ok(titled.collect())
}

/// What one channel of a track plays, in our ticks from the start of the file.
#[derive(Default)]
struct Part {
    notes: Vec<Note>,
    /// The notes that sound, by pitch, the oldest first: an off ends the oldest.
    sounding: BTreeMap<u8, Vec<(Ticks, Velocity)>>,
    pedal: Vec<(Ticks, Pedal)>,
    bend: Vec<(Ticks, Bend)>,
    mod_wheel: Vec<(Ticks, Amount)>,
    pressure: Vec<(Ticks, Amount)>,
    controllers: BTreeMap<Controller, Vec<(Ticks, Amount)>>,
}

impl Part {
    fn hear(&mut self, tick: Ticks, message: MidiMessage) {
        let amount = |value: u7| Amount::nearest(i64::from(value.as_int()));
        match message {
            MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                let velocity = Velocity::nearest(i64::from(vel.as_int()));
                self.sounding
                    .entry(key.as_int())
                    .or_default()
                    .push((tick, velocity));
            }
            MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                self.end(key.as_int(), tick);
            }
            MidiMessage::Controller { controller, value } => match controller.as_int() {
                Controller::SUSTAIN_PEDAL => {
                    self.pedal
                        .push((tick, Pedal::nearest(i64::from(value.as_int()))));
                }
                Controller::MOD_WHEEL => self.mod_wheel.push((tick, amount(value))),
                number if IN_ORDER.contains(&number) => {}
                number => {
                    if let Ok(controller) = Controller::new(number) {
                        let lane = self.controllers.entry(controller).or_default();
                        lane.push((tick, amount(value)));
                    }
                }
            },
            MidiMessage::PitchBend { bend } => {
                self.bend
                    .push((tick, Bend::nearest(i64::from(bend.as_int()))));
            }
            MidiMessage::ChannelAftertouch { vel } => self.pressure.push((tick, amount(vel))),
            MidiMessage::Aftertouch { .. } | MidiMessage::ProgramChange { .. } => {}
        }
    }

    /// Ends the oldest sounding note of `pitch`, at least a tick long.
    fn end(&mut self, pitch: u8, tick: Ticks) {
        let Some(sounding) = self.sounding.get_mut(&pitch) else {
            return;
        };
        if sounding.is_empty() {
            return;
        }
        let (start, velocity) = sounding.remove(0);
        if let Ok(pitch) = Pitch::new(pitch) {
            self.notes.push(Note {
                start,
                length: Length::at_least_one(tick.saturating_sub(start)),
                pitch,
                velocity,
            });
        }
    }

    /// The clip of the part, whose track ends at `end`: none without a note. A note still
    /// sounding ends there.
    fn clip(mut self, end: Ticks) -> Option<Clip> {
        let pitches: Vec<u8> = self.sounding.keys().copied().collect();
        for pitch in pitches {
            while self
                .sounding
                .get(&pitch)
                .is_some_and(|sounding| !sounding.is_empty())
            {
                self.end(pitch, end);
            }
        }
        if self.notes.is_empty() {
            return None;
        }
        self.notes.sort();
        // A pedal that lifts where the part ends adds nothing: a clip lifts it there.
        self.pedal
            .retain(|(tick, value)| *tick < end || value.is_down());
        let pedal = pedal_moves(&self.pedal);
        let bend = lane(&self.bend, Some(end));
        let mod_wheel = lane(&self.mod_wheel, Some(end));
        let pressure = lane(&self.pressure, Some(end));
        let controllers: BTreeMap<_, _> = (self.controllers.iter())
            .map(|(controller, steps)| (*controller, lane(steps, None)))
            .collect();
        let last_tick = [
            self.notes.iter().map(Note::end).max(),
            pedal.last().map(|change| change.start + Ticks(1)),
            bend.last().map(|point| point.tick + Ticks(1)),
            mod_wheel.last().map(|point| point.tick + Ticks(1)),
            pressure.last().map(|point| point.tick + Ticks(1)),
            (controllers.values())
                .filter_map(|points| Some(points.last()?.tick + Ticks(1)))
                .max(),
        ];
        let length = last_tick.into_iter().flatten().fold(end, Ticks::max);
        let mut clip = Clip::new(Ticks(0), Length::at_least_one(length), self.notes);
        clip.pedal = pedal;
        clip.bend = bend;
        clip.mod_wheel = mod_wheel;
        clip.pressure = pressure;
        clip.controllers = controllers;
        Some(clip)
    }
}

/// The pedal moves, the last of each tick, without repeats.
fn pedal_moves(steps: &[(Ticks, Pedal)]) -> Vec<PedalChange> {
    let mut moves: Vec<PedalChange> = Vec::new();
    for &(start, value) in steps {
        if moves.last().is_some_and(|last| last.start == start) {
            moves.pop();
        }
        if moves.last().is_none_or(|last| last.value != value) {
            moves.push(PedalChange { start, value });
        }
    }
    moves
}

/// The points of a lane from the steps a file sends, the last of each tick: each step holds
/// its value until the tick before the next one, then thinned to the lines it follows.
///
/// An expression lane, which holds its first value from the start of its clip and rests after
/// its `end`, starts at rest at tick 0 when it moves later, and a rest from its end on adds
/// nothing.
fn lane<V: ExpressionValue>(steps: &[(Ticks, V)], end: Option<Ticks>) -> Vec<Point<V>> {
    let mut values: Vec<Point<V>> = Vec::new();
    let resting_after =
        |tick: Ticks, value: V| end.is_some_and(|end| tick >= end && value == V::REST);
    for &(tick, value) in steps
        .iter()
        .filter(|(tick, value)| !resting_after(*tick, *value))
    {
        if values.last().is_some_and(|last| last.tick == tick) {
            values.pop();
        }
        if values.last().is_none_or(|last| last.value != value) {
            values.push(Point { tick, value });
        }
    }
    if end.is_some()
        && let Some(first) = values.first()
        && first.tick > Ticks(0)
        && first.value != V::REST
    {
        values.insert(
            0,
            Point {
                tick: Ticks(0),
                value: V::REST,
            },
        );
    }
    let mut points = Vec::with_capacity(values.len() * 2);
    for pair in values.windows(2) {
        let [step, next] = pair else {
            continue;
        };
        points.push(*step);
        if next.tick.0 - step.tick.0 > 1 {
            points.push(Point {
                tick: Ticks(next.tick.0 - 1),
                value: step.value,
            });
        }
    }
    points.extend(values.last());
    thinned(&points)
}

/// The clip as a file of one track at 960 ticks a quarter, from its start: its notes as they
/// sound, ending at most where the clip does, its pedal and its lanes as steps, each where the
/// value moved by a step of what a keyboard sends, with `bpm` and the time signature
/// `beats`/`beat` at its start. A pedal still down at the end goes up on its last tick, as a
/// take does.
pub fn write(clip: &Clip, title: &str, bpm: f64, (beats, beat): (u8, u8)) -> Vec<u8> {
    let length = clip.length.ticks();
    // Of one tick: the pedal and the lanes first, then the offs, then the ons, as a player
    // sends them.
    let mut events: Vec<(Ticks, u8, TrackEventKind<'_>)> = Vec::new();
    let midi = |message| TrackEventKind::Midi {
        channel: u4::new(0),
        message,
    };
    let micros = (60_000_000.0 / bpm.max(1.0)).round() as u32;
    events.push((
        Ticks(0),
        0,
        TrackEventKind::Meta(MetaMessage::Tempo(u24::new(micros.min(0xFF_FFFF)))),
    ));
    let denominator = beat.max(1).ilog2() as u8;
    events.push((
        Ticks(0),
        0,
        TrackEventKind::Meta(MetaMessage::TimeSignature(beats, denominator, 24, 8)),
    ));
    if !title.is_empty() {
        events.push((
            Ticks(0),
            0,
            TrackEventKind::Meta(MetaMessage::TrackName(title.as_bytes())),
        ));
    }
    let controller = |number: u8, value: u8| {
        midi(MidiMessage::Controller {
            controller: u7::new(number),
            value: u7::new(value),
        })
    };
    let mut pedal = clip.pedal.clone();
    if pedal.last().is_some_and(|change| change.value.is_down()) {
        pedal.push(PedalChange {
            start: Ticks(length.0 - 1),
            value: Pedal::UP,
        });
    }
    for change in pedal_moves(
        &pedal
            .iter()
            .map(|change| (change.start, change.value))
            .collect::<Vec<_>>(),
    ) {
        events.push((
            change.start,
            1,
            controller(Controller::SUSTAIN_PEDAL, change.value.value()),
        ));
    }
    for (tick, value) in steps(&clip.bend, length, true) {
        let bend = PitchBend::from_int(value.value());
        events.push((tick, 1, midi(MidiMessage::PitchBend { bend })));
    }
    for (tick, value) in steps(&clip.mod_wheel, length, true) {
        events.push((tick, 1, controller(Controller::MOD_WHEEL, value.value())));
    }
    for (tick, value) in steps(&clip.pressure, length, true) {
        events.push((
            tick,
            1,
            midi(MidiMessage::ChannelAftertouch {
                vel: u7::new(value.value()),
            }),
        ));
    }
    for (number, points) in &clip.controllers {
        for (tick, value) in steps(points, length, false) {
            events.push((tick, 1, controller(number.number(), value.value())));
        }
    }
    for note in &clip.notes {
        let (key, vel) = (u7::new(note.pitch.number()), u7::new(note.velocity.value()));
        events.push((note.start, 3, midi(MidiMessage::NoteOn { key, vel })));
        let end = note.end().min(length);
        events.push((
            end,
            2,
            midi(MidiMessage::NoteOff {
                key,
                vel: u7::new(0),
            }),
        ));
    }
    events.push((length, 4, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    events.sort_by_key(|(tick, order, _)| (*tick, *order));
    let mut before = Ticks(0);
    let track = events.into_iter().map(|(tick, _, kind)| {
        let delta = u28::new(
            u32::try_from(tick.0 - before.0)
                .unwrap_or(u32::MAX)
                .min(0x0FFF_FFFF),
        );
        before = tick;
        TrackEvent { delta, kind }
    });
    let header = Header::new(
        Format::SingleTrack,
        Timing::Metrical(u15::new(QUARTER as u16)),
    );
    let smf = Smf {
        header,
        tracks: vec![track.collect()],
    };
    let mut bytes = Vec::new();
    // Writing into memory fails only past MIDI's 4 GB chunks, which no clip reaches.
    if smf.write_std(&mut bytes).is_err() {
        bytes.clear();
    }
    bytes
}

/// A lane as steps for a file: its value at each tick where it moved by a step, and at its
/// points. An `expression` lane holds its first value from tick 0 and rests at `length`, as bend
/// plays.
fn steps<V: ExpressionValue>(
    points: &[Point<V>],
    length: Ticks,
    expression: bool,
) -> Vec<(Ticks, V)> {
    let Some(first) = points.first() else {
        return Vec::new();
    };
    let from = if expression { Ticks(0) } else { first.tick };
    let last = points
        .last()
        .map_or(from, |point| point.tick)
        .min(Ticks(length.0.saturating_sub(1)));
    let mut sent: Vec<(Ticks, V)> = Vec::new();
    for tick in from.0..=last.0 {
        let tick = Ticks(tick);
        let Some(value) = value_at(points, tick) else {
            continue;
        };
        let on_point = points
            .binary_search_by_key(&tick, |point| point.tick)
            .is_ok();
        let moved = sent
            .last()
            .is_none_or(|(_, last)| (value.number() - last.number()).abs() >= V::STEP);
        let changed = sent.last().is_none_or(|(_, last)| *last != value);
        if changed && (moved || on_point) {
            sent.push((tick, value));
        }
    }
    if expression && sent.last().is_some_and(|(_, last)| *last != V::REST) {
        sent.push((length, V::REST));
    }
    sent
}
