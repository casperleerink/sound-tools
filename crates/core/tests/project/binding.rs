//! Edges of the engine binding: a failing behaviour and a processor that changes type.

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

fn open() -> (Project, Engine, tempfile::TempDir) {
    let folder = tempfile::tempdir().unwrap();
    let mut registry = registry();
    registry
        .tool::<Switch>(EXTENSION)
        .unwrap()
        .behaviour(apply_switch);
    let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let project = Project::open(folder.path(), registry, control).unwrap();
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
fn the_project_can_move_to_another_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<Project>();
}
