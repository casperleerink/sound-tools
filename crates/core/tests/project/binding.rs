//! Edges of the engine binding: a failing behaviour and a processor that changes type.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sound_core::{
    BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, OutputEndpoint, Project,
    ProjectError, State,
};

use crate::tools::{Constant, Dc, EXTENSION, Gain, SAMPLE_RATE, id, registry};

/// Keeps one processor named `main`, of a type the state picks, and can refuse to apply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Switch {
    constant: bool,
    fail: bool,
}

impl State for Switch {
    const TOOL: &'static str = "test.switch";
}

fn apply_switch(state: &Switch, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let output = if state.constant {
        let constant = context.processor("main", || Constant::new(0.5))?;
        OutputEndpoint::new(constant, Constant::OUTPUT)
    } else {
        let gain = context.processor("main", || Gain::new(1.0))?;
        OutputEndpoint::new(gain, Gain::OUTPUT)
    };
    context.connect(output.to_device(0))?;
    if state.fail {
        return Err(BehaviourError::Other("told to fail".to_string()));
    }
    Ok(())
}

/// Sounds by itself: it connects its own processor to the device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SelfRouted {
    value: f32,
}

impl State for SelfRouted {
    const TOOL: &'static str = "test.self-routed";
}

fn apply_self_routed(
    state: &SelfRouted,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let constant = context.processor("constant", || Constant::new(0.0))?;
    context.update(constant, state.value)?;
    let output = OutputEndpoint::new(constant, Constant::OUTPUT);
    context.connect(output.to_device(0))?;
    context.output("out", output);
    Ok(())
}

/// An owner that may connect its child to the device too: the same connection, declared twice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Sharer {
    connect: bool,
}

impl State for Sharer {
    const TOOL: &'static str = "test.sharer";
    const OWNS_CHILDREN: bool = true;
}

fn apply_sharer(state: &Sharer, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    if state.connect
        && let Some(output) = context.child_output("child", "out")
    {
        context.connect(output.to_device(0))?;
    }
    Ok(())
}

fn open_at(folder: &Path) -> (Project, Engine) {
    let mut registry = registry();
    registry
        .tool::<Switch>(EXTENSION)
        .unwrap()
        .behaviour(apply_switch);
    registry
        .tool::<SelfRouted>(EXTENSION)
        .unwrap()
        .behaviour(apply_self_routed);
    registry
        .tool::<Sharer>(EXTENSION)
        .unwrap()
        .behaviour(apply_sharer);
    let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let project = Project::open(folder, registry, control).unwrap();
    (project, engine)
}

fn open() -> (Project, Engine, tempfile::TempDir) {
    let folder = tempfile::tempdir().unwrap();
    let (project, engine) = open_at(folder.path());
    (project, engine, folder)
}

fn level(engine: &mut Engine) -> f32 {
    let mut buffer = [0.0; 128];
    engine.process_block(&mut buffer);
    buffer[127]
}

#[test]
fn a_processor_name_can_change_its_type() {
    let (mut project, mut engine, _folder) = open();
    let mut changes = Changes::new();
    let constant = Switch {
        constant: true,
        fail: false,
    };
    let switch = changes.create(id("switch"), constant);
    project.commit("Add switch", changes).unwrap();
    assert_eq!(level(&mut engine), 0.5);

    let mut edit = project.begin("Switch");
    project
        .update(&mut edit, &switch, |state| state.constant = false)
        .unwrap();
    project.finish(edit).unwrap();
    assert_eq!(level(&mut engine), 0.0);
    project.undo().unwrap();
    assert_eq!(level(&mut engine), 0.5);
}

#[test]
fn a_failing_behaviour_rejects_the_whole_group() {
    let (mut project, mut engine, _folder) = open();
    let mut changes = Changes::new();
    let working = Switch {
        constant: true,
        fail: false,
    };
    let switch = changes.create(id("switch"), working.clone());
    project.commit("Add switch", changes).unwrap();
    project.drain_events();

    // A valid instance and a failing one in one group: neither applies.
    let mut changes = Changes::new();
    changes.create(id("dc"), Dc { value: 0.25 });
    changes.set(
        &switch,
        Switch {
            constant: false,
            fail: true,
        },
    );
    let error = project.commit("Fails", changes).unwrap_err();
    assert!(matches!(error, ProjectError::Behaviour { .. }), "{error}");
    assert_eq!(
        error.to_string(),
        "the behaviour of switch failed: told to fail"
    );
    assert_eq!(project.state(&switch), Some(&working));
    assert_eq!(project.instances().count(), 1);
    assert_eq!(project.drain_events(), []);
    assert_eq!(project.undo_label(), Some("Add switch"));
    assert_eq!(level(&mut engine), 0.5);

    // The binding is as it was: the next edit works on the same processor.
    let mut changes = Changes::new();
    changes.delete(switch.id());
    project.commit("Delete", changes).unwrap();
    assert_eq!(level(&mut engine), 0.0);
}

#[test]
fn a_connection_two_instances_declare_stays_until_the_last_one_stops() {
    let (mut project, mut engine, _folder) = open();
    let mut changes = Changes::new();
    let sharer = changes.create(id("sharer"), Sharer { connect: true });
    let child = changes.create(id("sharer/child"), SelfRouted { value: 0.5 });
    project.commit("Add", changes).unwrap();
    // Declared twice, in the graph once: the signal is not doubled.
    assert_eq!(level(&mut engine), 0.5);

    // The owner stops declaring it. The child still does, so it stays connected.
    let mut edit = project.begin("Owner lets go");
    project
        .update(&mut edit, &sharer, |state| state.connect = false)
        .unwrap();
    project.finish(edit).unwrap();
    assert_eq!(level(&mut engine), 0.5);

    project.undo().unwrap();
    assert_eq!(level(&mut engine), 0.5);
    let mut changes = Changes::new();
    changes.delete(child.id());
    project.commit("Delete child", changes).unwrap();
    assert_eq!(level(&mut engine), 0.0);
}

#[test]
fn a_record_whose_behaviour_fails_is_left_out_when_the_project_opens() {
    let folder = tempfile::tempdir().unwrap();
    let write = |relative: &str, contents: &str| {
        let path = folder.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    };
    let failing = r#"{"tool": "test.switch", "state": {"constant": true, "fail": true}}"#;
    write("state/switch.json", failing);
    write(
        "state/sharer/instance.json",
        r#"{"tool": "test.sharer", "state": {"connect": true}}"#,
    );
    write(
        "state/sharer/child.json",
        r#"{"tool": "test.self-routed", "state": {"value": 0.25}}"#,
    );

    let (mut project, mut engine) = open_at(folder.path());
    let live: Vec<_> = project.instances().map(|(id, _)| id.to_string()).collect();
    assert_eq!(live, ["sharer", "sharer/child"]);
    assert_eq!(level(&mut engine), 0.25);
    let problems = project.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].path, "state/switch.json");
    assert_eq!(
        problems[0].message,
        "not loaded: its behaviour failed: told to fail"
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("state/switch.json")).unwrap(),
        failing
    );

    // Once the record is fixed from outside, it loads.
    let fixed = r#"{"tool": "test.switch", "state": {"constant": true, "fail": false}}"#;
    write("state/switch.json", fixed);
    let path = project.root().join("state/switch.json");
    assert_eq!(project.apply_outside_changes(&[path]).unwrap(), 1);
    assert_eq!(project.problems(), []);
    assert_eq!(level(&mut engine), 0.75);
}

#[test]
fn the_project_can_move_to_another_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<Project>();
}
