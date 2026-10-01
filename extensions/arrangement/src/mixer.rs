//! The mixer of a track: gain, pan, mute and solo, after the effects and before the master.
//!
//! The record holds units an agent can reason about (decibels, -1 to 1, on or off). The
//! behaviour of the arrangement sends the volume and the pan, and whether mute or solo silence
//! the track. The volume and the pan are automatable like the numbers of a device: lanes of the
//! track reach the mixer as automation events. The processor works out one gain per channel
//! when something moves and ramps to the new pair, so no change clicks, and keeps the peaks of
//! what it sends on, which is the meter of the track.

use sound_core::{
    AudioInput, AudioOutput, Automated, Automation, AutomationRamp, CHANNELS, EventInput,
    Parameter, Peaks, Ports, PrepareConfig, ProcessContext, Processor, Scale, Smoothed, amplitude,
    pan_gains,
};

use crate::TrackState;

/// How long a change of gain, pan or mute takes to arrive. A jump would click.
pub const RAMP_SECONDS: f32 = 0.02;

/// The gain of each channel, left first.
pub type ChannelGains = [f32; CHANNELS];

/// What a track sends its mixer.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Mix {
    pub gain_db: f32,
    pub pan: f32,
    /// Muted, or left out by a solo. A track that solo leaves out sounds as a muted one does.
    pub silent: bool,
}

impl Mix {
    /// The mix of a track record. Its owner silences it when a solo leaves it out.
    pub fn of(track: &TrackState) -> Self {
        Self {
            gain_db: track.gain_db,
            pan: track.pan,
            silent: track.mute,
        }
    }

    /// The gain of each channel.
    ///
    /// The pan law is the equal power law of the SDK, [`pan_gains`]: a track keeps its
    /// loudness wherever it is panned, and in the middle it is untouched, so a project from
    /// before this existed sounds the same.
    pub fn gains(&self) -> ChannelGains {
        if self.silent {
            return [0.0; CHANNELS];
        }
        pan_gains(f64::from(amplitude(self.gain_db)), self.pan)
    }
}

/// The gain of each channel of a track, from its record.
pub fn channel_gains(track: &TrackState) -> ChannelGains {
    Mix::of(track).gains()
}

/// The volume of a track, as a lane of the track moves it: on the scale of its fader.
pub(crate) const GAIN: Parameter<Mix> = Parameter {
    field: "gain_db",
    min: f32::NEG_INFINITY,
    max: TrackState::MAX_GAIN_DB,
    default: 0.0,
    scale: Scale::Fader,
    get: |mix| mix.gain_db,
    set: |mix, value| mix.gain_db = value,
};
pub(crate) const PAN: Parameter<Mix> = Parameter {
    field: "pan",
    min: TrackState::PAN.0,
    max: TrackState::PAN.1,
    default: 0.0,
    scale: Scale::Linear,
    get: |mix| mix.pan,
    set: |mix, value| mix.pan = value,
};

/// What a lane of the track itself can move, in the order of the index of its events.
pub(crate) const MIX_PARAMETERS: [&Parameter<Mix>; 2] = [&GAIN, &PAN];

/// One per track. It multiplies each channel by its gain and ramps to a new one.
pub struct Mixer {
    /// The record, with the values of the lanes of the track.
    mix: Automated<Mix, { MIX_PARAMETERS.len() }>,
    gains: [Smoothed; CHANNELS],
    /// The frames a change takes. One until `prepare` runs.
    ramp_frames: f32,
    /// What the track sends on, for its meter.
    peaks: Peaks,
}

impl Mixer {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: EventInput<Automation> = EventInput::new(0);

    /// Starts at this mix, so a track that opens or is added is not faded in.
    pub fn new(mix: Mix, peaks: Peaks) -> Self {
        Self {
            mix: Automated::new(MIX_PARAMETERS, mix),
            gains: mix.gains().map(Smoothed::new),
            ramp_frames: 1.0,
            peaks,
        }
    }

    /// Aims at the gains of the record and its lanes, reached in `ramp` frames.
    fn aim(&mut self, ramp: f32) {
        for (gain, target) in self.gains.iter_mut().zip(self.mix.state().gains()) {
            gain.set_target(target, ramp);
        }
    }
}

impl Processor for Mixer {
    type Update = Mix;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.ramp_frames = (RAMP_SECONDS * config.sample_rate as f32).max(1.0);
    }

    fn update(&mut self, update: &mut Mix) {
        self.mix.set_record(*update);
        self.aim(self.ramp_frames);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        match self.mix.follow(context.event_inputs.get(Self::AUTOMATION)) {
            Some(AutomationRamp::Block) => self.aim(context.frames as f32),
            Some(AutomationRamp::Edit) => self.aim(self.ramp_frames),
            None => {}
        }
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
