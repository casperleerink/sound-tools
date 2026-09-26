//! Editing the arrangement in the window, step 9a of the third milestone: several clips at once,
//! copy and paste, renaming a track, tempo changes in the ruler and the snap setting. Every
//! action is driven with the simulated mouse and keys and checked for its records, for exactly
//! one undo step, and for undo giving the files back byte for byte. For each, the same thing
//! written as a file from outside shows in the window at once.

use arrangement::TrackState;
use arrangement::view::snap::Snap;
use gpui::{Modifiers, TestAppContext, point, px};
use sound_core::{Changes, InstanceId, Ticks};

use crate::support::{self, BAR, Opened, STEP, clip, id, mark, note, one_undo_step, write_outside};

const PART: &str = "arrangement/track-1/part";
const HOOK: &str = "arrangement/track-2/hook";

/// Two tracks. `part` on the first, bars 2 and 3, and `hook` on the second, bar 5. No undo
/// history.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        let mut changes = Changes::new();
        changes.create(id(PART), part());
        changes.create(id(HOOK), hook());
        project.commit("Add clips", changes).unwrap();
        project.clear_history();
    })
}

fn part() -> sound_notes::Clip {
    clip(BAR, 2 * BAR, vec![note(960, 480, 60)])
}

fn hook() -> sound_notes::Clip {
    clip(4 * BAR, BAR, vec![note(0, 240, 72)])
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

fn cmd() -> Modifiers {
    Modifiers {
        platform: true,
        ..Modifiers::default()
    }
}

fn ids(names: &[&str]) -> Vec<InstanceId> {
    let mut ids: Vec<_> = names.iter().map(|name| id(name)).collect();
    ids.sort();
    ids
}

fn track_name(opened: &mut Opened<'_>, track: &str) -> String {
    let track = id(track);
    opened.project(|project| {
        let track = project.resolve::<TrackState>(&track).unwrap();
        project.state(&track).unwrap().name.clone()
    })
}

/// What the name field of the timeline holds, while it is open.
fn name_field(opened: &mut Opened<'_>) -> Option<String> {
    let timeline = opened.timeline.clone();
    opened.cx.read(|cx| {
        let (_, input) = timeline.read(cx).name_field()?;
        Some(input.read(cx).text().to_string())
    })
}

/// One frame of the window.
fn frame(opened: &mut Opened<'_>) {
    opened.cx.update(|window, _| window.refresh());
    opened.cx.run_until_parked();
}

fn selected_tempo(opened: &mut Opened<'_>) -> Option<Ticks> {
    let timeline = opened.timeline.clone();
    opened.cx.read(|cx| timeline.read(cx).selected_tempo())
}

fn snap(opened: &mut Opened<'_>) -> Snap {
    let timeline = opened.timeline.clone();
    opened.cx.read(|cx| timeline.read(cx).snap())
}

/// Picks a snap setting in the corner above the track headers.
fn pick_snap(opened: &mut Opened<'_>, label: &str) {
    let menu = opened.control("snap");
    opened.click(menu);
    let row = opened.control(&format!("menu-{label}"));
    opened.click(row);
}

#[gpui::test]
fn shift_and_cmd_click_add_clips_to_the_selection_and_take_them_out(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (on_part, on_hook) = (opened.at(BAR + 960, 0), opened.at(4 * BAR + 960, 1));
    opened.click(on_part);
    opened.click_with(on_hook, shift());
    assert_eq!(opened.selected_clips(), ids(&[PART, HOOK]));
    // The last one added comes first: it is what the note editor would show.
    assert_eq!(opened.selected_clip(), Some(id(HOOK)));
    opened.click_with(on_hook, cmd());
    assert_eq!(opened.selected_clips(), ids(&[PART]));
    opened.click_with(on_hook, cmd());
    assert_eq!(opened.selected_clips(), ids(&[PART, HOOK]));
    // A plain click on one of them, without a move, selects it alone.
    opened.click(on_part);
    assert_eq!(opened.selected_clips(), ids(&[PART]));
    // Selecting is no edit.
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn a_drag_of_several_clips_moves_them_all_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let (on_part, on_hook) = (opened.at(BAR + 960, 0), opened.at(4 * BAR + 960, 1));
    opened.click(on_part);
    opened.click_with(on_hook, shift());
    // One bar later, and a row down: the hook is on the last track already, so neither goes
    // down, and both keep their distance.
    let to = opened.at(2 * BAR + 960, 1);
    opened.drag(on_part, to);
    assert_eq!(opened.clip(PART).unwrap().start, Ticks(2 * BAR));
    assert_eq!(opened.clip(HOOK).unwrap().start, Ticks(5 * BAR));
    assert_eq!(opened.clip(PART).unwrap().notes, part().notes);
    assert_eq!(opened.selected_clips(), ids(&[PART, HOOK]));
    one_undo_step(&mut opened, "Move clips", &before);
}

#[gpui::test]
fn a_drag_of_several_clips_to_other_tracks_moves_their_files_together(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        let mut changes = Changes::new();
        changes.create(id(PART), part());
        changes.create(id(HOOK), hook());
        project.commit("Add clips", changes).unwrap();
        project.clear_history();
    });
    let before = mark(&mut opened);
    let (on_part, on_hook) = (opened.at(BAR + 960, 0), opened.at(4 * BAR + 960, 1));
    opened.click(on_part);
    opened.click_with(on_hook, cmd());
    let down = opened.at(BAR + 960, 1);
    opened.drag(on_part, down);
    let (moved_part, moved_hook) = ("arrangement/track-2/part", "arrangement/track-3/hook");
    assert_eq!(opened.clip(moved_part), Some(part()));
    assert_eq!(opened.clip(moved_hook), Some(hook()));
    assert_eq!((opened.clip(PART), opened.clip(HOOK)), (None, None));
    assert_eq!(opened.selected_clips(), ids(&[moved_part, moved_hook]));
    assert!(opened.clip_file(moved_hook).is_some());
    assert_eq!(opened.clip_file(HOOK), None);
    one_undo_step(&mut opened, "Move clips", &before);
    // Undo put the selection back on the clips at their old ids.
    opened.keys("cmd-z");
    assert_eq!(opened.selected_clips(), ids(&[PART, HOOK]));
}

#[gpui::test]
fn a_rectangle_on_empty_space_selects_the_clips_it_touches(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // From empty space in bar 1 of the first track to bar 4 of the second: it touches the part
    // and not the hook, which starts in bar 5.
    let (from, to) = (opened.at(BAR / 2, 0), opened.at(3 * BAR + 960, 1));
    opened.drag(from, to);
    assert_eq!(opened.selected_clips(), ids(&[PART]));
    assert_eq!(opened.undo_label(), None, "selecting is no edit");
    // With shift it adds to what was selected.
    let (from, to) = (opened.at(6 * BAR, 1), opened.at(4 * BAR + 100, 1));
    opened.drag_with(from, to, shift());
    assert_eq!(opened.selected_clips(), ids(&[PART, HOOK]));
    // A click on empty space selects nothing.
    let empty = opened.at(8 * BAR, 0);
    opened.click(empty);
    assert!(opened.selected_clips().is_empty());
    // Cmd-a selects every clip.
    opened.keys("cmd-a");
    assert_eq!(opened.selected_clips(), ids(&[PART, HOOK]));
}

#[gpui::test]
fn delete_and_the_arrows_act_on_every_selected_clip_as_one_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let on_part = opened.at(BAR + 960, 0);
    opened.click(on_part);
    opened.keys("cmd-a");

    let before = mark(&mut opened);
    opened.keys("right");
    assert_eq!(opened.clip(PART).unwrap().start, Ticks(BAR + STEP));
    assert_eq!(opened.clip(HOOK).unwrap().start, Ticks(4 * BAR + STEP));
    one_undo_step(&mut opened, "Nudge clips", &before);

    // Up: the part is on the first track already, so nothing moves and there is no step.
    let before = mark(&mut opened);
    opened.keys("up");
    assert_eq!(support::files(opened.folder.path()), before.files);
    assert_eq!(opened.undo_label(), before.undo_label);

    let before = mark(&mut opened);
    opened.keys("delete");
    assert_eq!((opened.clip(PART), opened.clip(HOOK)), (None, None));
    assert!(opened.selected_clips().is_empty());
    one_undo_step(&mut opened, "Delete clips", &before);
}

#[gpui::test]
fn copy_and_paste_puts_the_clips_at_the_playhead_on_the_selected_track(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let on_part = opened.at(BAR + 960, 0);
    opened.click(on_part);
    opened.keys("cmd-a cmd-c");
    // The playhead to bar 9, and the first track selected.
    let bar_nine = opened.ruler(8 * BAR);
    opened.click(bar_nine);
    opened.settle();
    assert_eq!(opened.playhead().tick, Ticks(8 * BAR));
    let header = opened.track_header(0);
    opened.click(header);
    opened.keys("escape");

    let before = mark(&mut opened);
    opened.keys("cmd-v");
    let (pasted_part, pasted_hook) = ("arrangement/track-1/part-2", "arrangement/track-2/hook-2");
    // The earliest start lands on the playhead, and the hook keeps its three bars and its row.
    let expected_part = clip(8 * BAR, 2 * BAR, part().notes);
    assert_eq!(opened.clip(pasted_part), Some(expected_part));
    assert_eq!(
        opened.clip(pasted_hook),
        Some(clip(11 * BAR, BAR, hook().notes))
    );
    assert_eq!(opened.clip(PART), Some(part()), "the original stays");
    assert_eq!(opened.selected_clips(), ids(&[pasted_part, pasted_hook]));
    one_undo_step(&mut opened, "Paste clips", &before);

    // Onto the last track: the row that would be below it lands on it too.
    let header = opened.track_header(1);
    opened.click(header);
    opened.keys("escape");
    opened.keys("cmd-v");
    assert!(opened.clip("arrangement/track-2/part").is_some());
    assert!(opened.clip("arrangement/track-2/hook-3").is_some());
}

#[gpui::test]
fn cut_takes_the_clips_away_and_paste_brings_them_back(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let on_hook = opened.at(4 * BAR + 960, 1);
    opened.click(on_hook);
    opened.keys("cmd-x");
    assert_eq!(opened.clip(HOOK), None);
    one_undo_step(&mut opened, "Cut clip", &before);

    // The clipboard kept it: pasted at the playhead, on the second track, which was selected
    // last because the hook was on it.
    let header = opened.track_header(1);
    opened.click(header);
    opened.keys("escape cmd-v");
    assert_eq!(opened.clip(HOOK), Some(clip(0, BAR, hook().notes)));
    assert_eq!(opened.undo_label().as_deref(), Some("Paste clip"));
}

#[gpui::test]
fn duplicate_puts_a_copy_right_after_the_selection(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let on_part = opened.at(BAR + 960, 0);
    opened.click(on_part);
    opened.keys("cmd-d");
    let copy = "arrangement/track-1/part-2";
    assert_eq!(
        opened.clip(copy),
        Some(clip(3 * BAR, 2 * BAR, part().notes))
    );
    assert_eq!(opened.selected_clips(), ids(&[copy]));
    one_undo_step(&mut opened, "Duplicate clip", &before);
    // Again from the copy: the next one follows it.
    let on_copy = opened.at(3 * BAR + 960, 0);
    opened.click(on_copy);
    opened.keys("cmd-d");
    assert_eq!(
        opened.clip("arrangement/track-1/part-3").unwrap().start,
        Ticks(5 * BAR)
    );
}

/// A clip that an agent writes is on screen where its file says and cmd-a selects it with the
/// others, and a selected clip that it deletes leaves the selection, so delete does not name it.
#[gpui::test]
fn clips_written_or_deleted_from_outside_join_or_leave_the_selection(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    write_outside(
        &mut opened,
        "state/arrangement/track-2/late.json",
        r#"{"tool": "arrangement.clip", "state": {"start": 30720, "length": 3840, "notes": []}}"#,
    );
    let empty = opened.at(12 * BAR, 0);
    opened.click(empty);
    opened.keys("cmd-a");
    let late = "arrangement/track-2/late";
    assert_eq!(opened.selected_clips(), ids(&[PART, HOOK, late]));
    // It is on screen where the file says: a click hits it.
    let on_late = opened.at(8 * BAR + 960, 1);
    opened.click(on_late);
    assert_eq!(opened.selected_clips(), ids(&[late]));

    opened.keys("cmd-a");
    let path = opened.path("state/arrangement/track-2/hook.json");
    std::fs::remove_file(&path).unwrap();
    opened.edit(|project| project.apply_outside_changes(std::slice::from_ref(&path)));
    assert_eq!(opened.selected_clips(), ids(&[PART, late]));
    opened.keys("delete");
    assert_eq!(opened.undo_label().as_deref(), Some("Delete clips"));
    assert_eq!(opened.clip(late), None);
}

#[gpui::test]
fn a_double_click_on_a_track_name_renames_it_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let header = opened.track_header(0);
    opened.double_click(header);
    assert_eq!(name_field(&mut opened).as_deref(), Some("Track 1"));
    // The name is selected, so typing replaces it. Space is not play while the field is open.
    opened.cx.simulate_input("Keys");
    opened.keys("space");
    opened.settle();
    assert!(!opened.playhead().playing);
    assert_eq!(opened.undo_label(), None, "nothing is written while typing");
    opened.keys("enter");
    assert_eq!(name_field(&mut opened), None);
    assert_eq!(track_name(&mut opened, "arrangement/track-1"), "Keys");
    one_undo_step(&mut opened, "Rename track", &before);
    // The folder keeps its name: the id of a track never changes.
    assert!(
        opened
            .path("state/arrangement/track-1/instance.json")
            .exists()
    );
}

#[gpui::test]
fn escape_cancels_a_rename_and_a_click_elsewhere_finishes_it(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.track_header(1);
    opened.double_click(header);
    opened.cx.simulate_input("Nope");
    opened.keys("escape");
    assert_eq!(name_field(&mut opened), None);
    assert_eq!(track_name(&mut opened, "arrangement/track-2"), "Track 2");
    assert_eq!(opened.undo_label(), None);
    // Escape went to the field, not to the panel below, which stays open.
    assert!(opened.track_panel().is_some());

    // Enter on the selected track opens the field too, and a click elsewhere gives the name.
    opened.keys("enter");
    assert_eq!(name_field(&mut opened).as_deref(), Some("Track 2"));
    opened.cx.simulate_input("Bass");
    // A view hears that it lost the focus from one frame to the next, and only in the window
    // in front: a frame shows the field with the focus first, as the screen does.
    opened.cx.update(|window, _| window.activate_window());
    frame(&mut opened);
    let elsewhere = opened.at(10 * BAR, 0);
    opened.click(elsewhere);
    frame(&mut opened);

    assert_eq!(name_field(&mut opened), None);
    assert_eq!(track_name(&mut opened, "arrangement/track-2"), "Bass");
    assert_eq!(opened.undo_label().as_deref(), Some("Rename track"));

    // An empty name is no name: the track keeps the one it had.
    opened.keys("enter");
    opened.keys("backspace");
    opened.keys("enter");
    assert_eq!(track_name(&mut opened, "arrangement/track-2"), "Bass");
}

#[gpui::test]
fn a_name_written_from_outside_shows_in_the_header_and_its_field(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    write_outside(
        &mut opened,
        "state/arrangement/track-1/instance.json",
        r#"{"tool": "arrangement.track", "state": {"name": "Piano", "order": 0}}"#,
    );
    let header = opened.track_header(0);
    opened.double_click(header);
    assert_eq!(name_field(&mut opened).as_deref(), Some("Piano"));
    opened.keys("escape");
}

#[gpui::test]
fn a_double_click_in_the_ruler_adds_a_tempo_change_and_delete_removes_it(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    // Bar 5 and a bit: the change lands on the grid, with the tempo that played there.
    let bar_five = opened.ruler(4 * BAR + 100);
    opened.double_click(bar_five);
    assert_eq!(opened.tempo_changes(), [(0, 120.0), (4 * BAR, 120.0)]);
    assert_eq!(selected_tempo(&mut opened), Some(Ticks(4 * BAR)));
    one_undo_step(&mut opened, "Add tempo change", &before);

    // The playhead is on it, so the tempo of the transport is its tempo, and a drag there or
    // the arrows change it and nothing else.
    opened.settle();
    let tempo = opened.control("number-tempo");
    opened.click(tempo);
    opened.keys("up");
    assert_eq!(opened.tempo_changes(), [(0, 120.0), (4 * BAR, 121.0)]);

    // Click the mark to select it, then delete takes it out.
    let before = mark(&mut opened);
    let empty = opened.ruler(10 * BAR);
    opened.click(empty);
    assert_eq!(selected_tempo(&mut opened), None);
    let mark = opened.ruler(4 * BAR) + point(px(24.), px(0.));
    opened.click(mark);
    assert_eq!(selected_tempo(&mut opened), Some(Ticks(4 * BAR)));
    opened.settle();
    assert_eq!(opened.playhead().tick, Ticks(4 * BAR));
    assert_eq!(opened.shown_tempo(), 121.0);
    opened.keys("delete");
    assert_eq!(opened.tempo_changes(), [(0, 120.0)]);
    assert_eq!(selected_tempo(&mut opened), None);
    one_undo_step(&mut opened, "Remove tempo change", &before);
}

#[gpui::test]
fn t_adds_a_tempo_change_at_the_playhead(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let bar_three = opened.ruler(2 * BAR);
    opened.click(bar_three);
    opened.settle();
    opened.keys("t");
    assert_eq!(opened.tempo_changes(), [(0, 120.0), (2 * BAR, 120.0)]);
    one_undo_step(&mut opened, "Add tempo change", &before);
    // Again on the same tick adds nothing.
    opened.keys("t");
    assert_eq!(opened.undo_label().as_deref(), Some("Add tempo change"));
    assert_eq!(opened.tempo_changes().len(), 2);
    // The change at tick 0 cannot be taken away: it has no mark.
    let start = opened.ruler(0);
    opened.click(start);
    opened.settle();
    opened.keys("delete");
    assert_eq!(opened.tempo_changes().len(), 2);
}

#[gpui::test]
fn a_tempo_change_written_from_outside_shows_as_a_mark(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    opened.write_tempo_map(
        r#"{"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}, {"tick": 23040, "bpm": 90.0}]}"#,
    );
    let mark = opened.ruler(6 * BAR) + point(px(24.), px(0.));
    opened.click(mark);
    assert_eq!(selected_tempo(&mut opened), Some(Ticks(6 * BAR)));
    opened.settle();
    assert_eq!(opened.shown_tempo(), 90.0);
    // Removed from outside, it is no longer selected, and delete does nothing.
    opened.write_tempo_map(
        r#"{"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]}"#,
    );
    assert_eq!(selected_tempo(&mut opened), None);
}

#[gpui::test]
fn the_snap_setting_is_the_grid_of_every_drag_and_cmd_bypasses_it(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    assert_eq!(snap(&mut opened), Snap::Sixteenth);
    pick_snap(&mut opened, "Bar");
    assert_eq!(snap(&mut opened), Snap::Bar);
    assert_eq!(opened.undo_label(), None, "the snap is not saved");

    // A drag of one bar and a half moves by two bars now, whole bars from where it began.
    let from = opened.at(BAR + 960, 0);
    let to = opened.at(BAR + 960 + BAR + BAR / 2 + 10, 0);
    opened.drag(from, to);
    assert_eq!(opened.clip(PART).unwrap().start, Ticks(3 * BAR));
    // A new clip starts on the bar under the pointer, and the arrows move by a bar.
    let empty = opened.at(8 * BAR + 3000, 1);
    opened.double_click(empty);
    assert_eq!(
        opened.clip("arrangement/track-2/clip").unwrap().start,
        Ticks(8 * BAR)
    );
    opened.keys("left");
    assert_eq!(
        opened.clip("arrangement/track-2/clip").unwrap().start,
        Ticks(7 * BAR)
    );

    // Cmd held during the drag: the pointer's own distance, to the tick.
    let from = opened.at(3 * BAR + 960, 0);
    let to = opened.at(3 * BAR + 960 + 700, 0);
    opened.press(from);
    opened.drag_to_with(to, cmd());
    opened.release(to);
    let moved = opened.clip(PART).unwrap().start.0;
    assert!(
        moved != 3 * BAR && moved.abs_diff(3 * BAR + 700) < 50,
        "a free drag lands where the pointer is, not on a bar: {moved}"
    );

    // Off: the same without cmd. The arrows then move by a sixteenth.
    pick_snap(&mut opened, "Off");
    let start = opened.clip(PART).unwrap().start.0;
    let from = opened.at(start + 960, 0);
    let to = opened.at(start + 960 + 350, 0);
    opened.drag(from, to);
    let moved = opened.clip(PART).unwrap().start.0;
    assert!(moved.abs_diff(start + 350) < 50, "{moved}");
    opened.keys("right");
    assert_eq!(opened.clip(PART).unwrap().start.0, moved + STEP);

    // The note editor takes the same grid.
    pick_snap(&mut opened, "1/8");
    let on_hook = opened.at(4 * BAR + 100, 1);
    opened.double_click(on_hook);
    let inside = opened.in_editor(4 * BAR + 2 * 960 + 300, 67);
    opened.double_click(inside);
    let drawn = opened.clip(HOOK).unwrap().notes;
    assert!(
        drawn.iter().any(|note| note.pitch.number() == 67
            && note.start.0 == 2 * 960
            && note.length.ticks() == Ticks(480)),
        "a drawn note is an eighth on the grid of eighths: {drawn:?}"
    );
}

/// Two tracks with a clip named `clip` each, as double clicks leave them, and a third track.
fn open_with_clips_of_one_name(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        let mut changes = Changes::new();
        changes.create(id("arrangement/track-1/clip"), part());
        changes.create(id("arrangement/track-2/clip"), hook());
        project.commit("Add clips", changes).unwrap();
        project.clear_history();
    })
}

/// Cmd held from the press: a drag without the snap that keeps the selection, not a click that
/// takes the clip out of it.
#[gpui::test]
fn a_drag_with_cmd_held_from_the_press_bypasses_the_snap_and_keeps_the_selection(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    let on_part = opened.at(BAR + 960, 0);
    opened.click(on_part);
    let before = mark(&mut opened);
    let to = opened.at(BAR + 960 + 700, 0);
    opened.drag_with(on_part, to, cmd());
    let start = opened.clip(PART).unwrap().start.0;
    assert!(
        !start.is_multiple_of(STEP) && start.abs_diff(BAR + 700) < 50,
        "a cmd drag lands off the grid, where the pointer is: {start}"
    );
    assert_eq!(opened.selected_clips(), ids(&[PART]));
    one_undo_step(&mut opened, "Move clip", &before);
    // A cmd drag of a clip that is not selected moves it with the selection.
    let on_hook = opened.at(4 * BAR + 960, 1);
    let to = opened.at(4 * BAR + 960 + 300, 1);
    opened.drag_with(on_hook, to, cmd());
    assert_eq!(opened.selected_clips(), ids(&[PART, HOOK]));
    assert!(opened.clip(PART).unwrap().start.0.abs_diff(start + 300) < 50);
    // A cmd-click without a move still takes a clip out.
    opened.click_with(on_hook, cmd());
    assert_eq!(opened.selected_clips(), ids(&[PART]));
}

/// Down and up again gives the clip its id back, also where the other track has a clip of its
/// name: no `clip-2` is left behind.
#[gpui::test]
fn a_clip_nudged_away_and_back_keeps_its_id(cx: &mut TestAppContext) {
    let mut opened = open_with_clips_of_one_name(cx);
    let on_clip = opened.at(BAR + 960, 0);
    opened.click(on_clip);
    opened.keys("down");
    assert_eq!(
        opened.selected_clips(),
        ids(&["arrangement/track-2/clip-2"])
    );
    opened.keys("up");
    assert_eq!(opened.selected_clips(), ids(&["arrangement/track-1/clip"]));
    assert_eq!(opened.clip("arrangement/track-1/clip"), Some(part()));
    assert_eq!(opened.clip_file("arrangement/track-2/clip-2"), None);
}

/// Undo of a move of two clips of one name selects both again, each where undo put it back.
#[gpui::test]
fn undo_of_a_move_selects_the_clips_again_where_they_were(cx: &mut TestAppContext) {
    let mut opened = open_with_clips_of_one_name(cx);
    let (first, second) = (opened.at(BAR + 960, 0), opened.at(4 * BAR + 960, 1));
    opened.click(first);
    opened.click_with(second, shift());
    let down = opened.at(BAR + 960, 1);
    opened.drag(first, down);
    let moved = ["arrangement/track-2/clip-2", "arrangement/track-3/clip"];
    assert_eq!(opened.selected_clips(), ids(&moved));
    opened.keys("cmd-z");
    let back = ["arrangement/track-1/clip", "arrangement/track-2/clip"];
    assert_eq!(opened.selected_clips(), ids(&back));
    opened.keys("shift-cmd-z");
    assert_eq!(opened.selected_clips(), ids(&moved));
}

/// A selected clip deleted from outside during a drag of several leaves the drag, and the rest
/// go on moving.
#[gpui::test]
fn a_clip_deleted_during_a_drag_of_several_leaves_the_drag(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (on_part, on_hook) = (opened.at(BAR + 960, 0), opened.at(4 * BAR + 960, 1));
    opened.click(on_part);
    opened.click_with(on_hook, shift());
    opened.press(on_part);
    let half = opened.at(2 * BAR + 960, 0);
    opened.drag_to(half);
    assert_eq!(opened.clip(HOOK).unwrap().start, Ticks(5 * BAR));
    let path = opened.path("state/arrangement/track-2/hook.json");
    std::fs::remove_file(&path).unwrap();
    opened.edit(|project| project.apply_outside_changes(std::slice::from_ref(&path)));
    assert!(opened.gesture_open(), "the drag went on");
    let to = opened.at(3 * BAR + 960, 0);
    opened.drag_to(to);
    opened.release(to);
    assert_eq!(opened.clip(PART).unwrap().start, Ticks(3 * BAR));
    assert_eq!(opened.clip(HOOK), None);
    assert_eq!(opened.selected_clips(), ids(&[PART]));
}

#[gpui::test]
fn escape_puts_back_the_selection_of_before_a_rectangle(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let on_hook = opened.at(4 * BAR + 960, 1);
    opened.click(on_hook);
    let (from, to) = (opened.at(BAR / 2, 0), opened.at(3 * BAR, 0));
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(opened.selected_clips(), ids(&[PART]));
    opened.keys("escape");
    assert_eq!(opened.selected_clips(), ids(&[HOOK]));
    opened.release(to);
    assert_eq!(opened.selected_clips(), ids(&[HOOK]));
}

#[gpui::test]
fn escape_lets_go_of_a_tempo_change_before_it_closes_the_panel(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.track_header(0);
    opened.click(header);
    assert!(opened.track_panel().is_some());
    let bar_five = opened.ruler(4 * BAR);
    opened.double_click(bar_five);
    assert_eq!(selected_tempo(&mut opened), Some(Ticks(4 * BAR)));
    opened.keys("escape");
    assert_eq!(selected_tempo(&mut opened), None);
    assert!(opened.track_panel().is_some());
    opened.keys("escape");
    assert!(opened.track_panel().is_none());
}

#[gpui::test]
fn t_while_playing_puts_the_tempo_change_on_the_grid(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let empty = opened.at(12 * BAR, 0);
    opened.click(empty);
    opened.keys("space");
    opened.settle();
    // A second of playing and a bit, so the playhead is well past tick 0.
    opened.render(48_000 + 1_000);
    opened.settle();
    let tick = opened.playhead().tick.0;
    assert!(
        !tick.is_multiple_of(STEP),
        "the playhead is between two steps: {tick}"
    );
    opened.keys("t");
    let (added, _) = opened.tempo_changes()[1];
    assert!(added.is_multiple_of(STEP));
    assert!(added.abs_diff(tick) <= STEP / 2);
}

/// Cmd pressed during a draw frees the end of the note and never its start.
#[gpui::test]
fn cmd_during_a_draw_keeps_the_start_of_the_note_on_the_grid(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let on_hook = opened.at(4 * BAR + 100, 1);
    opened.double_click(on_hook);
    let press = opened.in_editor(4 * BAR + 960 + 100, 67);
    opened.double_press(press);
    let to = opened.in_editor(4 * BAR + 2 * 960 + 130, 67);
    opened.drag_to_with(to, cmd());
    opened.release(to);
    let drawn = opened.clip(HOOK).unwrap().notes;
    let note = drawn.iter().find(|note| note.pitch.number() == 67).unwrap();
    assert_eq!(
        note.start.0, 960,
        "the start stays in the cell of the press"
    );
    assert!(
        !note.end().0.is_multiple_of(STEP),
        "the end is free: {:?}",
        note.end()
    );
}
