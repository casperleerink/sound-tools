//! The pitch correction processor: a pitch tracker, the note the voice is moved to, how fast,
//! and the [`Shifter`] that moves it.
//!
//! The target is the allowed note nearest to what the voice sings, in any octave, so a
//! correction is never more than half an octave and a note read an octave off still finds the
//! right one. The target held keeps a little of its pull ([`STICK`]), so a voice between two
//! notes does not flip from one to the other. Held notes on `notes` are the allowed notes in
//! every octave, and they apply to the sound sung when they changed: the readings describe the
//! voice a little back, so the changes wait for them.
//!
//! The correction, target less sung, in semitones, moves to its goal like a capacitor charging:
//! most of the way in the retune time. A slow retune follows the average of a vibrato and
//! leaves the vibrato; a retune of 0 takes the goal at once and flattens it. Through a short
//! moment without a clear pitch, as in a breath inside a note, the goal holds; after that, and
//! in a chord or noise, it goes back to 0, so nothing without a pitch is moved.
//!
//! Each voice range has its own tracker, made before the processor reaches the audio thread.
//! A change of range starts the new one listening next to the old, and switches once it has
//! heard enough to read a note.

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, DelayLine, EventInput, Ports,
    PrepareConfig, ProcessContext, Processor, Smoothed, Targets, Watch, all_held_silent, held,
};
use sound_media::analysis::{Pitch, PitchSettings, PitchTracker};
use sound_media::{Shift, Shifter};
use sound_notes::NoteEvent;

use crate::{MIX, PARAMETERS, PitchCorrectionState, Voice};

type CorrectionTargets = Targets<PitchCorrectionState, { PARAMETERS.len() }>;

/// How clearly the voice must repeat to read as a note: between strict and tolerant.
const TOLERANCE: f64 = 0.5;
/// How long a change of the mix takes.
const RAMP_SECONDS: f32 = 0.02;
/// How long the goal holds through a moment without a clear pitch.
const HOLD_SECONDS: f64 = 0.05;
/// The retune time reaches its own value in a third of it, so it is 95 % of the way there.
const RETUNE_PARTS: f64 = 3.0;
/// The longest a note's retune time grows for with humanize.
const HUMANIZE_SECONDS: f64 = 0.4;
/// How much nearer, in semitones, another allowed note must be than the target to take over.
const STICK: f64 = 0.1;
/// The frames from one working out of how fast the correction moves to the next, counted from
/// the start, so a render is the same in any buffers.
const KEEPING_FRAMES: u64 = 64;
/// The changes of held notes that may wait for the readings at once.
const WAITING: usize = 32;

/// What the card shows, from the audio thread: the note heard (a MIDI note number with its
/// fraction), the note it is moved to, and how far it is moved now, in semitones. A note is 0
/// while none is heard.
#[derive(Clone, Debug, Default)]
pub struct Meters {
    pub heard: Watch,
    pub target: Watch,
    pub correction: Watch,
}

impl Meters {
    /// The names the behaviour keeps them under, for [`sound_core::Project::watched`].
    pub const HEARD: &str = "heard";
    pub const TARGET: &str = "target";
    pub const CORRECTION: &str = "correction";
}

pub struct PitchCorrection {
    /// The record, with the values of the lanes that automate it.
    state: Automated<PitchCorrectionState, { PARAMETERS.len() }>,
    meters: Meters,
    rate: f64,
    ramp_frames: f32,
    mix: Smoothed,
    /// A tracker for each of [`Voice::ALL`], the one the correction reads, and the one that
    /// listens until it can take over, with the frames it has heard.
    trackers: Box<[PitchTracker]>,
    listening: usize,
    starting: Option<(usize, usize)>,
    shifter: Shifter,
    /// The input, as late as the shifter, for the mix.
    dry: [DelayLine; 2],
    latency: usize,
    /// Frames heard since the processor started: where each held note changes.
    frame: u64,
    /// The ons of each pitch held on `notes`, the pitch classes held, one bit per semitone
    /// above C, and those that the readings have reached.
    ons: [u32; 128],
    held: u16,
    reached: u16,
    /// The changes of the held pitch classes the readings have not reached yet, oldest first,
    /// from `waiting_from`.
    waiting: [(u64, u16); WAITING],
    waiting_from: usize,
    waiting_count: usize,
    /// The note moved to, how long it has been, and how many frames have had no clear pitch.
    target: Option<i32>,
    target_frames: u64,
    unclear_frames: usize,
    heard: Option<f64>,
    /// Where the correction goes, where it is, in semitones, and the part of its distance to
    /// the goal it keeps per frame.
    goal: f64,
    correction: f64,
    keeping: f64,
    /// Frames in a row of silent input.
    quiet: usize,
    rest_frames: usize,
}

impl PitchCorrection {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    /// Notes held here are the notes the voice is moved to, in place of the key and scale.
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const AUTOMATION: AutomationInput<PitchCorrectionState, { PARAMETERS.len() }> =
        AutomationInput::new(1, PARAMETERS);

    pub fn new(state: PitchCorrectionState, meters: Meters) -> Self {
        let rate = 48_000.0;
        let (trackers, shifter) = Self::make(rate);
        let latency = shifter.latency();
        let mut correction = Self {
            state: Automated::new(Self::AUTOMATION, state),
            meters,
            rate,
            ramp_frames: 1.0,
            mix: Smoothed::new(state.mix),
            listening: voice_index(state.voice),
            starting: None,
            dry: [DelayLine::new(latency), DelayLine::new(latency)],
            rest_frames: Self::rest_frames(&trackers, latency),
            trackers,
            shifter,
            latency,
            frame: 0,
            ons: [0; 128],
            held: 0,
            reached: 0,
            waiting: [(0, 0); WAITING],
            waiting_from: 0,
            waiting_count: 0,
            target: None,
            target_frames: 0,
            unclear_frames: 0,
            heard: None,
            goal: 0.0,
            correction: 0.0,
            keeping: 0.0,
            quiet: 0,
        };
        correction.shifter.keep_formants(state.keep_formants);
        correction
    }

    /// The trackers of every voice range at `rate`, and a shifter for the lowest of them.
    fn make(rate: f64) -> (Box<[PitchTracker]>, Shifter) {
        let trackers: Box<[PitchTracker]> = (Voice::ALL.iter())
            .map(|voice| {
                let (lowest, highest) = voice.range_hz();
                let settings = PitchSettings {
                    lowest,
                    highest,
                    tolerance: TOLERANCE,
                };
                PitchTracker::new(rate, settings)
            })
            .collect();
        let lowest = (Voice::ALL.iter())
            .map(|voice| voice.range_hz().0)
            .fold(f64::MAX, f64::min);
        let oldest = trackers.iter().map(PitchTracker::age).fold(0.0, f64::max);
        (trackers, Shifter::new(rate, rate / lowest, oldest))
    }

    /// Silent frames after which the trackers, the shifter and the mix hold only silence.
    fn rest_frames(trackers: &[PitchTracker], latency: usize) -> usize {
        let longest = trackers.iter().map(PitchTracker::length).max().unwrap_or(0);
        2 * (latency + longest)
    }

    /// Sets the targets from the record and its lanes.
    fn aim(&mut self, targets: &CorrectionTargets) {
        let state = *self.state;
        self.mix.set_target(state.mix, targets.ramp(&MIX));
        if targets.snaps() {
            self.mix.snap();
        }
        self.shifter.keep_formants(state.keep_formants);
        self.keeping = self.keeping_now();
        let wanted = voice_index(state.voice);
        let starting = self.starting.map(|(index, _)| index);
        if wanted == self.listening {
            self.starting = None;
        } else if starting != Some(wanted) {
            self.starting = Some((wanted, 0));
        }
    }

    /// Hears the notes of this block: each change of the held pitch classes waits for the
    /// readings of its frame.
    fn hear_notes(&mut self, context: &ProcessContext<'_>) {
        for timed in context.event_inputs.get(Self::NOTES) {
            match timed.event {
                NoteEvent::On { pitch, .. } => {
                    if let Some(ons) = self.ons.get_mut(usize::from(pitch.number())) {
                        *ons = ons.saturating_add(1);
                    }
                }
                NoteEvent::Off { pitch } => {
                    if let Some(ons) = self.ons.get_mut(usize::from(pitch.number())) {
                        *ons = ons.saturating_sub(1);
                    }
                }
                NoteEvent::AllOff => self.ons = [0; 128],
                _ => continue,
            }
            let held = (self.ons.iter().enumerate())
                .filter(|(_, ons)| **ons > 0)
                .fold(0_u16, |held, (pitch, _)| held | 1 << (pitch % 12));
            if held == self.held {
                continue;
            }
            self.held = held;
            if self.waiting_count == WAITING {
                // More changes than wait at once: the oldest applies now.
                self.reach_oldest();
            }
            let at = (self.waiting_from + self.waiting_count) % WAITING;
            if let Some(slot) = self.waiting.get_mut(at) {
                *slot = (self.frame + timed.offset as u64, held);
                self.waiting_count += 1;
            }
        }
    }

    fn reach_oldest(&mut self) {
        if let Some((_, held)) = self.waiting.get(self.waiting_from) {
            self.reached = *held;
        }
        self.waiting_from = (self.waiting_from + 1) % WAITING;
        self.waiting_count -= 1;
    }

    /// The notes allowed for the sound of `frame`.
    fn allowed(&mut self, frame: u64) -> u16 {
        while self.waiting_count > 0
            && self
                .waiting
                .get(self.waiting_from)
                .is_some_and(|(at, _)| *at <= frame)
        {
            self.reach_oldest();
        }
        match self.reached {
            0 => self.state.scale.notes(self.state.key),
            held => held,
        }
    }

    /// The factor the correction keeps of its distance to the goal per frame, for a target held
    /// so long.
    fn keeping_now(&self) -> f64 {
        let state = *self.state;
        let held = self.target_frames as f64 / self.rate;
        let seconds = f64::from(state.retune_ms) / 1_000.0
            + f64::from(state.humanize) * held.min(HUMANIZE_SECONDS);
        let frames = seconds / RETUNE_PARTS * self.rate;
        match frames < 1.0 {
            true => 0.0,
            false => (-1.0 / frames).exp(),
        }
    }

    /// Reads the tracker after a frame: the goal, the correction a frame on towards it, and how
    /// to shift the sound the reading describes.
    fn read(&mut self, offset: usize) -> Shift {
        let Some(tracker) = self.trackers.get(self.listening) else {
            return Shift::NONE;
        };
        let (pitch, age) = (tracker.pitch(), tracker.age());
        let described = (self.frame + offset as u64).saturating_sub(age as u64);
        let allowed = self.allowed(described);
        let period = match pitch {
            Pitch::Note(note) => {
                self.unclear_frames = 0;
                self.heard = Some(note);
                let target = nearest_allowed(note, allowed, self.target);
                match target == self.target {
                    true => self.target_frames += 1,
                    false => self.target_frames = 0,
                }
                self.target = target;
                self.goal = target.map_or(0.0, |target| f64::from(target) - note);
                let hz = 440.0 * 2_f64.powf((note - 69.0) / 12.0);
                Some(self.rate / hz)
            }
            Pitch::Quiet | Pitch::Unclear => {
                self.unclear_frames += 1;
                if self.unclear_frames as f64 > HOLD_SECONDS * self.rate {
                    (self.target, self.heard, self.goal) = (None, None, 0.0);
                }
                None
            }
        };
        self.correction = self.goal + self.keeping * (self.correction - self.goal);
        Shift {
            period,
            ratio: 2_f64.powf(self.correction / 12.0),
            age,
        }
    }

    /// Hears one frame of the voice in the tracker that reads, and in one that starts.
    fn listen(&mut self, mono: f32) {
        if let Some(tracker) = self.trackers.get_mut(self.listening) {
            tracker.push([mono]);
        }
        let Some((index, heard)) = &mut self.starting else {
            return;
        };
        let Some(tracker) = self.trackers.get_mut(*index) else {
            self.starting = None;
            return;
        };
        tracker.push([mono]);
        *heard += 1;
        if *heard >= tracker.length() {
            self.listening = *index;
            self.starting = None;
        }
    }

    fn show(&self) {
        let note = |note: Option<f64>| note.unwrap_or(0.0) as f32;
        self.meters.heard.set(note(self.heard));
        self.meters.target.set(note(self.target.map(f64::from)));
        self.meters.correction.set(match self.heard {
            Some(_) => self.correction as f32,
            None => 0.0,
        });
    }
}

/// The index of `voice` in [`Voice::ALL`].
fn voice_index(voice: Voice) -> usize {
    Voice::ALL
        .iter()
        .position(|each| *each == voice)
        .unwrap_or(0)
}

/// The note `allowed` (one bit per semitone above C) holds nearest to `note`, a MIDI note
/// number with its fraction, in any octave. The `held` target stays unless another is nearer
/// by more than [`STICK`]. `None` when nothing is allowed.
fn nearest_allowed(note: f64, allowed: u16, held: Option<i32>) -> Option<i32> {
    let is_allowed = |candidate: i32| allowed & (1 << candidate.rem_euclid(12)) != 0;
    let middle = note.round() as i32;
    let nearest = (middle - 6..=middle + 6)
        .filter(|candidate| is_allowed(*candidate))
        .min_by(|a, b| {
            let distance = |candidate: &i32| (f64::from(*candidate) - note).abs();
            distance(a).total_cmp(&distance(b))
        })?;
    match held {
        Some(held)
            if is_allowed(held)
                && (f64::from(held) - note).abs() < (f64::from(nearest) - note).abs() + STICK =>
        {
            Some(held)
        }
        _ => Some(nearest),
    }
}

impl Processor for PitchCorrection {
    type Update = PitchCorrectionState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::NOTES)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        let rate = f64::from(config.sample_rate);
        self.ramp_frames = (RAMP_SECONDS * config.sample_rate as f32).max(1.0);
        if rate == self.rate {
            return;
        }
        self.rate = rate;
        (self.trackers, self.shifter) = Self::make(rate);
        self.shifter.keep_formants(self.state.keep_formants);
        self.latency = self.shifter.latency();
        self.dry = [DelayLine::new(self.latency), DelayLine::new(self.latency)];
        self.rest_frames = Self::rest_frames(&self.trackers, self.latency);
    }

    fn update(&mut self, update: &mut PitchCorrectionState) {
        let targets = self.state.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn latency(&self) -> u32 {
        self.latency as u32
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        self.hear_notes(context);
        let frames = context.frames;
        let [left, right] = context.audio_inputs.get(Self::INPUT);
        if all_held_silent(left) && all_held_silent(right) && self.quiet >= self.rest_frames {
            // Everything holds silence: so does the output.
            self.frame += frames as u64;
            self.mix.snap();
            return;
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let outputs = left_out.iter_mut().zip(right_out.iter_mut());
        for (offset, ((left, right), (left_out, right_out))) in
            left.iter().zip(right).zip(outputs).enumerate()
        {
            let frame = [held(*left), held(*right)];
            // Counted per frame, not per block, so where the rest starts and ends, and so the
            // render, is the same in any buffers.
            self.quiet = match frame == [0.0; 2] {
                true => self.quiet.saturating_add(1),
                false => 0,
            };
            if self.quiet > self.rest_frames {
                continue;
            }
            if (self.frame + offset as u64) % KEEPING_FRAMES == 0 {
                self.keeping = self.keeping_now();
            }
            self.listen(0.5 * (frame[0] + frame[1]));
            let shift = self.read(offset);
            self.shifter.push(frame, shift);
            let wet = self.shifter.pull();
            let position = (self.frame + offset as u64) as usize;
            let mix = self.mix.advance(1);
            for ((line, sample), (wet, out)) in (self.dry.iter_mut().zip(frame))
                .zip(wet.into_iter().zip([&mut *left_out, &mut *right_out]))
            {
                line.write(position, sample);
                let dry = line.read(position, self.latency);
                *out = dry + mix * (wet - dry);
            }
        }
        self.frame += frames as u64;
        self.show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_allowed_note_is_in_any_octave_and_a_held_target_keeps_a_little_pull() {
        let c_major = crate::Scale::Major.notes(crate::Key::C);
        // C#4 and a little up: D4 is nearer than C4.
        assert_eq!(nearest_allowed(61.2, c_major, None), Some(62));
        // Only A held: from E4, A4 is 5 semitones up and A3 7 down.
        assert_eq!(nearest_allowed(64.0, 1 << 9, None), Some(69));
        // Half way from C4 to D4, a little nearer D4: C4 held stays, until D4 is nearer by more.
        assert_eq!(nearest_allowed(61.05, c_major, Some(60)), Some(60));
        assert_eq!(nearest_allowed(61.15, c_major, Some(60)), Some(62));
        assert_eq!(nearest_allowed(61.0, 0, None), None);
    }
}
