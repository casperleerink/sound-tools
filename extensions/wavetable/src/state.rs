//! The saved state of the synth: a record an agent can write by hand and a view can make knobs
//! from.
//!
//! The record is nested: an object per oscillator, filter, envelope and LFO. Every number of
//! each object type is one [`Parameter`](sound_core::Parameter) constant of that type, with its
//! range, its default and its scale: `OSC_POSITION` is a `Parameter<Oscillator>`, and reads and writes
//! `osc_1` or `osc_2` alike. Each type lists its constants in `PARAMETERS`. A field that a
//! record leaves out takes the default of its type, so `"state": {}` is the default patch and
//! `"osc_2": {}` a default oscillator.
//!
//! An automation lane names a number by its path, as an error does: `filter_1.cutoff_hz`. The
//! lists of those are at the end, and [`AUTOMATED`] holds them all.

use serde::{Deserialize, Serialize};
use sound_core::{FilterSlope, FilterType, LfoShape, Scale};
use sound_notes::{Division, Feel};

use crate::matrix::{Destination, MAX_ROUTES, Route, Source};
use crate::tables::Table;

type Parameter<S> = sound_core::Parameter<S>;

/// How an oscillator bends its table as it reads it. Each has one amount, from 0 (the table as
/// it is) to 1.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    None,
    /// A sine at the pitch of the oscillator bends where it reads: brighter, then growling.
    /// At four times the sample rate, like every effect.
    Fm,
    /// Reads the frame up to 16 times faster, starting again at every cycle, as a hard synced
    /// oscillator does. Its restart is rounded off, so it does not alias.
    Sync,
    /// Squeezes the start of each cycle and stretches its end, up to 8 times.
    Warp,
    /// Drives the sound up to 8 times into a wavefolder.
    Fold,
}

impl Effect {
    pub const ALL: [Self; 5] = [Self::None, Self::Fm, Self::Sync, Self::Warp, Self::Fold];
}

/// A wavetable oscillator.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Oscillator {
    /// Off costs nothing.
    pub on: bool,
    pub table: Table,
    /// Where in the table it reads, from its first frame at 0 to its last at 1. Between two
    /// frames it plays a mix of both, so a sweep morphs smoothly.
    pub position: f32,
    pub effect: Effect,
    /// How far the effect bends the sound. 0 is the table as it is.
    pub effect_amount: f32,
    pub octave: i8,
    pub semitone: i8,
    pub detune_cents: f32,
    /// A factor on its level into the filters.
    pub gain: f32,
    /// -1 is left, 0 the middle and 1 right.
    pub pan: f32,
}

pub const OSC_POSITION: Parameter<Oscillator> = Parameter {
    field: "position",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    scale: Scale::Linear,
    get: |osc| osc.position,
    set: |osc, value| osc.position = value,
};
pub const OSC_EFFECT_AMOUNT: Parameter<Oscillator> = Parameter {
    field: "effect_amount",
    min: 0.0,
    max: 1.0,
    default: 0.4,
    scale: Scale::Linear,
    get: |osc| osc.effect_amount,
    set: |osc, value| osc.effect_amount = value,
};
pub const OSC_OCTAVE: Parameter<Oscillator> = Parameter {
    field: "octave",
    min: -3.0,
    max: 3.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |osc| f32::from(osc.octave),
    set: |osc, value| osc.octave = value as i8,
};
pub const OSC_SEMITONE: Parameter<Oscillator> = Parameter {
    field: "semitone",
    min: -12.0,
    max: 12.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |osc| f32::from(osc.semitone),
    set: |osc, value| osc.semitone = value as i8,
};
pub const OSC_DETUNE: Parameter<Oscillator> = Parameter {
    field: "detune_cents",
    min: -50.0,
    max: 50.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |osc| osc.detune_cents,
    set: |osc, value| osc.detune_cents = value,
};
pub const OSC_GAIN: Parameter<Oscillator> = Parameter {
    field: "gain",
    min: 0.0,
    max: 1.0,
    default: 0.7,
    scale: Scale::Linear,
    get: |osc| osc.gain,
    set: |osc, value| osc.gain = value,
};
pub const OSC_PAN: Parameter<Oscillator> = Parameter {
    field: "pan",
    min: -1.0,
    max: 1.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |osc| osc.pan,
    set: |osc, value| osc.pan = value,
};

impl Oscillator {
    pub const PARAMETERS: [&Parameter<Self>; 7] = [
        &OSC_POSITION,
        &OSC_EFFECT_AMOUNT,
        &OSC_OCTAVE,
        &OSC_SEMITONE,
        &OSC_DETUNE,
        &OSC_GAIN,
        &OSC_PAN,
    ];
}

impl Default for Oscillator {
    fn default() -> Self {
        Self {
            on: true,
            table: Table::BasicShapes,
            position: OSC_POSITION.default,
            effect: Effect::None,
            effect_amount: OSC_EFFECT_AMOUNT.default,
            octave: OSC_OCTAVE.default as i8,
            semitone: OSC_SEMITONE.default as i8,
            detune_cents: OSC_DETUNE.default,
            gain: OSC_GAIN.default,
            pan: OSC_PAN.default,
        }
    }
}

/// How far under the note the sub oscillator plays. Saved as the octave, `-1` or `-2`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i8", into = "i8")]
pub enum SubOctave {
    Down1,
    Down2,
}

impl SubOctave {
    pub const ALL: [Self; 2] = [Self::Down1, Self::Down2];

    pub const fn octaves(self) -> i8 {
        match self {
            Self::Down1 => -1,
            Self::Down2 => -2,
        }
    }
}

impl TryFrom<i8> for SubOctave {
    type Error = String;

    fn try_from(octave: i8) -> Result<Self, String> {
        match octave {
            -1 => Ok(Self::Down1),
            -2 => Ok(Self::Down2),
            _ => Err(format!("octave must be -1 or -2, not {octave}")),
        }
    }
}

impl From<SubOctave> for i8 {
    fn from(octave: SubOctave) -> Self {
        octave.octaves()
    }
}

/// A sine one or two octaves under the note, in the middle, into the filters.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sub {
    pub octave: SubOctave,
    /// 0 is off, and costs nothing.
    pub gain: f32,
}

pub const SUB_GAIN: Parameter<Sub> = Parameter {
    field: "gain",
    min: 0.0,
    max: 1.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |sub| sub.gain,
    set: |sub, value| sub.gain = value,
};

impl Sub {
    pub const PARAMETERS: [&Parameter<Self>; 1] = [&SUB_GAIN];
}

impl Default for Sub {
    fn default() -> Self {
        Self {
            octave: SubOctave::Down1,
            gain: SUB_GAIN.default,
        }
    }
}

/// Copies of both wavetable oscillators per note, spread in pitch and across the stereo field.
/// More copies are about as loud as one.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Unison {
    pub voices: u8,
    /// How far the copies spread: at 1, 50 cents up and down and from left to right.
    pub amount: f32,
}

pub const UNISON_VOICES: Parameter<Unison> = Parameter {
    field: "voices",
    min: 1.0,
    max: crate::synth::MAX_UNISON as f32,
    default: 1.0,
    scale: Scale::Linear,
    get: |unison| f32::from(unison.voices),
    set: |unison, value| unison.voices = value as u8,
};
pub const UNISON_AMOUNT: Parameter<Unison> = Parameter {
    field: "amount",
    min: 0.0,
    max: 1.0,
    default: 0.3,
    scale: Scale::Linear,
    get: |unison| unison.amount,
    set: |unison, value| unison.amount = value,
};

impl Unison {
    pub const PARAMETERS: [&Parameter<Self>; 2] = [&UNISON_VOICES, &UNISON_AMOUNT];
}

impl Default for Unison {
    fn default() -> Self {
        Self {
            voices: UNISON_VOICES.default as u8,
            amount: UNISON_AMOUNT.default,
        }
    }
}

/// A filter of each voice: the state variable filter of the SDK, the one the Filter effect
/// plays, with a drive in front of it.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Filter {
    /// Off lets the sound through as it is.
    pub on: bool,
    #[serde(rename = "type")]
    pub kind: FilterType,
    pub slope: FilterSlope,
    /// Where it starts to cut, or the middle of its band or its gap.
    pub cutoff_hz: f32,
    /// 0 is no peak. 1 is a strong ringing peak at the cutoff that never runs away.
    pub resonance: f32,
    /// Gain into a soft saturation before the filter, at four times the sample rate. 0 is
    /// clean and costs nothing.
    pub drive_db: f32,
}

pub const FILTER_CUTOFF: Parameter<Filter> = Parameter {
    field: "cutoff_hz",
    min: 20.0,
    max: 20_000.0,
    default: 1_000.0,
    scale: Scale::Logarithmic,
    get: |filter| filter.cutoff_hz,
    set: |filter, value| filter.cutoff_hz = value,
};
pub const FILTER_RESONANCE: Parameter<Filter> = Parameter {
    field: "resonance",
    min: 0.0,
    max: 1.0,
    default: 0.2,
    scale: Scale::Linear,
    get: |filter| filter.resonance,
    set: |filter, value| filter.resonance = value,
};
pub const FILTER_DRIVE: Parameter<Filter> = Parameter {
    field: "drive_db",
    min: 0.0,
    max: 24.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |filter| filter.drive_db,
    set: |filter, value| filter.drive_db = value,
};

impl Filter {
    pub const PARAMETERS: [&Parameter<Self>; 3] =
        [&FILTER_CUTOFF, &FILTER_RESONANCE, &FILTER_DRIVE];
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            on: true,
            kind: FilterType::LowPass,
            slope: FilterSlope::TwentyFour,
            cutoff_hz: FILTER_CUTOFF.default,
            resonance: FILTER_RESONANCE.default,
            drive_db: FILTER_DRIVE.default,
        }
    }
}

/// Where the oscillators go through the filters.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Routing {
    /// Everything through filter 1, then filter 2.
    Serial,
    /// Everything through both filters side by side, half of each.
    Parallel,
    /// Oscillator 1 through filter 1, oscillator 2 through filter 2, the sub half through each.
    Split,
}

impl Routing {
    pub const ALL: [Self; 3] = [Self::Serial, Self::Parallel, Self::Split];
}

/// An envelope: attack, decay to the sustain level, and release, each on a curve. The amp
/// envelope shapes the level of a voice. The other two are modulation sources from 0 to 1.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Adsr {
    /// From note on to the top.
    pub attack_seconds: f32,
    /// From the top to the sustain level.
    pub decay_seconds: f32,
    /// Where a held note settles, as a part of the top.
    pub sustain: f32,
    /// From note off to 0, from the top.
    pub release_seconds: f32,
    /// How each stage bends: 0 is a straight line, 1 a strong curve, fast at first and slow
    /// near its end.
    pub attack_curve: f32,
    pub decay_curve: f32,
    pub release_curve: f32,
}

/// An envelope time, heard in ratios. The lower end keeps every stage long enough not to
/// click.
const fn time(
    field: &'static str,
    default: f32,
    get: fn(&Adsr) -> f32,
    set: fn(&mut Adsr, f32),
) -> Parameter<Adsr> {
    Parameter {
        field,
        min: 0.001,
        max: 10.0,
        default,
        scale: Scale::Logarithmic,
        get,
        set,
    }
}

const fn curve(
    field: &'static str,
    default: f32,
    get: fn(&Adsr) -> f32,
    set: fn(&mut Adsr, f32),
) -> Parameter<Adsr> {
    Parameter {
        field,
        min: 0.0,
        max: 1.0,
        default,
        scale: Scale::Linear,
        get,
        set,
    }
}

pub const ENV_ATTACK: Parameter<Adsr> = time(
    "attack_seconds",
    0.005,
    |env| env.attack_seconds,
    |env, value| env.attack_seconds = value,
);
pub const ENV_DECAY: Parameter<Adsr> = time(
    "decay_seconds",
    0.4,
    |env| env.decay_seconds,
    |env, value| env.decay_seconds = value,
);
pub const ENV_SUSTAIN: Parameter<Adsr> = Parameter {
    field: "sustain",
    min: 0.0,
    max: 1.0,
    default: 0.6,
    scale: Scale::Linear,
    get: |env| env.sustain,
    set: |env, value| env.sustain = value,
};
pub const ENV_RELEASE: Parameter<Adsr> = time(
    "release_seconds",
    0.3,
    |env| env.release_seconds,
    |env, value| env.release_seconds = value,
);
pub const ENV_ATTACK_CURVE: Parameter<Adsr> = curve(
    "attack_curve",
    0.5,
    |env| env.attack_curve,
    |env, value| env.attack_curve = value,
);
pub const ENV_DECAY_CURVE: Parameter<Adsr> = curve(
    "decay_curve",
    0.8,
    |env| env.decay_curve,
    |env, value| env.decay_curve = value,
);
pub const ENV_RELEASE_CURVE: Parameter<Adsr> = curve(
    "release_curve",
    0.8,
    |env| env.release_curve,
    |env, value| env.release_curve = value,
);

impl Adsr {
    pub const PARAMETERS: [&Parameter<Self>; 7] = [
        &ENV_ATTACK,
        &ENV_DECAY,
        &ENV_SUSTAIN,
        &ENV_RELEASE,
        &ENV_ATTACK_CURVE,
        &ENV_DECAY_CURVE,
        &ENV_RELEASE_CURVE,
    ];
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            attack_seconds: ENV_ATTACK.default,
            decay_seconds: ENV_DECAY.default,
            sustain: ENV_SUSTAIN.default,
            release_seconds: ENV_RELEASE.default,
            attack_curve: ENV_ATTACK_CURVE.default,
            decay_curve: ENV_DECAY_CURVE.default,
            release_curve: ENV_RELEASE_CURVE.default,
        }
    }
}

/// An LFO of each voice, a modulation source from -1 to 1.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LfoSettings {
    pub shape: LfoShape,
    /// Cycles per second, when it does not follow the tempo.
    pub rate_hz: f32,
    /// Whether a cycle is a note of the tempo, `division` and `feel`, instead of `rate_hz`.
    pub sync: bool,
    pub division: Division,
    pub feel: Feel,
    /// Whether each note starts it from the start of its cycle. Off, a note picks it up where
    /// a free-running one is, and while the project plays that is in time with the bars.
    pub retrigger: bool,
}

pub const LFO_RATE: Parameter<LfoSettings> = Parameter {
    field: "rate_hz",
    min: 0.01,
    max: 40.0,
    default: 1.0,
    scale: Scale::Logarithmic,
    get: |lfo| lfo.rate_hz,
    set: |lfo, value| lfo.rate_hz = value,
};

impl LfoSettings {
    pub const PARAMETERS: [&Parameter<Self>; 1] = [&LFO_RATE];
}

impl Default for LfoSettings {
    fn default() -> Self {
        Self {
            shape: LfoShape::Sine,
            rate_hz: LFO_RATE.default,
            sync: false,
            division: Division::Quarter,
            feel: Feel::Straight,
            retrigger: true,
        }
    }
}

/// Poly plays up to `polyphony` notes at once. Mono plays one, and a key pressed while another
/// is held moves the note without starting it again (legato).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceMode {
    Poly,
    Mono,
}

impl VoiceMode {
    pub const ALL: [Self; 2] = [Self::Poly, Self::Mono];
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Voicing {
    pub mode: VoiceMode,
    pub polyphony: u8,
    /// How long a new note slides from the pitch of the last one. 0 is no glide.
    pub glide_seconds: f32,
}

pub const POLYPHONY: Parameter<Voicing> = Parameter {
    field: "polyphony",
    min: 1.0,
    max: crate::synth::VOICES as f32,
    default: 8.0,
    scale: Scale::Linear,
    get: |voicing| f32::from(voicing.polyphony),
    set: |voicing, value| voicing.polyphony = value as u8,
};
pub const GLIDE: Parameter<Voicing> = Parameter {
    field: "glide_seconds",
    min: 0.0,
    max: 5.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |voicing| voicing.glide_seconds,
    set: |voicing, value| voicing.glide_seconds = value,
};

impl Voicing {
    pub const PARAMETERS: [&Parameter<Self>; 2] = [&POLYPHONY, &GLIDE];
}

impl Default for Voicing {
    fn default() -> Self {
        Self {
            mode: VoiceMode::Poly,
            polyphony: POLYPHONY.default as u8,
            glide_seconds: GLIDE.default,
        }
    }
}

/// The saved state. It is also what the processor gets in an update, with the tables it names.
/// Phases, envelope levels and voices are runtime state and are not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default patch.
/// Inside an object, a field left out takes the default of that object's type: the default
/// patch differs from those only in `osc_2.detune_cents`, 7, and `filter_2.on`, false.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WavetableState {
    pub osc_1: Oscillator,
    pub osc_2: Oscillator,
    pub sub: Sub,
    pub unison: Unison,
    pub filter_1: Filter,
    pub filter_2: Filter,
    pub routing: Routing,
    /// The level of each voice.
    pub amp_env: Adsr,
    pub env_2: Adsr,
    pub env_3: Adsr,
    pub lfo_1: LfoSettings,
    pub lfo_2: LfoSettings,
    pub voicing: Voicing,
    /// Up to [`MAX_ROUTES`] routes, each from a source to a destination. Routes to the same
    /// destination add up.
    pub matrix: Vec<Route>,
    /// Linear output gain. With the default patch one note at velocity 127 peaks at about this
    /// value. Tracks sum to the device with no mixer yet, so the default is low.
    pub gain: f32,
}

pub const GAIN: Parameter<WavetableState> = Parameter {
    field: "gain",
    min: 0.0,
    max: 1.0,
    default: 0.15,
    scale: Scale::Linear,
    get: |state| state.gain,
    set: |state, value| state.gain = value,
};

impl WavetableState {
    pub const PARAMETERS: [&Parameter<Self>; 1] = [&GAIN];

    pub fn oscillators(&self) -> [&Oscillator; 2] {
        [&self.osc_1, &self.osc_2]
    }

    pub fn filters(&self) -> [&Filter; 2] {
        [&self.filter_1, &self.filter_2]
    }

    pub fn lfos(&self) -> [&LfoSettings; 2] {
        [&self.lfo_1, &self.lfo_2]
    }
}

impl Default for WavetableState {
    fn default() -> Self {
        let route = |source, destination, amount| Route {
            source,
            destination,
            amount,
        };
        Self {
            osc_1: Oscillator::default(),
            // A second saw a few cents up beats slowly against the first: a wider sound.
            osc_2: Oscillator {
                detune_cents: 7.0,
                ..Oscillator::default()
            },
            sub: Sub::default(),
            unison: Unison::default(),
            filter_1: Filter::default(),
            filter_2: Filter {
                on: false,
                ..Filter::default()
            },
            routing: Routing::Serial,
            amp_env: Adsr::default(),
            env_2: Adsr::default(),
            env_3: Adsr::default(),
            lfo_1: LfoSettings::default(),
            lfo_2: LfoSettings::default(),
            voicing: Voicing::default(),
            matrix: vec![
                route(Source::Env2, Destination::Filter1Cutoff, 0.4),
                route(Source::Key, Destination::Filter1Cutoff, 0.5),
                route(Source::Velocity, Destination::AmpLevel, 0.5),
                route(Source::ModWheel, Destination::Osc1Position, 0.5),
            ],
            gain: GAIN.default,
        }
    }
}

/// Numbers of objects of the record as numbers of the whole record, each named by its path, as
/// `object.field: PARAMETER`: the field of the object, with the range of the parameter of its
/// object type.
macro_rules! lanes {
    ($($object:ident . $field:ident : $parameter:ident),+ $(,)?) => {
        [$($parameter.at(
            concat!(stringify!($object), ".", stringify!($field)),
            |state: &WavetableState| state.$object.$field,
            |state: &mut WavetableState, value| state.$object.$field = value,
        )),+]
    };
}

/// The numbers of each oscillator that a lane can move, in this order. The octave and the
/// semitone are whole numbers, and a lane is a straight line, so they are left out.
pub static OSCILLATOR_LANES: [[Parameter<WavetableState>; 5]; 2] = [
    lanes![
        osc_1.position: OSC_POSITION,
        osc_1.effect_amount: OSC_EFFECT_AMOUNT,
        osc_1.detune_cents: OSC_DETUNE,
        osc_1.gain: OSC_GAIN,
        osc_1.pan: OSC_PAN,
    ],
    lanes![
        osc_2.position: OSC_POSITION,
        osc_2.effect_amount: OSC_EFFECT_AMOUNT,
        osc_2.detune_cents: OSC_DETUNE,
        osc_2.gain: OSC_GAIN,
        osc_2.pan: OSC_PAN,
    ],
];

/// The numbers of each filter that a lane can move, in this order.
pub static FILTER_LANES: [[Parameter<WavetableState>; 3]; 2] = [
    lanes![
        filter_1.cutoff_hz: FILTER_CUTOFF,
        filter_1.resonance: FILTER_RESONANCE,
        filter_1.drive_db: FILTER_DRIVE,
    ],
    lanes![
        filter_2.cutoff_hz: FILTER_CUTOFF,
        filter_2.resonance: FILTER_RESONANCE,
        filter_2.drive_db: FILTER_DRIVE,
    ],
];

/// The level of the sub and the spread of the unison copies, in this order. The count of the
/// copies is a whole number.
pub static VOICE_LANES: [Parameter<WavetableState>; 2] =
    lanes![sub.gain: SUB_GAIN, unison.amount: UNISON_AMOUNT];

/// What a lane moves at once, with no glide of its own: the envelopes, the rates of the LFOs and
/// the glide of the notes. The polyphony is a whole number.
pub static TIMING_LANES: [Parameter<WavetableState>; 24] = lanes![
    amp_env.attack_seconds: ENV_ATTACK,
    amp_env.decay_seconds: ENV_DECAY,
    amp_env.sustain: ENV_SUSTAIN,
    amp_env.release_seconds: ENV_RELEASE,
    amp_env.attack_curve: ENV_ATTACK_CURVE,
    amp_env.decay_curve: ENV_DECAY_CURVE,
    amp_env.release_curve: ENV_RELEASE_CURVE,
    env_2.attack_seconds: ENV_ATTACK,
    env_2.decay_seconds: ENV_DECAY,
    env_2.sustain: ENV_SUSTAIN,
    env_2.release_seconds: ENV_RELEASE,
    env_2.attack_curve: ENV_ATTACK_CURVE,
    env_2.decay_curve: ENV_DECAY_CURVE,
    env_2.release_curve: ENV_RELEASE_CURVE,
    env_3.attack_seconds: ENV_ATTACK,
    env_3.decay_seconds: ENV_DECAY,
    env_3.sustain: ENV_SUSTAIN,
    env_3.release_seconds: ENV_RELEASE,
    env_3.attack_curve: ENV_ATTACK_CURVE,
    env_3.decay_curve: ENV_DECAY_CURVE,
    env_3.release_curve: ENV_RELEASE_CURVE,
    lfo_1.rate_hz: LFO_RATE,
    lfo_2.rate_hz: LFO_RATE,
    voicing.glide_seconds: GLIDE,
];

/// Every number an automation lane can move, the output gain last. The amounts of the matrix are
/// left out: a route has no name of its own, and its place changes when one before it goes.
pub const AUTOMATED: [&Parameter<WavetableState>; 43] = {
    let groups: [&[Parameter<WavetableState>]; 7] = [
        &OSCILLATOR_LANES[0],
        &OSCILLATOR_LANES[1],
        &FILTER_LANES[0],
        &FILTER_LANES[1],
        &VOICE_LANES,
        &TIMING_LANES,
        std::slice::from_ref(&GAIN),
    ];
    let mut all = [&GAIN; 43];
    let (mut group, mut next) = (0, 0);
    while group < groups.len() {
        let mut index = 0;
        while index < groups[group].len() {
            all[next] = &groups[group][index];
            (index, next) = (index + 1, next + 1);
        }
        group += 1;
    }
    assert!(next == all.len(), "every lane is in the list once");
    all
};

/// Checks every number of `object` against its range, naming it by `path`.
fn check<S>(path: &str, object: &S, parameters: &[&Parameter<S>]) -> Result<(), String> {
    parameters
        .iter()
        .try_for_each(|parameter| parameter.check(object))
        .map_err(|error| format!("{path}.{error}"))
}

impl sound_core::State for WavetableState {
    const TOOL: &'static str = "wavetable";

    fn validate(&self) -> Result<(), String> {
        let [osc_1, osc_2] = self.oscillators();
        check("osc_1", osc_1, &Oscillator::PARAMETERS)?;
        check("osc_2", osc_2, &Oscillator::PARAMETERS)?;
        check("sub", &self.sub, &Sub::PARAMETERS)?;
        check("unison", &self.unison, &Unison::PARAMETERS)?;
        check("filter_1", &self.filter_1, &Filter::PARAMETERS)?;
        check("filter_2", &self.filter_2, &Filter::PARAMETERS)?;
        check("amp_env", &self.amp_env, &Adsr::PARAMETERS)?;
        check("env_2", &self.env_2, &Adsr::PARAMETERS)?;
        check("env_3", &self.env_3, &Adsr::PARAMETERS)?;
        check("lfo_1", &self.lfo_1, &LfoSettings::PARAMETERS)?;
        check("lfo_2", &self.lfo_2, &LfoSettings::PARAMETERS)?;
        check("voicing", &self.voicing, &Voicing::PARAMETERS)?;
        if self.matrix.len() > MAX_ROUTES {
            return Err(format!(
                "matrix may have at most {MAX_ROUTES} routes, not {}",
                self.matrix.len()
            ));
        }
        for (index, route) in self.matrix.iter().enumerate() {
            check(&format!("matrix[{index}]"), route, &Route::PARAMETERS)?;
        }
        Self::PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}
