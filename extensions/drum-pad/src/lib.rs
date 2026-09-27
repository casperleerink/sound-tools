//! Drum pad: an instrument of 16 pads on notes 36 to 51, each a synthesized drum sound or a
//! sample, with a volume, a pitch, a decay and a pan, and one choke group for the hi-hats.
//! The default kit is made by synthesis in this crate ([`kit`]); no sample file ships.
//!
//! A record on disk, `instrument.json` in a track folder. The pads are keyed by their note, and
//! a pad or a field left out takes the default of that pad:
//!
//! ```json
//! {
//!   "tool": "drum-pad",
//!   "state": {
//!     "pads": {
//!       "36": {"sound": "kick", "volume_db": 0.0, "pitch_semitones": 0.0, "decay_ms": 600.0, "pan": 0.0, "choke": false},
//!       "48": {"sample": "shaker.wav", "decay_ms": 900.0}
//!     }
//!   }
//! }
//! ```
//!
//! `README.md` in this crate has the sound, the ranges and the ports. [`view`] is the card of
//! the Drum pad, and the only module here that uses GPUI.

mod kit;
mod processor;
mod sounds;
pub mod view;

use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_media::AudioAsset;
use sound_notes::{AUDIO_OUTPUT, NOTES_INPUT, Pitch};

pub use processor::{DrumPad, DrumUpdate, FADE_SECONDS, RAMP_SECONDS, VOICES, pad_gains};
pub use sounds::{MAX_SAMPLE_SECONDS, Rendered, render};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "drum-pad";

/// How many pads there are: a 4 by 4 grid.
pub const PADS: usize = 16;

/// The note of the first pad, the bottom left one. The pads are notes 36 to 51, row by row
/// from the bottom, on the General MIDI drum map, so a drum part from elsewhere plays the
/// right sounds.
pub const FIRST_NOTE: u8 = 36;

/// The pad a note plays, if any.
pub fn pad_of(pitch: Pitch) -> Option<usize> {
    let pad = usize::from(pitch.number()).checked_sub(usize::from(FIRST_NOTE))?;
    (pad < PADS).then_some(pad)
}

/// The note of pad `pad`.
pub fn note_of(pad: usize) -> u8 {
    FIRST_NOTE + pad as u8
}

/// A synthesized sound of the default kit.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    Kick,
    Snare,
    Clap,
    Rim,
    Hat,
    OpenHat,
    Tom,
    Crash,
    Ride,
}

impl Sound {
    /// In the order of the `Sound` list of the card.
    pub const ALL: [Self; 9] = [
        Self::Kick,
        Self::Snare,
        Self::Clap,
        Self::Rim,
        Self::Hat,
        Self::OpenHat,
        Self::Tom,
        Self::Crash,
        Self::Ride,
    ];

    /// What the card calls it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Kick => "Kick",
            Self::Snare => "Snare",
            Self::Clap => "Clap",
            Self::Rim => "Rim",
            Self::Hat => "Hat",
            Self::OpenHat => "Open hat",
            Self::Tom => "Tom",
            Self::Crash => "Crash",
            Self::Ride => "Ride",
        }
    }

    /// How it is written in a record: `"open_hat"`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Snare => "snare",
            Self::Clap => "clap",
            Self::Rim => "rim",
            Self::Hat => "hat",
            Self::OpenHat => "open_hat",
            Self::Tom => "tom",
            Self::Crash => "crash",
            Self::Ride => "ride",
        }
    }

    /// The loudest sample of one hit at velocity 127 and 0 dB, at any pitch and decay. The
    /// kit is balanced by these, so a beat of it sits under the ceiling of the master with the
    /// track at 0 dB and leaves room for the other tracks.
    pub const fn level(self) -> f32 {
        match self {
            Self::Kick => 0.35,
            Self::Snare => 0.3,
            Self::Clap => 0.26,
            Self::Rim => 0.2,
            Self::Hat | Self::OpenHat => 0.14,
            Self::Tom => 0.28,
            Self::Crash => 0.18,
            Self::Ride => 0.15,
        }
    }
}

/// What a pad plays.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A sound of the default kit.
    Sound(Sound),
    /// A file under `assets/audio/`.
    Sample(AudioAsset),
}

/// One pad. In the record a field left out takes the default of that pad.
#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    pub source: Source,
    /// The level of the pad, in dB.
    pub volume_db: f32,
    /// Semitones up or down. A sample plays faster or slower, as on a sampler; a synthesized
    /// sound is tuned and keeps its length.
    pub pitch_semitones: f32,
    /// From the hit to silence. A synthesized sound is made for this length and falls to
    /// -60 dB in it; a sample fades out over it and is silent at its end.
    pub decay_ms: f32,
    /// -1 is left, 1 is right, with the pan law of a track.
    pub pan: f32,
    /// In the choke group: a hit of a pad of the group cuts every other pad of it.
    pub choke: bool,
}

/// A pad of the default kit: its name on the card and its settings.
pub struct KitPad {
    pub name: &'static str,
    pub sound: Sound,
    pub volume_db: f32,
    pub pitch_semitones: f32,
    pub decay_ms: f32,
    pub pan: f32,
    pub choke: bool,
}

const fn kit_pad(
    name: &'static str,
    sound: Sound,
    (volume_db, pitch_semitones, decay_ms, pan): (f32, f32, f32, f32),
    choke: bool,
) -> KitPad {
    KitPad {
        name,
        sound,
        volume_db,
        pitch_semitones,
        decay_ms,
        pan,
        choke,
    }
}

/// The default kit, pad 1 (note 36) first. The toms are one sound tuned apart and spread from
/// the right (the floor tom, as the drummer hears it) to the left; the crash is a little left
/// and the ride a little right. Hat, Pedal hat and Open hat are the choke group.
pub const KIT: [KitPad; PADS] = [
    kit_pad("Kick", Sound::Kick, (0.0, 0.0, 600.0, 0.0), false),
    kit_pad("Rim", Sound::Rim, (0.0, 0.0, 70.0, 0.0), false),
    kit_pad("Snare", Sound::Snare, (0.0, 0.0, 350.0, 0.0), false),
    kit_pad("Clap", Sound::Clap, (0.0, 0.0, 380.0, 0.0), false),
    kit_pad("Snare 2", Sound::Snare, (-1.0, 3.0, 240.0, 0.0), false),
    kit_pad("Tom 1", Sound::Tom, (0.0, -7.0, 800.0, 0.35), false),
    kit_pad("Hat", Sound::Hat, (0.0, 0.0, 180.0, 0.0), true),
    kit_pad("Tom 2", Sound::Tom, (0.0, -4.0, 720.0, 0.25), false),
    kit_pad("Pedal hat", Sound::Hat, (-3.0, -1.0, 110.0, 0.0), true),
    kit_pad("Tom 3", Sound::Tom, (0.0, -2.0, 650.0, 0.1), false),
    kit_pad("Open hat", Sound::OpenHat, (0.0, 0.0, 650.0, 0.0), true),
    kit_pad("Tom 4", Sound::Tom, (0.0, 1.0, 600.0, -0.05), false),
    kit_pad("Tom 5", Sound::Tom, (0.0, 3.0, 550.0, -0.15), false),
    kit_pad("Crash", Sound::Crash, (0.0, 0.0, 1800.0, -0.25), false),
    kit_pad("Tom 6", Sound::Tom, (0.0, 6.0, 500.0, -0.25), false),
    kit_pad("Ride", Sound::Ride, (0.0, 0.0, 2500.0, 0.25), false),
];

impl Pad {
    /// Pad `pad` as a new Drum pad has it.
    pub fn default_at(pad: usize) -> Self {
        let kit = &KIT[pad % PADS];
        Self {
            source: Source::Sound(kit.sound),
            volume_db: kit.volume_db,
            pitch_semitones: kit.pitch_semitones,
            decay_ms: kit.decay_ms,
            pan: kit.pan,
            choke: kit.choke,
        }
    }

    /// What the card writes on pad `pad`: the name of the kit while it plays the sound of the
    /// kit, the name of another sound, or the file name of a sample without its extension.
    pub fn name(&self, pad: usize) -> String {
        match &self.source {
            Source::Sound(sound) if *sound == KIT[pad % PADS].sound => KIT[pad % PADS].name.into(),
            Source::Sound(sound) => sound.name().into(),
            Source::Sample(asset) => {
                let file = asset.to_string();
                let stem = file
                    .rsplit_once('.')
                    .map_or(file.as_str(), |(stem, _)| stem);
                stem.to_string()
            }
        }
    }
}

/// One number of a pad, with its range and the default of its pad.
pub type PadParameter = sound_core::Parameter<Pad>;

const fn volume(default: f32) -> PadParameter {
    PadParameter {
        field: "volume_db",
        min: -48.0,
        max: 12.0,
        default,
        get: |pad| pad.volume_db,
        set: |pad, value| pad.volume_db = value,
    }
}

const fn pitch(default: f32) -> PadParameter {
    PadParameter {
        field: "pitch_semitones",
        min: -24.0,
        max: 24.0,
        default,
        get: |pad| pad.pitch_semitones,
        set: |pad, value| pad.pitch_semitones = value,
    }
}

const fn decay(default: f32) -> PadParameter {
    PadParameter {
        field: "decay_ms",
        min: 10.0,
        max: 10_000.0,
        default,
        get: |pad| pad.decay_ms,
        set: |pad, value| pad.decay_ms = value,
    }
}

const fn pan(default: f32) -> PadParameter {
    PadParameter {
        field: "pan",
        min: -1.0,
        max: 1.0,
        default,
        get: |pad| pad.pan,
        set: |pad, value| pad.pan = value,
    }
}

/// The numbers of pad `pad`: volume, pitch, decay and pan, with the defaults of that pad.
const fn numbers(pad: usize) -> [PadParameter; 4] {
    let kit = &KIT[pad];
    [
        volume(kit.volume_db),
        pitch(kit.pitch_semitones),
        decay(kit.decay_ms),
        pan(kit.pan),
    ]
}

/// The numbers of every pad, pad 1 first: volume, pitch, decay and pan. Each pad has its own
/// defaults, so each has its own parameters with one range.
pub static PARAMETERS: [[PadParameter; 4]; PADS] = [
    numbers(0),
    numbers(1),
    numbers(2),
    numbers(3),
    numbers(4),
    numbers(5),
    numbers(6),
    numbers(7),
    numbers(8),
    numbers(9),
    numbers(10),
    numbers(11),
    numbers(12),
    numbers(13),
    numbers(14),
    numbers(15),
];

/// Which of [`PARAMETERS`] of a pad is which.
pub const VOLUME: usize = 0;
pub const PITCH: usize = 1;
pub const DECAY: usize = 2;
pub const PAN: usize = 3;

/// The saved state: the 16 pads. The sounds made from it and the voices are runtime state and
/// are not saved, and neither is which pad the card has selected.
///
/// A pad or a field that a record leaves out takes the default of that pad, so `"state": {}`
/// is the default kit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DrumPadState {
    /// Keyed by note, `"36"` to `"51"`, in the record.
    #[serde(serialize_with = "pads_to_map", deserialize_with = "pads_from_map")]
    pub pads: [Pad; PADS],
}

impl Default for DrumPadState {
    fn default() -> Self {
        Self {
            pads: std::array::from_fn(Pad::default_at),
        }
    }
}

/// A pad as a record writes it: any field may be left out, and it plays a sound or a sample.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PadRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    sound: Option<Sound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample: Option<AudioAsset>,
    volume_db: Option<f32>,
    pitch_semitones: Option<f32>,
    decay_ms: Option<f32>,
    pan: Option<f32>,
    choke: Option<bool>,
}

fn pads_to_map<S: Serializer>(pads: &[Pad; PADS], serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(PADS))?;
    for (index, pad) in pads.iter().enumerate() {
        let (sound, sample) = match &pad.source {
            Source::Sound(sound) => (Some(*sound), None),
            Source::Sample(asset) => (None, Some(asset.clone())),
        };
        let record = PadRecord {
            sound,
            sample,
            volume_db: Some(pad.volume_db),
            pitch_semitones: Some(pad.pitch_semitones),
            decay_ms: Some(pad.decay_ms),
            pan: Some(pad.pan),
            choke: Some(pad.choke),
        };
        map.serialize_entry(&note_of(index).to_string(), &record)?;
    }
    map.end()
}

/// The pads of a record: each by its note, a field that is left out from the default of its
/// pad, and a pad that is left out the default pad.
fn pads_from_map<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[Pad; PADS], D::Error> {
    let listed = serde_json::Map::deserialize(deserializer)?;
    let mut pads: [Pad; PADS] = std::array::from_fn(Pad::default_at);
    let last = note_of(PADS - 1);
    for (note, value) in listed {
        let pad = note
            .parse::<u8>()
            .ok()
            .and_then(|number| Pitch::new(number).ok())
            .and_then(pad_of)
            .ok_or_else(|| {
                D::Error::custom(format!(
                    "pads has a pad {note:?}, and the pads are the notes \"{FIRST_NOTE}\" to \"{last}\""
                ))
            })?;
        let record = PadRecord::deserialize(value)
            .map_err(|error| D::Error::custom(format!("pad \"{note}\": {error}")))?;
        let default = &mut pads[pad];
        let source = match (record.sound, record.sample) {
            (Some(_), Some(_)) => {
                return Err(D::Error::custom(format!(
                    "pad \"{note}\" has a `sound` and a `sample`: a pad plays one of them, so write one"
                )));
            }
            (Some(sound), None) => Source::Sound(sound),
            (None, Some(asset)) => Source::Sample(asset),
            (None, None) => default.source.clone(),
        };
        *default = Pad {
            source,
            volume_db: record.volume_db.unwrap_or(default.volume_db),
            pitch_semitones: record.pitch_semitones.unwrap_or(default.pitch_semitones),
            decay_ms: record.decay_ms.unwrap_or(default.decay_ms),
            pan: record.pan.unwrap_or(default.pan),
            choke: record.choke.unwrap_or(default.choke),
        };
    }
    Ok(pads)
}

impl State for DrumPadState {
    const TOOL: &'static str = "drum-pad";

    fn validate(&self) -> Result<(), String> {
        for (index, pad) in self.pads.iter().enumerate() {
            for parameter in &PARAMETERS[index] {
                parameter
                    .check(pad)
                    .map_err(|error| format!("pad \"{}\": {error}", note_of(index)))?;
            }
        }
        Ok(())
    }
}

/// The doc of the Drum pad record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "drums",
    when: "You write a drum part, or give a track the Drum pad and change its sounds",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the Drum pad tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    // A pad whose file is not there yet plays it once the file arrives.
    registry
        .tool::<DrumPadState>(EXTENSION)?
        .behaviour(apply)
        .rebinds_on_assets(sound_media::AUDIO_FOLDER);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// The name of the peaks of pad `pad`, which say how loud it sounds for the card.
pub fn peaks_name(pad: usize) -> String {
    format!("pad-{}", note_of(pad))
}

/// The name of the processor, for [`sound_core::Project::send`]: a click on a pad plays it.
pub const PROCESSOR: &str = "drums";

/// Runs for every valid state, from every source. The sound of each pad is made or loaded
/// here, on the control thread, and kept while anything plays it, so an edit of a volume or a
/// pan makes nothing again. The processor is kept between runs, so what sounds goes on.
fn apply(state: &DrumPadState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let rate = context.prepare_config().sample_rate;
    let peaks = std::array::from_fn(|pad| context.peaks(&peaks_name(pad)));
    let mut problems = Vec::new();
    let pads = std::array::from_fn(|index| {
        let pad = &state.pads[index];
        match render(pad, context.assets(), rate) {
            Ok(sound) => processor::PadPlay::new(pad, Some(sound), rate),
            Err(error) => {
                let name = pad.name(index);
                problems.push(format!(
                    "pad \"{}\" ({name}) is silent: {error}. The rest of the pads play",
                    note_of(index)
                ));
                processor::PadPlay::new(pad, None, rate)
            }
        }
    });
    for problem in problems {
        context.problem(problem);
    }
    let drums = context.processor(PROCESSOR, || DrumPad::new(peaks))?;
    context.update(drums, DrumUpdate::kit(pads))?;
    context.input(NOTES_INPUT, InputEndpoint::new(drums, DrumPad::NOTES));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(drums, DrumPad::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<DrumPadState, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }

    #[test]
    fn the_pads_are_notes_36_to_51_on_the_general_midi_drum_map() {
        let names: Vec<&str> = KIT.iter().map(|pad| pad.name).collect();
        assert_eq!(
            names,
            [
                "Kick",
                "Rim",
                "Snare",
                "Clap",
                "Snare 2",
                "Tom 1",
                "Hat",
                "Tom 2",
                "Pedal hat",
                "Tom 3",
                "Open hat",
                "Tom 4",
                "Tom 5",
                "Crash",
                "Tom 6",
                "Ride"
            ]
        );
        let choke: Vec<&str> = KIT
            .iter()
            .filter(|pad| pad.choke)
            .map(|pad| pad.name)
            .collect();
        assert_eq!(choke, ["Hat", "Pedal hat", "Open hat"]);
        assert_eq!(pad_of(Pitch::new(36).unwrap()), Some(0));
        assert_eq!(pad_of(Pitch::new(51).unwrap()), Some(15));
        assert_eq!(pad_of(Pitch::new(35).unwrap()), None);
        assert_eq!(pad_of(Pitch::new(52).unwrap()), None);
        // The toms rise in pitch in the order of the map: low floor, high floor, low, low-mid,
        // high-mid, high.
        let toms: Vec<f32> = KIT
            .iter()
            .filter(|pad| pad.sound == Sound::Tom)
            .map(|pad| pad.pitch_semitones)
            .collect();
        assert!(toms.windows(2).all(|pair| pair[0] < pair[1]), "{toms:?}");
    }

    #[test]
    fn a_pad_or_a_field_left_out_takes_the_default_of_its_pad() {
        assert_eq!(parse("{}"), Ok(DrumPadState::default()));
        assert_eq!(parse(r#"{"pads": {}}"#), Ok(DrumPadState::default()));
        let state =
            parse(r#"{"pads": {"42": {"decay_ms": 80.0}, "48": {"sample": "shaker.wav"}}}"#)
                .unwrap();
        let mut expected = DrumPadState::default();
        expected.pads[6].decay_ms = 80.0;
        expected.pads[12].source = Source::Sample(AudioAsset::new("shaker.wav").unwrap());
        assert_eq!(state, expected);
        for sound in Sound::ALL {
            let json = format!(r#"{{"pads": {{"36": {{"sound": "{}"}}}}}}"#, sound.key());
            assert_eq!(
                parse(&json).map(|state| state.pads[0].source.clone()),
                Ok(Source::Sound(sound))
            );
        }
    }

    #[test]
    fn a_pad_outside_the_notes_both_sources_or_an_unknown_field_do_not_load() {
        let outside = parse(r#"{"pads": {"52": {}}}"#);
        assert!(
            outside
                .as_ref()
                .is_err_and(|error| error.contains(r#"the pads are the notes "36" to "51""#)),
            "{outside:?}"
        );
        assert!(parse(r#"{"pads": {"kick": {}}}"#).is_err());
        let both = parse(r#"{"pads": {"36": {"sound": "kick", "sample": "kick.wav"}}}"#);
        assert!(both.is_err_and(|error| error.contains("a pad plays one of them")));
        let unknown = parse(r#"{"pads": {"36": {"gain_db": 3.0}}}"#);
        assert!(unknown.is_err_and(|error| error.contains("unknown field `gain_db`")));
        let sound = parse(r#"{"pads": {"36": {"sound": "cowbell"}}}"#);
        assert!(sound.is_err_and(|error| error.contains("unknown variant `cowbell`")));
    }

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        for pad in 0..PADS {
            for parameter in &PARAMETERS[pad] {
                for value in [parameter.min, parameter.default, parameter.max] {
                    let mut state = DrumPadState::default();
                    (parameter.set)(&mut state.pads[pad], value);
                    assert_eq!((parameter.get)(&state.pads[pad]), value);
                    assert_eq!(state.validate(), Ok(()));
                }
                let mut state = DrumPadState::default();
                (parameter.set)(&mut state.pads[pad], parameter.max * 2.0 + 1.0);
                let error = state.validate().unwrap_err();
                assert!(error.contains(parameter.field), "{error}");
                assert!(
                    error.starts_with(&format!("pad \"{}\": ", note_of(pad))),
                    "{error}"
                );
            }
        }
    }

    /// The runtime writes every field of every pad, so an agent reads the whole kit, and what
    /// it writes loads back as it was.
    #[test]
    fn the_record_is_written_whole_and_reads_back() {
        let json = serde_json::to_string(&DrumPadState::default()).unwrap();
        assert!(
            json.starts_with(r#"{"pads":{"36":{"sound":"kick","volume_db":0.0,"pitch_semitones":0.0,"decay_ms":600.0,"pan":0.0,"choke":false},"37":"#),
            "{json}"
        );
        assert_eq!(parse(&json), Ok(DrumPadState::default()));
        let mut state = DrumPadState::default();
        state.pads[12].source = Source::Sample(AudioAsset::new("shaker.wav").unwrap());
        let json = serde_json::to_string(&state).unwrap();
        assert!(
            json.contains(r#""48":{"sample":"shaker.wav","volume_db""#),
            "{json}"
        );
        assert_eq!(parse(&json), Ok(state));
    }

    #[test]
    fn a_pad_is_named_by_its_kit_its_sound_or_its_file() {
        let mut pad = Pad::default_at(12);
        assert_eq!(pad.name(12), "Tom 5");
        pad.source = Source::Sound(Sound::Clap);
        assert_eq!(pad.name(12), "Clap");
        pad.source = Source::Sample(AudioAsset::new("shaker-loop.wav").unwrap());
        assert_eq!(pad.name(12), "shaker-loop");
        assert_eq!(Pad::default_at(8).name(8), "Pedal hat");
    }
}
