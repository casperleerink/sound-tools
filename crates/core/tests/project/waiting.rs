//! A tool that waits for an asset: when a file under the folder it names in `assets/` arrives,
//! its instances with a problem run their behaviour again. Other tools are not woken.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sound_core::{
    AssetName, BehaviourContext, BehaviourError, Engine, EngineConfig, Project, Registry, State,
};

/// Names a file `assets/<folder>/<asset>.bin` and says so while it is not there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Waiter {
    asset: String,
}

impl State for Waiter {
    const TOOL: &'static str = "test.waiter";
}

/// The same, for a tool that does not ask to be woken.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Sleeper {
    asset: String,
}

impl State for Sleeper {
    const TOOL: &'static str = "test.sleeper";
}

fn needs(context: &mut BehaviourContext<'_>, asset: &str) -> Result<(), BehaviourError> {
    let name = AssetName::new("things", asset, "bin").unwrap();
    if context.assets().read(&name).unwrap().is_none() {
        context.problem(format!("assets/{name} is not there"));
    }
    Ok(())
}

struct Opened {
    project: Project,
    runs: [Rc<Cell<usize>>; 2],
    _folder: tempfile::TempDir,
}

fn open() -> Opened {
    let runs = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let mut registry = Registry::new();
    let waiter_runs = runs[0].clone();
    registry
        .tool::<Waiter>("test")
        .unwrap()
        .behaviour(move |state: &Waiter, context| {
            waiter_runs.set(waiter_runs.get() + 1);
            needs(context, &state.asset)
        })
        .rebinds_on_assets("things");
    let sleeper_runs = runs[1].clone();
    registry
        .tool::<Sleeper>("test")
        .unwrap()
        .behaviour(move |state: &Sleeper, context| {
            sleeper_runs.set(sleeper_runs.get() + 1);
            needs(context, &state.asset)
        });
    let folder = tempfile::tempdir().unwrap();
    let (control, _engine) = Engine::new(EngineConfig::new(48_000, 1));
    let project = Project::open(folder.path(), registry, control).unwrap();
    Opened {
        project,
        runs,
        _folder: folder,
    }
}

fn write(project: &Project, relative: &str, contents: &str) -> PathBuf {
    let path = project.root().join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn an_asset_that_arrives_wakes_the_instances_that_wait_for_it_and_nothing_else() {
    let mut opened = open();
    let project = &mut opened.project;
    let records = [
        write(
            project,
            "state/waiter.json",
            r#"{"tool": "test.waiter", "state": {"asset": "a"}}"#,
        ),
        write(
            project,
            "state/sleeper.json",
            r#"{"tool": "test.sleeper", "state": {"asset": "a"}}"#,
        ),
    ];
    assert_eq!(project.apply_outside_changes(&records).unwrap(), 2);
    assert_eq!(project.problems().len(), 2);
    let runs = |opened: &Opened| opened.runs.clone().map(|runs| runs.get());
    assert_eq!(runs(&opened), [1, 1]);

    // A file in another folder wakes nobody, and the file waited for wakes the waiter only.
    let project = &mut opened.project;
    let other = write(project, "assets/others/a.bin", "x");
    assert_eq!(project.apply_outside_changes(&[other]).unwrap(), 0);
    assert_eq!(runs(&opened), [1, 1]);
    let project = &mut opened.project;
    let asset = write(project, "assets/things/a.bin", "x");
    let undo = project.undo_label().map(str::to_string);
    assert_eq!(project.apply_outside_changes(&[asset]).unwrap(), 0);
    let problems = project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "state/sleeper.json");
    // It is no edit: nothing to undo.
    assert_eq!(project.undo_label().map(str::to_string), undo);
    assert_eq!(runs(&opened), [2, 1]);

    // An instance that has no problem is not run again.
    let project = &mut opened.project;
    let again = write(project, "assets/things/b.bin", "x");
    project.apply_outside_changes(&[again]).unwrap();
    assert_eq!(runs(&opened), [2, 1]);
}

#[test]
fn the_watcher_sees_an_asset_folder_made_after_the_project_opened() {
    let mut opened = open();
    opened.project.watch().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let record = write(
        &opened.project,
        "state/waiter.json",
        r#"{"tool": "test.waiter", "state": {"asset": "a"}}"#,
    );
    assert_eq!(opened.project.apply_outside_changes(&[record]).unwrap(), 1);
    assert_eq!(opened.project.problems().len(), 1);

    write(&opened.project, "assets/things/a.bin", "x");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !opened.project.problems().is_empty() {
        opened.project.poll().unwrap();
        assert!(Instant::now() < deadline, "the asset was not seen");
        std::thread::sleep(Duration::from_millis(10));
    }
}
