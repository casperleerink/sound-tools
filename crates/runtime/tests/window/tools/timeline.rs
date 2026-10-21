//! The timeline of the DAW template as the arrangement had it: the snap in the corner is the
//! grid of the clips, a clip's start drags with its notes left where they are, a duplicate goes
//! after the chosen clips and is chosen, and a paste keeps the rows the clips came from.

use gpui::{Modifiers, MouseButton, MouseUpEvent, TestAppContext, point, px};

use super::Opened;
use super::song::open_song;
use sound_core::Ticks;

use crate::song::{ACROSS, BAR, LEAD};

impl Opened<'_> {
    fn starts(&mut self, track: &str) -> Vec<u64> {
        let mut starts: Vec<u64> = (self.clips().into_iter())
            .filter(|(id, _)| id.starts_with(&format!("song/{track}/")))
            .filter_map(|(_, state)| serde_json::from_str::<serde_json::Value>(&state).ok())
            .filter_map(|state| state["start"].as_u64())
            .collect();
        starts.sort_unstable();
        starts
    }

    /// A new snap makes the clips' shapes again, with drags on its grid: they are there.
    fn snap_applied(&mut self) {
        for _ in 0..4 {
            self.wait_for_bun();
            self.settle();
        }
    }
}

#[gpui::test]
fn with_snap_off_a_clip_moves_to_the_tick_and_with_bars_to_the_bar(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let snap = window.until_found("div-song-snap");
    window.click(snap);
    // Off, Bar, Beat, 1/8, 1/16, 1/32, as the arrangement's menu.
    window.pick("menu-0");
    window.snap_applied();
    let at = window.on_lane(BAR / 2, 1);
    window.drag(at, at + point(px(37.), px(0.)));
    window.until("the clip moved", |window| {
        window.record("song/t01/c00")["start"] != 0
    });
    assert_eq!(
        window.record("song/t01/c00")["start"],
        (37. / ACROSS) as u64
    );

    window.click(snap);
    window.pick("menu-1");
    window.snap_applied();
    let at = window.on_lane(BAR, 1);
    window.drag(at, at + point(px(30.), px(0.)));
    window.until("the clip moved to a bar", |window| {
        window.record("song/t01/c00")["start"] == BAR
    });
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_clip_dragged_with_cmd_held_lands_off_the_grid(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let cmd = Modifiers {
        platform: true,
        ..Modifiers::default()
    };
    let at = window.on_lane(BAR / 2, 1);
    window.press(at);
    for right in [20., 37.] {
        let to = at + point(px(right), px(0.));
        window.cx.simulate_mouse_move(to, MouseButton::Left, cmd);
        window.cx.run_until_parked();
    }
    window.release(at + point(px(37.), px(0.)));
    window.until("the clip moved", |window| {
        window.record("song/t01/c00")["start"] != 0
    });
    // On the sixteenths it would be 1440.
    assert_eq!(window.record("song/t01/c00")["start"], 1480);
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_clips_start_dragged_left_keeps_its_notes_where_they_are(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 4, 12);
    window.wait_for_bun();
    let before = window.record("song/t01/c01");
    let start = before["start"].as_u64().unwrap_or_default();
    let length = before["length"].as_u64().unwrap_or_default();
    let body = window.on_lane(start + BAR / 2, 1);
    window.click(body);
    // The left edge, inside the clip.
    let edge = window.on_lane(start, 1) + point(px(2.), px(0.));
    window.drag(edge, edge - point(px(96.), px(0.)));
    window.until("the start moved", |window| {
        window.record("song/t01/c01")["start"] == start - BAR
    });
    let after = window.record("song/t01/c01");
    assert_eq!(after["length"], length + BAR);
    let first = |clip: &serde_json::Value| clip["notes"][0]["start"].as_u64();
    assert_eq!(first(&after), first(&before).map(|tick| tick + BAR));
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Resize clips"));
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_duplicate_goes_after_the_chosen_clips_and_is_chosen(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let length = window.record("song/t01/c00")["length"]
        .as_u64()
        .unwrap_or_default();
    let at = window.on_lane(BAR / 2, 1);
    window.click(at);
    window.cx.simulate_keystrokes("cmd-d");
    window.until("one copy", |window| window.starts("t01").len() == 2);
    window.wait_for_bun();
    // The copy is chosen: the next duplicate goes after it, not over it.
    window.cx.simulate_keystrokes("cmd-d");
    window.until("two copies", |window| window.starts("t01").len() == 3);
    assert_eq!(window.starts("t01"), [0, length, 2 * length]);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Duplicate clips"));
}

#[gpui::test]
fn clips_of_two_tracks_paste_on_the_chosen_track_and_the_one_under_it(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 3, 3);
    window.wait_for_bun();
    let at = window.on_lane(BAR / 2, 0);
    window.click(at);
    let cmd = Modifiers {
        platform: true,
        ..Modifiers::default()
    };
    let second = window.on_lane(BAR / 2, 1);
    window.press_with(second, 1, cmd);
    window.release(second);
    window.wait_for_bun();
    window.cx.simulate_keystrokes("cmd-c");
    // The second track chosen: the first clip lands there, the second on the third track.
    let at = window.on_lane(BAR / 2, 1);
    window.click(at);
    window.wait_for_bun();
    window.cx.simulate_keystrokes("cmd-v");
    window.until("the clips pasted", |window| {
        window.starts("t01").len() == 2 && window.starts("t02").len() == 2
    });
    assert_eq!(window.starts("t00").len(), 1);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Paste clips"));
}

#[gpui::test]
fn a_section_clicked_in_the_ruler_is_chosen_played_from_and_deleted(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let ruler = window.space_at("ruler");
    let at = |tick: u64, right: f32| ruler + point(px(LEAD + tick as f32 * ACROSS + right), px(8.));
    window.double_click(at(2 * BAR + 100, 0.));
    window.until_found("div-ruler-edit");
    // Enter keeps the name it has and closes the field.
    window.cx.simulate_keystrokes("enter");
    window.until("the field closed", |window| {
        window.find("div-ruler-edit").is_none()
    });
    window.until("a section", |window| {
        window.project_file()["sections"] == serde_json::json!([{ "bar": 3, "name": "Section 1" }])
    });
    window.wait_for_bun();
    // Its name, after the number of its bar.
    window.click(at(2 * BAR, 30.));
    let session = window.session.clone();
    window.until("the playhead on its bar", |window| {
        window
            .cx
            .read(|cx| session.read(cx).playhead().read(cx).tick)
            == Ticks(2 * BAR)
    });
    window.wait_for_bun();
    window.cx.simulate_keystrokes("backspace");
    window.until("the section removed", |window| {
        window.project_file()["sections"] == serde_json::json!([])
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Remove a section"));
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_time_signature_added_in_the_ruler_is_changed_by_a_drag_in_the_transport(
    cx: &mut TestAppContext,
) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let ruler = window.space_at("ruler");
    window.click_with(
        ruler + point(px(LEAD + (2 * BAR) as f32 * ACROSS), px(8.)),
        MouseButton::Right,
    );
    window.pick("menu-1");
    let signatures =
        |window: &mut Opened<'_>| window.project_file()["tempo_map"]["time_signatures"].clone();
    // Bar 3 starts a change to what plays there, and the playhead is on it.
    window.until("a change at bar 3", |window| {
        signatures(window)
            == serde_json::json!([{ "signature": "4/4", "bars": 2 }, { "signature": "4/4", "bars": 1 }])
    });
    let session = window.session.clone();
    window.until("the playhead on bar 3", |window| {
        window
            .cx
            .read(|cx| session.read(cx).playhead().read(cx).tick)
            == Ticks(2 * BAR)
    });
    window.wait_for_bun();
    // Twenty points up: two beats more, from bar 3 on.
    let beats = window.space_at("transport-beats") + point(px(7.), px(9.));
    window.press(beats);
    window.drag_to(beats - point(px(0.), px(10.)));
    window.drag_to(beats - point(px(0.), px(20.)));
    window.release(beats - point(px(0.), px(20.)));
    window.until("6/4 from bar 3", |window| {
        signatures(window)
            == serde_json::json!([{ "signature": "4/4", "bars": 2 }, { "signature": "6/4", "bars": 1 }])
    });
    window.after_the_runs();
    assert_eq!(
        window.undo_label().as_deref(),
        Some("Change the time signature")
    );
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_region_dragged_with_alt_is_copied_on_the_grid(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    let inside = window.on_lane(2 * BAR + 960, 2);
    window.until_found("button-mixer-toggle");
    window.click(inside);
    window.wait_for_bun();
    let alt = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    let to = inside + point(px(96. + 4.), px(0.));
    window.press_with(inside, 1, alt);
    window
        .cx
        .simulate_mouse_move(inside + point(px(20.), px(0.)), MouseButton::Left, alt);
    window.cx.simulate_mouse_move(to, MouseButton::Left, alt);
    window.cx.simulate_event(MouseUpEvent {
        position: to,
        modifiers: alt,
        button: MouseButton::Left,
        click_count: 1,
    });
    window.until("a copy", |window| window.has("song/audio-voice/region-2"));
    // A bar and a bit on, on the sixteenth.
    assert_eq!(
        window.record("song/audio-voice/region-2")["start"],
        3 * BAR + 240
    );
    assert_eq!(window.record("song/audio-voice/region")["start"], 2 * BAR);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Copy clips"));
}

/// Clips and regions are one selection, as the arrangement's clips of either kind were: a box
/// over both chooses both, and copy, paste, the arrows and delete take them together.
#[gpui::test]
fn clips_and_regions_are_chosen_copied_pasted_nudged_and_deleted_together(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    window.drop_sine();
    window.until_found("button-mixer-toggle");
    // From empty space past the region, up and left over the second track's clip.
    let (from, to) = (window.on_lane(5 * BAR, 2), window.on_lane(BAR / 2, 1));
    window.press(from);
    window.drag_to(from - point(px(20.), px(0.)));
    window.drag_to(to);
    window.release(to);
    window.wait_for_bun();
    window.cx.simulate_keystrokes("cmd-c");
    // At the playhead, the start: each on the row it came from.
    window.cx.simulate_keystrokes("cmd-v");
    window.until("both pasted", |window| {
        window.has("song/t01/c00-2") && window.has("song/audio-voice/region-2")
    });
    assert_eq!(window.record("song/t01/c00-2")["start"], 0);
    assert_eq!(window.record("song/audio-voice/region-2")["start"], 2 * BAR);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Paste clips"));

    window.cx.simulate_keystrokes("right");
    window.until("both nudged", |window| {
        window.record("song/t01/c00-2")["start"] == 240
            && window.record("song/audio-voice/region-2")["start"] == 2 * BAR + 240
    });
    window.after_the_runs();
    window.cx.simulate_keystrokes("backspace");
    window.until("both deleted", |window| {
        !window.has("song/t01/c00-2") && !window.has("song/audio-voice/region-2")
    });
    assert!(window.has("song/t01/c00") && window.has("song/audio-voice/region"));
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Remove clips"));
    assert_eq!(window.problems(), []);
}

/// A cut takes the automation under a clip along, and a paste puts it down where the clip lands,
/// as the arrangement's did: the cutoff that opens the first clip opens it at bar 9.
#[gpui::test]
fn a_cut_clip_takes_its_automation_and_a_paste_puts_it_down(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let length = window.record("song/t00/c00")["length"].as_u64().unwrap();
    let points = |window: &mut Opened<'_>| window.record("song/t00/lane-cutoff")["points"].clone();
    let at_tick = |points: &serde_json::Value, tick: u64| {
        (points.as_array().into_iter().flatten())
            .find(|point| point["tick"].as_u64() == Some(tick))
            .map(|point| point["value"].as_f64().unwrap_or_default().round())
    };
    let clip = window.on_lane(BAR / 2, 0);
    window.click(clip);
    window.wait_for_bun();
    window.cx.simulate_keystrokes("cmd-x");
    window.until("the clip cut", |window| !window.has("song/t00/c00"));
    let cut = points(&mut window);
    // Where it was, the line is straight from its end on.
    assert_eq!(at_tick(&cut, 0), None, "{cut}");
    assert!(at_tick(&cut, length).is_some(), "{cut}");
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Cut clips"));

    let ruler = window.space_at("ruler");
    window.click(ruler + point(px(LEAD + (8 * BAR) as f32 * ACROSS), px(8.)));
    window.wait_for_bun();
    window.cx.simulate_keystrokes("cmd-v");
    window.until("the clip pasted", |window| window.has("song/t00/c00"));
    assert_eq!(window.record("song/t00/c00")["start"], 8 * BAR);
    let pasted = points(&mut window);
    assert_eq!(at_tick(&pasted, 8 * BAR), Some(6000.), "{pasted}");
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Paste clips"));
    assert_eq!(window.problems(), []);
}
