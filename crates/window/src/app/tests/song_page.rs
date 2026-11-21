//! The song page as the composer sees it: what it lays out, and its menus reached with the keys
//! alone, where the menu key or Shift-F10 on what has the keys opens what a right press opens.

use serde_json::json;

use super::{act, controls, key, laid_out, new_project, opened, redraw};
use crate::app::State;

fn song() -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let window = laid_out(opened(&scratch.path().join("song")));
    (scratch, window)
}

/// Gives `key` the keys, as Tab would, and presses Shift-F10: the labels of the controls then.
fn menu_of(window: &mut rune::Runtime<State>, element: &str) -> Vec<String> {
    window.update(|_, tx| tx.focus(element, true));
    redraw(window);
    window.key(rune::KeyEvent {
        key: rune::Key::Function(10),
        mods: rune::Mods {
            shift: true,
            ..rune::Mods::default()
        },
    });
    redraw(window);
    let labels = controls::collect(&window.state).controls.into_iter();
    let labels = labels.map(|control| control.label).collect();
    key(window, rune::Key::Escape);
    labels
}

#[test]
fn shift_f10_opens_the_menu_of_a_clip_a_head_a_note_and_a_device() {
    let (_scratch, mut window) = song();
    let has = |labels: &[String], label: &str| labels.iter().any(|each| each == label);

    assert!(has(&menu_of(&mut window, "clip-t00/bass"), "Export notes…"));
    assert!(has(&menu_of(&mut window, "track-t01"), "Show automation"));
    assert!(has(
        &menu_of(&mut window, "slot-instrument-header"),
        "Sampler"
    ));

    act(
        &mut window.state,
        json!({ "key": "clip-t00/bass", "action": "press", "value": { "clicks": 2 } }),
    );
    redraw(&mut window);
    assert!(has(&menu_of(&mut window, "note-0"), "Humanize"));
}

/// Whether `inner` lies inside `outer`, both [x, y, w, h].
fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
    inner[0] >= outer[0]
        && inner[1] >= outer[1]
        && inner[0] + inner[2] <= outer[0] + outer[2]
        && inner[1] + inner[3] <= outer[1] + outer[3]
}

#[test]
fn every_toggle_of_a_chosen_audio_track_lies_inside_its_head() {
    let (_scratch, mut window) = song();
    act(
        &mut window.state,
        json!({ "key": "add-track-kind", "action": "open" }),
    );
    act(
        &mut window.state,
        json!({ "key": "add-track-kind-Audio track", "action": "press" }),
    );
    act(
        &mut window.state,
        json!({ "key": "track-audio-1", "action": "press" }),
    );
    redraw(&mut window);
    let head = window.presented("track-audio-1").expect("the head").0;
    for toggle in ["lanes", "arm", "mute", "solo"] {
        let key = format!("{toggle}-audio-1");
        let rect = window.presented(&key).expect("the toggle").0;
        assert!(
            inside(rect, head),
            "{key} {rect:?} out of the head {head:?}"
        );
    }
}
