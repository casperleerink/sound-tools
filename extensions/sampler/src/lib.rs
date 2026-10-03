//! Sampler: one sample played across the keyboard, or an SFZ instrument of many. It plays the
//! note events of the `sound-notes` contract, and reads its files through `sound-media`.
//!
//! A record on disk, `instrument.json` inside a track folder:
//!
//! ```json
//! {
//!   "tool": "sampler",
//!   "state": {
//!     "sample": "kalimba.wav",
//!     "root": 60,
//!     "start_seconds": 0.0,
//!     "attack_seconds": 0.002,
//!     "decay_seconds": 0.4,
//!     "sustain": 1.0,
//!     "release_seconds": 0.3,
//!     "velocity_to_volume": 0.5,
//!     "gain_db": 0.0
//!   }
//! }
//! ```
//!
//! With `"sfz": "cello/cello.sfz"` instead of `sample` it plays the SFZ file
//! `assets/instruments/cello/cello.sfz` and the samples it names ([`sfz`], [`instrument`]).
//!
//! [`view`] is the card of the sampler, and the only module here that uses GPUI.

mod catalog;
pub mod instrument;
pub mod library;
mod processor;
pub mod sfz;
pub mod view;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, Scale, State,
};
use sound_media::{Audio, AudioAsset, MediaError};
use sound_notes::{AUDIO_OUTPUT, NOTES_INPUT, Pitch};

pub use instrument::{INSTRUMENTS_FOLDER, Instrument, SfzPath};
pub use library::{LibraryId, Status};
pub use processor::{LAYERS, Sampler, SamplerUpdate, VOICES};

/// The name to enable in `project.json`, and the tool.
pub const EXTENSION: &str = "sampler";

/// The peaks the sampler keeps its place in the sample in: where the last note it started is
/// now, in seconds of the file. The card draws it as a green line.
pub const POSITION: &str = "position";

/// The saved state. Voices and where they are in the sample are runtime state and not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is a sampler with no
/// sample, which is silent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SamplerState {
    /// The file it plays, under `assets/audio/`. None is an empty sampler, unless it has `sfz`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<AudioAsset>,
    /// The SFZ instrument it plays instead of `sample`, under `assets/instruments/`. Its zones
    /// have their own pitch, part of the file and envelope, so only the gain of the record
    /// applies to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sfz: Option<SfzPath>,
    /// An instrument of the library instead, by its id. It is downloaded on first use, and
    /// plays like an SFZ instrument.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<LibraryId>,
    /// The key that plays the sample at its own pitch.
    pub root: Pitch,
    /// Where in the file a note starts, in seconds of the file.
    pub start_seconds: f64,
    /// Where in the file a note ends at the latest, in seconds of the file. None is the end of
    /// the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_seconds: Option<f64>,
    /// From note on to full level.
    pub attack_seconds: f32,
    /// From full level to within 0.1 % of the sustain level.
    pub decay_seconds: f32,
    /// The level a held note settles at, as a part of full level.
    pub sustain: f32,
    /// From note off to silence, for a note at full level.
    pub release_seconds: f32,
    /// How much the velocity of a note changes its volume: 0 plays every note at full volume,
    /// 1 plays velocity 64 a quarter as loud as 127.
    pub velocity_to_volume: f32,
    pub gain_db: f32,
}

/// One number of the saved state, with its range and its default. See [`sound_core::Parameter`].
pub type Parameter = sound_core::Parameter<SamplerState>;

/// An envelope time. The lower end keeps every stage long enough not to click.
const fn time(
    field: &'static str,
    default: f32,
    get: fn(&SamplerState) -> f32,
    set: fn(&mut SamplerState, f32),
) -> Parameter {
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

/// The root is a key, so it is saved as a whole note number; as a parameter it is that number.
pub const ROOT: Parameter = Parameter {
    field: "root",
    min: 0.0,
    max: 127.0,
    default: 60.0,
    scale: Scale::Linear,
    get: |state| f32::from(state.root.number()),
    set: |state, value| state.root = Pitch::nearest(value.round() as i64),
};
pub const ATTACK: Parameter = time(
    "attack_seconds",
    0.002,
    |state| state.attack_seconds,
    |state, value| state.attack_seconds = value,
);
pub const DECAY: Parameter = time(
    "decay_seconds",
    0.4,
    |state| state.decay_seconds,
    |state, value| state.decay_seconds = value,
);
pub const SUSTAIN: Parameter = Parameter {
    field: "sustain",
    min: 0.0,
    max: 1.0,
    default: 1.0,
    scale: Scale::Linear,
    get: |state| state.sustain,
    set: |state, value| state.sustain = value,
};
pub const RELEASE: Parameter = time(
    "release_seconds",
    0.3,
    |state| state.release_seconds,
    |state, value| state.release_seconds = value,
);
pub const VELOCITY: Parameter = Parameter {
    field: "velocity_to_volume",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    scale: Scale::Linear,
    get: |state| state.velocity_to_volume,
    set: |state, value| state.velocity_to_volume = value,
};
/// The range of the gain of an audio clip, so a sample and a clip of it go as far.
pub const GAIN: Parameter = Parameter {
    field: "gain_db",
    min: -48.0,
    max: 24.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |state| state.gain_db,
    set: |state, value| state.gain_db = value,
};

/// Every number of the state with a fixed range, in the order of its fields. Start and end are
/// seconds of the file, whose range is the length of the file.
pub const PARAMETERS: [&Parameter; 7] =
    [&ROOT, &ATTACK, &DECAY, &SUSTAIN, &RELEASE, &VELOCITY, &GAIN];

impl Default for SamplerState {
    fn default() -> Self {
        Self {
            sample: None,
            sfz: None,
            library: None,
            root: Pitch::nearest(ROOT.default as i64),
            start_seconds: 0.0,
            end_seconds: None,
            attack_seconds: ATTACK.default,
            decay_seconds: DECAY.default,
            sustain: SUSTAIN.default,
            release_seconds: RELEASE.default,
            velocity_to_volume: VELOCITY.default,
            gain_db: GAIN.default,
        }
    }
}

impl State for SamplerState {
    const TOOL: &'static str = EXTENSION;

    fn validate(&self) -> Result<(), String> {
        let sources = [
            self.sample.is_some(),
            self.sfz.is_some(),
            self.library.is_some(),
        ];
        if sources.into_iter().filter(|source| *source).count() > 1 {
            return Err(
                "a Sampler plays one of `sample`, `sfz` and `library`: leave the others out"
                    .to_string(),
            );
        }
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))?;
        let start = self.start_seconds;
        if !(start.is_finite() && start >= 0.0) {
            return Err(format!(
                "start_seconds must be a number of seconds, 0 or more, not {start}"
            ));
        }
        match self.end_seconds {
            Some(end) if !(end.is_finite() && end > start) => Err(format!(
                "end_seconds must be after start_seconds ({start}), not {end}"
            )),
            _ => Ok(()),
        }
    }
}

/// The doc of the sampler record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "sampler",
    when: "A track plays an audio file across the keyboard, or a sampled instrument (SFZ)",
    markdown: include_str!("../agent-doc.md"),
};

/// How to write an SFZ instrument, apart, so an agent reads it only to make one.
pub const SFZ_AGENT_DOC: AgentDoc = AgentDoc {
    name: "sfz",
    when: "Making an SFZ instrument from samples",
    markdown: include_str!("../sfz-agent-doc.md"),
};

/// The Samplers of the project of `assets` whose library instrument finished downloading, or
/// whose instrument finished loading in the background, since the last call. Run their
/// behaviour again (`Project::rebind`), once per poll of the session.
pub fn take_ready(assets: &sound_core::Assets) -> Vec<sound_core::InstanceId> {
    let mut ready = library::take_finished(assets);
    ready.extend(instrument::take_loaded(assets));
    ready
}

/// The instruments of the library, apart, so an agent reads them only to pick one.
pub const LIBRARY_AGENT_DOC: AgentDoc = AgentDoc {
    name: "library",
    when: "Picking a sampled piano, strings, brass, guitar, drums and more",
    markdown: include_str!("../library-agent-doc.md"),
};

/// Registers the sampler tool. Call it before the project opens.
///
/// A sampler whose file is missing runs its behaviour again when a file under `assets/audio/`
/// or `assets/instruments/` arrives, so an agent may write the record first and copy the files
/// in after it.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<SamplerState>(EXTENSION)?
        .behaviour(apply)
        .rebinds_on_assets(sound_media::AUDIO_FOLDER)
        .rebinds_on_assets(INSTRUMENTS_FOLDER);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    registry.agent_doc(EXTENSION, SFZ_AGENT_DOC)?;
    registry.agent_doc(EXTENSION, LIBRARY_AGENT_DOC)?;
    Ok(())
}

/// What the state plays, read on this thread the first time something names it, or a line for
/// `problems.txt` that says why it plays nothing. An empty line is an empty Sampler.
/// `None` while it loads in the background, see [`instrument::load_in_background`].
fn instrument(
    state: &SamplerState,
    context: &mut BehaviourContext<'_>,
) -> Result<Option<Arc<Instrument>>, String> {
    let instrument = match (&state.library, &state.sfz) {
        (Some(id), _) => from_library(id.entry(), context)?,
        (None, Some(path)) => instrument::load_sfz(context.assets(), path, context.id())?,
        (None, None) => {
            return sample(state, context)
                .map(|audio| Some(Arc::new(Instrument::of_sample(audio))));
        }
    };
    if let Some(problem) = instrument
        .as_ref()
        .and_then(|instrument| instrument.problem())
    {
        context.problem(problem.to_string());
    }
    Ok(instrument)
}

/// A library instrument when it is on this machine. Else the Sampler waits for a download of
/// it, which only the composer starts, and the line says how that goes.
fn from_library(
    entry: &'static library::Entry,
    context: &mut BehaviourContext<'_>,
) -> Result<Option<Arc<Instrument>>, String> {
    if library::status(entry) != Status::Here {
        library::wait_for(entry, context.assets(), context.id());
    }
    let name = entry.name;
    let size = library::size_text(entry.disk_bytes);
    match library::status(entry) {
        Status::Here => instrument::load_library(entry, context.assets(), context.id()),
        Status::Downloading { .. } => Err(format!(
            "the Sampler is silent while {name} downloads into the library of this machine; it plays when it is done"
        )),
        Status::Failed(error) => Err(format!(
            "the download of {name} failed, so the Sampler is silent: {error}. Download on the Sampler card tries again"
        )),
        Status::Missing => Err(format!(
            "{name} is not downloaded on this machine, so the Sampler is silent. Ask the composer to click Download on the Sampler card ({size} on disk)"
        )),
        Status::NoLibrary => Err(format!(
            "{name} cannot play: this machine has no library folder"
        )),
    }
}

/// The file of the state, see [`sound_media::load`].
fn sample(state: &SamplerState, context: &BehaviourContext<'_>) -> Result<Arc<Audio>, String> {
    let Some(asset) = &state.sample else {
        return Err(String::new());
    };
    let audio = sound_media::load(context.assets(), asset).map_err(|error| match error {
        MediaError::Missing { path } => format!(
            "the sample {path} is not there, so the Sampler is silent. Copy the file into assets/audio/ under that name and it plays, or correct `sample`"
        ),
        error => format!("the Sampler is silent: {error}"),
    })?;
    if state.start_seconds >= audio.seconds() {
        return Err(format!(
            "the Sampler plays nothing: start_seconds {} is at or past the end of {asset}, which is {:.3} s long",
            state.start_seconds,
            audio.seconds()
        ));
    }
    Ok(audio)
}

/// Runs for every valid state, from every source. The processor is kept between runs, so held
/// notes go on through parameter edits.
fn apply(state: &SamplerState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let update = match instrument(state, context) {
        Ok(Some(instrument)) => SamplerUpdate::new(state, Some(instrument)),
        // It plays what it played until the new one is there.
        Ok(None) => SamplerUpdate::keeping(state),
        Err(problem) => {
            if !problem.is_empty() {
                context.problem(problem);
            }
            SamplerUpdate::new(state, None)
        }
    };
    let position = context.peaks(POSITION);
    let sampler = context.processor("sampler", || Sampler::new(position))?;
    context.update(sampler, update)?;
    context.input(NOTES_INPUT, InputEndpoint::new(sampler, Sampler::NOTES));
    context.automation(sampler, Sampler::AUTOMATION);
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(sampler, Sampler::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent doc gives the ranges to agents, and the defaults in an example that another test
    /// loads. They are checked against the one definition, so they cannot drift from it.
    #[test]
    fn the_docs_give_the_range_of_every_parameter() {
        let docs = [("agent-doc.md", include_str!("../agent-doc.md"))];
        for (name, doc) in docs {
            for Parameter {
                field, min, max, ..
            } in PARAMETERS
            {
                let row = format!("| `{field}` |");
                let row = doc.lines().find(|line| line.starts_with(&row));
                let row = row.unwrap_or_else(|| panic!("{name} has no row for {field}"));
                assert!(
                    row.contains(&format!("| {min} to {max} |")),
                    "{name}: {row}"
                );
            }
        }
    }

    /// The library doc lists every instrument of the catalog with the disk it takes,
    /// so it cannot drift from the catalog.
    #[test]
    fn the_library_doc_lists_every_instrument() {
        let doc = include_str!("../library-agent-doc.md");
        for entry in library::CATALOG {
            let row = format!("| `{}` | {} |", entry.id, entry.name);
            let row = doc.lines().find(|line| line.starts_with(&row));
            let row = row.unwrap_or_else(|| panic!("no row for {}", entry.id));
            let size = library::size_text(entry.disk_bytes);
            assert!(row.ends_with(&format!("| {size} |")), "{row}");
        }
        let rows = doc.lines().filter(|line| line.starts_with("| `")).count();
        assert_eq!(rows, library::CATALOG.len());
    }

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        for parameter in PARAMETERS {
            for value in [parameter.min, parameter.default, parameter.max] {
                let mut state = SamplerState::default();
                (parameter.set)(&mut state, value);
                assert_eq!((parameter.get)(&state), value);
                assert_eq!(state.validate(), Ok(()));
            }
        }
        for (field, value) in [("attack_seconds", 20.0), ("gain_db", 30.0)] {
            let mut state = SamplerState::default();
            let parameter = PARAMETERS.iter().find(|p| p.field == field).unwrap();
            (parameter.set)(&mut state, value);
            assert!(state.validate().is_err_and(|error| error.contains(field)));
        }
    }

    #[test]
    fn the_part_of_the_file_that_plays_starts_before_it_ends() {
        let state = |start, end| SamplerState {
            start_seconds: start,
            end_seconds: end,
            ..SamplerState::default()
        };
        assert_eq!(state(0.5, Some(1.0)).validate(), Ok(()));
        assert_eq!(state(0.5, None).validate(), Ok(()));
        assert!(state(-0.1, None).validate().is_err());
        assert!(state(f64::NAN, None).validate().is_err());
        let error = state(1.0, Some(1.0)).validate().unwrap_err();
        assert_eq!(error, "end_seconds must be after start_seconds (1), not 1");
    }

    #[test]
    fn a_record_is_short_and_an_empty_one_is_the_default() {
        let empty: SamplerState = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, SamplerState::default());
        let written = serde_json::to_string(&SamplerState::default()).unwrap();
        assert!(!written.contains("sample\""), "{written}");
        assert!(!written.contains("end_seconds"), "{written}");
        let wrong = serde_json::from_str::<SamplerState>(r#"{"root": 128}"#);
        assert!(wrong.is_err());
        let wrong = serde_json::from_str::<SamplerState>(r#"{"sample": "../kick.wav"}"#);
        assert!(wrong.is_err());
    }
}
