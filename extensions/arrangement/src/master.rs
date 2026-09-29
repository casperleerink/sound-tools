//! The master: the sum of every track, its volume and the limiter that keeps the output under
//! its ceiling. It is part of the arrangement, as the mixer of a track is part of the track:
//! the record of the arrangement holds it, and one processor plays it.
//!
//! The limiter is [`PeakLimiter`] of the core, which the Limiter effect uses too; its doc says
//! how it works. It says its lookahead as the latency of the master, so every track is led by
//! it and reaches the device in time. Without one, which is the default, it adds no latency:
//! nothing played live waits for it.

use serde::{Deserialize, Serialize};
use sound_core::{
    AudioInput, AudioOutput, PeakLimiter, Peaks, Ports, PrepareConfig, ProcessContext, Processor,
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

/// The one processor of the master: volume, limiter, and the peaks of what it sends out.
pub(crate) struct Master {
    settings: MasterSettings,
    volume: Smoothed,
    gain: Smoothed,
    ramp_frames: f32,
    limiter: PeakLimiter,
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
            ramp_frames: 1.0,
            limiter: PeakLimiter::new(),
            peaks,
            reduction,
        }
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
        let sample_rate = config.sample_rate as f32;
        self.ramp_frames = (RAMP_SECONDS * sample_rate).max(1.0);
        let longest = LimiterState::LOOKAHEAD_MS.1 / 1000.0;
        self.limiter.prepare(sample_rate, longest);
        self.limiter.set_lookahead(self.settings.lookahead_seconds);
        self.limiter.set_release(self.settings.release_seconds);
    }

    fn update(&mut self, update: &mut MasterSettings) {
        let settings = *update;
        self.volume.set_target(settings.volume, self.ramp_frames);
        self.gain.set_target(settings.gain, self.ramp_frames);
        self.limiter.set_release(settings.release_seconds);
        self.limiter.set_lookahead(settings.lookahead_seconds);
        // A bypass keeps the gain it worked out while it was off, so the frames already in the
        // delay come out with the gain they need when it is switched on again.
        self.settings = settings;
    }

    fn latency(&self) -> u32 {
        u32::try_from(self.limiter.latency()).unwrap_or(u32::MAX)
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
            let peak = limited[0].abs().max(limited[1].abs());
            let through = match bypass {
                true => [left, right],
                false => limited,
            };
            let (delayed, applied) = self.limiter.next(peak, through, ceiling);
            [*out_left, *out_right] = match bypass {
                true => delayed,
                false => {
                    most_reduced = most_reduced.min(applied);
                    PeakLimiter::limit(delayed, applied, ceiling)
                }
            };
        }
        self.peaks.record_block([&*output_left, &*output_right]);
        if most_reduced < 1.0 {
            self.reduction.record(0, 1.0 / most_reduced);
        }
    }
}
