//! The song of the DAW template (`templates/daw`) as a composer uses it: tracks added,
//! coloured, moved and deleted; a clip renamed, copied, pasted and nudged; automation lanes
//! added and their points edited; the note editor's lanes and grid; the ruler's tempo changes,
//! sections and repeat; a region chosen and deleted. Each gesture is one undo step.

use gpui::{
    Bounds, ExternalPaths, FileDropEvent, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    Pixels, Point, TestAppContext, point, px,
};
use sound_core::Ticks;

use super::song::open_song;
use super::{Opened, write_sine};
use crate::song::{ACROSS, BAR, HEADER, LEAD};

/// The note editor's rows and lane, as `clip-editor.tsx` has them: 12 points a pitch, a lane 56
/// tall, and as it opens on a clip, 8 points before it and 96 after it, 24 to 192 points a beat.
const EDITOR_ROW: f32 = 12.;
const EDITOR_LANE: f32 = 56.;

/// The note editor of one clip, as it opened on it.
pub(super) struct Editor {
    roll: Bounds<Pixels>,
    lane: Point<Pixels>,
    across: f32,
    top: f32,
}

impl Editor {
    /// Where `tick` of the clip and the middle of the row of `pitch` are in the roll.
    pub(super) fn at(&self, tick: f32, pitch: u8) -> Point<Pixels> {
        let row = 127. - f32::from(pitch) - self.top;
        self.roll.origin
            + point(
                px(8. + tick * self.across),
                px(row * EDITOR_ROW + EDITOR_ROW / 2.),
            )
    }

    /// Where `tick` of the clip is in the lane, `down` points from its top.
    pub(super) fn lane(&self, tick: f32, down: f32) -> Point<Pixels> {
        self.lane + point(px(8. + tick * self.across), px(down))
    }
}

impl Opened<'_> {
    /// The editor of `clip`, `track/clip` of the song, once it is open and fitted to the clip.
    pub(super) fn editor(&mut self, clip: &str) -> Editor {
        let key = format!("editor-{clip}");
        let roll = self.space_at(&key);
        self.wait_for_bun();
        let selector: &'static str = format!("space-{key}").leak();
        let roll = self
            .cx
            .debug_bounds(selector)
            .map_or(Bounds::new(roll, Default::default()), |bounds| bounds);
        let lane = self.space_at(&format!("{key}-lane"));
        let record = self.record(&format!("song/{clip}"));
        let notes = record["notes"].as_array().cloned().unwrap_or_default();
        let field = |note: &serde_json::Value, name: &str| note[name].as_f64().unwrap_or(0.) as f32;
        let end = notes
            .iter()
            .map(|note| field(note, "start") + field(note, "length"))
            .fold(1., f32::max);
        let length = record["length"]
            .as_f64()
            .map_or(end, |length| length as f32);
        let width = f32::from(roll.size.width);
        let across = ((width - 104.) / length).clamp(0.025, 0.2);
        let pitches = notes.iter().map(|note| field(note, "pitch"));
        let middle = match (pitches.clone().reduce(f32::min), pitches.reduce(f32::max)) {
            (Some(low), Some(high)) => (low + high) / 2.,
            _ => 60.,
        };
        let height = f32::from(roll.size.height);
        let top = ((127. - middle + 0.5) * EDITOR_ROW - height / 2.).round() / EDITOR_ROW;
        Editor {
            roll,
            lane,
            across,
            top,
        }
    }

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
        self.until("the region added", |window| {
            window.has("song/audio-voice/region")
        });
        // Its width is what it says it plays, once its file is read.
        let region = crate::support::id("song/audio-voice/region");
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
    // Add track adds an instrument track; its chevron offers the other kinds.
    let add = window.until_found("div-add-track");
    window.click(add);
    window.until("the track added", |window| window.has("song/track"));
    let mut parts = window.children_of("song/track");
    parts.sort();
    // A send to each return of the song.
    assert_eq!(
        parts,
        ["chain", "mix", "rec", "send-ret-a", "send-ret-b", "synth"]
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
        window.record("song/track")["colour"] == "mauve"
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
    // The first number the synth offers, after the track's own volume and pan.
    window.pick("menu-2");
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
    window.until_found("div-lane-name-t00-lane-cutoff");
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
    // Dragged off the grid, it lands on a sixteenth: a whole tick the lane takes.
    window.drag(at, at + point(px(37.), px(0.)));
    window.until("the point moved", |window| {
        window.record("song/t00/lane-cutoff")["points"][1]["tick"] != 2 * BAR
    });
    let tick = window.record("song/t00/lane-cutoff")["points"][1]["tick"].as_u64();
    assert!(tick.is_some_and(|tick| tick % (BAR / 16) == 0), "{tick:?}");
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Move a point"));
    assert_eq!(window.problems(), []);
    let at = window.on_lane(tick.unwrap_or_default(), 1);
    window.alt_click(at);
    window.until("the point removed", |window| points(window) == Some(2));
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Remove a point"));

    // The filter's card shows what the lane plays into its cutoff.
    let session = window.session.clone();
    let filter = crate::support::id("song/t00/chain/filter");
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
    let editor = window.editor("t01/c00");

    // The velocity of the first note, which starts the clip, dragged up.
    let velocity = window.record("song/t01/c00")["notes"][0]["velocity"]
        .as_u64()
        .unwrap_or(100);
    let bar = editor.lane(1., EDITOR_LANE - 4. - 44. * velocity as f32 / 127. + 3.);
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
    let lanes = window.until_found("div-editor-lane-pick");
    window.click(lanes);
    window.pick("menu-2");
    let at = editor.lane(1000., EDITOR_LANE / 2.);
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
    let snap = window.until_found("div-song-snap");
    window.click(snap);
    window.pick("menu-0");
    let notes = window.record("song/t01/c00")["notes"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let pitch = free_pitch(&window.record("song/t01/c00"), 1.5 * BAR as f32);
    window.double_click(editor.at(1.5 * BAR as f32, pitch));
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
    let third = ruler + point(px(LEAD + (2 * BAR + 200) as f32 * ACROSS), px(8.));
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

    // A drag over the ruler, away from the repeat's bar, repeats bars 6 and 7; it is no edit.
    let strip = window.space_at("ruler") + point(px(LEAD + (5 * BAR) as f32 * ACROSS), px(8.));
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
        looped.on && looped.range == (Ticks(6 * BAR)..Ticks(8 * BAR))
    });
    assert_eq!(window.undo_label().as_deref(), Some("Name a section"));
}

#[gpui::test]
fn the_repeat_bar_turns_on_and_off_moves_and_stretches_and_a_double_click_adds_a_section(
    cx: &mut TestAppContext,
) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let session = window.session.clone();
    let looped = |window: &mut Opened<'_>| {
        let looped = window
            .cx
            .read(|cx| session.read(cx).looped().read(cx).clone());
        (
            looped.on,
            looped.range.start.0 / BAR,
            looped.range.end.0 / BAR,
        )
    };
    // The repeat's bar is at the foot of the ruler, over bars 1 to 4 until it moves.
    let ruler = window.space_at("ruler");
    let on_bar =
        |tick: u64, nudge: f32| ruler + point(px(LEAD + tick as f32 * ACROSS + nudge), px(26.));
    // Each press waits for the bar where the last change put it.
    let drag = |window: &mut Opened<'_>, from: Point<Pixels>, by: f32| {
        window.wait_for_bun();
        window.press(from);
        window.drag_to(from + point(px(by / 2.), px(0.)));
        window.drag_to(from + point(px(by), px(0.)));
        window.release(from + point(px(by), px(0.)));
    };
    let bar = BAR as f32 * ACROSS;

    window.click(on_bar(2 * BAR, 0.));
    window.until("the repeat on", |window| looped(window) == (true, 0, 4));
    window.click(on_bar(2 * BAR, 0.));
    window.until("the repeat off", |window| !looped(window).0);
    window.click(on_bar(2 * BAR, 0.));
    window.until("the repeat on again", |window| looped(window).0);

    drag(&mut window, on_bar(2 * BAR, 0.), bar);
    window.until("the repeat moved a bar on", |window| {
        looped(window) == (true, 1, 5)
    });
    drag(&mut window, on_bar(5 * BAR, -3.), bar);
    window.until("its end a bar later", |window| {
        looped(window) == (true, 1, 6)
    });
    drag(&mut window, on_bar(BAR, 3.), bar);
    window.until("its start a bar later", |window| {
        looped(window) == (true, 2, 6)
    });

    // A double click on the ruler adds a section at the nearest bar line, its name open.
    window.double_click(ruler + point(px(LEAD + (4 * BAR + 100) as f32 * ACROSS), px(8.)));
    window.until_found("div-ruler-edit");
    window.cx.simulate_keystrokes("enter");
    window.until("a section", |window| {
        window.project_file()["sections"] == serde_json::json!([{ "bar": 5, "name": "Section 1" }])
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Add a section"));
}

#[gpui::test]
fn a_region_dropped_on_the_audio_track_is_chosen_and_deleted(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    let region = window.record("song/audio-voice/region");
    assert_eq!(region["asset"], "sine.wav");
    assert_eq!(region["start"], 2 * BAR);
    // Its waveform shows once its length is known: a click on it chooses it.
    let inside = window.on_lane(2 * BAR + 960, 2);
    window.until_found("button-mixer-toggle");
    window.click(inside);
    // Its card opens in the panel of the audio track.
    window.until_found("card-song/audio-voice/region-header");
    window.cx.simulate_keystrokes("backspace");
    window.until("the region deleted", |window| {
        !window.has("song/audio-voice/region")
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
    window.until_found("button-mixer-toggle");
    window.press(end);
    window.drag_to(end - point(px(5.), px(0.)));
    window.drag_to(end - point(px(960. * ACROSS), px(0.)));
    window.release(end - point(px(960. * ACROSS), px(0.)));
    window.until("the end trimmed", |window| {
        window.record("song/audio-voice/region")["file_end_seconds"]
            .as_f64()
            .is_some()
    });
    let trimmed = window.record("song/audio-voice/region")["file_end_seconds"]
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
    window.editor("t00/c00");
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
    window.editor("t00/c00");
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
    window.editor("t00/c00");
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
    window.until_found("button-mixer-toggle");
    window.press(end);
    window.drag_to(end - point(px(5.), px(0.)));
    window.drag_to(end - point(px(480. * ACROSS), px(0.)));
    window.release(end - point(px(480. * ACROSS), px(0.)));
    window.until("the end trimmed", |window| {
        window.record("song/audio-voice/region")["file_end_seconds"]
            .as_f64()
            .is_some()
    });
    let trimmed = window.record("song/audio-voice/region")["file_end_seconds"]
        .as_f64()
        .unwrap();
    assert!((trimmed - 0.5).abs() < 1e-3, "{trimmed}");
    assert_eq!(window.record("song/audio-voice/region")["start"], 2 * BAR);
}

/// The files of `song_files(tracks, clips)` with `change` made to the record at `path`.
fn files_with(
    tracks: usize,
    clips: usize,
    path: &str,
    change: impl FnOnce(&mut serde_json::Value),
) -> Vec<(String, String)> {
    let mut files = crate::song::song_files(tracks, clips);
    let mut change = Some(change);
    for (each, text) in &mut files {
        if each == path
            && let Some(change) = change.take()
        {
            let mut record: serde_json::Value = serde_json::from_str(text).unwrap();
            change(&mut record);
            *text = record.to_string();
        }
    }
    files
}

fn open_files<'a>(cx: &'a mut TestAppContext, files: &[(String, String)]) -> Opened<'a> {
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    super::open(cx, &files)
}

/// Copy in a clip's menu copies it as cmd-c does: cmd-v pastes it.
#[gpui::test]
fn a_clip_copied_from_its_menu_is_pasted(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let clip = window.on_lane(BAR / 2, 1);
    window.click_with(clip, MouseButton::Right);
    window.pick("menu-2");
    window.wait_for_bun();
    window.click(clip);
    window.cx.simulate_keystrokes("cmd-v");
    window.until("the clip pasted", |window| window.has("song/t01/clip"));
    assert_eq!(window.problems(), []);
}

/// Notes pasted into a clip with no `length`, which plays to its last note, are kept.
#[gpui::test]
fn notes_pasted_into_a_clip_with_no_length_are_kept(cx: &mut TestAppContext) {
    let files = files_with(1, 1, "state/song/t00/c00.json", |record| {
        record["state"].as_object_mut().unwrap().remove("length");
    });
    let mut window = open_files(cx, &files);
    let notes = |window: &mut Opened| {
        window.record("song/t00/c00")["notes"]
            .as_array()
            .map_or(0, Vec::len)
    };
    let count = notes(&mut window);
    window.wait_for_bun();
    let at = window.on_lane(BAR / 4, 0);
    window.double_click(at);
    window.editor("t00/c00");
    window.cx.simulate_keystrokes("cmd-a cmd-c cmd-v");
    window.until("the notes pasted", |window| notes(window) == 2 * count);
}

/// The timeline reaches past its last clip, wherever that is: a clip at bar 88 scrolled to the
/// left edge is chosen and deleted there.
#[gpui::test]
fn the_timeline_scrolls_to_its_last_clip(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 1, 30);
    let last = window.record("song/t00/c29")["start"].as_u64().unwrap();
    assert!(last > 64 * BAR);
    let timeline = window.space_at("timeline");
    window.wait_for_bun();
    window.cx.simulate_event(gpui::ScrollWheelEvent {
        position: timeline + point(px(200.), px(10.)),
        delta: gpui::ScrollDelta::Pixels(point(px(-(last as f32) * ACROSS), px(0.))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    window.cx.run_until_parked();
    let at = window.on_lane(BAR / 2, 0);
    window.click(at);
    window.wait_for_bun();
    window.cx.simulate_keystrokes("backspace");
    window.until("the last clip deleted", |window| {
        !window.has("song/t00/c29")
    });
    assert!(window.has("song/t00/c28"));
}

/// The end of a region that plays its whole file drags no further out: its file has no more.
#[gpui::test]
fn a_region_that_plays_its_whole_file_trims_no_further_out(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    let end = window.on_lane(2 * BAR + 1920, 2) - point(px(2.), px(0.));
    window.until_found("button-mixer-toggle");
    window.press(end);
    window.drag_to(end + point(px(5.), px(0.)));
    window.drag_to(end + point(px(960. * ACROSS), px(0.)));
    window.release(end + point(px(960. * ACROSS), px(0.)));
    window.after_the_runs();
    window.wait_for_bun();
    assert_eq!(
        window.record("song/audio-voice/region")["file_end_seconds"],
        serde_json::Value::Null
    );
    assert_eq!(window.problems(), []);
}

/// A chosen note stays chosen by what it is when another tool puts a note before it in the
/// list: delete removes it, not the note now in its place.
#[gpui::test]
fn a_chosen_note_stays_chosen_when_the_notes_move_in_the_list(cx: &mut TestAppContext) {
    let note = |start: u64, pitch: u64| serde_json::json!({ "start": start, "length": 960, "pitch": pitch, "velocity": 100 });
    let files = files_with(1, 1, "state/song/t00/c00.json", |record| {
        record["state"] = serde_json::json!({ "start": 0, "length": BAR, "notes": [note(0, 60)] });
    });
    let mut window = open_files(cx, &files);
    window.wait_for_bun();
    let at = window.on_lane(BAR / 2, 0);
    window.double_click(at);
    // The note of pitch 60.
    let editor = window.editor("t00/c00");
    window.click(editor.at(30., 60));
    window.wait_for_bun();
    let session = window.session.clone();
    let notes =
        serde_json::json!({ "start": 0, "length": BAR, "notes": [note(1920, 62), note(0, 60)] });
    window.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut edit = project.begin("Another tool");
                let mut changes = sound_core::Changes::new();
                project.set_json(
                    &mut changes,
                    crate::support::id("song/t00/c00"),
                    "timeline",
                    notes,
                )?;
                project.publish(&mut edit, changes)?;
                project.finish(edit)
            })
        })
    });
    window.wait_for_bun();
    window.cx.simulate_keystrokes("backspace");
    window.until("the note deleted", |window| {
        window.record("song/t00/c00")["notes"]
            .as_array()
            .map_or(0, Vec::len)
            == 1
    });
    assert_eq!(window.record("song/t00/c00")["notes"][0]["pitch"], 62);
}

/// In 3/4, a double-click in the second bar adds a clip from its start, a bar of 3/4 long.
#[gpui::test]
fn a_clip_added_in_three_four_fills_its_bar(cx: &mut TestAppContext) {
    let files = files_with(1, 0, "project.json", |project| {
        project["tempo_map"]["time_signature"] = "3/4".into();
    });
    let mut window = open_files(cx, &files);
    window.wait_for_bun();
    let at = window.on_lane(3 * 960 + 480, 0);
    window.double_click(at);
    window.until("the clip added", |window| window.has("song/t00/clip"));
    let clip = window.record("song/t00/clip");
    assert_eq!(
        (clip["start"].as_u64(), clip["length"].as_u64()),
        (Some(2880), Some(2880))
    );
}

/// A pitch the editor shows of `clip` with no note at `tick`, near the middle of its notes.
pub(super) fn free_pitch(clip: &serde_json::Value, tick: f32) -> u8 {
    let notes = clip["notes"].as_array().cloned().unwrap_or_default();
    let field = |note: &serde_json::Value, name: &str| note[name].as_f64().unwrap_or(0.) as f32;
    let taken = |pitch: f32| {
        notes.iter().any(|note| {
            field(note, "pitch") == pitch
                && field(note, "start") <= tick
                && tick < field(note, "start") + field(note, "length")
        })
    };
    let pitches: Vec<f32> = notes.iter().map(|note| field(note, "pitch")).collect();
    let middle = pitches.iter().copied().fold(0., f32::max).min(127.) / 2.
        + pitches.iter().copied().fold(127., f32::min) / 2.;
    (0..20)
        .flat_map(|step| [middle + step as f32, middle - step as f32])
        .map(f32::round)
        .find(|pitch| !taken(*pitch))
        .unwrap_or(middle) as u8
}

/// A menu a div opens hangs under it, as a select's list: the snap's list starts under the
/// snap, from its left edge.
#[gpui::test]
fn a_menu_opens_under_the_div_that_opened_it(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.until_found("div-song-snap");
    let snap = window.bounds("div-song-snap").unwrap();
    // The pointer is in the middle of it; the menu is not.
    window.click(snap.center());
    window.until_found("menu-0");
    let first = window.bounds("menu-0").unwrap();
    assert!(first.top() > snap.bottom(), "{first:?} under {snap:?}");
    assert!(
        (first.left() - snap.left()).abs() < px(24.),
        "{first:?} from the left of {snap:?}"
    );
}

/// The card of a chosen region turns its fades: a drag of Fade in is one undo step of the
/// region's record.
#[gpui::test]
fn the_card_of_a_chosen_region_turns_its_fade_in(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    let inside = window.on_lane(2 * BAR + 960, 2);
    window.until_found("button-mixer-toggle");
    window.click(inside);
    let knob = window.control("knob-fade-in");
    window.drag(knob, knob - point(px(0.), px(60.)));
    window.until("the fade in turned", |window| {
        window.record("song/audio-voice/region")["fade_in_ms"]
            .as_f64()
            .is_some_and(|ms| ms > 0.)
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Change fade in"));
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_clip_added_after_the_song_opened_is_drawn_once_after_its_track_lays_out_again(
    cx: &mut TestAppContext,
) {
    let mut window = open_song(cx, 1, 1);
    window.wait_for_bun();
    // A clip the song did not have when it opened: its shapes come after the first draw.
    let empty = window.on_lane(6 * BAR, 0);
    window.double_click(empty);
    window.until("the clip added", |window| window.clips().len() == 2);
    window.after_the_runs();
    window.wait_for_bun();
    let shell = window.shell.clone();
    let nodes = |window: &mut Opened<'_>| {
        window.cx.read(|cx| {
            let page = shell.read(cx).main_view().unwrap();
            sound_typescript::node_count(page, cx)
        })
    };
    let before = nodes(&mut window);
    // Showing the track's lanes and hiding them again lays its row out anew, twice.
    let lanes = window.until_found("toggle-lanes-t00");
    window.click(lanes);
    window.wait_for_bun();
    window.click(lanes);
    window.wait_for_bun();
    assert_eq!(nodes(&mut window), before);
}

#[gpui::test]
fn the_mixer_and_a_track_panel_take_turns_under_the_timeline(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let toggle = window.until_found("button-mixer-toggle");
    window.click(toggle);
    window.until_found("div-strip-name-t01");
    // A track's head shows its panel in the mixer's place.
    let name = window.until_found("div-name-t00");
    window.click(name);
    window.until_found("button-close-panel");
    window.wait_for_bun();
    assert!(window.find("div-strip-name-t01").is_none());
    // The button shows the mixer again, and a second press shows nothing there.
    let toggle = window.until_found("button-mixer-toggle");
    window.click(toggle);
    window.until_found("div-strip-name-t01");
    let toggle = window.until_found("button-mixer-toggle");
    window.click(toggle);
    window.wait_for_bun();
    assert!(window.find("div-strip-name-t01").is_none());
    assert!(window.find("button-close-panel").is_none());
}
