//! The modulation matrix: routes from a source, such as an envelope or the mod wheel, to a
//! destination, such as a cutoff, each with an amount from -1 to 1.
//!
//! What a route does is `amount × source × scale`, added to the destination, where the scale is
//! the whole range of the destination: [`Destination::scale`]. The amp level is the one
//! exception: a route to it can only turn a voice down, see [`Destination::AmpLevel`].
//!
//! Every voice works its routes out once per block of at most 64 frames, and a position, gain,
//! cutoff or pan glides across the block to where the routes put it, so nothing steps.

use serde::{Deserialize, Serialize};

/// The most routes a matrix has.
pub const MAX_ROUTES: usize = 16;

/// A key this many semitones above middle C is at 1 as a source, and as many below at -1. So
/// Key to a cutoff at amount 1 makes the cutoff follow the keys one octave per octave.
pub const KEY_SEMITONES: f32 = 96.0;

/// Where a route comes from.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// The second envelope, from 0 to 1.
    #[serde(rename = "env_2")]
    Env2,
    /// The third envelope, from 0 to 1.
    #[serde(rename = "env_3")]
    Env3,
    /// The first LFO, from -1 to 1.
    #[serde(rename = "lfo_1")]
    Lfo1,
    /// The second LFO, from -1 to 1.
    #[serde(rename = "lfo_2")]
    Lfo2,
    /// How hard the note was played, from 0 to 1.
    #[serde(rename = "velocity")]
    Velocity,
    /// The pitch of the note: 0 at middle C, 1 at [`KEY_SEMITONES`] above it, -1 as far below.
    #[serde(rename = "key")]
    Key,
    /// The modulation wheel, from 0 to 1. It also adds the vibrato every bundled instrument has.
    #[serde(rename = "mod_wheel")]
    ModWheel,
    /// How hard the keys are pressed, from 0 to 1. The same for every note.
    #[serde(rename = "pressure")]
    Pressure,
    /// A value from -1 to 1 that each note draws when it starts, the same on every render.
    #[serde(rename = "random")]
    Random,
}

impl Source {
    pub const ALL: [Self; 9] = [
        Self::Env2,
        Self::Env3,
        Self::Lfo1,
        Self::Lfo2,
        Self::Velocity,
        Self::Key,
        Self::ModWheel,
        Self::Pressure,
        Self::Random,
    ];

    /// The name a composer sees.
    pub fn name(self) -> &'static str {
        match self {
            Self::Env2 => "Env 2",
            Self::Env3 => "Env 3",
            Self::Lfo1 => "LFO 1",
            Self::Lfo2 => "LFO 2",
            Self::Velocity => "Velocity",
            Self::Key => "Key",
            Self::ModWheel => "Mod wheel",
            Self::Pressure => "Pressure",
            Self::Random => "Random",
        }
    }

    /// From -1 to 1, or else from 0 to 1.
    pub fn is_bipolar(self) -> bool {
        matches!(self, Self::Lfo1 | Self::Lfo2 | Self::Key | Self::Random)
    }
}

/// Where a route goes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Destination {
    #[serde(rename = "osc_1_position")]
    Osc1Position,
    #[serde(rename = "osc_2_position")]
    Osc2Position,
    #[serde(rename = "osc_1_effect")]
    Osc1Effect,
    #[serde(rename = "osc_2_effect")]
    Osc2Effect,
    #[serde(rename = "osc_1_pitch")]
    Osc1Pitch,
    #[serde(rename = "osc_2_pitch")]
    Osc2Pitch,
    #[serde(rename = "osc_1_gain")]
    Osc1Gain,
    #[serde(rename = "osc_2_gain")]
    Osc2Gain,
    #[serde(rename = "sub_gain")]
    SubGain,
    #[serde(rename = "filter_1_cutoff")]
    Filter1Cutoff,
    #[serde(rename = "filter_2_cutoff")]
    Filter2Cutoff,
    #[serde(rename = "filter_1_resonance")]
    Filter1Resonance,
    #[serde(rename = "filter_2_resonance")]
    Filter2Resonance,
    /// The level of the voice. A route to it only turns the voice down: by nothing where the
    /// source is at its top, and by the amount where it is at its bottom. So velocity at amount
    /// 1 makes a note as loud as it was played, and an LFO at 1 is a full tremolo. A negative
    /// amount turns it down where the source is high instead. Routes to it multiply.
    #[serde(rename = "amp_level")]
    AmpLevel,
    /// Where the voice sits, after the filters.
    #[serde(rename = "pan")]
    Pan,
    #[serde(rename = "lfo_1_rate")]
    Lfo1Rate,
    #[serde(rename = "lfo_2_rate")]
    Lfo2Rate,
    #[serde(rename = "unison_amount")]
    UnisonAmount,
}

impl Destination {
    pub const ALL: [Self; 18] = [
        Self::Osc1Position,
        Self::Osc2Position,
        Self::Osc1Effect,
        Self::Osc2Effect,
        Self::Osc1Pitch,
        Self::Osc2Pitch,
        Self::Osc1Gain,
        Self::Osc2Gain,
        Self::SubGain,
        Self::Filter1Cutoff,
        Self::Filter2Cutoff,
        Self::Filter1Resonance,
        Self::Filter2Resonance,
        Self::AmpLevel,
        Self::Pan,
        Self::Lfo1Rate,
        Self::Lfo2Rate,
        Self::UnisonAmount,
    ];

    /// The name a composer sees.
    pub fn name(self) -> &'static str {
        match self {
            Self::Osc1Position => "Osc 1 position",
            Self::Osc2Position => "Osc 2 position",
            Self::Osc1Effect => "Osc 1 effect",
            Self::Osc2Effect => "Osc 2 effect",
            Self::Osc1Pitch => "Osc 1 pitch",
            Self::Osc2Pitch => "Osc 2 pitch",
            Self::Osc1Gain => "Osc 1 gain",
            Self::Osc2Gain => "Osc 2 gain",
            Self::SubGain => "Sub gain",
            Self::Filter1Cutoff => "Filter 1 cutoff",
            Self::Filter2Cutoff => "Filter 2 cutoff",
            Self::Filter1Resonance => "Filter 1 resonance",
            Self::Filter2Resonance => "Filter 2 resonance",
            Self::AmpLevel => "Amp level",
            Self::Pan => "Pan",
            Self::Lfo1Rate => "LFO 1 rate",
            Self::Lfo2Rate => "LFO 2 rate",
            Self::UnisonAmount => "Unison amount",
        }
    }

    /// What a route at amount 1 adds with its source at 1, in the unit of the destination:
    /// the whole range of a position, an effect amount, a gain, a resonance, the unison amount
    /// and the pan from the middle to one side; 24 semitones of pitch; 8 octaves of cutoff; 4
    /// octaves of LFO rate. For [`Self::AmpLevel`] see there.
    pub fn scale(self) -> f32 {
        match self {
            Self::Osc1Pitch | Self::Osc2Pitch => 24.0,
            Self::Filter1Cutoff | Self::Filter2Cutoff => 8.0,
            Self::Lfo1Rate | Self::Lfo2Rate => 4.0,
            _ => 1.0,
        }
    }
}

/// One route of the matrix.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub source: Source,
    pub destination: Destination,
    /// From -1 to 1. A negative amount works the other way round.
    pub amount: f32,
}

pub const ROUTE_AMOUNT: sound_core::Parameter<Route> = sound_core::Parameter {
    field: "amount",
    min: -1.0,
    max: 1.0,
    default: 0.0,
    scale: sound_core::Scale::Linear,
    get: |route| route.amount,
    set: |route, value| route.amount = value,
};

impl Route {
    pub const PARAMETERS: [&sound_core::Parameter<Self>; 1] = [&ROUTE_AMOUNT];
}

/// The value of every source for one voice at one moment, in the order of [`Source::ALL`].
pub(crate) type Sources = [f32; Source::ALL.len()];

/// What the routes do to one voice at one moment.
pub(crate) struct Modulation {
    /// `amount × source × scale` of every route, summed per destination, in the order of
    /// [`Destination::ALL`].
    sums: [f32; Destination::ALL.len()],
    /// The factor the amp level routes make, from 0 to 1.
    amp: f32,
}

impl Modulation {
    pub fn new(routes: &[Route], sources: &Sources) -> Self {
        let mut modulation = Self {
            sums: [0.0; Destination::ALL.len()],
            amp: 1.0,
        };
        for route in routes {
            let source = sources[route.source as usize];
            if route.destination == Destination::AmpLevel {
                // Where the source is between its bottom and its top, from 0 to 1.
                let (bottom, top) = if route.source.is_bipolar() {
                    (-1.0, 1.0)
                } else {
                    (0.0, 1.0)
                };
                let mut high = (source - bottom) / (top - bottom);
                if route.amount < 0.0 {
                    high = 1.0 - high;
                }
                modulation.amp *= (1.0 - route.amount.abs() * (1.0 - high)).max(0.0);
            } else {
                let destination = route.destination;
                modulation.sums[destination as usize] +=
                    route.amount * source * destination.scale();
            }
        }
        modulation
    }

    /// What the routes add to a destination, in its unit. Not for [`Destination::AmpLevel`].
    pub fn get(&self, destination: Destination) -> f32 {
        self.sums[destination as usize]
    }

    /// The factor on the level of the voice, from 0 to 1.
    pub fn amp(&self) -> f32 {
        self.amp
    }
}
