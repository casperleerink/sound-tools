//! Clip editors: the grid JSON, the piano roll, the editor of the project, and what is wrong
//! with the editor or a view of a clip.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use sound_core::Ticks;
use sound_notes::{
    Clip, EditorName, EditorSpec, Editors, FieldName, Grid, Length, Note, NoteShape, Pitch,
    Velocity, View, ViewField, piano_roll,
};

fn grid(value: Value) -> Grid {
    serde_json::from_value(value).unwrap()
}

fn view(value: Value) -> View {
    serde_json::from_value(value).unwrap()
}

fn rows(pitches: &[u8]) -> Value {
    let rows: Vec<Value> = pitches
        .iter()
        .map(|pitch| json!({"pitch": pitch}))
        .collect();
    json!({ "rows": rows })
}

fn fields(value: Value) -> Vec<(FieldName, ViewField)> {
    let map: serde_json::Map<String, Value> = serde_json::from_value(value).unwrap();
    map.into_iter()
        .map(|(name, field)| {
            let field = serde_json::from_value(field).unwrap();
            (FieldName::new(&name).unwrap(), field)
        })
        .collect()
}

/// `scale` with the clip field `scale`, major or minor, and the note toggle `accent`.
fn scale_editor(grids: Vec<(Value, Value)>) -> Result<EditorSpec, String> {
    let grids = grids
        .into_iter()
        .map(|(choices, value)| (view(choices), grid(value)));
    EditorSpec::new(
        EditorName::new("scale").unwrap(),
        "Scale".to_owned(),
        Some("scale.ts".to_owned()),
        fields(
            json!({"scale": {"kind": "choice", "options": ["major", "minor"], "default": "major"}}),
        ),
        fields(json!({"accent": {"kind": "toggle", "default": false}})),
        grids,
    )
}

fn scale() -> EditorSpec {
    scale_editor(vec![
        (json!({"scale": "major"}), rows(&[60, 62, 64])),
        (json!({"scale": "minor"}), rows(&[60, 62, 63])),
    ])
    .unwrap()
}

/// `drums` with the note field `flam`.
fn drums() -> EditorSpec {
    EditorSpec::new(
        EditorName::new("drums").unwrap(),
        "Drums".to_owned(),
        None,
        Vec::new(),
        fields(json!({"flam": {"kind": "choice", "options": [1, 2], "default": 1}})),
        [(View::default(), grid(rows(&[36, 38])))],
    )
    .unwrap()
}

fn note() -> Note {
    Note::new(
        Ticks(0),
        Length::new(Ticks(240)).unwrap(),
        Pitch::new(60).unwrap(),
        Velocity::new(100).unwrap(),
    )
}

fn clip(editor: Option<&str>, clip_view: Value, note_views: &[Value]) -> Clip {
    let notes = note_views.iter().map(|value| Note {
        view: view(value.clone()),
        ..note()
    });
    let mut clip = Clip::new(Ticks(0), Length::new(Ticks(3840)).unwrap(), notes.collect());
    clip.editor = editor.map(|name| EditorName::new(name).unwrap());
    clip.view = view(clip_view);
    clip
}

fn editors() -> Editors {
    let editors = Editors::default();
    assert!(editors.set_project(vec![scale(), drums()]).is_empty());
    editors
}

/// The piano roll as the note editor drew it before it was an editor.
#[test]
fn the_piano_roll_has_a_row_per_pitch_the_black_keys_shaded_and_the_cs_named() {
    let is_black_key = |pitch: u8| matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
    let key_label = |pitch: u8| {
        pitch
            .is_multiple_of(12)
            .then(|| format!("C{}", i32::from(pitch / 12) - 1))
    };
    let grid = piano_roll().grid_for(&View::default());
    assert_eq!((grid.row_height, grid.notes), (12.0, NoteShape::Bar));
    assert_eq!(grid.rows.len(), 128);
    for (number, row) in (0..=127).zip(&grid.rows) {
        assert_eq!(row.pitch.number(), number);
        assert_eq!(row.shade, is_black_key(number), "pitch {number}");
        assert_eq!(row.label, key_label(number), "pitch {number}");
    }
    assert_eq!(grid.rows[60].label.as_deref(), Some("C4"));
    assert!(grid.check().is_ok());
}

#[test]
fn a_grid_loads_from_the_json_of_the_sdk_with_its_defaults() {
    let drums = grid(json!({
        "rows": [{"pitch": 36, "label": "Kick", "shade": true}, {"pitch": 38}],
        "rowHeight": 20,
        "notes": "hit",
    }));
    assert_eq!((drums.row_height, drums.notes), (20.0, NoteShape::Hit));
    assert_eq!(drums.rows[0].label.as_deref(), Some("Kick"));
    assert!(drums.rows[0].shade && !drums.rows[1].shade);
    let plain = grid(rows(&[60]));
    assert_eq!((plain.row_height, plain.notes), (12.0, NoteShape::Bar));
    let snake = json!({"rows": [{"pitch": 60}], "row_height": 20});
    assert!(serde_json::from_value::<Grid>(snake).is_err());
    let unknown = json!({"rows": [], "notes": "dot"});
    assert!(serde_json::from_value::<Grid>(unknown).is_err());
}

#[test]
fn a_grid_has_1_to_128_rows_going_up_of_a_readable_height_with_short_labels() {
    let check = |value: Value| grid(value).check();
    assert!(check(rows(&[36, 38])).is_ok());
    assert_eq!(
        check(rows(&[])),
        Err("a grid has 1 to 128 rows, not 0".into())
    );
    let all: Vec<Value> = (0..=128)
        .map(|pitch| json!({"pitch": pitch.min(127)}))
        .collect();
    assert_eq!(
        check(json!({ "rows": all })),
        Err("a grid has 1 to 128 rows, not 129".into())
    );
    let falling = check(rows(&[60, 59]));
    assert_eq!(
        falling,
        Err("rows[1].pitch must be higher than 60, the pitch of the row before it: rows go from the lowest pitch up".into())
    );
    assert!(check(rows(&[60, 60])).is_err());
    for height in [7.9, 40.1] {
        let value = json!({"rows": [{"pitch": 60}], "rowHeight": height});
        assert!(
            check(value)
                .unwrap_err()
                .starts_with("rowHeight must be from 8 to 40")
        );
    }
    let long = "A label much longer than it";
    let value = json!({"rows": [{"pitch": 60, "label": long}]});
    assert_eq!(
        check(value),
        Err("rows[0].label must be at most 24 characters, not 27".into())
    );
}

#[test]
fn an_editor_needs_a_grid_that_checks_for_every_combination_of_its_choices() {
    let major = (json!({"scale": "major"}), rows(&[60]));
    let minor = (json!({"scale": "minor"}), rows(&[60]));
    let missing = scale_editor(vec![major.clone()]).unwrap_err();
    assert_eq!(
        missing,
        r#"it has no grid for the choices {"scale": "minor"}"#
    );
    let extra = (json!({"scale": "dorian"}), rows(&[60]));
    let error = scale_editor(vec![major.clone(), minor, extra]).unwrap_err();
    assert!(error.contains(r#"{"scale": "dorian"}, which its clip fields do not make"#));
    let bad = (json!({"scale": "minor"}), rows(&[]));
    let error = scale_editor(vec![major, bad]).unwrap_err();
    assert_eq!(
        error,
        r#"its grid with the choices {"scale": "minor"} fails: a grid has 1 to 128 rows, not 0"#
    );

    let field = |value: Value| {
        EditorSpec::new(
            EditorName::new("odd").unwrap(),
            "Odd".to_owned(),
            None,
            fields(value),
            Vec::new(),
            [(View::default(), grid(rows(&[60])))],
        )
        .unwrap_err()
    };
    let not_an_option = json!({"size": {"kind": "choice", "options": [1, 2], "default": 3}});
    assert_eq!(
        field(not_an_option),
        "the clip field size has the default 3, which is not an option"
    );
    let empty = json!({"size": {"kind": "choice", "options": [], "default": 3}});
    assert_eq!(field(empty), "the clip field size has no options");
    let knob = json!({"size": {"kind": "knob", "min": 0, "max": 1, "default": 0}});
    assert!(serde_json::from_value::<ViewField>(knob["size"].clone()).is_err());
}

#[test]
fn a_clip_gets_the_grid_of_its_choices_and_the_default_for_what_it_lacks() {
    let scale = scale();
    let pitches = |clip_view: Value| -> Vec<u8> {
        let grid = scale.grid_for(&view(clip_view));
        grid.rows.iter().map(|row| row.pitch.number()).collect()
    };
    assert_eq!(pitches(json!({"scale": "minor"})), [60, 62, 63]);
    assert_eq!(pitches(json!({})), [60, 62, 64]);
    assert_eq!(pitches(json!({"scale": "lydian"})), [60, 62, 64]);
    assert_eq!(pitches(json!({"scale": 3, "other": true})), [60, 62, 64]);
}

#[test]
fn the_piano_roll_is_a_built_in_name_and_a_name_is_taken_once() {
    let editors = Editors::default();
    let before = editors.generation();
    let mut piano = scale();
    piano.name = EditorName::piano_roll();
    let refused = editors.set_project(vec![piano, scale(), scale()]);
    assert_eq!(
        refused,
        [
            (
                "piano-roll".to_owned(),
                "it is the name of a built-in editor".to_owned()
            ),
            (
                "scale".to_owned(),
                "another editor of the project has this name".to_owned()
            ),
        ]
    );
    let names: Vec<String> = editors
        .all()
        .iter()
        .map(|spec| spec.name.as_str().to_owned())
        .collect();
    assert_eq!(names, ["piano-roll", "scale"]);
    assert!(editors.generation() > before);
}

#[test]
fn a_clip_shows_in_its_editor_and_in_the_piano_roll_when_the_project_lacks_it() {
    let editors = editors();
    let shown = editors.of(&clip(Some("scale"), json!({"scale": "minor"}), &[]));
    assert_eq!(shown.spec.name.as_str(), "scale");
    assert_eq!(shown.grid.rows.len(), 3);
    assert_eq!(shown.missing, None);
    let shown = editors.of(&clip(Some("drumz"), json!({}), &[]));
    assert_eq!(shown.spec.name, EditorName::piano_roll());
    assert_eq!(shown.missing, Some(EditorName::new("drumz").unwrap()));
    let shown = editors.of(&clip(None, json!({}), &[]));
    assert_eq!(
        (shown.spec.name.as_str(), shown.grid.rows.len()),
        ("piano-roll", 128)
    );
}

#[test]
fn a_clip_names_an_editor_of_the_project_and_holds_values_it_takes() {
    let editors = editors();
    let problem = |editor, clip_view, note_views: &[Value]| {
        editors.problem(&clip(editor, clip_view, note_views))
    };
    let fine = [
        problem(None, json!({}), &[json!({})]),
        problem(
            Some("scale"),
            json!({"scale": "minor"}),
            &[json!({"accent": true})],
        ),
        // A field of another editor is left alone: the clip may go back to it.
        problem(None, json!({"scale": "minor"}), &[json!({"flam": 2})]),
        problem(
            Some("drums"),
            json!({"scale": "lydian"}),
            &[json!({"accent": 3})],
        ),
    ];
    assert_eq!(fine, [None, None, None, None]);

    assert_eq!(
        problem(Some("drumz"), json!({}), &[]).unwrap(),
        r#"names the clip editor "drumz", which this project does not have, so the window shows it in the piano roll and keeps its editor and view. The editors: piano-roll, scale, drums"#
    );
    assert_eq!(
        problem(Some("scale"), json!({"scale": "lydian"}), &[]).unwrap(),
        r#"has "lydian" in view.scale, which the editor scale does not take, so it shows as "major". Use one of "major", "minor""#
    );
    assert_eq!(
        problem(Some("scale"), json!({}), &[json!({}), json!({"accent": 1})]).unwrap(),
        "has 1 in notes[1].view.accent, which the editor scale does not take, so it shows as false. Use true or false"
    );
    assert_eq!(
        problem(Some("scale"), json!({"scal": "minor"}), &[]).unwrap(),
        "has view.scal, which no editor of this project has as a clip field, so nothing reads it"
    );
    // A note field on the clip is a typo too.
    assert_eq!(
        problem(None, json!({"accent": true}), &[]).unwrap(),
        "has view.accent, which no editor of this project has as a clip field, so nothing reads it"
    );
    assert_eq!(
        problem(None, json!({}), &[json!({"stress": true})]).unwrap(),
        "has notes[0].view.stress, which no editor of this project has as a note field, so nothing reads it"
    );
    // One problem per clip: the first.
    let several = problem(
        Some("scale"),
        json!({"scale": 1, "x": 2}),
        &[json!({"y": 3})],
    );
    assert!(several.unwrap().starts_with("has 1 in view.scale"));
}
