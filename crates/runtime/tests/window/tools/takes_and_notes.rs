//! The DAW template's recording and note editor, as the arrangement had them: the keyboard
//! records on the first track while none is chosen, a stop ends the recording, and the editor's
//! keys, ruler and velocity lane.

use gpui::{
    KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, MouseUpEvent, TestAppContext,
    point, px,
};
use serde_json::Value;
use sound_ui::LiveNote;

use super::Opened;
use super::song::open_song;
use crate::song::{ACROSS, BAR, ROW};

impl Opened<'_> {
    /// A key down and up, as a person presses it, and Bun's answer.
    fn tap(&mut self, key: &str) {
        let keystroke = Keystroke::parse(key).unwrap();
        self.cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        self.cx.simulate_event(KeyUpEvent { keystroke });
        self.wait_for_bun();
    }

    fn notes_of(&mut self, clip: &str) -> Vec<Value> {
        self.record(clip)["notes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn takes(&mut self) -> Vec<String> {
        (self.clips().into_iter())
            .map(|(id, _)| id)
            .filter(|id| id.contains("take"))
            .collect()
    }

    /// The engine plays on for about a quarter of a second, and the window polls.
    fn play_on(&mut self) {
        for _ in 0..6 {
            self.loudest(2_048);
            self.settle();
        }
    }
}

/// The note of `notes` at `start` on `pitch`.
fn at<'a>(notes: &'a [Value], start: &Value, pitch: &Value) -> &'a Value {
    (notes.iter())
        .find(|note| note["start"] == *start && note["pitch"] == *pitch)
        .unwrap()
}

/// With nothing chosen the keyboard plays the first track, and R records it there.
#[gpui::test]
fn r_with_nothing_chosen_records_the_keyboard_on_the_first_track(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.tap("r");
    window.play_on();
    window.play_midi(60);
    window.play_on();
    window.tap("r");
    window.until("the take", |window| {
        window.play_on();
        !window.takes().is_empty()
    });
    let takes = window.takes();
    assert!(
        takes.iter().all(|take| take.starts_with("song/t00/")),
        "{takes:?}"
    );
    assert_eq!(window.problems(), []);
}

/// While it records, a take shows the notes it heard so far: a key still held has no end.
#[gpui::test]
fn a_take_shows_its_notes_while_it_records(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.tap("r");
    window.play_on();
    window.play_midi(60);
    let recorder = crate::support::id("song/t00/rec");
    window.until("the held key in the take", |window| {
        window.play_on();
        let session = window.session.clone();
        let notes = window.cx.read(|cx| {
            let recording = session.read(cx).recording().read(cx);
            recording.notes_of(&recorder).map(|take| take.notes.clone())
        });
        matches!(
            notes.as_deref(),
            Some([LiveNote {
                pitch: 60,
                end: None,
                ..
            }])
        )
    });
}

/// A stop ends the recording: playing again records nothing.
#[gpui::test]
fn a_stop_ends_the_recording(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let record = window.until_found("button-record");
    window.cx.simulate_click(record, Modifiers::default());
    window.wait_for_bun();
    window.play_on();
    window.play_midi(60);
    window.play_on();
    let session = window.session.clone();
    let stop = |window: &mut Opened| {
        (window.cx).update(|_, cx| session.update(cx, |session, _| session.engine().stop()));
        window.play_on();
        window.wait_for_bun();
    };
    stop(&mut window);
    window.until("the take", |window| {
        window.play_on();
        window.takes().len() == 1
    });
    window.start_playing();
    window.play_midi(62);
    window.play_on();
    stop(&mut window);
    assert_eq!(window.takes().len(), 1, "{:?}", window.takes());
}

/// Alt-up steps the chosen velocities, cmd-click adds a note to the choice and a click on one of
/// them chooses it alone, cmd-d copies right after the chosen notes, and escape lets go of them
/// and then closes the editor.
#[gpui::test]
fn the_editor_steps_velocities_duplicates_after_and_closes_on_escape(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 1, 1);
    window.wait_for_bun();
    let clip = window.on_lane(BAR / 2, 0);
    window.double_click(clip);
    let editor = window.editor("t00/c00");
    // Two notes in the middle rows, which the editor shows as it opens.
    let mut notes = window.notes_of("song/t00/c00");
    notes.sort_by_key(|note| note["pitch"].as_u64());
    let (a, b) = (
        notes[notes.len() / 2].clone(),
        notes[notes.len() / 2 + 1].clone(),
    );
    let middle = |note: &Value| {
        editor.at(
            note["start"].as_f64().unwrap() as f32 + 20.,
            note["pitch"].as_u64().unwrap() as u8,
        )
    };
    window.click(middle(&a));
    window.press_with(middle(&b), 1, Modifiers::command());
    window.cx.simulate_event(MouseUpEvent {
        position: middle(&b),
        modifiers: Modifiers::command(),
        button: MouseButton::Left,
        click_count: 1,
    });
    window.wait_for_bun();
    window.cx.simulate_keystrokes("alt-up");
    window.wait_for_bun();
    let stepped = window.notes_of("song/t00/c00");
    for note in [&a, &b] {
        let now = at(&stepped, &note["start"], &note["pitch"]);
        let was = note["velocity"].as_u64().unwrap();
        assert_eq!(
            now["velocity"].as_u64(),
            Some((was + 10).min(127)),
            "{note}"
        );
    }

    // A click on one of the two, which does not move it, chooses it alone.
    let a = at(&stepped, &a["start"], &a["pitch"]).clone();
    window.click(middle(&a));
    window.wait_for_bun();
    window.cx.simulate_keystrokes("cmd-d");
    window.wait_for_bun();
    let duplicated = window.notes_of("song/t00/c00");
    let added: Vec<&Value> = (duplicated.iter())
        .filter(|note| !stepped.contains(note))
        .collect();
    let end = a["start"].as_u64().unwrap() + a["length"].as_u64().unwrap();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(added[0]["start"].as_u64(), Some(end));

    window.tap("escape");
    assert!(window.find("space-editor-t00/c00").is_some());
    window.tap("escape");
    window.until("the editor closed", |window| {
        window.find("space-editor-t00/c00").is_none()
    });
}

/// A click on the editor's ruler plays from there, on the grid, and a line drawn across the
/// velocity lane gives each note it passes the velocity of its height.
#[gpui::test]
fn the_editor_ruler_seeks_and_a_line_draws_velocities(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 1, 1);
    window.wait_for_bun();
    let clip = window.on_lane(BAR / 2, 0);
    window.double_click(clip);
    let editor = window.editor("t00/c00");
    let ruler = window.space_at("editor-t00/c00-ruler");
    window.click(point(editor.at(970., 60).x, ruler.y + px(16.)));
    let session = window.session.clone();
    window.until("the playhead on the beat", |window| {
        window
            .cx
            .read(|cx| session.read(cx).playhead().read(cx).tick)
            == sound_core::Ticks(960)
    });

    // From the bottom before the first note to the top half way through the clip.
    let notes = window.notes_of("song/t00/c00");
    let from = editor.lane(-20., 52.);
    let to = editor.lane(BAR as f32 / 2., 8.);
    window.press(from);
    window.drag_to(from + point(px(10.), px(0.)));
    window.drag_to(to);
    window.release(to);
    window.until("the velocities drawn", |window| {
        window.notes_of("song/t00/c00") != notes
    });
    let drawn = window.notes_of("song/t00/c00");
    let mut passed: Vec<(u64, u64)> = (drawn.iter())
        .filter(|note| note["start"].as_u64() < Some(BAR / 2))
        .map(|note| {
            (
                note["start"].as_u64().unwrap(),
                note["velocity"].as_u64().unwrap(),
            )
        })
        .collect();
    passed.sort_unstable();
    assert!(passed.len() > 2, "{passed:?}");
    assert!(
        passed.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "{passed:?}"
    );
    assert!(
        passed[0].1 < 20 && passed.last().unwrap().1 > 100,
        "{passed:?}"
    );
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Draw velocities"));
}

/// A chosen region's handles at its top: the fade in drags across and the gain up, each one
/// undo step, as on the arrangement's clips.
#[gpui::test]
fn a_regions_handles_drag_its_fade_in_and_its_gain(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    window.until_found("button-mixer-toggle");
    let middle = window.on_lane(2 * BAR + 960, 2);
    window.click(middle);
    window.wait_for_bun();
    // The handles are 7 points under the top of the region, which is 4 under the row's.
    let top = middle.y - px(ROW / 2.) + px(11.);
    let start = window.on_lane(2 * BAR, 2).x;

    let fade = point(start + px(5.), top);
    window.press(fade);
    window.drag_to(fade + point(px(5.), px(0.)));
    window.drag_to(fade + point(px(480. * ACROSS), px(0.)));
    window.release(fade + point(px(480. * ACROSS), px(0.)));
    window.until("the fade in", |window| {
        window.record("song/audio-voice/region")["fade_in_ms"].as_f64() > Some(0.)
    });
    let ms = window.record("song/audio-voice/region")["fade_in_ms"]
        .as_f64()
        .unwrap();
    assert!((ms - 250.).abs() < 10., "{ms}");
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Change fade in"));

    let gain = point(middle.x, top);
    window.press(gain);
    window.drag_to(gain - point(px(0.), px(5.)));
    window.drag_to(gain - point(px(0.), px(50.)));
    window.release(gain - point(px(0.), px(50.)));
    window.until("the gain", |window| {
        window.record("song/audio-voice/region")["gain_db"].as_f64() > Some(0.)
    });
    let db = window.record("song/audio-voice/region")["gain_db"]
        .as_f64()
        .unwrap();
    assert!((db - 18.).abs() < 0.5, "{db}");
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Change gain"));
    assert_eq!(window.problems(), []);
}

/// Alt-up steps a chosen region's gain by a decibel, as the arrangement's keys did.
#[gpui::test]
fn alt_up_steps_a_chosen_regions_gain(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    window.until_found("button-mixer-toggle");
    let region = window.on_lane(2 * BAR + 960, 2);
    window.click(region);
    window.wait_for_bun();
    window.cx.simulate_keystrokes("alt-up alt-up");
    window.until("the gain stepped", |window| {
        window.record("song/audio-voice/region")["gain_db"].as_f64() == Some(2.)
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Change gain"));
}
