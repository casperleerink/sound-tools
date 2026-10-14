//! Two clip editors of a project, built from the JSON Bun sends for an `editor({...})` of a
//! file under `extensions/`, so the window is tested with them without Bun.

use serde_json::{Value, json};
use sound_notes::{EditorName, EditorSpec, FieldName, ViewField};

/// The drum kit of General MIDI, from 36.
const KIT: [&str; 16] = [
    "Kick",
    "Side stick",
    "Snare",
    "Clap",
    "Snare 2",
    "Low floor tom",
    "Closed hat",
    "High floor tom",
    "Pedal hat",
    "Low tom",
    "Open hat",
    "Low mid tom",
    "High mid tom",
    "Crash",
    "High tom",
    "Ride",
];

/// `drums`: a row per sound of the kit, 36 to 51, 20 pt high, notes as hits.
pub(crate) fn drums() -> EditorSpec {
    let rows: Vec<Value> = (36..)
        .zip(KIT)
        .map(|(pitch, label)| json!({"pitch": pitch, "label": label}))
        .collect();
    let grid = json!({"rows": rows, "rowHeight": 20, "notes": "hit"});
    spec(
        "drums",
        "Drums",
        json!({}),
        json!({}),
        vec![(json!({}), grid)],
    )
}

/// `scale`: the notes of each of `scales`, major or minor, from C3 to C6, the Cs named, with the
/// clip field `scale` whose options they are, the first the default, and the note toggle
/// `accent`.
pub(crate) fn scale(scales: &[&str]) -> EditorSpec {
    let clip = json!({"scale": {"kind": "choice", "options": scales, "default": scales[0]}});
    let note = json!({"accent": {"kind": "toggle", "default": false, "label": "Accent"}});
    let grids = scales.iter().map(|scale| {
        let steps = match *scale {
            "minor" => [0, 2, 3, 5, 7, 8, 10],
            _ => [0, 2, 4, 5, 7, 9, 11],
        };
        let pitches = (48..=84).filter(|pitch| steps.contains(&(pitch % 12)));
        let rows: Vec<Value> = pitches
            .map(|pitch| match pitch % 12 {
                0 => json!({"pitch": pitch, "label": format!("C{}", pitch / 12 - 1)}),
                _ => json!({"pitch": pitch}),
            })
            .collect();
        (
            json!({"scale": scale}),
            json!({"rows": rows, "rowHeight": 16}),
        )
    });
    spec("scale", "Scale", clip, note, grids.collect())
}

fn spec(
    name: &str,
    title: &str,
    clip_fields: Value,
    note_fields: Value,
    grids: Vec<(Value, Value)>,
) -> EditorSpec {
    let fields = |fields: Value| -> Vec<(FieldName, ViewField)> {
        let fields: serde_json::Map<String, Value> = serde_json::from_value(fields).unwrap();
        let fields = fields.into_iter();
        fields
            .map(|(name, field)| {
                let field = serde_json::from_value(field).unwrap();
                (FieldName::new(&name).unwrap(), field)
            })
            .collect()
    };
    let grids = grids.into_iter().map(|(choices, grid)| {
        let choices = serde_json::from_value(choices).unwrap();
        (choices, serde_json::from_value(grid).unwrap())
    });
    EditorSpec::new(
        EditorName::new(name).unwrap(),
        title.into(),
        Some(format!("{name}.ts")),
        fields(clip_fields),
        fields(note_fields),
        grids,
    )
    .unwrap()
}
