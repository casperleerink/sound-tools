//! The mixer of a track: gain, pan and mute, after the instrument and before the output.
//!
//! The record holds units an agent can reason about (decibels, -1 to 1, on or off). The
//! behaviour of the track turns them into one gain per channel and sends those, so the audio
//! thread works out no pan law. The processor ramps to a new pair, so no change clicks.

use std::f64::consts::{FRAC_PI_2, SQRT_2};

use sound_core::{
    AudioInput, AudioOutput, CHANNELS, Ports, PrepareConfig, ProcessContext, Processor, Smoothed,
};

use crate::TrackState;

/// How long a change of gain, pan or mute takes to arrive. A jump would click.
pub const RAMP_SECONDS: f32 = 0.02;

/// What a track sends its mixer: the gain of each channel, left first.
pub type ChannelGains = [f32; CHANNELS];

/// The gain of each channel of a track, from its record.
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
    let level = 10_f64.powf(f64::from(track.gain_db) / 20.0);
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
}

impl Mixer {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    /// Starts at these gains, so a track that opens or is added is not faded in.
    pub fn new(gains: ChannelGains) -> Self {
        Self {
            gains: gains.map(Smoothed::new),
            ramp_frames: 1.0,
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
        let output = context.audio_outputs.get(Self::OUTPUT);
        for ((output, input), gain) in output.into_iter().zip(input).zip(&mut self.gains) {
            let before = gain.current();
            let step = (gain.advance(frames) - before) / frames as f32;
            for (frame, (output, input)) in output.iter_mut().zip(input).enumerate() {
                *output = input * (before + step * (frame + 1) as f32);
            }
        }
    }
}
