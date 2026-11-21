//! Analyzer: a built-in effect that shows the sound and leaves it as it is. It goes in an effect
//! slot of a track, and its card shows the spectrum, the level (peak and loudness) and the note
//! the sound plays, with how far off it is in cents.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`. It has
//! nothing to set:
//!
//! ```json
//! {
//!   "tool": "analyzer",
//!   "state": {}
//! }
//! ```
//!
//! The processor only copies its input to its output, into a [`Scope`] and its peaks. Its feeds
//! work out what a card shows from the scope, off the audio thread, with the measuring code of
//! `--analyze` ([`sound_media::analysis`]).
//!
//! [`Scope`]: sound_core::Scope

mod analysis;
mod processor;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::feeds::{FeedContext, SILENT_DB};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

use analysis::{Analysis, Reading};
pub use processor::Analyzer;

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "analyzer";

/// The saved state: nothing. What the card shows is runtime state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerState {}

impl State for AnalyzerState {
    const TOOL: &'static str = "analyzer";
}

/// The doc of the analyzer record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "analyzer",
    group: "Devices",
    when: "The composer wants to see the spectrum, level or pitch of a track",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the analyzer tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<AnalyzerState>(EXTENSION)?
        .behaviour(apply)
        .schema(&[])
        .feed(
            "spectrum",
            "The level of each of 96 bands from 20 Hz to 20 kHz, evenly in octaves, in dB from -90 up; the level of a sine in its band.",
            |_, context| listen(context, |reading| reading.spectrum.to_vec()),
        )
        .feed(
            "loudness",
            "The loudness of the last 400 ms, in LUFS; -120 for silence.",
            |_, context| {
                listen(context, |reading| reading.loudness.unwrap_or(SILENT_DB))
            },
        )
        .feed(
            "note",
            "The note it hears, a MIDI note number with its fraction (69 is A4 at 440 Hz); 0 while it hears none.",
            |_, context| listen(context, |reading| reading.note.unwrap_or(0.0) as f32),
        );
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// What the analyzer heard, while a view shows it.
#[derive(Default)]
struct Listening {
    analysis: Option<Analysis>,
    /// The count of frames of the scope to read from next.
    next: u64,
    /// The frame it last looked in, so every feed of a frame reads one look.
    looked: Option<u64>,
}

/// Hears the frames that came since the last frame, looks once a frame, and reads what it found.
fn listen<T>(context: &FeedContext<'_>, read: impl FnOnce(&Reading) -> T) -> T {
    let scope = context.scope(Analyzer::SCOPE);
    let (frame, seconds, rate) = (context.frame(), context.seconds(), context.sample_rate);
    context.kept("listening", |listening: &mut Listening| {
        let analysis = listening
            .analysis
            .get_or_insert_with(|| Analysis::new(rate));
        if listening.looked != Some(frame) {
            listening.looked = Some(frame);
            if let Some(scope) = scope {
                let mut frames = Vec::new();
                listening.next = scope.read(listening.next, &mut frames);
                analysis.hear(&frames);
            }
            analysis.look(seconds);
        }
        read(analysis.reading())
    })
}

/// Runs for every valid state, from every source.
fn apply(_: &AnalyzerState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let (scope, peaks) = (
        context.scope(Analyzer::SCOPE),
        context.peaks(Analyzer::PEAKS),
    );
    let analyzer = context.processor("analyzer", || Analyzer::new(scope, peaks))?;
    context.input(AUDIO_INPUT, InputEndpoint::new(analyzer, Analyzer::INPUT));
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(analyzer, Analyzer::OUTPUT),
    );
    Ok(())
}
