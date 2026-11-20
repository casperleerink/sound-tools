//! What the Bun host of a project reads of it and does to it: the watch of `extensions/`, the
//! records and watches its code gets, and the edits and controls it sends.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use serde_json::Value;
use sound_core::{Changes, InstanceId, Problem, Project};
use sound_signals::SignalsUpdate;

use crate::bun::{Played, RecordEdit};
use crate::feeds::played_name;
use crate::tools::{Input, ToolInfo};
use crate::{FOLDER, OWNED};

/// Watches `folder` and its folders but `node_modules`, and says on `changed` when a file
/// that may be imported changes, or a folder. Not hidden files, such as an editor's. With
/// what it cannot watch, which is a problem.
pub(crate) fn watch(
    folder: &Path,
    changed: smol::channel::Sender<()>,
) -> (Option<RecommendedWatcher>, Option<Problem>) {
    let not_heard = |path: &Path, error: notify::Error| {
        let message = format!("changes in {} are not heard: {error}", path.display());
        Some(crate::folder_problem(message))
    };
    let skipped = |name: &str| name.starts_with('.') || name == "node_modules";
    // FSEvents gives the real path of a change, Windows the path as it was watched, which may
    // have short names (`RUNNER~1`). Only what is inside counts: the folder itself may be in a
    // hidden one, such as a temporary folder.
    let roots = [
        dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf()),
        folder.to_path_buf(),
    ];
    let counts = move |path: &Path, removed: bool| {
        let inside = (roots.iter())
            .find_map(|root| path.strip_prefix(root).ok())
            .unwrap_or_else(|| Path::new(path.file_name().unwrap_or_default()));
        let hidden = (inside.components()).any(|part| skipped(&part.as_os_str().to_string_lossy()));
        let importable = (path.extension()).is_some_and(|extension| {
            ["ts", "tsx", "mts", "js", "jsx", "mjs", "cjs", "json"]
                .iter()
                .any(|known| extension == *known)
        });
        // A folder that is gone is no folder any more.
        let folder = path.is_dir() || (removed && path.extension().is_none());
        let owned = OWNED.iter().any(|owned| inside.starts_with(owned));
        !hidden && !owned && (importable || folder)
    };
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let event = match event {
            Ok(event) => event,
            Err(error) => return eprintln!("error: watching {FOLDER}/: {error}"),
        };
        // An event with no paths asks to look again at everything, as after a lost event.
        let (paths, removed) = (&event.paths, event.kind.is_remove());
        if !event.kind.is_access()
            && (paths.is_empty() || paths.iter().any(|path| counts(path, removed)))
        {
            changed.try_send(()).ok();
        }
    });
    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => return (None, not_heard(folder, error)),
    };
    let entries = std::fs::read_dir(folder).into_iter().flatten().flatten();
    let folders = (entries.map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| skipped(&name.to_string_lossy()))
        })
        .map(|path| (path, RecursiveMode::Recursive));
    // The first that fails says so; the others are watched all the same.
    let mut problem = None;
    for (path, mode) in
        std::iter::once((folder.to_path_buf(), RecursiveMode::NonRecursive)).chain(folders)
    {
        if let Err(error) = watcher.watch(&path, mode) {
            problem = problem.or_else(|| not_heard(&path, error));
        }
    }
    (Some(watcher), problem)
}

/// What moves the number input `name` of a tool to `value` at the frame `at`, or sends the
/// bang `name` when `value` is `None`, or why it cannot.
pub(crate) fn control_update(
    info: &ToolInfo,
    name: &str,
    value: Option<f32>,
    at: Option<u64>,
) -> Result<SignalsUpdate, String> {
    match (info.input(name), value) {
        (Some((index, Input::Number { .. })), Some(value)) => {
            Ok(SignalsUpdate::Live { index, value, at })
        }
        (Some((index, Input::Bang { .. })), None) => Ok(SignalsUpdate::Trigger { index, at }),
        _ => {
            let kind = if value.is_some() {
                "number input"
            } else {
                "bang input"
            };
            let names: Vec<&str> = (info.inputs.0.iter())
                .filter(|(_, input)| matches!(input, Input::Number { .. }) == value.is_some())
                .map(|(name, _)| name.as_str())
                .collect();
            let names = match names.is_empty() {
                true => "none".to_string(),
                false => names.join(", "),
            };
            Err(format!("has no {kind} {name}: its {kind}s are {names}"))
        }
    }
}

pub(crate) fn find<'a>(tools: &'a [Rc<ToolInfo>], name: &str) -> Option<&'a Rc<ToolInfo>> {
    tools.iter().find(|info| info.name == name)
}

pub(crate) fn state_of(project: &Project, id: &InstanceId) -> Option<serde_json::Value> {
    serde_json::from_str(&project.state_json(id)?).ok()
}

/// An instance as its code gets it, with its watches as they are now.
pub(crate) fn played<'a>(project: &Project, id: &'a InstanceId) -> Played<'a> {
    Played {
        instance: id,
        watches: watches_of(project, id),
    }
}

/// Every instance of `project`, as changed records when a Bun starts: it holds none.
pub(crate) fn every_instance(project: &Project) -> BTreeSet<InstanceId> {
    project.instances().map(|(id, _)| id.clone()).collect()
}

/// Whether Bun holds the record of `id`: one of a tool of the project, or one inside one at
/// any depth, which the code of that tool reads.
pub(crate) fn told_to_bun(project: &Project, tools: &[Rc<ToolInfo>], id: &InstanceId) -> bool {
    let ours =
        |id: &InstanceId| (project.tool_of(id)).is_some_and(|tool| find(tools, tool).is_some());
    ours(id) || id.ancestors().any(|owner| ours(&owner))
}

/// The watches of `id` as its code reads them: its own, and those of each instance inside it
/// by its path from `id`, as `<child>.<watch>` or `<child>/<grandchild>.<watch>`, such as what
/// a looping timeline plays.
fn watches_of(project: &Project, id: &InstanceId) -> BTreeMap<String, f32> {
    let inside = (project.instances()).filter(|(inner, _)| inner.is_inside(id));
    let children = inside.flat_map(|(inner, _)| {
        let path = &inner.as_str()[id.as_str().len() + 1..];
        let watches = project.watches(inner).into_iter();
        watches.map(move |(name, watch)| (format!("{path}.{name}"), watch))
    });
    let watches = project.watches(id).into_iter().chain(children);
    // What a cable plays is read as it plays, by live values, and tells Bun nothing.
    let (played, inside) = (played_name(""), format!(".{}", played_name("")));
    let watches = watches.filter(|(name, _)| !name.starts_with(&played) && !name.contains(&inside));
    watches.map(|(name, watch)| (name, watch.get())).collect()
}

/// The changes of `edits` that the code of `instance` of the tool `info` made, on the records as
/// they are now, with each record they touch. Or what is wrong with one of them.
pub(crate) fn changes_of(
    project: &Project,
    behaviour: bool,
    instance: &InstanceId,
    edits: Vec<RecordEdit>,
) -> Result<(Changes, Vec<InstanceId>), String> {
    // A path from the instance: `track` or `track/clip`, at any depth.
    let child = |path: &str| instance.child(path).map_err(|error| error.to_string());
    let mut changes = Changes::new();
    let mut touched = Vec::new();
    for edit in edits {
        let id = match edit {
            RecordEdit::Fields {
                child: name,
                set,
                removed,
            } => {
                let id = name
                    .as_deref()
                    .map_or_else(|| Ok(instance.clone()), child)?;
                let (Some(tool), Some(Value::Object(mut fields))) =
                    (project.tool_of(&id), state_of(project, &id))
                else {
                    return Err(format!("{id} is gone"));
                };
                for name in removed {
                    fields.remove(&name);
                }
                fields.extend(set);
                project
                    .set_json(&mut changes, id.clone(), tool, Value::Object(fields))
                    .map_err(|error| error.to_string())?;
                id
            }
            RecordEdit::Record {
                child: name,
                tool,
                state,
            } => {
                if !behaviour {
                    return Err(format!(
                        "it adds the child {name:?}, but only a tool with a behaviour has children"
                    ));
                }
                // The core refuses one whose parent owns no children.
                let id = child(&name)?;
                project
                    .set_json(&mut changes, id.clone(), &tool, state)
                    .map_err(|error| error.to_string())?;
                id
            }
            RecordEdit::Delete { child: name } => {
                let id = child(&name)?;
                changes.delete(&id);
                id
            }
        };
        touched.push(id);
    }
    Ok((changes, touched))
}
