//! A short piece made by hand: from the default project, with nothing but a simulated mouse
//! and keys. Then the project closes and opens again, and the piece is there, byte for byte
//! on disk and sample for sample in an offline render.

use gpui::{TestAppContext, point, px};
use runtime::{OFFLINE, open_or_create, open_read_only};
use sound_core::Engine;
use sound_notes::Clip;

use crate::support::{self, BAR, Opened, STEP, clip, id, note, peak};

const MELODY: &str = "arrangement/track-1/clip";
const BASS: &str = "arrangement/track-2/clip";

/// Draws one note from just inside its first cell to its end.
fn draw(opened: &mut Opened<'_>, start: u64, length: u64, pitch: u8) {
    let from = opened.in_editor(start + 20, pitch);
    let to = opened.in_editor(start + length, pitch);
    opened.draw(from, to);
}

/// Plays the first three bars of the project folder offline, without its lock.
fn rendered(folder: &std::path::Path) -> Vec<f32> {
    let (mut project, mut engine, plugins) = open_read_only(folder).unwrap();
    project.engine().play();
    let frames = 6 * OFFLINE.sample_rate as usize;
    runtime::render(&mut project, &mut engine, &plugins, frames).unwrap()
}

#[gpui::test]
fn a_short_piece_is_made_by_hand_and_is_there_after_closing_and_opening(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |_| {});

    // A second track, from the project menu with the keys: open it, go to the first item.
    opened.keys("tab");
    opened.press_enter();
    opened.keys("down");
    opened.press_enter();
    assert_eq!(opened.undo_label().as_deref(), Some("Add track"));
    let tracks = opened.project(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        arrangement::tracks(project, arrangement.id()).len()
    });
    assert_eq!(tracks, 2);

    // A clip of one bar on each track, by double clicks.
    let first = opened.at(100, 0);
    opened.double_click(first);
    let second = opened.at(100, 1);
    opened.double_click(second);
    assert_eq!(opened.clip(MELODY), Some(clip(0, BAR, vec![])));
    assert_eq!(opened.clip(BASS), Some(clip(0, BAR, vec![])));

    // The melody: eight eighth notes, drawn in the editor of the first clip.
    let melody_clip = opened.at(1920, 0);
    opened.double_click(melody_clip);
    assert_eq!(opened.editor_clip(), Some(id(MELODY)));
    let melody = [60, 62, 64, 65, 67, 65, 64, 62];
    for (index, pitch) in melody.into_iter().enumerate() {
        draw(&mut opened, index as u64 * 480, 480, pitch);
    }

    // The bass line: a click on the other clip brings it into the editor. It is low, so the
    // editor scrolls down first, as far as it takes to clear the transport.
    let bass_clip = opened.at(1920, 1);
    opened.click(bass_clip);
    assert_eq!(opened.editor_clip(), Some(id(BASS)));
    let over_editor = opened.in_editor(1920, 64);
    opened.scroll(over_editor, 0., -144.);
    let bass = [48, 48, 55, 53];
    for (index, pitch) in bass.into_iter().enumerate() {
        draw(&mut opened, index as u64 * 960, 960, pitch);
    }

    // Move and resize: the bass clip gets a second bar, its last note goes down to the root
    // and rings into that bar, and the last melody note becomes a quarter.
    let edge = opened.at(BAR, 1) - point(px(2.), px(0.));
    let longer = opened.at(2 * BAR, 1) - point(px(2.), px(0.));
    opened.drag(edge, longer);
    opened.click(bass_clip);
    let (from, to) = (
        opened.in_editor(2880 + 400, 53),
        opened.in_editor(2880 + 400, 48),
    );
    opened.drag(from, to);
    let end = opened.in_editor(BAR, 48) - point(px(3.), px(0.));
    let later = opened.in_editor(BAR + 960, 48) - point(px(3.), px(0.));
    opened.drag(end, later);
    assert_eq!(opened.undo_label().as_deref(), Some("Resize note"));

    // Undo and redo, once each.
    let with_long_note = opened.clip(BASS).unwrap();
    opened.keys("cmd-z");
    assert_eq!(opened.clip(BASS).unwrap().notes[3], note(2880, 960, 48));
    opened.keys("shift-cmd-z");
    assert_eq!(opened.clip(BASS), Some(with_long_note));

    let expected_melody: Vec<_> = melody
        .into_iter()
        .enumerate()
        .map(|(index, pitch)| note(index as u64 * 480, 480, pitch))
        .collect();
    let expected_bass = vec![
        note(0, 960, 48),
        note(960, 960, 48),
        note(1920, 960, 55),
        note(2880, 1920, 48),
    ];
    let piece: [(&str, Clip); 2] = [
        (MELODY, clip(0, BAR, expected_melody)),
        (BASS, clip(0, 2 * BAR, expected_bass)),
    ];
    for (clip_id, expected) in &piece {
        assert_eq!(opened.clip(clip_id).as_ref(), Some(expected));
    }
    assert_eq!(opened.notice(), None);
    assert!(opened.project(|project| project.problems().is_empty()));

    // It plays in the window: the first note sounds.
    opened.keys("space");
    assert!(peak(&opened.render(OFFLINE.sample_rate as usize / 4)) > 0.01);
    opened.keys("space");
    opened.settle();

    // What is on disk now is the piece. Render it, close, open again, compare.
    let root = opened.path("");
    let files_before = support::files(&root);
    let audio_before = rendered(&root);
    assert!(peak(&audio_before) > 0.05);
    let folder = opened.close();

    let (control, _engine) = Engine::new(OFFLINE);
    let (project, _plugins) = open_or_create(folder.path(), control).unwrap();
    assert!(project.problems().is_empty());
    for (clip_id, expected) in &piece {
        let instance = project.resolve::<Clip>(&id(clip_id)).unwrap();
        assert_eq!(project.state(&instance), Some(expected));
    }
    assert_eq!(support::files(&root), files_before);
    drop(project);
    assert_eq!(support::files(&root), files_before);
    assert!(rendered(&root) == audio_before);
    // One snap step is a sixteenth, and every note of the piece is on it.
    assert!(
        piece
            .iter()
            .all(|(_, clip)| clip.notes.iter().all(|note| note.start.0 % STEP == 0))
    );
}
