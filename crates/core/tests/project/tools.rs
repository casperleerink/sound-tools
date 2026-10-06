//! Test-only tools and a small harness. The tools are unmusical on purpose: every processor
//! puts out a constant, so a rendered sample shows the state exactly.
//!
//! - `test.dc`: one record, one processor, one output. Connected through `project.json`.
//! - `test.bank`: a composite. It owns many `test.level` records, which are data only, and
//!   reads them into one snapshot for its one processor. It sends its signal through its
//!   owned `output` child, a `test.amplifier`, and on to the device with no `project.json`
//!   connection. This is the shape of a track with clips and an instrument.
//! - `test.chain`: two gains in a row, connected by its behaviour. With `feedback` it also
//!   connects the last to the side input of the first, which closes a cycle.
//! - `test.pair`: an owner that plays a constant through its children `a` and `b`, two chains.
//!   With `feedback` it also connects `b` back into `a`, which closes a cycle, as an
//!   arrangement does whose tracks each listen to the other.
//! - `test.reporter`: data only, with a derive that reports problems.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, AudioInput, AudioOutput, BehaviourContext, BehaviourError, Derived, Engine,
    EngineConfig, InputEndpoint, InstanceId, OutputEndpoint, Place, Ports, PrepareConfig,
    ProcessContext, Processor, Project, ProjectError, Registry, State, Was,
};

pub(crate) const EXTENSION: &str = "test";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Dc {
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
pub(crate) struct Constant(f32);

impl Constant {
    pub(crate) fn new(value: f32) -> Self {
        Self(value)
    }

    pub(crate) const OUTPUT: AudioOutput = AudioOutput::new(0);
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
        for channel in context.audio_outputs.get(Self::OUTPUT) {
            channel.fill(self.0);
        }
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
pub(crate) struct Level {
    pub value: f32,
}

impl State for Level {
    const TOOL: &'static str = "test.level";
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Amplifier {
    pub gain: f32,
}

impl State for Amplifier {
    const TOOL: &'static str = "test.amplifier";
}

pub(crate) struct Gain(f32);

impl Gain {
    pub(crate) fn new(gain: f32) -> Self {
        Self(gain)
    }

    pub(crate) const INPUT: AudioInput = AudioInput::new(0);
    /// Heard by nothing. It is there for a chain to close a cycle through.
    pub(crate) const SIDE: AudioInput = AudioInput::new(1);
    pub(crate) const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for Gain {
    type Update = f32;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .side_audio_input(Self::SIDE)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut f32) {
        self.0 = *update;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let input = context.audio_inputs.get(Self::INPUT);
        let output = context.audio_outputs.get(Self::OUTPUT);
        for (output, input) in output.into_iter().zip(input) {
            for (output, input) in output.iter_mut().zip(input) {
                *output = input * self.0;
            }
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Chain {
    #[serde(default)]
    pub feedback: bool,
}

impl State for Chain {
    const TOOL: &'static str = "test.chain";
}

pub(crate) const CHAIN_RECORD: &str = r#"{"tool": "test.chain", "state": {}}"#;

/// The last gain is made first, so the connection between the two goes to the lower id.
fn apply_chain(state: &Chain, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let last = context.processor("last", || Gain::new(1.0))?;
    let first = context.processor("first", || Gain::new(1.0))?;
    let output = OutputEndpoint::new(first, Gain::OUTPUT);
    context.connect(output.to(InputEndpoint::new(last, Gain::INPUT)))?;
    if state.feedback {
        let output = OutputEndpoint::new(last, Gain::OUTPUT);
        context.connect(output.to(InputEndpoint::new(first, Gain::SIDE)))?;
    }
    context.input("in", InputEndpoint::new(first, Gain::INPUT));
    context.output("out", OutputEndpoint::new(last, Gain::OUTPUT));
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pair {
    #[serde(default)]
    pub feedback: bool,
}

impl State for Pair {
    const TOOL: &'static str = "test.pair";
    const OWNS_CHILDREN: bool = true;
}

fn apply_pair(state: &Pair, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let source = context.processor("source", || Constant::new(0.5))?;
    let (Some(a_in), Some(a_out), Some(b_in), Some(b_out)) = (
        context.child_input("a", "in"),
        context.child_output("a", "out"),
        context.child_input("b", "in"),
        context.child_output("b", "out"),
    ) else {
        return Ok(());
    };
    context.connect(OutputEndpoint::new(source, Constant::OUTPUT).to(a_in))?;
    context.connect(a_out.to(b_in))?;
    if state.feedback {
        context.connect(b_out.to(a_in))?;
    }
    context.connect(b_out.to_device(0))?;
    Ok(())
}

/// Its derive reports its message as a problem. The message "fail" also derives an invalid
/// record, so the whole group fails.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reporter {
    pub message: String,
}

impl State for Reporter {
    const TOOL: &'static str = "test.reporter";
}

fn derive_reporter(
    project: &Project,
    reporter: &sound_core::Instance<Reporter>,
    _: Was<'_, Reporter>,
    derived: &mut Derived,
) {
    let Some(state) = project.state(reporter) else {
        return;
    };
    derived.problem(state.message.clone());
    if state.message == "fail" {
        derived.changes().create(id("dc"), Dc { value: 5.0 });
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bank {
    pub gain: f32,
}

impl State for Bank {
    const TOOL: &'static str = "test.bank";
    const OWNS_CHILDREN: bool = true;
}

/// The name of the child a bank plays through.
pub(crate) const BANK_OUTPUT: &str = "output";

#[derive(Default)]
pub(crate) struct BankUpdate {
    gain: f32,
    levels: Arc<Vec<f32>>,
}

/// Puts out the sum of one immutable snapshot of levels, times a gain.
#[derive(Default)]
pub(crate) struct Summer(BankUpdate);

impl Summer {
    pub(crate) const OUTPUT: AudioOutput = AudioOutput::new(0);
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
        for channel in context.audio_outputs.get(Self::OUTPUT) {
            channel.fill(sum * self.0.gain);
        }
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

pub(crate) fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.tool::<Dc>(EXTENSION).unwrap().behaviour(apply_dc);
    registry.tool::<Level>(EXTENSION).unwrap();
    registry
        .tool::<Amplifier>(EXTENSION)
        .unwrap()
        .behaviour(apply_amplifier);
    registry
        .tool::<Chain>(EXTENSION)
        .unwrap()
        .behaviour(apply_chain);
    registry
        .tool::<Pair>(EXTENSION)
        .unwrap()
        .behaviour(apply_pair);
    registry
        .tool::<Reporter>(EXTENSION)
        .unwrap()
        .derive(derive_reporter);
    registry
        .tool::<Bank>(EXTENSION)
        .unwrap()
        .behaviour(apply_bank)
        .summary(summarize_bank);
    registry.tool::<Shelf>(EXTENSION).unwrap();
    registry.tool::<Book>(EXTENSION).unwrap();
    registry.agent_doc(EXTENSION, TEST_AGENT_DOC).unwrap();
    registry
}

/// Two tools with a fixed place, data only: a shelf lives at the top, a book on a shelf.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Shelf {}

impl State for Shelf {
    const TOOL: &'static str = "test.shelf";
    const OWNS_CHILDREN: bool = true;
    const PLACE: Place = Place::Root;
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Book {}

impl State for Book {
    const TOOL: &'static str = "test.book";
    const PLACE: Place = Place::In(Shelf::TOOL);
}

pub(crate) const SHELF_RECORD: &str = r#"{"tool": "test.shelf", "state": {}}"#;
pub(crate) const BOOK_RECORD: &str = r#"{"tool": "test.book", "state": {}}"#;

pub(crate) const TEST_AGENT_DOC: AgentDoc = AgentDoc {
    name: "test-tools",
    when: "You work on a test record",
    markdown: "# Test tools\n\nA record of a test tool.\n",
};

/// A bank tells what it owns in one line, so a summary does not list every level.
fn summarize_bank(project: &Project, bank: &sound_core::Instance<Bank>) -> String {
    let levels = project.children::<Level>(bank.id()).count();
    format!("bank {} with {levels} levels", bank.id())
}

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// An open project on a temporary folder with an offline mono engine.
pub(crate) struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// The time of the last outside change. See [`Harness::apply_outside_changes`].
    pub now: Instant,
    /// Last, so the folder outlives the project that holds its lock.
    pub folder: tempfile::TempDir,
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self::open(tempfile::tempdir().unwrap())
    }

    /// A project with two device channels, so a test can tell the channels apart.
    pub(crate) fn stereo() -> Self {
        let folder = tempfile::tempdir().unwrap();
        let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let project = Project::open(folder.path(), registry(), control).unwrap();
        Self {
            project,
            engine,
            now: Instant::now(),
            folder,
        }
    }

    /// Renders one device buffer and gives the last frame: the two channels of a stereo
    /// harness. Every test processor puts out a constant, so one frame shows the state.
    pub(crate) fn frame(&mut self) -> [f32; 2] {
        let mut buffer = [0.0; 480];
        self.engine.process_block(&mut buffer);
        [buffer[478], buffer[479]]
    }

    pub(crate) fn open(folder: tempfile::TempDir) -> Self {
        let (project, engine) = open(folder.path());
        Self {
            project,
            engine,
            now: Instant::now(),
            folder,
        }
    }

    /// Applies outside changes a minute after the last ones. Tests run in milliseconds, and
    /// outside groups that close together are one undo step (`OUTSIDE_UNDO_WINDOW`). With
    /// the minute, each call is a step of its own, as for changes a person makes by hand.
    pub(crate) fn apply_outside_changes(
        &mut self,
        paths: &[PathBuf],
    ) -> Result<usize, ProjectError> {
        self.now += Duration::from_secs(60);
        self.project.apply_outside_changes_at(paths, self.now)
    }

    /// Closes the project and opens the same folder again.
    pub(crate) fn reopen(self) -> Self {
        let Self {
            project,
            engine,
            folder,
            ..
        } = self;
        drop((project, engine));
        Self::open(folder)
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    /// Writes a file the way an agent would, without telling the project.
    pub(crate) fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }

    pub(crate) fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path(relative)).unwrap()
    }

    /// Writes a file and applies it, as the watcher would.
    pub(crate) fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.write(relative, contents);
        self.apply_outside_changes(&[path]).unwrap()
    }

    /// Renders one device buffer and gives its last sample. Every test processor puts out a
    /// constant, so one sample shows the state.
    pub(crate) fn level(&mut self) -> f32 {
        let mut buffer = [0.0; 480];
        self.engine.process_block(&mut buffer);
        buffer[479]
    }

    pub(crate) fn batches(&mut self) -> u64 {
        self.level();
        self.project.engine().poll().unwrap().batches_applied
    }

    pub(crate) fn problem_at(&self, path: &str) -> Option<String> {
        let problems = self.project.problems();
        let problem = problems.iter().find(|problem| problem.path == path)?;
        Some(problem.message.clone())
    }
}

pub(crate) fn open(folder: &Path) -> (Project, Engine) {
    let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let project = Project::open(folder, registry(), control).unwrap();
    (project, engine)
}

pub(crate) fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

pub(crate) fn dc_record(value: f32) -> String {
    format!(r#"{{"tool": "test.dc", "state": {{"value": {value}}}}}"#)
}

pub(crate) fn level_record(value: f32) -> String {
    format!(r#"{{"tool": "test.level", "state": {{"value": {value}}}}}"#)
}

pub(crate) const BANK_RECORD: &str = r#"{"tool": "test.bank", "state": {"gain": 1.0}}"#;

pub(crate) fn project_file(connections: &str) -> String {
    format!(
        r#"{{
  "format": 1,
  "extensions": ["test"],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{connections}]
}}"#
    )
}

pub(crate) fn dc_to_device(instance: &str) -> String {
    dc_to_device_channel(instance, 0)
}

pub(crate) fn dc_to_device_channel(instance: &str, channel: usize) -> String {
    format!(
        r#"{{"from": {{"instance": "{instance}", "port": "out"}}, "to": {{"device_output": {channel}}}}}"#
    )
}

/// Writes a file into a project folder, as an agent would, before the project is open.
pub(crate) fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
}

/// Every record file and `project.json` of a project folder, with its bytes. The generated
/// files of the runtime are left out: they are its own, not the composer's.
pub(crate) fn records(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(folder: &Path, root: &Path, found: &mut Vec<(PathBuf, Vec<u8>)>) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for path in entries.map(|entry| entry.unwrap().path()) {
            if path.is_dir() {
                walk(&path, root, found);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                found.push((relative, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut found = vec![(
        PathBuf::from("project.json"),
        std::fs::read(root.join("project.json")).unwrap(),
    )];
    walk(&root.join("state"), root, &mut found);
    found.sort();
    found
}
