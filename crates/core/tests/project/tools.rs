//! Test-only tools and a small harness. The tools are unmusical on purpose: every processor
//! puts out a constant, so a rendered sample shows the state exactly.
//!
//! - `test.dc`: one record, one processor, one output. Connected through `project.json`.
//! - `test.bank`: a composite. It owns many `test.level` records, which are data only, and
//!   reads them into one snapshot for its one processor. It sends its signal through its
//!   owned `output` child, a `test.amplifier`, and on to the device with no `project.json`
//!   connection. This is the shape of a track with clips and an instrument.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sound_core::{
    AudioInput, AudioOutput, BehaviourContext, BehaviourError, Engine, EngineConfig, InputEndpoint,
    InstanceId, OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, Project, Registry,
    State,
};

pub const EXTENSION: &str = "test";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dc {
    pub value: f32,
}

impl State for Dc {
    const TOOL: &'static str = "test.dc";

    fn validate(&self) -> Result<(), String> {
        if (-1.0..=1.0).contains(&self.value) {
            Ok(())
        } else {
            Err(format!("value must be from -1 to 1, not {}", self.value))
        }
    }
}

/// Puts out the value it was last sent.
pub struct Constant(f32);

impl Constant {
    pub fn new(value: f32) -> Self {
        Self(value)
    }

    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for Constant {
    type Update = f32;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut f32) {
        self.0 = *update;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        context.audio_outputs.get(Self::OUTPUT).fill(self.0);
    }
}

fn apply_dc(state: &Dc, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let constant = context.processor("constant", || Constant::new(0.0))?;
    context.update(constant, state.value)?;
    context.output("out", OutputEndpoint::new(constant, Constant::OUTPUT));
    Ok(())
}

/// Data only: it has no behaviour. Its owner reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub value: f32,
}

impl State for Level {
    const TOOL: &'static str = "test.level";
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Amplifier {
    pub gain: f32,
}

impl State for Amplifier {
    const TOOL: &'static str = "test.amplifier";
}

pub struct Gain(f32);

impl Gain {
    pub fn new(gain: f32) -> Self {
        Self(gain)
    }

    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for Gain {
    type Update = f32;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut f32) {
        self.0 = *update;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let input = context.audio_inputs.get(Self::INPUT);
        let output = context.audio_outputs.get(Self::OUTPUT);
        for (output, input) in output.iter_mut().zip(input) {
            *output = input * self.0;
        }
    }
}

fn apply_amplifier(
    state: &Amplifier,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let gain = context.processor("gain", || Gain::new(0.0))?;
    context.update(gain, state.gain)?;
    context.input("in", InputEndpoint::new(gain, Gain::INPUT));
    context.output("out", OutputEndpoint::new(gain, Gain::OUTPUT));
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bank {
    pub gain: f32,
}

impl State for Bank {
    const TOOL: &'static str = "test.bank";
    const OWNS_CHILDREN: bool = true;
}

/// The name of the child a bank plays through.
pub const BANK_OUTPUT: &str = "output";

#[derive(Default)]
pub struct BankUpdate {
    gain: f32,
    levels: Arc<Vec<f32>>,
}

/// Puts out the sum of one immutable snapshot of levels, times a gain.
#[derive(Default)]
pub struct Summer(BankUpdate);

impl Summer {
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for Summer {
    type Update = BankUpdate;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut BankUpdate) {
        // The old snapshot rides back to the control thread inside the update.
        std::mem::swap(&mut self.0, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let sum: f32 = self.0.levels.iter().sum();
        context
            .audio_outputs
            .get(Self::OUTPUT)
            .fill(sum * self.0.gain);
    }
}

fn apply_bank(state: &Bank, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let levels: Vec<f32> = context
        .children::<Level>()
        .map(|(_, level)| level.value)
        .collect();
    let summer = context.processor("summer", Summer::default)?;
    let update = BankUpdate {
        gain: state.gain,
        levels: Arc::new(levels),
    };
    context.update(summer, update)?;

    // Through the owned output child when there is one, else straight to the device.
    let mut output = OutputEndpoint::new(summer, Summer::OUTPUT);
    if let Some(input) = context.child_input(BANK_OUTPUT, "in")
        && let Some(child_output) = context.child_output(BANK_OUTPUT, "out")
    {
        context.connect(output.to(input))?;
        output = child_output;
    }
    context.connect(output.to_device(0))?;
    context.output("out", output);
    Ok(())
}

pub fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.tool::<Dc>(EXTENSION).unwrap().behaviour(apply_dc);
    registry.tool::<Level>(EXTENSION).unwrap();
    registry
        .tool::<Amplifier>(EXTENSION)
        .unwrap()
        .behaviour(apply_amplifier);
    registry
        .tool::<Bank>(EXTENSION)
        .unwrap()
        .behaviour(apply_bank)
        .summary(summarize_bank);
    registry.agent_doc(EXTENSION, TEST_AGENT_DOC);
    registry
}

pub const TEST_AGENT_DOC: &str =
    "## Test tools\n\nA bar is {{ticks_per_bar}} ticks in {{time_signature}}.\n";

/// A bank tells what it owns in one line, so a summary does not list every level.
fn summarize_bank(project: &Project, bank: &sound_core::Instance<Bank>) -> String {
    let levels = project.children::<Level>(bank.id()).count();
    format!("bank {} with {levels} levels", bank.id())
}

pub const SAMPLE_RATE: u32 = 48_000;

/// An open project on a temporary folder with an offline mono engine.
pub struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// Last, so the folder outlives the project that holds its lock.
    pub folder: tempfile::TempDir,
}

impl Harness {
    pub fn new() -> Self {
        Self::open(tempfile::tempdir().unwrap())
    }

    pub fn open(folder: tempfile::TempDir) -> Self {
        let (project, engine) = open(folder.path());
        Self {
            project,
            engine,
            folder,
        }
    }

    /// Closes the project and opens the same folder again.
    pub fn reopen(self) -> Self {
        let Self {
            project,
            engine,
            folder,
        } = self;
        drop((project, engine));
        Self::open(folder)
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    /// Writes a file the way an agent would, without telling the project.
    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }

    pub fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path(relative)).unwrap()
    }

    /// Writes a file and applies it, as the watcher would.
    pub fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.write(relative, contents);
        self.project.apply_outside_changes(&[path]).unwrap()
    }

    /// Renders one device buffer and gives its last sample. Every test processor puts out a
    /// constant, so one sample shows the state.
    pub fn level(&mut self) -> f32 {
        let mut buffer = [0.0; 480];
        self.engine.process_block(&mut buffer);
        buffer[479]
    }

    pub fn batches(&mut self) -> u64 {
        self.level();
        self.project.engine().poll().unwrap().batches_applied
    }

    pub fn problem_at(&self, path: &str) -> Option<String> {
        let problems = self.project.problems();
        let problem = problems.iter().find(|problem| problem.path == path)?;
        Some(problem.message.clone())
    }
}

pub fn open(folder: &Path) -> (Project, Engine) {
    let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let project = Project::open(folder, registry(), control).unwrap();
    (project, engine)
}

pub fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

pub fn dc_record(value: f32) -> String {
    format!(r#"{{"tool": "test.dc", "state": {{"value": {value}}}}}"#)
}

pub fn level_record(value: f32) -> String {
    format!(r#"{{"tool": "test.level", "state": {{"value": {value}}}}}"#)
}

pub const BANK_RECORD: &str = r#"{"tool": "test.bank", "state": {"gain": 1.0}}"#;

pub fn project_file(connections: &str) -> String {
    format!(
        r#"{{
  "format": 1,
  "extensions": ["test"],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{connections}]
}}"#
    )
}

pub fn dc_to_device(instance: &str) -> String {
    format!(
        r#"{{"from": {{"instance": "{instance}", "port": "out"}}, "to": {{"device_output": 0}}}}"#
    )
}
