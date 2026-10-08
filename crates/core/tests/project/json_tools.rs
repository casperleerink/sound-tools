//! A tool of the project, whose records are JSON checked by a function: it loads with no
//! extension enabled, arrives while the project is open, and can be defined again.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use serde_json::{Value, json};
use sound_core::{Engine, EngineConfig, JsonTool, JsonToolDoc, Project, Registry, RegistryError};

use crate::tools::{id, write};

/// What the behaviours of a tool saw, in order.
type Seen = Rc<RefCell<Vec<Value>>>;

/// A tool `wobble` whose `rate` must be at most `max`. Its behaviour notes `version` and the
/// state it ran with.
fn wobble(max: f64, version: u32, seen: &Seen) -> JsonTool {
    let seen = seen.clone();
    JsonTool {
        name: "wobble".to_string(),
        check: Arc::new(
            move |state: &Value| match state.get("rate").and_then(Value::as_f64) {
                Some(rate) if rate <= max => Ok(()),
                Some(rate) => Err(format!("state.rate: {rate} is above {max}")),
                None => Err("state.rate: missing".to_string()),
            },
        ),
        behaviour: Box::new(move |state, _| {
            seen.borrow_mut()
                .push(json!({ "version": version, "state": state }));
            Ok(())
        }),
        doc: Some(JsonToolDoc {
            when: format!("You want wobble {version}"),
            markdown: format!("# Wobble {version}\n"),
        }),
    }
}

const RECORD: &str = r#"{"tool": "wobble", "state": {"rate": 3}}"#;

fn open(registry: Registry) -> (Project, tempfile::TempDir) {
    let folder = tempfile::tempdir().unwrap();
    // A project that enables no extension: a tool of the project needs none.
    write(
        folder.path(),
        "project.json",
        r#"{"format": 1, "extensions": [], "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]}, "connections": []}"#,
    );
    write(folder.path(), "state/a.json", RECORD);
    let (control, _engine) = Engine::new(EngineConfig::new(48_000, 1));
    let project = Project::open(folder.path(), registry, control).unwrap();
    (project, folder)
}

fn problem_paths(project: &Project) -> Vec<String> {
    project
        .problems()
        .into_iter()
        .map(|problem| problem.path)
        .collect()
}

#[test]
fn a_record_of_a_tool_of_the_project_loads_in_a_project_that_enables_no_extension() {
    let seen = Seen::default();
    let mut registry = Registry::new();
    registry.json_tool(wobble(10.0, 1, &seen)).unwrap();
    let (project, _folder) = open(registry);

    assert!(project.problems().is_empty(), "{:?}", project.problems());
    assert_eq!(
        *seen.borrow(),
        vec![json!({ "version": 1, "state": { "rate": 3 } })]
    );
    assert_eq!(project.state_json(&id("a")).unwrap(), r#"{"rate":3}"#);
}

#[test]
fn a_record_that_waited_for_its_tool_loads_when_the_tool_is_defined() {
    let seen = Seen::default();
    let (mut project, _folder) = open(Registry::new());
    assert_eq!(problem_paths(&project), ["state/a.json"]);

    project.define_json_tool(wobble(10.0, 1, &seen)).unwrap();

    assert!(project.problems().is_empty(), "{:?}", project.problems());
    assert_eq!(project.tool_of(&id("a")), Some("wobble"));
    assert_eq!(seen.borrow().len(), 1);
}

#[test]
fn a_tool_defined_again_runs_its_new_behaviour_and_its_new_check_refuses_what_it_must() {
    let seen = Seen::default();
    let mut registry = Registry::new();
    registry.json_tool(wobble(10.0, 1, &seen)).unwrap();
    let (mut project, _folder) = open(registry);

    // New code that sounds different: every instance runs it, from the record it has.
    project.define_json_tool(wobble(10.0, 2, &seen)).unwrap();
    assert_eq!(
        seen.borrow().last(),
        Some(&json!({ "version": 2, "state": { "rate": 3 } }))
    );
    assert!(project.problems().is_empty());

    // A check the record fails: the file is a problem that names the field.
    project.define_json_tool(wobble(2.0, 3, &seen)).unwrap();
    let problems = project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "state/a.json");
    assert!(
        problems[0].message.contains("state.rate: 3 is above 2"),
        "{problems:?}"
    );

    // Back to a check it passes, and the problem goes.
    project.define_json_tool(wobble(10.0, 4, &seen)).unwrap();
    assert!(project.problems().is_empty(), "{:?}", project.problems());
}

#[test]
fn an_edit_of_a_record_of_a_tool_of_the_project_goes_through_its_check() {
    let seen = Seen::default();
    let mut registry = Registry::new();
    registry.json_tool(wobble(10.0, 1, &seen)).unwrap();
    let (mut project, _folder) = open(registry);

    let mut edit = project.begin("Change rate");
    let error = project
        .update_json(&mut edit, &id("a"), |state| state["rate"] = json!(11))
        .unwrap_err();
    assert!(
        error.to_string().contains("state.rate: 11 is above 10"),
        "{error}"
    );

    project
        .update_json(&mut edit, &id("a"), |state| state["rate"] = json!(7))
        .unwrap();
    project.finish(edit).unwrap();
    assert_eq!(project.state_json(&id("a")).unwrap(), r#"{"rate":7}"#);
}

#[test]
fn the_doc_of_a_tool_of_the_project_is_in_the_map_and_follows_the_tool() {
    let seen = Seen::default();
    let mut registry = Registry::new();
    registry.json_tool(wobble(10.0, 1, &seen)).unwrap();
    let (mut project, folder) = open(registry);
    let doc = folder.path().join("agent-docs/wobble.md");
    assert!(
        std::fs::read_to_string(&doc)
            .unwrap()
            .contains("# Wobble 1")
    );
    assert!(project.agent_doc().contains("You want wobble 1"));
    assert!(project.agent_doc().contains("| `wobble` | `<name>.json` |"));

    project.define_json_tool(wobble(10.0, 2, &seen)).unwrap();
    project.poll().unwrap();
    assert!(
        std::fs::read_to_string(&doc)
            .unwrap()
            .contains("# Wobble 2")
    );
    assert!(!project.agent_doc().contains("You want wobble 1"));
}

#[test]
fn a_tool_of_the_project_cannot_take_the_name_of_a_built_in_tool() {
    let mut registry = crate::tools::registry();
    let seen = Seen::default();
    let mut tool = wobble(10.0, 1, &seen);
    tool.name = "test.dc".to_string();
    let error = registry.json_tool(tool).unwrap_err();
    assert_eq!(error, RegistryError::DuplicateTool("test.dc"));
}
