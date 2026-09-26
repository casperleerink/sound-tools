//! EQ: the built-in equaliser effect. It goes in an effect slot of a track, like an effect
//! plugin, and raises or lowers parts of the sound with four bands, each a low cut, a low
//! shelf, a bell, a notch, a high shelf or a high cut, then an output gain.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`. The
//! bands are a list, band 1 first:
//!
//! ```json
//! {
//!   "tool": "eq",
//!   "state": {
//!     "bands": [
//!       {"on": true, "shape": "low_cut", "frequency_hz": 80.0, "gain_db": 0.0, "q": 0.71},
//!       {"on": true, "shape": "bell", "frequency_hz": 400.0, "gain_db": -3.0, "q": 1.5}
//!     ],
//!     "output_gain_db": 0.0
//!   }
//! }
//! ```
//!
//! `README.md` in this crate has the sound, the ranges and the ports. [`view`] is the card of
//! the EQ, and the only module here that uses GPUI.

mod processor;
pub mod view;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{Eq, band_response, response};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "eq";

/// How many bands the EQ has. Four, as DESIGN.md draws it: a cut and two or three moves are
/// what most tracks need, four numbered handles stay apart on the display of a laptop, and the
/// file stays short for an agent. A list with fewer bands loads, so more bands later would not
/// break a saved project.
pub const BANDS: usize = 4;

/// What a band does around its frequency.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Takes away what is below the frequency, 12 dB per octave. Q is the peak at the corner.
    LowCut,
    /// Raises or lowers everything below the frequency by the gain.
    LowShelf,
    /// Raises or lowers a band around the frequency by the gain. Q is how narrow.
    Bell,
    /// Takes one narrow band out. Q is how narrow.
    Notch,
    /// Raises or lowers everything above the frequency by the gain.
    HighShelf,
    /// Takes away what is above the frequency, 12 dB per octave. Q is the peak at the corner.
    HighCut,
}

impl Shape {
    pub const ALL: [Self; 6] = [
        Self::LowCut,
        Self::LowShelf,
        Self::Bell,
        Self::Notch,
        Self::HighShelf,
        Self::HighCut,
    ];

    /// Whether the gain of the band does anything. A cut or a notch takes sound away and has
    /// no gain: the gain is kept, and used again when the shape changes back.
    pub const fn has_gain(self) -> bool {
        matches!(self, Self::LowShelf | Self::Bell | Self::HighShelf)
    }

    /// The place of the shape in [`Self::ALL`].
    pub const fn index(self) -> usize {
        match self {
            Self::LowCut => 0,
            Self::LowShelf => 1,
            Self::Bell => 2,
            Self::Notch => 3,
            Self::HighShelf => 4,
            Self::HighCut => 5,
        }
    }
}

/// One band. In the record a field left out takes the default of that band.
#[derive(Copy, Clone, Debug, PartialEq, Serialize)]
pub struct Band {
    /// Off, the band leaves the sound as it is and keeps its settings.
    pub on: bool,
    pub shape: Shape,
    pub frequency_hz: f32,
    /// Only for a shelf or a bell.
    pub gain_db: f32,
    pub q: f32,
}

/// One number of a band, with its range and its default.
pub type BandParameter = sound_core::Parameter<Band>;

/// Each band starts at its own frequency, so it has a parameter of its own with that default.
const fn frequency(default: f32) -> BandParameter {
    BandParameter {
        field: "frequency_hz",
        min: 20.0,
        max: 20_000.0,
        default,
        get: |band| band.frequency_hz,
        set: |band, value| band.frequency_hz = value,
    }
}

/// The frequency of each band: 100 Hz, 400 Hz, 2 kHz and 8 kHz by default, spread over the
/// scale as the handles are.
pub static FREQUENCIES: [BandParameter; BANDS] = [
    frequency(100.0),
    frequency(400.0),
    frequency(2_000.0),
    frequency(8_000.0),
];
pub const GAIN: BandParameter = BandParameter {
    field: "gain_db",
    min: -15.0,
    max: 15.0,
    default: 0.0,
    get: |band| band.gain_db,
    set: |band, value| band.gain_db = value,
};
pub const Q: BandParameter = BandParameter {
    field: "q",
    min: 0.1,
    max: 18.0,
    default: 0.71,
    get: |band| band.q,
    set: |band, value| band.q = value,
};

/// The shape of each band by default: a shelf at each end and two bells between them. At 0 dB
/// all four leave the sound exactly as it is.
pub const SHAPES: [Shape; BANDS] = [Shape::LowShelf, Shape::Bell, Shape::Bell, Shape::HighShelf];

/// The numbers of band `index`, in the order of its fields.
pub fn band_parameters(index: usize) -> [&'static BandParameter; 3] {
    [&FREQUENCIES[index], &GAIN, &Q]
}

impl Band {
    /// Band `index` as a new EQ has it.
    pub fn default_at(index: usize) -> Self {
        Self {
            on: true,
            shape: SHAPES[index],
            frequency_hz: FREQUENCIES[index].default,
            gain_db: GAIN.default,
            q: Q.default,
        }
    }
}

/// One number of the whole EQ, with its range and its default.
pub type Parameter = sound_core::Parameter<EqState>;

pub const OUTPUT_GAIN: Parameter = Parameter {
    field: "output_gain_db",
    min: -12.0,
    max: 12.0,
    default: 0.0,
    get: |state| state.output_gain_db,
    set: |state, value| state.output_gain_db = value,
};

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. The
/// memory of the bands is runtime state and is not saved, and neither is which band the card
/// has selected.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default EQ.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EqState {
    /// Band 1 first. A list with fewer bands gives the rest their defaults.
    #[serde(deserialize_with = "bands_from_list")]
    pub bands: [Band; BANDS],
    /// A gain after every band.
    pub output_gain_db: f32,
}

impl Default for EqState {
    fn default() -> Self {
        Self {
            bands: std::array::from_fn(Band::default_at),
            output_gain_db: OUTPUT_GAIN.default,
        }
    }
}

/// A band as a record may write it: any field may be left out.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BandInRecord {
    on: Option<bool>,
    shape: Option<Shape>,
    frequency_hz: Option<f32>,
    gain_db: Option<f32>,
    q: Option<f32>,
}

/// The bands of a record: at most [`BANDS`], each field that is left out from the default of
/// its band, and a band that is left out the default band.
fn bands_from_list<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[Band; BANDS], D::Error> {
    let listed = Vec::<BandInRecord>::deserialize(deserializer)?;
    if listed.len() > BANDS {
        let count = listed.len();
        return Err(D::Error::custom(format!(
            "bands lists {count} bands, and the EQ has {BANDS}"
        )));
    }
    let mut bands: [Band; BANDS] = std::array::from_fn(Band::default_at);
    for (band, listed) in bands.iter_mut().zip(listed) {
        *band = Band {
            on: listed.on.unwrap_or(band.on),
            shape: listed.shape.unwrap_or(band.shape),
            frequency_hz: listed.frequency_hz.unwrap_or(band.frequency_hz),
            gain_db: listed.gain_db.unwrap_or(band.gain_db),
            q: listed.q.unwrap_or(band.q),
        };
    }
    Ok(bands)
}

impl State for EqState {
    const TOOL: &'static str = "eq";

    fn validate(&self) -> Result<(), String> {
        for (index, band) in self.bands.iter().enumerate() {
            for parameter in band_parameters(index) {
                parameter
                    .check(band)
                    .map_err(|error| format!("band {}: {error}", index + 1))?;
            }
        }
        OUTPUT_GAIN.check(self)
    }
}

/// The doc of the EQ record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "eq",
    when: "You put an EQ on a track, or change one: cut the lows, less boxy, more air, a notch",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the EQ tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<EqState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit and every change glides.
fn apply(state: &EqState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let eq = context.processor("eq", || Eq::new(*state))?;
    context.update(eq, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(eq, Eq::INPUT));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(eq, Eq::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<'a>(name: &str, doc: &'a str, field: &str) -> &'a str {
        let row = format!("| `{field}` |");
        let row = doc.lines().find(|line| line.starts_with(&row));
        row.unwrap_or_else(|| panic!("{name} has no row for {field}"))
    }

    /// The docs give the ranges and the defaults to agents and to people. They are checked
    /// against the one definition, so they cannot drift from it. The frequency has a default
    /// per band, and its row lists them in the order of the bands.
    #[test]
    fn the_docs_give_the_range_and_the_default_of_every_parameter() {
        let docs = [
            ("agent-doc.md", include_str!("../agent-doc.md")),
            ("README.md", include_str!("../README.md")),
        ];
        let frequencies = FREQUENCIES
            .each_ref()
            .map(|parameter| parameter.default.to_string());
        let shapes = SHAPES.map(|shape| format!("`\"{}\"`", snake_case(shape)));
        for (name, doc) in docs {
            let ranges = [&FREQUENCIES[0], &GAIN, &Q].map(|parameter| {
                let (field, min, max) = (parameter.field, parameter.min, parameter.max);
                (field, format!("| {min} to {max} |"))
            });
            let output = (OUTPUT_GAIN.field, {
                let (min, max) = (OUTPUT_GAIN.min, OUTPUT_GAIN.max);
                format!("| {min} to {max} |")
            });
            for (field, range) in ranges.into_iter().chain([output]) {
                let row = row(name, doc, field);
                assert!(row.contains(&range), "{name}: {row}");
            }
            let defaults = [
                (FREQUENCIES[0].field, frequencies.join(", ")),
                (GAIN.field, GAIN.default.to_string()),
                (Q.field, Q.default.to_string()),
                (OUTPUT_GAIN.field, OUTPUT_GAIN.default.to_string()),
                ("shape", shapes.join(", ")),
            ];
            for (field, default) in defaults {
                let row = row(name, doc, field);
                assert!(row.contains(&format!("| {default} |")), "{name}: {row}");
            }
        }
    }

    fn snake_case(shape: Shape) -> String {
        let debug = format!("{shape:?}");
        let mut name = String::new();
        for (index, letter) in debug.chars().enumerate() {
            if letter.is_uppercase() && index > 0 {
                name.push('_');
            }
            name.push(letter.to_ascii_lowercase());
        }
        name
    }

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        for index in 0..BANDS {
            for parameter in band_parameters(index) {
                for value in [parameter.min, parameter.default, parameter.max] {
                    let mut state = EqState::default();
                    (parameter.set)(&mut state.bands[index], value);
                    assert_eq!((parameter.get)(&state.bands[index]), value);
                    assert_eq!(state.validate(), Ok(()));
                }
                let mut state = EqState::default();
                (parameter.set)(&mut state.bands[index], parameter.max * 2.0 + 1.0);
                let error = state.validate().unwrap_err();
                assert!(error.contains(parameter.field), "{error}");
                assert!(
                    error.starts_with(&format!("band {}: ", index + 1)),
                    "{error}"
                );
            }
        }
        for value in [OUTPUT_GAIN.min, OUTPUT_GAIN.default, OUTPUT_GAIN.max] {
            let mut state = EqState::default();
            (OUTPUT_GAIN.set)(&mut state, value);
            assert_eq!(state.validate(), Ok(()));
        }
    }

    fn parse(json: &str) -> Result<EqState, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }

    #[test]
    fn a_band_or_a_field_left_out_takes_the_default_of_its_band() {
        assert_eq!(parse("{}"), Ok(EqState::default()));
        let state = parse(r#"{"bands": [{"shape": "low_cut"}, {}, {"gain_db": -4.0}]}"#);
        let mut expected = EqState::default();
        expected.bands[0].shape = Shape::LowCut;
        expected.bands[2].gain_db = -4.0;
        assert_eq!(state, Ok(expected));
        // Every shape by its name.
        for shape in Shape::ALL {
            let json = format!(r#"{{"bands": [{{"shape": "{}"}}]}}"#, snake_case(shape));
            assert_eq!(parse(&json).map(|state| state.bands[0].shape), Ok(shape));
        }
    }

    #[test]
    fn too_many_bands_an_unknown_shape_or_an_unknown_field_do_not_load() {
        let five = r#"{"bands": [{}, {}, {}, {}, {}]}"#;
        assert!(
            parse(five).is_err_and(|error| error.contains("bands lists 5 bands, and the EQ has 4"))
        );
        let peak = r#"{"bands": [{"shape": "peak"}]}"#;
        assert!(parse(peak).is_err_and(|error| error.contains("unknown variant `peak`")));
        let wrong = r#"{"bands": [{"gain": 3.0}]}"#;
        assert!(parse(wrong).is_err_and(|error| error.contains("unknown field `gain`")));
    }

    /// The runtime writes every field of every band, so an agent reads the whole EQ.
    #[test]
    fn the_record_is_written_whole() {
        let json = serde_json::to_string(&EqState::default()).unwrap();
        assert!(json.starts_with(r#"{"bands":[{"on":true,"shape":"low_shelf","frequency_hz":100.0,"gain_db":0.0,"q":0.71},"#), "{json}");
        assert_eq!(parse(&json), Ok(EqState::default()));
    }
}
