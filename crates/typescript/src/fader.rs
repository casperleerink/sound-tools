//! The `fader`: a child of a parent of the project that turns a sound up or down on a fader's
//! scale, pans it and mutes it, with no click, and keeps the level of what it sends on for a
//! view. A mixer strip, a send or a master volume is one.
//!
//! The volume, the pan and the mute are automatable like the numbers of a device. The processor
//! works out one gain per channel when something moves and ramps to the new pair, so no change
//! clicks.

mod decibels;

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, BehaviourContext, BehaviourError,
    CHANNELS, InputEndpoint, JsonTool, JsonToolDoc, OutputEndpoint, Parameter, Peaks, Ports,
    PrepareConfig, ProcessContext, Processor, Scale, Smoothed, State, Targets, ToolSchema,
    all_positive_zero, amplitude, pan_gains,
};

use crate::meter::LEVEL;

pub(crate) const NAME: &str = "fader";
const PROCESSOR: &str = "fader";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    /// How much louder or quieter it plays, in decibels, up to 6. `"-inf"` is silence.
    #[serde(default, with = "decibels")]
    #[schemars(with = "Gain")]
    gain_db: f32,
    /// Where it sits between the two channels: -1 hard left, 0 the middle, 1 hard right.
    #[serde(default)]
    pan: f32,
    /// Silent, with a short fade.
    #[serde(default)]
    mute: bool,
}

/// What a gain field reads: a number of decibels, or `"-inf"` for silence.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum Gain {
    Decibels(f32),
    Silence(Silence),
}

#[derive(JsonSchema)]
#[allow(dead_code)]
enum Silence {
    #[serde(rename = "-inf")]
    Silence,
}

impl State for Record {
    const TOOL: &'static str = NAME;
}

/// The tool for the core, which the tools of the project define with theirs.
pub(crate) fn json_tool() -> JsonTool {
    JsonTool {
        name: NAME.to_string(),
        check: Arc::new(|state| record(state).map(drop)),
        doc: Some(JsonToolDoc {
            when: "A parent of this project turns a sound up or down, pans or mutes it: a mixer strip, a send or a master volume"
                .to_string(),
            markdown: include_str!("fader.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
        end: None,
    }
}

/// The record of the fader, for the SDK: its gain and its pan are numbers a cable moves.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        numbers: [GAIN, PAN, MUTE]
            .map(|number| (number.info(), number.default))
            .into(),
        ..ToolSchema::of::<Record>(&[])
    }
}

fn record(state: &Value) -> Result<Record, String> {
    let record: Record =
        serde_json::from_value(state.clone()).map_err(|error| format!("state: {error}"))?;
    decibels::check("state.gain_db", record.gain_db, MAX_GAIN_DB)?;
    if !(-1.0..=1.0).contains(&record.pan) {
        return Err(format!(
            "state.pan must be from -1 to 1, not {}",
            record.pan
        ));
    }
    Ok(record)
}

fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let record = record(state).map_err(BehaviourError::Other)?;
    let setting = Setting {
        gain_db: record.gain_db,
        pan: record.pan,
        silent: record.mute,
    };
    let peaks = context.peaks(LEVEL);
    let fader = context.processor(PROCESSOR, || Fader::new(setting, peaks))?;
    context.update(fader, setting)?;
    context.input("audio", InputEndpoint::new(fader, Fader::INPUT));
    context.automation_of(fader, Fader::AUTOMATION, &setting);
    context.output("audio", OutputEndpoint::new(fader, Fader::OUTPUT));
    Ok(())
}

/// The highest gain of a fader, in decibels. Anything under it is a gain too, down to
/// `-inf`.
const MAX_GAIN_DB: f32 = 6.0;

/// How long a change of gain, pan or mute takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// The gain of each channel, left first.
type ChannelGains = [f32; CHANNELS];

/// What a fader plays.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Setting {
    gain_db: f32,
    pan: f32,
    /// Muted.
    silent: bool,
}

impl Setting {
    /// The gain of each channel.
    ///
    /// The pan law is the equal power law of the SDK, [`pan_gains`]: a sound keeps its
    /// loudness wherever it is panned, and in the middle it is untouched.
    fn gains(&self) -> ChannelGains {
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

/// The volume, as a lane moves it: on the scale of a fader.
const GAIN: Parameter<Setting> = Parameter {
    field: "gain_db",
    min: f32::NEG_INFINITY,
    max: MAX_GAIN_DB,
    default: 0.0,
    scale: Scale::Fader,
    get: |setting| setting.gain_db,
    set: |setting, value| setting.gain_db = value,
};
const PAN: Parameter<Setting> = Parameter {
    field: "pan",
    min: -1.0,
    max: 1.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |setting| setting.pan,
    set: |setting, value| setting.pan = value,
};
/// The mute, as a cable moves it: 1 is silent and 0 plays, so a solo silences a track whose
/// volume a lane moves.
const MUTE: Parameter<Setting> = Parameter {
    field: "mute",
    min: 0.0,
    max: 1.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |setting| f32::from(u8::from(setting.silent)),
    set: |setting, value| setting.silent = value >= 0.5,
};

/// The processor: it multiplies each channel by its gain and ramps to a new one.
///
/// The volume and the pan glide apart, each with its own ramp, and the gains of the channels
/// are worked out from where both are at the end of each block. So a pan lane that moves does
/// not cut short the glide of a volume lane that just took over.
struct Fader {
    /// The record, with the values of its lanes.
    setting: Automated<Setting, 3>,
    /// The factor of the volume, 0 when silent.
    level: Smoothed,
    pan: Smoothed,
    /// The gains at the end of the last block, where the next block starts.
    gains: ChannelGains,
    /// The frames a change takes. One until `prepare` runs.
    ramp_frames: f32,
    /// What it sends on, for a meter.
    peaks: Peaks,
}

impl Fader {
    const INPUT: AudioInput = AudioInput::new(0);
    const OUTPUT: AudioOutput = AudioOutput::new(0);
    /// What a lane moves.
    const AUTOMATION: AutomationInput<Setting, 3> = AutomationInput::new(0, [&GAIN, &PAN, &MUTE]);

    /// Starts at `setting`, so a fader that opens or is added is not faded in.
    fn new(setting: Setting, peaks: Peaks) -> Self {
        Self {
            setting: Automated::new(Self::AUTOMATION, setting),
            level: Smoothed::new(setting.level()),
            pan: Smoothed::new(setting.pan),
            gains: setting.gains(),
            ramp_frames: 1.0,
            peaks,
        }
    }

    /// Aims at the volume and the pan of the record and its lanes, each in its own ramp.
    fn aim(&mut self, targets: &Targets<Setting, 3>) {
        let setting = *self.setting;
        let ramp = targets.ramp(&GAIN).max(targets.ramp(&MUTE));
        self.level.set_target(setting.level(), ramp);
        self.pan.set_target(setting.pan, targets.ramp(&PAN));
        // Taken at once, so the block starts there too.
        if targets.snaps() {
            self.gains = pan_gains(f64::from(self.level.current()), self.pan.current());
        }
    }
}

impl Processor for Fader {
    type Update = Setting;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.ramp_frames = (RAMP_SECONDS * config.sample_rate as f32).max(1.0);
    }

    fn update(&mut self, update: &mut Setting) {
        let targets = self.setting.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.setting.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        if !self.level.is_moving() && self.level.current() == 0.0 {
            // A silent fader that has finished its fade touches nothing.
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
