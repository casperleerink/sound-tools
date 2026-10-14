//! What the track says about the editor and the views of its clips, with no window: the
//! problem is on the track, and the clip plays and keeps its data.

use sound_notes::{Clip, EditorName, EditorSpec, Editors, FieldName, View, ViewField};

use crate::support::{Harness, TICK, clip, id, level_changes, note};

const PIANO: &str = "state/arrangement/piano/instance.json";

/// `scale`, whose clip field `scale` is major or minor.
fn scale() -> EditorSpec {
    let field: ViewField = serde_json::from_str(
        r#"{"kind": "choice", "options": ["major", "minor"], "default": "major"}"#,
    )
    .unwrap();
    let grids = ["major", "minor"].map(|scale| {
        let choices = serde_json::from_str(&format!(r#"{{"scale": "{scale}"}}"#)).unwrap();
        let grid = serde_json::from_str(r#"{"rows": [{"pitch": 60}, {"pitch": 64}]}"#).unwrap();
        (choices, grid)
    });
    let fields = vec![(FieldName::new("scale").unwrap(), field)];
    let name = EditorName::new("scale").unwrap();
    EditorSpec::new(name, "Scale".into(), None, fields, Vec::new(), grids).unwrap()
}

fn in_editor(editor: &str, clip_view: &str) -> Clip {
    let mut clip = clip(0, 3840, vec![note(0, 480, 60)]);
    clip.editor = Some(EditorName::new(editor).unwrap());
    clip.view = serde_json::from_str::<View>(clip_view).unwrap();
    clip
}

#[test]
fn a_clip_whose_editor_or_view_is_wrong_is_a_problem_of_its_track_and_still_plays() {
    let harness = Harness::new();
    let editors = harness.project.provided::<Editors>().unwrap();
    assert!(editors.set_project(vec![scale()]).is_empty());
    let clips = vec![
        in_editor("scale", r#"{"scale": "minor"}"#),
        in_editor("drumz", r#"{"kit": "808"}"#),
        in_editor("scale", r#"{"scale": "lydian"}"#),
    ];
    let mut harness = harness.and_clips(clips.clone());
    assert_eq!(
        harness.problems(),
        [
            format!(
                r#"{PIANO}: clip-1.json names the clip editor "drumz", which this project does not have, so the window shows it in the piano roll and keeps its editor and view. The editors: piano-roll, scale"#
            ),
            format!(
                r#"{PIANO}: clip-2.json has "lydian" in view.scale, which the editor scale does not take, so it shows as "major". Use one of "major", "minor""#
            ),
        ]
    );
    // All three play their note, and keep what they had.
    assert_eq!(
        level_changes(&harness.play(960 * TICK)),
        [(0, 180.0), (480 * TICK, 0.0)]
    );
    for (index, clip) in clips.iter().enumerate() {
        let instance = harness
            .project
            .resolve::<Clip>(&id(&format!("arrangement/piano/clip-{index}")))
            .unwrap();
        assert_eq!(harness.project.state(&instance), Some(clip));
    }
    let file = std::fs::read_to_string(harness.path("state/arrangement/piano/clip-1.json"));
    assert!(file.unwrap().contains(r#""editor": "drumz""#));
}
