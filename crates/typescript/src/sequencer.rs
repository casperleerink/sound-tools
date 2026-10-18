//! The `sequencer`: a child of a parent of the project that loops note patterns in time with
//! the bars and sends them out of its output `notes`. Its record says what plays before any
//! launch, so a render plays it; a launch is a number into its input `launch`, from a cable or
//! from a performer, and nothing saves it.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, EventInput, EventOutput, InputEndpoint, JsonTool,
    JsonToolDoc, OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, Ticks,
    ToolSchema,
};
use sound_notes::{
    Launch, Length, Note, NoteEvent, NotePlayer, NotePlayerUpdate, Pattern, PlayerWatches, Quantize,
};
use sound_signals::Number;

use crate::tools::key;

pub(crate) const NAME: &str = "sequencer";
/// Its input of launches, which a performer reaches as a live control.
pub(crate) const LAUNCH: &str = "launch";
const NOTES: &str = "notes";
/// The name of its processor in its instance.
pub(crate) const PROCESSOR: &str = "sequencer";

#[derive(Debug, Hash, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    /// The patterns it loops, one at a time.
    #[serde(default)]
    patterns: Vec<PatternRecord>,
    /// The pattern it plays before any launch, by its index from 0, so a render plays it; a
    /// change launches it.
    #[serde(default)]
    plays: Option<usize>,
    /// Where a launch lands while the piece plays. While it is stopped, at once.
    #[serde(default)]
    quantize: Quantize,
}

/// Notes that loop.
#[derive(Debug, Hash, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PatternRecord {
    /// In ticks: 3840 is a bar of 4/4.
    length: u64,
    /// Notes that start inside it, in ticks from its start.
    notes: Vec<Note>,
}

/// The tool for the core, which the tools of the project define with theirs.
pub(crate) fn json_tool() -> JsonTool {
    JsonTool {
        name: NAME.to_string(),
        check: Arc::new(|state| record(state).map(drop)),
        doc: Some(JsonToolDoc {
            when: "A parent of this project plays note patterns that loop in time with the bars"
                .to_string(),
            markdown: include_str!("sequencer.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
    }
}

/// The record of the sequencer, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        tool: NAME,
        schema: schemars::schema_for!(Record).to_value(),
        numbers: Vec::new(),
        in_code: false,
        check: |state| record(&state).map(drop),
    }
}

/// The record of `state`, or what is wrong with it, by its path.
fn record(state: &Value) -> Result<Record, String> {
    serde_path_to_error::deserialize(state).map_err(|error| {
        let path = error.path().to_string();
        match path.as_str() {
            "." => format!("state: {}", error.inner()),
            _ => format!("state.{path}: {}", error.inner()),
        }
    })
}

/// Plays the patterns of the record, and launches `plays` when the sequencer is first declared
/// and each time `plays` changes, as a launch at its `quantize`: while the project does not
/// play that lands at once, so a render from the start plays it from tick 0.
fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let record = record(state).map_err(BehaviourError::Other)?;
    let watches = PlayerWatches {
        playing: context.watch("playing"),
        queued: context.watch("queued"),
        phase: context.watch("phase"),
    };
    let node = context.processor(PROCESSOR, || Sequencer {
        player: NotePlayer::watched(watches),
        quantize: Quantize::default(),
        heard: None,
    })?;
    // Checked every run, so a problem stays as long as the pattern does.
    let mut patterns = Vec::with_capacity(record.patterns.len());
    for (index, pattern) in record.patterns.iter().enumerate() {
        patterns.push(pattern_of(pattern, &format!("patterns[{index}]"), context)?);
    }
    if let Some(plays) = record.plays
        && plays >= patterns.len()
    {
        context.problem(format!(
            "plays {plays} is no pattern: there are {}, from 0",
            patterns.len()
        ));
    }
    if context.changed("patterns", key(&record.patterns)) {
        let patterns = NotePlayerUpdate::Patterns(Arc::new(patterns));
        context.update(node, SequencerUpdate::Player(patterns))?;
    }
    if context.changed("quantize", key(&record.quantize)) {
        context.update(node, SequencerUpdate::Quantize(record.quantize))?;
    }
    if context.changed("plays", key(&record.plays)) {
        context.update(node, SequencerUpdate::Launch(record.plays))?;
    }
    context.output(NOTES, OutputEndpoint::new(node, Sequencer::NOTES));
    context.input(LAUNCH, InputEndpoint::new(node, Sequencer::LAUNCH));
    Ok(())
}

/// The pattern `declared` at `at` of the record. One that cannot play, such as one with a note
/// outside it, is silent and a problem.
fn pattern_of(
    declared: &PatternRecord,
    at: &str,
    context: &mut BehaviourContext<'_>,
) -> Result<Pattern, BehaviourError> {
    let made = Length::new(Ticks(declared.length))
        .map_err(|error| format!("{at}.{error}"))
        .and_then(|length| {
            Pattern::new(length, &declared.notes).map_err(|error| format!("{at}.{error}"))
        });
    match made {
        Ok(pattern) => Ok(pattern),
        Err(error) => {
            context.problem(format!("a pattern cannot play, so it is silent: {error}"));
            let silent = Pattern::new(Length::at_least_one(Ticks(declared.length)), &[]);
            silent.map_err(|error| BehaviourError::Other(error.to_string()))
        }
    }
}

/// A [`NotePlayer`] of patterns that launches what its input `launch` hears.
pub(crate) struct Sequencer {
    player: NotePlayer,
    quantize: Quantize,
    /// What the input last launched: a number a cable sends every block launches once.
    heard: Option<Option<usize>>,
}

pub(crate) enum SequencerUpdate {
    /// Its patterns. The old ones ride back in here.
    Player(NotePlayerUpdate),
    Quantize(Quantize),
    /// Plays the pattern of this index from its quantize point, or stops with `None`.
    Launch(Option<usize>),
}

impl Sequencer {
    pub(crate) const LAUNCH: EventInput<Number> = EventInput::new(0);
    pub(crate) const NOTES: EventOutput<NoteEvent> = NotePlayer::NOTES;

    fn launch(&mut self, pattern: Option<usize>) {
        let launch = Launch {
            pattern,
            quantize: self.quantize,
        };
        self.player.update(&mut NotePlayerUpdate::Launch(launch));
    }
}

/// The pattern a number launches: its index, rounded, or a stop below 0.
pub(crate) fn launched(value: f32) -> Option<usize> {
    (value > -0.5).then(|| value.round() as usize)
}

impl Processor for Sequencer {
    type Update = SequencerUpdate;

    fn ports(&self) -> Ports {
        self.player.ports().event_input(Self::LAUNCH)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.player.prepare(config);
    }

    fn update(&mut self, update: &mut SequencerUpdate) {
        match update {
            SequencerUpdate::Player(update) => self.player.update(update),
            SequencerUpdate::Quantize(quantize) => self.quantize = *quantize,
            SequencerUpdate::Launch(pattern) => self.launch(*pattern),
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        // The last of the block: two cables would launch each other's pattern again every block.
        if let Some(timed) = context.event_inputs.get(Self::LAUNCH).last() {
            let pattern = launched(timed.event.0);
            if self.heard != Some(pattern) {
                self.heard = Some(pattern);
                self.launch(pattern);
            }
        }
        self.player.process(context);
    }
}
