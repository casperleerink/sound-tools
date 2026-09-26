//! The mixer of a track: gain, pan, mute and solo, after the effects and before the master.
//!
//! The record holds units an agent can reason about (decibels, -1 to 1, on or off). The
//! behaviour of the arrangement turns them into one gain per channel and sends those, so the
//! audio thread works out no pan law. The processor ramps to a new pair, so no change clicks,
//! and keeps the peaks of what it sends on, which is the meter of the track.

use std::f64::consts::{FRAC_PI_2, SQRT_2};

use sound_core::{
    AudioInput, AudioOutput, CHANNELS, Peaks, Ports, PrepareConfig, ProcessContext, Processor,
    Smoothed,
};

use crate::{TrackState, decibels};

/// How long a change of gain, pan or mute takes to arrive. A jump would click.
pub const RAMP_SECONDS: f32 = 0.02;

/// What a track sends its mixer: the gain of each channel, left first.
pub type ChannelGains = [f32; CHANNELS];

/// The gain of each channel of a track, from its record. A track that solo leaves out is
/// silenced by its owner with the gains of a muted one, so the two sound the same.
///
/// The pan law is equal power: the two gains are the sine and the cosine of a quarter turn,
/// scaled so that the middle is exactly 1. A track keeps its loudness wherever it is panned.
/// In the middle it is untouched, so a project from before this existed sounds the same. Hard
/// left or right, the channel that plays it is √2, which is 3 dB above the middle.
pub fn channel_gains(track: &TrackState) -> ChannelGains {
    if track.mute {
        return [0.0; CHANNELS];
    }
    // In f64, so that 0 dB in the middle is exactly 1 and leaves every sample as it was.
    // `-inf` is exactly 0.
    let level = f64::from(decibels::amplitude(track.gain_db));
    let pan = f64::from(track.pan);
    // The part of the quarter turn each channel gets. Both are a sine, so a channel that is
    // panned away is exactly 0 and not a rounding of it.
    let parts = [(1.0 - pan) / 2.0, (1.0 + pan) / 2.0];
    parts.map(|part| (level * SQRT_2 * (part * FRAC_PI_2).sin()) as f32)
}

/// One per track. It multiplies each channel by its gain and ramps to a new one.
pub struct Mixer {
    gains: [Smoothed; CHANNELS],
    /// The frames a change takes. One until `prepare` runs.
    ramp_frames: f32,
    /// What the track sends on, for its meter.
    peaks: Peaks,
}

impl Mixer {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    /// Starts at these gains, so a track that opens or is added is not faded in.
    pub fn new(gains: ChannelGains, peaks: Peaks) -> Self {
        Self {
            gains: gains.map(Smoothed::new),
            ramp_frames: 1.0,
            peaks,
        }
    }
}

impl Processor for Mixer {
    type Update = ChannelGains;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.ramp_frames = (RAMP_SECONDS * config.sample_rate as f32).max(1.0);
    }

    fn update(&mut self, update: &mut ChannelGains) {
        for (gain, target) in self.gains.iter_mut().zip(*update) {
            gain.set_target(target, self.ramp_frames);
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let silent = |gain: &Smoothed| !gain.is_moving() && gain.current() == 0.0;
        if self.gains.iter().all(silent) {
            // A muted track that has finished its fade touches nothing.
            return;
        }
        let frames = context.frames;
        let input = context.audio_inputs.get(Self::INPUT);
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        let outputs = [&mut *left, &mut *right];
        for ((output, input), gain) in outputs.into_iter().zip(input).zip(&mut self.gains) {
            let before = gain.current();
            let step = (gain.advance(frames) - before) / frames as f32;
            for (frame, (output, input)) in output.iter_mut().zip(input).enumerate() {
                *output = input * (before + step * (frame + 1) as f32);
            }
        }
        self.peaks.record_block([&*left, &*right]);
    }
}
