//! The master: the sum of every track, its volume and the limiter that keeps the output under
//! its ceiling. It is part of the arrangement, as the mixer of a track is part of the track:
//! the record of the arrangement holds it, and one processor plays it.
//!
//! The gain of the limiter goes down at once to what a peak needs, so nothing goes over the
//! ceiling, and comes back with the release. With a lookahead it holds the sound back by that
//! time and lowers the gain along a straight line over it, so the gain is already down when
//! the peak arrives and the top of the wave keeps its shape. It then says its lookahead as its
//! latency, so every track is led by it and reaches the device in time. Without one, which is
//! the default, it adds no latency: nothing played live waits for it.
//!
//! Under the ceiling the gain is exactly 1, so the output is the input, sample for sample, one
//! lookahead later. A last clamp at the ceiling catches what rounding might leave over it, and
//! what was planned for a higher ceiling while the ceiling came down. With no lookahead, the
//! rising edge of the first peak over the ceiling is flattened there: that is a hard clip of the
//! edge, and the release then turns the next peaks down whole.

use serde::{Deserialize, Serialize};
use sound_core::{
    AudioInput, AudioOutput, CHANNELS, Peaks, Ports, PrepareConfig, ProcessContext, Processor,
    Smoothed,
};

use crate::decibels;
use crate::mixer::RAMP_SECONDS;

/// The master of an arrangement: its volume and its limiter. A record that leaves it out gets
/// 0 dB and the limiter at its defaults, which is on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MasterState {
    /// The volume of the master, before the limiter, so it can never push the output over the
    /// ceiling. A number up to 6, or `"-inf"` for silence.
    #[serde(with = "decibels")]
    pub gain_db: f32,
    pub limiter: LimiterState,
}

impl MasterState {
    pub const MAX_GAIN_DB: f32 = 6.0;
}

impl Default for MasterState {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            limiter: LimiterState::default(),
        }
    }
}

/// The limiter at the end of the master. Its controls are those of a mastering limiter: how
/// much to push into it, the ceiling, how fast the gain comes back, and how far it looks ahead.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LimiterState {
    /// Off, the sound passes through untouched, one lookahead later: the latency stays, so
    /// switching it never moves a track in time.
    pub bypass: bool,
    /// How much louder the sound goes into the limiter.
    pub gain_db: f32,
    /// The highest the output may reach, in dBFS.
    pub ceiling_db: f32,
    /// How long the gain takes to come back after a peak: the time constant, in milliseconds.
    pub release_ms: f32,
    /// How far ahead the limiter looks, in milliseconds. It is also its latency. 0 looks at
    /// nothing ahead and adds no latency.
    pub lookahead_ms: f32,
}

impl LimiterState {
    /// The ranges, written once: `validate`, the knobs of the card and the docs read them.
    pub const GAIN_DB: (f32, f32) = (0.0, 24.0);
    pub const CEILING_DB: (f32, f32) = (-24.0, 0.0);
    pub const RELEASE_MS: (f32, f32) = (10.0, 1000.0);
    pub const LOOKAHEAD_MS: (f32, f32) = (0.0, 10.0);
}

impl Default for LimiterState {
    fn default() -> Self {
        Self {
            bypass: false,
            gain_db: 0.0,
            // Full scale: the output never clips, and a project that never went over full
            // scale renders as it did, sample for sample.
            ceiling_db: 0.0,
            release_ms: 100.0,
            // None: a keyboard played into a track and a preview note wait for nothing, and a
            // project renders on the frames it always did.
            lookahead_ms: 0.0,
        }
    }
}

impl MasterState {
    pub(crate) fn validate(&self) -> Result<(), String> {
        decibels::check("master.gain_db", self.gain_db, Self::MAX_GAIN_DB)?;
        let limiter = &self.limiter;
        let ranges = [
            ("gain_db", limiter.gain_db, LimiterState::GAIN_DB),
            ("ceiling_db", limiter.ceiling_db, LimiterState::CEILING_DB),
            ("release_ms", limiter.release_ms, LimiterState::RELEASE_MS),
            (
                "lookahead_ms",
                limiter.lookahead_ms,
                LimiterState::LOOKAHEAD_MS,
            ),
        ];
        for (field, value, range) in ranges {
            crate::in_range(&format!("master.limiter.{field}"), value, range)?;
        }
        Ok(())
    }

    /// What the processor needs, worked out on the control thread.
    pub(crate) fn settings(&self) -> MasterSettings {
        let limiter = &self.limiter;
        MasterSettings {
            volume: decibels::amplitude(self.gain_db),
            bypass: limiter.bypass,
            gain: decibels::amplitude(limiter.gain_db),
            ceiling: decibels::amplitude(limiter.ceiling_db),
            release_seconds: limiter.release_ms / 1000.0,
            lookahead_seconds: limiter.lookahead_ms / 1000.0,
        }
    }
}

/// What the master processor is sent: every value as a factor or in seconds.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct MasterSettings {
    pub volume: f32,
    pub bypass: bool,
    pub gain: f32,
    pub ceiling: f32,
    pub release_seconds: f32,
    pub lookahead_seconds: f32,
}

/// Room for the longest lookahead at any sample rate the engine takes.
fn capacity(sample_rate: f32) -> usize {
    (LimiterState::LOOKAHEAD_MS.1 / 1000.0 * sample_rate).ceil() as usize + 1
}

/// The one processor of the master: volume, limiter, and the peaks of what it sends out.
pub(crate) struct Master {
    settings: MasterSettings,
    volume: Smoothed,
    gain: Smoothed,
    sample_rate: f32,
    ramp_frames: f32,
    /// Frames of lookahead, which is the latency.
    lookahead: usize,
    /// The sound of the last `lookahead` frames, per channel, and where the next one goes.
    delay: [Vec<f32>; CHANNELS],
    /// The lowest gain any frame of the window asks for, oldest first: a queue of (frame,
    /// gain) in which the gains rise, so its front is the lowest in the window.
    lowest: Vec<(u64, f32)>,
    lowest_start: usize,
    lowest_len: usize,
    /// The gain after the release, over the last `lookahead` frames, and their sum. Their mean
    /// is the gain applied, which turns each drop into a straight line over the lookahead.
    smoothing: Vec<f32>,
    smoothing_sum: f64,
    /// Where the next frame goes in `delay` and in `smoothing`.
    position: usize,
    /// Frames seen, to age the queue.
    frame: u64,
    /// Frames in a row whose gain after the release was exactly 1. Once there are a lookahead of
    /// them, the mean is exactly 1 and the sum is set to what it is, so no drift of it can stay.
    unity_run: usize,
    /// In f64: near 1 a release step of an f32 would be less than half of its last bit, and the
    /// gain would stop just under 1 for good.
    envelope: f64,
    release_factor: f64,
    peaks: Peaks,
    /// The largest reduction of each block, as the factor the input was above the output.
    reduction: Peaks,
}

impl Master {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    /// Starts at these settings, so a project that opens is not faded in.
    pub fn new(settings: MasterSettings, peaks: Peaks, reduction: Peaks) -> Self {
        Self {
            settings,
            volume: Smoothed::new(settings.volume),
            gain: Smoothed::new(settings.gain),
            sample_rate: 0.0,
            ramp_frames: 1.0,
            lookahead: 0,
            delay: [Vec::new(), Vec::new()],
            lowest: Vec::new(),
            lowest_start: 0,
            lowest_len: 0,
            smoothing: Vec::new(),
            smoothing_sum: 0.0,
            position: 0,
            frame: 0,
            unity_run: 0,
            envelope: 1.0,
            release_factor: 1.0,
            peaks,
            reduction,
        }
    }

    fn lookahead_of(&self, settings: &MasterSettings) -> usize {
        let frames = (settings.lookahead_seconds * self.sample_rate).round() as usize;
        frames.min(self.delay[0].len())
    }

    /// Forgets what the limiter was doing: no reduction, and silence in the delay.
    fn reset(&mut self) {
        for channel in &mut self.delay {
            channel.fill(0.0);
        }
        self.reset_gain();
    }

    /// No reduction: the window of gains is empty and the mean is exactly 1.
    fn reset_gain(&mut self) {
        let lookahead = self.lookahead;
        self.smoothing
            .iter_mut()
            .take(lookahead)
            .for_each(|gain| *gain = 1.0);
        self.smoothing_sum = lookahead as f64;
        self.lowest_start = 0;
        self.lowest_len = 0;
        self.unity_run = lookahead;
        self.envelope = 1.0;
    }

    /// The lowest gain the last `lookahead + 1` frames ask for, with this frame's.
    fn lowest_with(&mut self, frame: u64, gain: f32) -> f32 {
        let capacity = self.lowest.len();
        if capacity == 0 {
            return gain;
        }
        // Gains at the back that are not lower than this one can never be the lowest again.
        while self.lowest_len > 0 {
            let back = (self.lowest_start + self.lowest_len - 1) % capacity;
            if self
                .lowest
                .get(back)
                .is_some_and(|(_, lowest)| *lowest >= gain)
            {
                self.lowest_len -= 1;
            } else {
                break;
            }
        }
        let back = (self.lowest_start + self.lowest_len) % capacity;
        if let Some(entry) = self.lowest.get_mut(back) {
            *entry = (frame, gain);
            self.lowest_len += 1;
        }
        // Out of the window at the front.
        let oldest = frame.saturating_sub(self.lookahead as u64);
        while self.lowest_len > 1
            && self
                .lowest
                .get(self.lowest_start)
                .is_some_and(|(at, _)| *at < oldest)
        {
            self.lowest_start = (self.lowest_start + 1) % capacity;
            self.lowest_len -= 1;
        }
        self.lowest
            .get(self.lowest_start)
            .map_or(gain, |(_, lowest)| *lowest)
    }
}

impl Master {
    /// One frame of the gain computer: the gain that the frame one lookahead back gets, for a
    /// frame whose loudest channel is `peak` now.
    fn gain_for(&mut self, peak: f32, ceiling: f32) -> f32 {
        let wanted = if peak > ceiling { ceiling / peak } else { 1.0 };
        let lowest = f64::from(self.lowest_with(self.frame, wanted));
        // The release: down at once, back up along the time constant.
        self.envelope = match lowest < self.envelope {
            true => lowest,
            false => {
                let next = self.envelope + (lowest - self.envelope) * self.release_factor;
                match lowest - next < CLOSE_ENOUGH || next <= self.envelope {
                    true => lowest,
                    false => next,
                }
            }
        };
        let envelope = self.envelope as f32;
        let lookahead = self.lookahead;
        if lookahead == 0 {
            return envelope;
        }
        if let Some(oldest) = self.smoothing.get_mut(self.position) {
            self.smoothing_sum += f64::from(envelope) - f64::from(*oldest);
            *oldest = envelope;
        }
        self.unity_run = match envelope == 1.0 {
            true => self.unity_run.saturating_add(1),
            false => 0,
        };
        if self.unity_run >= lookahead {
            self.smoothing_sum = lookahead as f64;
            return 1.0;
        }
        ((self.smoothing_sum / lookahead as f64) as f32).min(1.0)
    }
}

impl Processor for Master {
    type Update = MasterSettings;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.ramp_frames = (RAMP_SECONDS * self.sample_rate).max(1.0);
        let capacity = capacity(self.sample_rate);
        self.delay = [vec![0.0; capacity], vec![0.0; capacity]];
        self.lowest = vec![(0, 1.0); capacity + 1];
        self.smoothing = vec![1.0; capacity];
        let settings = self.settings;
        self.lookahead = self.lookahead_of(&settings);
        self.release_factor = release_factor(settings.release_seconds, self.sample_rate);
        self.reset();
    }

    fn update(&mut self, update: &mut MasterSettings) {
        let settings = *update;
        self.volume.set_target(settings.volume, self.ramp_frames);
        self.gain.set_target(settings.gain, self.ramp_frames);
        self.release_factor = release_factor(settings.release_seconds, self.sample_rate);
        let lookahead = self.lookahead_of(&settings);
        if lookahead != self.lookahead {
            // Another latency: the engine leads every track by the new one from this block on,
            // and they start again from where they now have to be. The delay starts empty.
            self.lookahead = lookahead;
            self.position = 0;
            self.reset();
        }
        // A bypass keeps the gain it worked out while it was off, so the frames already in the
        // delay come out with the gain they need when it is switched on again.
        self.settings = settings;
    }

    fn latency(&self) -> u32 {
        u32::try_from(self.lookahead).unwrap_or(u32::MAX)
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let frames = context.frames;
        let [input_left, input_right] = context.audio_inputs.get(Self::INPUT);
        let [output_left, output_right] = context.audio_outputs.get(Self::OUTPUT);
        let volume_before = self.volume.current();
        let volume_step = (self.volume.advance(frames) - volume_before) / frames as f32;
        let gain_before = self.gain.current();
        let gain_step = (self.gain.advance(frames) - gain_before) / frames as f32;
        let MasterSettings {
            bypass, ceiling, ..
        } = self.settings;
        let lookahead = self.lookahead;
        let mut most_reduced = 1.0_f32;
        let samples = input_left.iter().zip(input_right);
        let outputs = output_left.iter_mut().zip(output_right.iter_mut());
        for (index, ((left, right), (out_left, out_right))) in samples.zip(outputs).enumerate() {
            let step = (index + 1) as f32;
            let volume = volume_before + volume_step * step;
            let gain = gain_before + gain_step * step;
            // Not a number and infinity are no sound. They would come out as full scale after
            // the clamp, since a comparison drops a not-a-number.
            let finite = |sample: f32| {
                if sample.is_finite() {
                    sample * volume
                } else {
                    0.0
                }
            };
            let (left, right) = (finite(*left), finite(*right));
            let limited = [left * gain, right * gain];
            // The gain works on the whole time, also while the limiter is bypassed.
            let applied = self.gain_for(limited[0].abs().max(limited[1].abs()), ceiling);
            let position = self.position;
            let mut delayed = match bypass {
                true => [left, right],
                false => limited,
            };
            if lookahead > 0 {
                for (channel, sample) in self.delay.iter_mut().zip(&mut delayed) {
                    if let Some(slot) = channel.get_mut(position) {
                        *sample = std::mem::replace(slot, *sample);
                    }
                }
            }
            let (left, right) = match bypass {
                true => (delayed[0], delayed[1]),
                false => {
                    most_reduced = most_reduced.min(applied);
                    let limit = |sample: f32| (sample * applied).max(-ceiling).min(ceiling);
                    (limit(delayed[0]), limit(delayed[1]))
                }
            };
            *out_left = left;
            *out_right = right;
            self.position = (position + 1) % lookahead.max(1);
            self.frame += 1;
        }
        self.peaks.record_block([&*output_left, &*output_right]);
        if most_reduced < 1.0 {
            self.reduction.record(0, 1.0 / most_reduced);
        }
    }
}

/// How much of the way back the gain goes per frame, for a time constant of `seconds`.
fn release_factor(seconds: f32, sample_rate: f32) -> f64 {
    let frames = f64::from(seconds) * f64::from(sample_rate);
    if frames <= 0.0 {
        return 1.0;
    }
    1.0 - (-1.0 / frames).exp()
}

/// Within this of its target the gain goes the rest of the way at once: 0.001 dB, far under
/// what anyone hears, and it makes the gain exactly 1 again after a peak.
const CLOSE_ENOUGH: f64 = 1e-4;
