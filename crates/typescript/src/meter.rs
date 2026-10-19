//! The `meter`: a child of a parent of the project that hears a sound on its input `audio` and
//! keeps its level and its last sound, for a view to show with `now.peak` and `now.scope`. It
//! plays nothing on.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    AudioInput, BehaviourContext, BehaviourError, InputEndpoint, JsonTool, JsonToolDoc, Peaks,
    Ports, PrepareConfig, ProcessContext, Processor, Scope, State, ToolSchema,
};

pub(crate) const NAME: &str = "meter";
/// What its peaks and its scope are kept under, which `now.peak` and `now.scope` read.
pub(crate) const LEVEL: &str = "level";
const PROCESSOR: &str = "meter";

#[derive(Debug, Clone, Default, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {}

impl State for Record {
    const TOOL: &'static str = NAME;
}

/// The tool for the core, which the tools of the project define with theirs.
pub(crate) fn json_tool() -> JsonTool {
    JsonTool {
        name: NAME.to_string(),
        check: Arc::new(|state| record(state).map(drop)),
        doc: Some(JsonToolDoc {
            when: "A parent of this project shows the level or the sound of something it plays"
                .to_string(),
            markdown: include_str!("meter.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
        end: None,
    }
}

/// The record of the meter, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema::of::<Record>(&[])
}

fn record(state: &Value) -> Result<Record, String> {
    serde_json::from_value(state.clone()).map_err(|error| format!("state: {error}"))
}

fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    record(state).map_err(BehaviourError::Other)?;
    let (peaks, scope) = (context.peaks(LEVEL), context.scope(LEVEL));
    let meter = context.processor(PROCESSOR, || Meter::new(peaks, scope))?;
    context.input("audio", InputEndpoint::new(meter, Meter::INPUT));
    Ok(())
}

pub(crate) struct Meter {
    peaks: Peaks,
    scope: Scope,
}

impl Meter {
    pub(crate) const INPUT: AudioInput = AudioInput::new(0);

    pub(crate) fn new(peaks: Peaks, scope: Scope) -> Self {
        Self { peaks, scope }
    }
}

impl Processor for Meter {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_input(Self::INPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let inputs = context.audio_inputs.get(Self::INPUT);
        self.peaks.record_block(inputs);
        self.scope.write(inputs);
    }
}
