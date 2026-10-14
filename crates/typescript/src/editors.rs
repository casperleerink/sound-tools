//! A clip editor of the project, as `editor({...})` in a file of `extensions/` defines it: its
//! fields, and its grid for every combination of its clip fields, which Bun made at load. The
//! window shows a clip in it and never asks Bun again.

use std::fmt;

use serde::Deserialize;
use sound_core::Problem;
use sound_notes::{EditorName, EditorSpec, Editors, FieldName, Grid, Refused, ViewField};

use crate::FOLDER;
use crate::bun::LoadedEditor;
use crate::tools::Named;

/// A clip editor as `host.ts` sends it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditorInfo {
    pub name: EditorName,
    /// Its file in `extensions/`.
    pub file: String,
    pub title: String,
    pub clip: Named<ViewField>,
    pub note: Named<ViewField>,
}

/// A problem of the editor `name` of `file`.
pub(crate) fn problem(file: &str, name: &str, message: impl fmt::Display) -> Problem {
    Problem {
        path: format!("{FOLDER}/{file}"),
        message: format!("editor {name}: {message}"),
    }
}

/// The editor Bun loaded, or the problem that refuses it.
pub(crate) fn spec_of(loaded: LoadedEditor) -> Result<EditorSpec, Problem> {
    let LoadedEditor { info, grids } = loaded;
    let (file, name) = (info.file.clone(), info.name.as_str().to_owned());
    let refused = |message: String| problem(&file, &name, message);
    let clip = fields(info.clip).map_err(&refused)?;
    let note = fields(info.note).map_err(&refused)?;
    let grids = grids
        .into_iter()
        .map(|(choices, grid)| {
            let grid = grid.and_then(|grid| {
                serde_json::from_str::<Grid>(&grid).map_err(|error| error.to_string())
            });
            match grid {
                Ok(grid) => Ok((choices, grid)),
                Err(error) => Err(format!(
                    "its grid with the choices {choices} fails: {error}"
                )),
            }
        })
        .collect::<Result<Vec<_>, String>>()
        .map_err(&refused)?;
    EditorSpec::new(info.name, info.title, Some(info.file), clip, note, grids).map_err(&refused)
}

fn fields(named: Named<ViewField>) -> Result<Vec<(FieldName, ViewField)>, String> {
    let fields = named.0.into_iter().map(|(name, field)| {
        let name = FieldName::try_from(name).map_err(|error| error.to_string())?;
        Ok((name, field))
    });
    fields.collect()
}

/// Makes `specs` the editors of the project in `editors`. Each one it refuses is a problem.
/// Whether the editors changed.
pub(crate) fn set_project(
    editors: &Editors,
    specs: Vec<EditorSpec>,
    problems: &mut Vec<Problem>,
) -> bool {
    let replaced = editors.set_project(specs);
    for Refused { spec, reason } in replaced.refused {
        let file = spec.file.as_deref().unwrap_or_default();
        problems.push(problem(file, spec.name.as_str(), reason));
    }
    replaced.changed
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn loaded(name: &str, grids: serde_json::Value) -> LoadedEditor {
        serde_json::from_value(json!({
            "info": {
                "name": name, "file": format!("{name}.ts"), "title": "Scale",
                "clip": {"scale": {"kind": "choice", "options": ["major", "minor"], "default": "major"}},
                "note": {}
            },
            "grids": grids
        }))
        .unwrap()
    }

    /// A grid of one row, as JSON text.
    fn row(pitch: u8) -> String {
        json!({"rows": [{"pitch": pitch}]}).to_string()
    }

    #[test]
    fn a_grid_that_fails_refuses_its_editor_with_the_choices_it_failed_for() {
        let thrown = loaded(
            "scale",
            json!([
                [{"scale": "major"}, {"Ok": row(60)}],
                [{"scale": "minor"}, {"Err": "Error: no minor"}]
            ]),
        );
        let refused = spec_of(thrown).unwrap_err();
        assert_eq!(refused.path, "extensions/scale.ts");
        assert_eq!(
            refused.message,
            r#"editor scale: its grid with the choices {"scale": "minor"} fails: Error: no minor"#
        );

        let unread = loaded(
            "scale",
            json!([
                [{"scale": "major"}, {"Ok": row(60)}],
                [{"scale": "minor"}, {"Ok": r#"{"rows": [{"pitch": 60, "colour": "red"}]}"#}]
            ]),
        );
        let refused = spec_of(unread).unwrap_err();
        assert!(
            refused
                .message
                .contains(r#"{"scale": "minor"} fails: unknown field `colour`"#),
            "{refused:?}"
        );

        let good = loaded(
            "scale",
            json!([
                [{"scale": "major"}, {"Ok": row(60)}],
                [{"scale": "minor"}, {"Ok": row(62)}]
            ]),
        );
        assert!(spec_of(good).is_ok());
    }

    #[test]
    fn an_editor_named_as_the_piano_roll_is_a_problem_of_its_file_and_the_others_are_kept() {
        let grids = json!([
            [{"scale": "major"}, {"Ok": row(60)}],
            [{"scale": "minor"}, {"Ok": row(62)}]
        ]);
        let specs =
            ["piano-roll", "scale"].map(|name| spec_of(loaded(name, grids.clone())).unwrap());
        let editors = Editors::default();
        let mut problems = Vec::new();
        set_project(&editors, specs.into(), &mut problems);
        assert_eq!(
            problems,
            [Problem {
                path: "extensions/piano-roll.ts".into(),
                message: "editor piano-roll: it is the name of a built-in editor".into(),
            }]
        );
        let names: Vec<String> = (editors.project().iter())
            .map(|spec| spec.name.as_str().to_owned())
            .collect();
        assert_eq!(names, ["scale"]);
    }
}
