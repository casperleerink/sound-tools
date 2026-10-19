//! The song of the DAW template (`templates/daw`) as a composer uses it: tracks added,
//! coloured, moved and deleted; a clip renamed, copied, pasted and nudged; automation lanes
//! added and their points edited; the note editor's lanes and grid; the ruler's tempo changes,
//! sections and repeat; a region chosen and deleted. Each gesture is one undo step.

use gpui::{
    ExternalPaths, FileDropEvent, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    Point, TestAppContext, point, px,
};
use sound_core::Ticks;

use super::song::open_song;
use super::{Opened, write_sine};
use crate::song::{ACROSS, BAR, HEADER};

/// Points per tick in the note editor at zoom 1, and the height of its lane, as `clip-editor.tsx` has them.
const EDITOR_ACROSS: f32 = 0.1;
const EDITOR_LANE: f32 = 64.;

impl Opened<'_> {
    fn alt_click(&mut self, position: Point<Pixels>) {
        let modifiers = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        self.cx.simulate_mouse_move(position, None, modifiers);
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    /// Picks the item `item` of the menu a press opened.
    pub(super) fn pick(&mut self, item: &'static str) {
        let item = self.until_found(item);
        self.click(item);
    }

    /// Drops a second of a sine from the system on bar 3 of the audio track, the third row, and
    /// waits for it to be drawn as long as it plays.
    fn drop_sine(&mut self) {
        let files = tempfile::tempdir().unwrap();
        write_sine(files.path(), 0.5);
        let file = files.path().join("assets/audio/sine.wav");
        let to = self.on_lane(2 * BAR, 2);
        self.cx.simulate_event(FileDropEvent::Entered {
            position: to - point(px(20.), px(0.)),
            paths: ExternalPaths(vec![file].into()),
        });
        for step in [10., 0.] {
            self.cx.update(|window, _| window.refresh());
            self.cx.run_until_parked();
            self.cx.simulate_event(FileDropEvent::Pending {
                position: to - point(px(step), px(0.)),
            });
        }
        self.cx
            .simulate_event(FileDropEvent::Submit { position: to });
        self.until("the region added", |window| window.has("song/voice/region"));
        // Its width is what it says it plays, once its file is read.
        let region = crate::support::id("song/voice/region");
        self.until("the region's length", |window| {
            window.project(|project| {
                (project.watches(&region).iter())
                    .any(|(name, watch)| name == "seconds" && watch.get() > 0.0)
            })
        });
        self.wait_for_bun();
    }

    fn project_file(&mut self) -> serde_json::Value {
        let session = self.session.clone();
        self.cx.read(|cx| {
            serde_json::to_value(session.read(cx).project().project_file()).unwrap_or_default()
        })
    }

    fn children_of(&mut self, instance: &str) -> Vec<String> {
        let session = self.session.clone();
        let id = crate::support::id(instance);
        self.cx.read(|cx| {
            let project = session.read(cx).project();
            (project.instances())
                .filter(|(child, _)| child.parent().as_ref() == Some(&id))
                .map(|(child, _)| child.name().to_string())
                .collect()
        })
    }
}

#[gpui::test]
fn a_track_is_added_coloured_and_deleted_from_menus(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let add = window.until_found("div-add-track");
    window.click(add);
    window.pick("menu-0");
    window.until("the track added", |window| window.has("song/track"));
    let mut parts = window.children_of("song/track");
    parts.sort();
    // A send to each return of the song.
    assert_eq!(
        parts,
        [
            "chain",
            "fx",
            "mix",
            "rec",
            "send-ret-a",
            "send-ret-b",
            "synth"
        ]
    );
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Add a track"));
    assert_eq!(window.problems(), []);

    // Right-click its name: colour, then delete.
    let name = window.until_found("div-name-track");
    window.click_with(name, MouseButton::Right);
    window.pick("menu-1");
    window.pick("menu-2");
    window.until("the track coloured", |window| {
        window.record("song/track")["colour"] == "#f0b44c"
    });
    let name = window.until_found("div-name-track");
    window.click_with(name, MouseButton::Right);
    window.pick("menu-4");
    window.until("the track deleted", |window| !window.has("song/track"));
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Delete a track"));
    assert!(
        window.record("song")["order"]
            .as_array()
            .is_some_and(|order| !order.iter().any(|name| name == "track"))
    );
}

#[gpui::test]
fn a_track_head_dragged_onto_another_goes_before_it(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 3, 2);
    window.wait_for_bun();
    // By the left edge of a head, where none of its buttons is.
    let edge = point(px(HEADER / 2. - 2.), px(0.));
    let from = window.until_found("div-head-t02") - edge;
    let to = window.until_found("div-head-t00") - edge;
    window.press(from);
    for step in [0.2, 0.5, 0.8, 1.0] {
        window.drag_to(from + (to - from) * step);
        window.find("div-head-t00");
    }
    window.release(to);
    window.until("the track moved", |window| {
        window.record("song")["order"]
            .get(0)
            .and_then(|name| name.as_str())
            == Some("t02")
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Move a track"));
}

#[gpui::test]
fn a_clip_is_renamed_copied_pasted_and_nudged(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let clip = window.on_lane(BAR / 2, 1);
    window.click_with(clip, MouseButton::Right);
    window.pick("menu-1");
    window.until_found("div-clip-name");
    window.cx.simulate_input("Verse");
    window.cx.simulate_keystrokes("enter");
    window.until("the clip renamed", |window| {
        window.record("song/t01/c00")["title"] == "Verse"
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Rename a clip"));

    // Chosen, copied, and pasted at the playhead, the start, on its track.
    let clip = window.on_lane(BAR / 2, 1);
    window.click(clip);
    window.cx.simulate_keystrokes("cmd-c");
    window.cx.simulate_keystrokes("cmd-v");
    window.until("the clip pasted", |window| window.has("song/t01/clip"));
    let pasted = window.record("song/t01/clip");
    assert_eq!(pasted["start"], 0);
    assert_eq!(pasted["notes"], window.record("song/t01/c00")["notes"]);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Paste clips"));

    // The pasted clip is the chosen one: an arrow nudges it a beat on.
    window.cx.simulate_keystrokes("right");
    window.until("the clip nudged", |window| {
        window.record("song/t01/clip")["start"] == 960
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Move clips"));
    assert_eq!(window.record("song/t01/c00")["start"], 0);
}

#[gpui::test]
fn a_lane_is_added_from_a_track_menu_and_its_points_edited(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let name = window.until_found("div-name-t00");
    window.click_with(name, MouseButton::Right);
    window.pick("menu-3");
    // The first number the synth offers.
    window.pick("menu-0");
    window.until("the lane added", |window| {
        window.record("song/t00")["automation"]
            .as_array()
            .map(Vec::len)
            == Some(2)
    });
    let added = window.record("song/t00")["automation"][1].clone();
    let lane = added["lane"].as_str().unwrap_or_default().to_string();
    assert!(lane.starts_with("lane-synth"), "{added}");
    // It starts where the number is, which the file of the synth leaves at its default.
    let number = added["target"]
        .as_str()
        .unwrap()
        .trim_start_matches("synth.")
        .to_string();
    let session = window.session.clone();
    let synth = crate::support::id("song/t00/synth");
    let record = (window.cx).read(|cx| {
        session
            .read(cx)
            .project()
            .automation(&synth, &number)
            .and_then(|number| number.record)
    });
    let first = window.record(&format!("song/t00/{lane}"))["points"][0]["value"].as_f64();
    assert_eq!(first.map(|value| value as f32), record, "{number}");
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Add a lane"));
    assert_eq!(window.problems(), []);

    // The lanes show under the track: the cutoff's, then the new one. A double click adds a
    // point to the cutoff's, and an alt click on it takes it away again.
    let at = window.on_lane(2 * BAR, 1);
    window.until_found("div-remove-t00-lane-cutoff");
    window.double_click(at);
    let points = |window: &mut Opened<'_>| {
        window.record("song/t00/lane-cutoff")["points"]
            .as_array()
            .map(Vec::len)
    };
    window.until("a point added", |window| points(window) == Some(3));
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Add a point"));
    let added = window.record("song/t00/lane-cutoff")["points"][1].clone();
    assert_eq!(added["tick"], 2 * BAR, "{added}");
    window.alt_click(at);
    window.until("the point removed", |window| points(window) == Some(2));
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Remove a point"));

    // The filter's card shows what the lane plays into its cutoff.
    let session = window.session.clone();
    let filter = crate::support::id("song/t00/fx");
    assert!(
        window
            .cx
            .read(|cx| session.read(cx).project().lanes(&filter).is_some())
    );
}

#[gpui::test]
fn the_note_editor_edits_velocity_and_a_bend_lane_on_its_grid(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let clip = window.on_lane(BAR / 2, 1);
    window.double_click(clip);
    let lane = window.space_at("editor-lane");

    // The velocity of the first note, which starts the clip, dragged up.
    let velocity = window.record("song/t01/c00")["notes"][0]["velocity"]
        .as_u64()
        .unwrap_or(100);
    let bar = lane + point(px(4.), px(EDITOR_LANE * (1. - velocity as f32 / 127.) + 3.));
    window.press(bar);
    window.drag_to(bar - point(px(0.), px(10.)));
    window.drag_to(bar - point(px(0.), px(20.)));
    window.release(bar - point(px(0.), px(20.)));
    window.until("the velocity changed", |window| {
        window.record("song/t01/c00")["notes"][0]["velocity"].as_u64() != Some(velocity)
    });
    assert!(window.record("song/t01/c00")["notes"][0]["velocity"].as_u64() > Some(velocity));
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Change velocity"));

    // The bend lane: a double click in its middle adds a point at rest.
    let bend = window.until_found("div-lane-bend");
    window.click(bend);
    let at = lane + point(px(960. * EDITOR_ACROSS), px(EDITOR_LANE / 2.));
    window.double_click(at);
    window.until("a bend point", |window| {
        window.record("song/t01/c00")["bend"]
            .as_array()
            .map(Vec::len)
            == Some(1)
    });
    let point_added = window.record("song/t01/c00")["bend"][0].clone();
    assert_eq!(point_added["tick"], 960);
    assert!(
        point_added["value"]
            .as_i64()
            .is_some_and(|value| value.abs() < 300),
        "{point_added}"
    );

    // A grid of bars: a note added half way into the second bar starts at the bar.
    let snap = window.until_found("div-snap");
    window.click(snap);
    window.pick("menu-0");
    let notes = window.record("song/t01/c00")["notes"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let editor = window.space_at("editor");
    window.double_click(editor + point(px(1.5 * BAR as f32 * EDITOR_ACROSS), px(2.)));
    window.until("a note added", |window| {
        window.record("song/t01/c00")["notes"]
            .as_array()
            .map(Vec::len)
            == Some(notes + 1)
    });
    let added = window.record("song/t01/c00")["notes"][notes].clone();
    assert_eq!(added["start"], BAR, "{added}");
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn the_ruler_adds_a_tempo_change_and_a_section_and_drags_a_repeat(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let ruler = window.space_at("ruler");
    let third = ruler + point(px((2 * BAR + 200) as f32 * ACROSS), px(8.));
    window.click_with(third, MouseButton::Right);
    window.pick("menu-0");
    window.until_found("div-ruler-edit");
    window.cx.simulate_input("90");
    window.cx.simulate_keystrokes("enter");
    window.until("a tempo change", |window| {
        let file = window.project_file();
        file["tempo_map"]["tempo_changes"]
            .as_array()
            .is_some_and(|changes| {
                changes
                    .iter()
                    .any(|change| change["tick"] == 2 * BAR && change["bpm"] == 90.0)
            })
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Change the tempo"));

    window.click_with(third, MouseButton::Right);
    window.pick("menu-2");
    window.until_found("div-ruler-edit");
    window.cx.simulate_input("Chorus");
    window.cx.simulate_keystrokes("enter");
    window.until("a section", |window| {
        window.project_file()["sections"] == serde_json::json!([{ "bar": 3, "name": "Chorus" }])
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Name a section"));

    // A drag under the ruler repeats bars 2 and 3; the repeat is no edit.
    let strip = window.space_at("loop") + point(px(0.), px(5.));
    let (from, to) = (
        strip + point(px(BAR as f32 * ACROSS), px(0.)),
        strip + point(px(3. * BAR as f32 * ACROSS), px(0.)),
    );
    window.press(from);
    window.drag_to(from + point(px(20.), px(0.)));
    window.drag_to(to);
    window.release(to);
    let session = window.session.clone();
    window.until("bars two and three repeat", |window| {
        let looped = window
            .cx
            .read(|cx| session.read(cx).looped().read(cx).clone());
        looped.on && looped.range == (Ticks(BAR)..Ticks(3 * BAR))
    });
    assert_eq!(window.undo_label().as_deref(), Some("Name a section"));
}

#[gpui::test]
fn a_region_dropped_on_the_audio_track_is_chosen_and_deleted(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    let region = window.record("song/voice/region");
    assert_eq!(region["asset"], "sine.wav");
    assert_eq!(region["start"], 2 * BAR);
    // Its waveform shows once its length is known: a click on it chooses it.
    let inside = window.on_lane(2 * BAR + 960, 2);
    window.until_found("div-mixer-toggle");
    window.click(inside);
    window.until_found("div-region-panel");
    window.cx.simulate_keystrokes("backspace");
    window.until("the region deleted", |window| {
        !window.has("song/voice/region")
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Remove a region"));
    assert_eq!(window.problems(), []);
}

/// A clip moved on its own track takes the line of its automation along: the line under it
/// lands where it lands, and the move is one undo step.
#[gpui::test]
fn a_clip_moved_on_its_track_takes_its_automation_along(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 4, 12);
    let before = window.record("song/t00/lane-cutoff");
    // The cutoff starts at 6 kHz on tick 0, under the first clip of the first track.
    assert_eq!(before["points"][0]["tick"], 0);
    let at = window.on_lane(BAR / 2, 0);
    window.wait_for_bun();
    window.press(at);
    window.drag_to(at + point(px(10.), px(0.)));
    window.drag_to(at + point(px(96. * 2.), px(0.)));
    window.release(at + point(px(96. * 2.), px(0.)));
    window.until("the clip moved", |window| {
        window.record("song/t00/c00")["start"] != 0
    });
    let start = window.record("song/t00/c00")["start"].as_u64().unwrap();
    let points = window.record("song/t00/lane-cutoff")["points"].clone();
    let at_tick = |tick: u64| {
        (points.as_array().unwrap().iter()).find(|point| point["tick"].as_u64() == Some(tick))
    };
    assert_eq!(
        at_tick(start).map(|point| point["value"].clone()),
        Some(6000.into())
    );
    assert!(at_tick(0).is_none(), "{points}");
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Move clips"));
    assert!(window.undo().is_some());
    assert_eq!(window.record("song/t00/lane-cutoff"), before);
    assert_eq!(window.record("song/t00/c00")["start"], 0);
}

/// The end of a region trims as its start does: a beat in from the end of a second of a sine at
/// 120 bpm leaves half a second, read from what the region says it plays.
#[gpui::test]
fn a_region_trims_from_its_end(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    // A second at 120 bpm is two beats: its end, a little inside.
    let end = window.on_lane(2 * BAR + 1920, 2) - point(px(2.), px(0.));
    window.until_found("div-mixer-toggle");
    window.press(end);
    window.drag_to(end - point(px(5.), px(0.)));
    window.drag_to(end - point(px(960. * ACROSS), px(0.)));
    window.release(end - point(px(960. * ACROSS), px(0.)));
    window.until("the end trimmed", |window| {
        window.record("song/voice/region")["file_end_seconds"]
            .as_f64()
            .is_some()
    });
    let trimmed = window.record("song/voice/region")["file_end_seconds"]
        .as_f64()
        .unwrap();
    assert!((trimmed - 0.5).abs() < 1e-3, "{trimmed}");
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Trim a region"));
}

/// A drag over empty space chooses the clips it covers, on the rows it covers: delete removes
/// those and only those.
#[gpui::test]
fn a_marquee_over_the_timeline_chooses_the_clips_under_it(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 4, 12);
    window.wait_for_bun();
    // Between the first and second clips of each track (every clip is one or two bars), down
    // to the middle of the third track: the second clips of the first three tracks.
    let from = window.on_lane(2 * BAR + BAR / 2, 0) - point(px(0.), px(10.));
    let to = window.on_lane(3 * BAR + BAR / 2, 2);
    window.press(from);
    window.drag_to(from + point(px(10.), px(10.)));
    window.drag_to(to);
    window.release(to);
    let session = window.session.clone();
    window.until("the clips chosen", |window| {
        (window.cx)
            .read(|cx| session.read(cx).selected_span())
            .is_some()
    });
    window.cx.simulate_keystrokes("backspace");
    window.until("the clips removed", |window| !window.has("song/t00/c01"));
    for (track, gone) in [("t00", true), ("t01", true), ("t02", true), ("t03", false)] {
        assert_eq!(!window.has(&format!("song/{track}/c01")), gone, "{track}");
        assert!(window.has(&format!("song/{track}/c00")), "{track}");
    }
}

/// Notes chosen in the editor are copied with cmd-c and pasted with cmd-v at the playhead, the
/// start of the clip here, as one undo step.
#[gpui::test]
fn notes_are_copied_and_pasted_in_the_editor(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 4, 12);
    let notes = |window: &mut Opened| {
        window.record("song/t00/c00")["notes"]
            .as_array()
            .map_or(0, Vec::len)
    };
    let count = notes(&mut window);
    window.wait_for_bun();
    let clip = window.on_lane(BAR / 2, 0);
    window.double_click(clip);
    window.space_at("editor");
    window.cx.simulate_keystrokes("cmd-a cmd-c cmd-v");
    window.until("the notes pasted", |window| notes(window) == 2 * count);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Paste notes"));
}

/// A clip whose file leaves out its notes reads as one of no notes: cmd-a and cmd-c in the
/// editor choose and copy nothing, and nothing fails.
#[gpui::test]
fn the_editor_of_a_clip_whose_file_has_no_notes_takes_its_keys(cx: &mut TestAppContext) {
    let mut files = crate::song::song_files(1, 1);
    for (path, text) in &mut files {
        if path == "state/song/t00/c00.json" {
            *text = format!(r#"{{"tool": "timeline", "state": {{"start": 0, "length": {BAR}}}}}"#);
        }
    }
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut window = super::open(cx, &files);
    window.wait_for_bun();
    let clip = window.on_lane(BAR / 2, 0);
    window.double_click(clip);
    window.space_at("editor");
    window.cx.simulate_keystrokes("cmd-a cmd-c");
    window.wait_for_bun();
    assert_eq!(window.problems(), []);
}

/// Notes the editor would put at or past the end of the clip are left out, and notes an undo
/// took away are no longer chosen: neither is a problem.
#[gpui::test]
fn the_editor_keeps_notes_inside_the_clip_and_forgets_what_an_undo_took(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 1, 1);
    let notes = |window: &mut Opened| {
        window.record("song/t00/c00")["notes"]
            .as_array()
            .map_or(0, Vec::len)
    };
    let count = notes(&mut window);
    window.wait_for_bun();
    let clip = window.on_lane(BAR / 2, 0);
    window.double_click(clip);
    window.space_at("editor");
    // A copy of every note a span on starts past the end of the clip.
    window.cx.simulate_keystrokes("cmd-a cmd-d");
    window.wait_for_bun();
    assert_eq!(notes(&mut window), count);
    window.cx.simulate_keystrokes("cmd-a cmd-c cmd-v");
    window.until("the notes pasted", |window| notes(window) == 2 * count);
    window.cx.simulate_keystrokes("cmd-z");
    window.until("the paste undone", |window| notes(window) == count);
    window.cx.simulate_keystrokes("cmd-c cmd-d");
    window.wait_for_bun();
    assert_eq!(window.problems(), []);
}

/// A clip made shorter than its notes loses what starts past its new end, in one undo step.
#[gpui::test]
fn a_clip_made_shorter_cuts_what_starts_past_its_end(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 1, 1);
    window.wait_for_bun();
    let length = window.record("song/t00/c00")["length"].as_u64().unwrap();
    let clip = window.on_lane(BAR / 4, 0);
    window.click(clip);
    window.after_the_runs();
    let end = window.on_lane(length, 0) - point(px(2.), px(0.));
    let half = px((length / 2) as f32 * ACROSS);
    window.press(end);
    window.drag_to(end - point(half / 2., px(0.)));
    window.drag_to(end - point(half, px(0.)));
    window.release(end - point(half, px(0.)));
    window.until("the clip shorter", |window| {
        window.record("song/t00/c00")["length"].as_u64() < Some(length)
    });
    let record = window.record("song/t00/c00");
    let short = record["length"].as_u64().unwrap();
    let starts = record["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|note| note["start"].as_u64().unwrap());
    assert!(starts.into_iter().all(|start| start < short), "{record}");
    assert_eq!(window.problems(), []);
}

/// A region after a change of tempo is as long as its file at that tempo, wherever the playhead
/// is: a second at 60 bpm is a beat, and its end trimmed half a beat in leaves half a second.
#[gpui::test]
fn a_region_after_a_tempo_change_is_as_long_as_its_file_at_that_tempo(cx: &mut TestAppContext) {
    let mut files = crate::song::song_files(2, 2);
    for (path, text) in &mut files {
        if path == "project.json" {
            let slow = format!(
                r#"{{ "tick": 0, "bpm": 120.0 }}, {{ "tick": {}, "bpm": 60.0 }}"#,
                2 * BAR
            );
            *text = text.replace(r#"{"tick":0,"bpm":120.0}"#, &slow);
        }
    }
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut window = super::open(cx, &files);
    assert_eq!(
        window.project_file()["tempo_map"]["tempo_changes"][1]["bpm"],
        60.0
    );
    window.wait_for_bun();
    window.drop_sine();
    let end = window.on_lane(2 * BAR + 960, 2) - point(px(2.), px(0.));
    window.until_found("div-mixer-toggle");
    window.press(end);
    window.drag_to(end - point(px(5.), px(0.)));
    window.drag_to(end - point(px(480. * ACROSS), px(0.)));
    window.release(end - point(px(480. * ACROSS), px(0.)));
    window.until("the end trimmed", |window| {
        window.record("song/voice/region")["file_end_seconds"]
            .as_f64()
            .is_some()
    });
    let trimmed = window.record("song/voice/region")["file_end_seconds"]
        .as_f64()
        .unwrap();
    assert!((trimmed - 0.5).abs() < 1e-3, "{trimmed}");
    assert_eq!(window.record("song/voice/region")["start"], 2 * BAR);
}
