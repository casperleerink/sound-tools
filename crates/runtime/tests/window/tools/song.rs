//! The song of `tooling/agent-eval/examples`: tracks of clips written in TypeScript, as an
//! agent writes them. A clip moves as one file, a clip's notes are edited below it, a track
//! mutes, and its meter follows the engine with nothing from Bun.

use super::{Window, open};
use crate::song::{ACROSS, BAR, HEADER, ROW, song_files};
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, TestAppContext, TouchPhase, point, px,
};

pub(super) fn open_song(
    cx: &mut TestAppContext,
    tracks: usize,
    clips: usize,
) -> Option<Window<'_>> {
    let files = song_files(tracks, clips);
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    open(cx, &files)
}

impl Window<'_> {
    /// Where `tick` is on the row of track `row` of the timeline, as it opens.
    pub(super) fn on_lane(&mut self, tick: u64, row: usize) -> Point<Pixels> {
        let timeline = self.space_at("timeline");
        timeline + point(px(tick as f32 * ACROSS), px(row as f32 * ROW + ROW / 2.))
    }

    pub(super) fn double_click(&mut self, position: Point<Pixels>) {
        for click_count in [1, 2] {
            self.cx
                .simulate_mouse_move(position, None, Modifiers::default());
            let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
            self.cx.simulate_event(MouseDownEvent {
                position,
                modifiers,
                button,
                click_count,
                first_mouse: false,
            });
            self.cx.simulate_event(MouseUpEvent {
                position,
                modifiers,
                button,
                click_count,
            });
            self.cx.run_until_parked();
        }
    }

    /// Every record of the song's clips, by id.
    fn clips(&mut self) -> Vec<(String, String)> {
        let session = self.session.clone();
        self.cx.read(|cx| {
            let project = session.read(cx).project();
            (project.instances())
                .filter(|(_, tool)| *tool == "timeline")
                .map(|(id, _)| (id.to_string(), project.state_json(id).unwrap_or_default()))
                .collect()
        })
    }
}

#[gpui::test]
fn a_clip_moves_on_the_grid_as_one_file_and_one_undo_step(cx: &mut TestAppContext) {
    let Some(mut window) = open_song(cx, 4, 12) else {
        return;
    };
    let before = window.clips();
    // The second clip of the second track, half a bar into it.
    let at = window.on_lane(3 * BAR + BAR / 2, 1);
    window.wait_for_bun();
    let questions = sound_typescript::asked_behaviours();
    window.press(at);
    window.drag_to(at + point(px(10.), px(0.)));
    // A bar and a bit on: it lands on the nearest beat line.
    window.drag_to(at + point(px(96. + 30.), px(0.)));
    window.release(at + point(px(96. + 30.), px(0.)));
    window.until("the clip moved", |window| {
        window.record("song/t01/c01")["start"] != 3 * BAR
    });
    assert_eq!(window.record("song/t01/c01")["start"], 3 * BAR + BAR + 960);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Move clips"));
    let after = window.clips();
    let changed: Vec<&String> = (before.iter().zip(&after))
        .filter(|(old, new)| old != new)
        .map(|(old, _)| &old.0)
        .collect();
    assert_eq!(changed, ["song/t01/c01"]);
    // The behaviour of the track reads the names and tools of its clips, not where they start.
    assert_eq!(sound_typescript::asked_behaviours(), questions);
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_double_click_opens_a_clip_below_and_a_double_click_there_adds_a_note(cx: &mut TestAppContext) {
    let Some(mut window) = open_song(cx, 4, 12) else {
        return;
    };
    let notes = |window: &mut Window| {
        window.record("song/t00/c00")["notes"]
            .as_array()
            .map_or(0, Vec::len)
    };
    let count = notes(&mut window);
    let clip = window.on_lane(BAR / 2, 0);
    window.wait_for_bun();
    window.double_click(clip);
    let editor = window.space_at("editor");
    // Past a sixteenth into the clip, high up, where no note is: on the sixteenth.
    window.double_click(editor + point(px(300. * ACROSS * 4.), px(4.)));
    window.until("the note added", |window| notes(window) == count + 1);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Add a note"));
    let added = window.record("song/t00/c00")["notes"][count].clone();
    assert_eq!(added["start"], 240);
    assert!(window.undo().is_some());
    assert_eq!(notes(&mut window), count);
}

#[gpui::test]
fn the_mute_of_a_track_mutes_its_mix_as_one_undo_step(cx: &mut TestAppContext) {
    let Some(mut window) = open_song(cx, 4, 12) else {
        return;
    };
    let lane = window.on_lane(0, 2);
    // The M of the third track: after its name, in the header left of the timeline.
    let mute = point(lane.x - px(HEADER) + px(80.), lane.y);
    window.wait_for_bun();
    window.press(mute);
    window.release(mute);
    window.until("the track muted", |window| {
        window.record("song/t02/mix")["mute"] == true
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Mute a track"));
    assert_eq!(window.record("song/t01/mix")["mute"], false);
}

#[gpui::test]
fn the_heads_of_the_tracks_scroll_with_the_timeline(cx: &mut TestAppContext) {
    // More tracks than the window is tall.
    let Some(mut window) = open_song(cx, 40, 40) else {
        return;
    };
    let timeline = window.space_at("timeline");
    window.wait_for_bun();
    window.cx.simulate_event(ScrollWheelEvent {
        position: timeline + point(px(200.), px(100.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-10. * ROW))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    window.cx.run_until_parked();
    // The M on the top row is the eleventh track's now.
    let mute = timeline + point(px(80.) - px(HEADER), px(ROW / 2.));
    window.press(mute);
    window.release(mute);
    window.until("the track muted", |window| {
        window.record("song/t10/mix")["mute"] == true
    });
    assert_eq!(window.record("song/t00/mix")["mute"], false);
}

#[gpui::test]
fn delete_removes_the_chosen_clip_and_space_still_plays(cx: &mut TestAppContext) {
    let Some(mut window) = open_song(cx, 4, 12) else {
        return;
    };
    let clip = window.on_lane(3 * BAR + BAR / 2, 1);
    window.wait_for_bun();
    window.press(clip);
    window.release(clip);
    window.wait_for_bun();
    window.cx.simulate_keystrokes("cmd-d");
    window.until("the clip copied on", |window| {
        window.clips().iter().any(|(id, _)| id == "song/t01/c01-2")
    });
    window.press(clip);
    window.release(clip);
    window.wait_for_bun();
    window.cx.simulate_keystrokes("delete");
    window.until("the clip removed", |window| {
        window.clips().iter().all(|(id, _)| id != "song/t01/c01")
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Remove clips"));
    // The page names its keys: space is the window's, and plays.
    window.cx.simulate_keystrokes("space");
    window.poll();
    assert!(window.playing());
}

#[gpui::test]
fn space_plays_the_song_and_the_meters_follow_with_nothing_from_bun(cx: &mut TestAppContext) {
    let Some(mut window) = open_song(cx, 4, 12) else {
        return;
    };
    window.space_at("meter-t00");
    window.wait_for_bun();
    window.focus_the_window();
    window.cx.simulate_keystrokes("space");
    window.poll();
    assert!(window.playing());
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    let batches = sound_typescript::heard_batches();
    window.until("the meter of the first track to show its synth", |window| {
        window.loudest(2_400);
        window.find("space-meter-t00");
        sound_typescript::shown_by_live_values("peak:t00/level").is_some_and(|peak| peak > 0.01)
    });
    assert_eq!(sound_typescript::sent_updates_and_events(), asked);
    assert_eq!(sound_typescript::heard_batches(), batches);
}
