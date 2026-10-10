//! The mixer of a track: gain, pan, mute and solo, after the effects and before the master.
//!
//! The record holds units an agent can reason about (decibels, -1 to 1, on or off). The
//! behaviour of the arrangement sends the volume and the pan, and whether mute or solo silence
//! the track. The volume and the pan are automatable like the numbers of a device: lanes of the
//! track reach the mixer as automation events. The processor works out one gain per channel
//! when something moves and ramps to the new pair, so no change clicks, and keeps the peaks of
//! what it sends on, which is the meter of the track.

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, CHANNELS, Parameter, Peaks, Ports,
    PrepareConfig, ProcessContext, Processor, Scale, Smoothed, Targets, all_positive_zero,
    amplitude, pan_gains,
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
        pan_gains(f64::from(self.level()), self.pan)
    }

    /// The factor of the volume, 0 when silent.
    fn level(&self) -> f32 {
        match self.silent {
            true => 0.0,
            false => amplitude(self.gain_db),
        }
    }
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

/// One per track. It multiplies each channel by its gain and ramps to a new one.
///
/// The volume and the pan glide apart, each with its own ramp, and the gains of the channels
/// are worked out from where both are at the end of each block. So a pan lane that moves does
/// not cut short the glide of a volume lane that just took over.
pub struct Mixer {
    /// The record, with the values of the lanes of the track.
    mix: Automated<Mix, 2>,
    /// The factor of the volume, 0 when silent.
    level: Smoothed,
    pan: Smoothed,
    /// The gains at the end of the last block, where the next block starts.
    gains: ChannelGains,
    /// The frames a change takes. One until `prepare` runs.
    ramp_frames: f32,
    /// What the track sends on, for its meter.
    peaks: Peaks,
}

impl Mixer {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    /// What a lane of the track itself moves.
    pub const AUTOMATION: AutomationInput<Mix, 2> = AutomationInput::new(0, [&GAIN, &PAN]);

    /// Starts at this mix, so a track that opens or is added is not faded in.
    pub fn new(mix: Mix, peaks: Peaks) -> Self {
        Self {
            mix: Automated::new(Self::AUTOMATION, mix),
            level: Smoothed::new(mix.level()),
            pan: Smoothed::new(mix.pan),
            gains: mix.gains(),
            ramp_frames: 1.0,
            peaks,
        }
    }

    /// Aims at the volume and the pan of the record and its lanes, each in its own ramp.
    fn aim(&mut self, targets: &Targets<Mix, 2>) {
        let mix = *self.mix;
        self.level.set_target(mix.level(), targets.ramp(&GAIN));
        self.pan.set_target(mix.pan, targets.ramp(&PAN));
        // Taken at once, so the block starts there too.
        if targets.snaps() {
            self.gains = pan_gains(f64::from(self.level.current()), self.pan.current());
        }
    }
}

impl Processor for Mixer {
    type Update = Mix;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.ramp_frames = (RAMP_SECONDS * config.sample_rate as f32).max(1.0);
    }

    fn update(&mut self, update: &mut Mix) {
        let targets = self.mix.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.mix.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        if !self.level.is_moving() && self.level.current() == 0.0 {
            // A muted track that has finished its fade touches nothing.
            self.gains = [0.0; CHANNELS];
            return;
        }
        let frames = context.frames;
        let before = self.gains;
        let level = self.level.advance(frames);
        let pan = self.pan.advance(frames);
        self.gains = pan_gains(f64::from(level), pan);
        let input = context.audio_inputs.get(Self::INPUT);
        // +0.0 times a gain that stays and is not negative is +0.0: the output as it starts,
        // with no peak.
        let steady = before == self.gains
            && before
                .iter()
                .all(|gain| gain.is_sign_positive() && gain.is_finite());
        if steady && input.iter().all(|channel| all_positive_zero(channel)) {
            return;
        }
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        let outputs = [&mut *left, &mut *right];
        let channels = outputs
            .into_iter()
            .zip(input)
            .zip(before.into_iter().zip(self.gains));
        for ((output, input), (before, after)) in channels {
            let step = (after - before) / frames as f32;
            for (frame, (output, input)) in output.iter_mut().zip(input).enumerate() {
                *output = input * (before + step * (frame + 1) as f32);
            }
        }
        self.peaks.record_block([&*left, &*right]);
    }
}
