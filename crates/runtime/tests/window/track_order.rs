//! Reordering tracks in the window: a track header dragged up or down, or alt-up and alt-down
//! on the selected track. The track keeps its id, so its clips, its panel, its selection and
//! its arm state go with it. Each move is one undo step that undo gives back byte for byte,
//! escape during a drag puts the track back, and undo waits while the drag goes on.

use arrangement::TrackState;
use gpui::{Pixels, Point, TestAppContext, point, px};
use sound_core::Changes;

use crate::support::{self, BAR, Opened, clip, files, id, mark, note, one_undo_step};

const FIRST: &str = "arrangement/track-1";
const SECOND: &str = "arrangement/track-2";
const AUDIO: &str = "arrangement/track-3";
const PART: &str = "arrangement/track-1/part";

/// Three tracks: two instrument tracks and an audio track, with a clip on the first. No undo
/// history.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        runtime::add_audio_track(project, &arrangement).unwrap();
        let mut changes = Changes::new();
        changes.create(id(PART), clip(BAR, BAR, vec![note(0, 480, 60)]));
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    })
}

/// The tracks by id, in the order the timeline shows them.
fn order(opened: &mut Opened<'_>) -> Vec<String> {
    opened.project(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        let tracks = arrangement::tracks(project, arrangement.id());
        tracks
            .iter()
            .map(|(track, _)| track.id().as_str().to_string())
            .collect()
    })
}

fn names(ids: &[&str]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

fn middle(from: Point<Pixels>, to: Point<Pixels>) -> Point<Pixels> {
    point((from.x + to.x) / 2., (from.y + to.y) / 2.)
}

fn armed(opened: &mut Opened<'_>, track: &str) -> bool {
    let session = opened.session.clone();
    let track = id(track);
    opened
        .cx
        .read(|cx| session.read(cx).recording().read(cx).is_armed(&track))
}

#[gpui::test]
fn a_header_dragged_down_moves_its_track_with_everything_it_has(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let session = opened.session.clone();
    opened.cx.update(|_, cx| {
        let recording = session.read(cx).recording().clone();
        recording.update(cx, |recording, cx| recording.set_armed(id(AUDIO), true, cx));
    });
    let before = mark(&mut opened);
    let (from, to) = (opened.track_header(0), opened.track_header(2));
    opened.drag(from, to);
    opened.settle();

    assert_eq!(order(&mut opened), names(&[SECOND, AUDIO, FIRST]));
    // The same track, so the same selection, panel and clips. Only the orders changed.
    assert_eq!(opened.selected_track(), Some(id(FIRST)));
    assert_eq!(opened.panel_track(), Some(id(FIRST)));
    assert!(opened.clip(PART).is_some());
    assert!(armed(&mut opened, AUDIO));
    // The arm toggle of the audio track is in its new row, the second.
    let toggle = opened.control("toggle-arm-track-3");
    let row = opened.track_header(1);
    assert!((toggle.y - row.y).abs() < px(8.), "{toggle:?} {row:?}");
    assert!(!opened.gesture_open());
    one_undo_step(&mut opened, "Move track", &before);

    // Up again to the top: one more step.
    let before = mark(&mut opened);
    let (from, to) = (opened.track_header(2), opened.track_header(0));
    opened.drag(from, to);
    opened.settle();
    assert_eq!(order(&mut opened), names(&[FIRST, SECOND, AUDIO]));
    one_undo_step(&mut opened, "Move track", &before);
}

#[gpui::test]
fn escape_during_a_track_drag_puts_the_track_back_and_undo_waits(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = support::files(opened.folder.path());
    let (from, to) = (opened.track_header(0), opened.track_header(2));
    opened.press(from);
    opened.drag_to(middle(from, to));
    opened.drag_to(to);
    // The rows follow the pointer while the drag goes on.
    assert_eq!(order(&mut opened), names(&[SECOND, AUDIO, FIRST]));
    assert!(opened.gesture_open());
    // Undo waits for the drag.
    opened.keys("cmd-z");
    assert_eq!(order(&mut opened), names(&[SECOND, AUDIO, FIRST]));
    assert!(opened.gesture_open());

    opened.keys("escape");
    assert_eq!(order(&mut opened), names(&[FIRST, SECOND, AUDIO]));
    assert!(!opened.gesture_open());
    opened.release(to);
    opened.settle();
    assert_eq!(order(&mut opened), names(&[FIRST, SECOND, AUDIO]));
    assert_eq!(files(opened.folder.path()), before);
    assert_eq!(opened.undo_label(), None);
    // Escape went to the drag and not to the panel, which stays open.
    assert_eq!(opened.panel_track(), Some(id(FIRST)));
}

#[gpui::test]
fn a_track_dragged_back_to_where_it_was_or_only_clicked_writes_nothing(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = files(opened.folder.path());
    let (from, away) = (opened.track_header(0), opened.track_header(2));
    opened.press(from);
    opened.drag_to(away);
    assert_eq!(order(&mut opened), names(&[SECOND, AUDIO, FIRST]));
    opened.drag_to(from);
    opened.release(from);
    opened.settle();
    assert_eq!(order(&mut opened), names(&[FIRST, SECOND, AUDIO]));
    assert_eq!(files(opened.folder.path()), before);
    assert_eq!(opened.undo_label(), None);

    // A press that shakes by less than the threshold is a click: it selects and moves nothing.
    let header = opened.track_header(1);
    opened.press(header);
    opened.drag_to(header + point(px(0.), px(3.)));
    opened.release(header);
    assert_eq!(opened.selected_track(), Some(id(SECOND)));
    assert_eq!(files(opened.folder.path()), before);
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn alt_up_and_alt_down_move_the_selected_track(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.track_header(0);
    opened.click(header);

    let before = mark(&mut opened);
    opened.keys("alt-down");
    assert_eq!(order(&mut opened), names(&[SECOND, FIRST, AUDIO]));
    assert_eq!(opened.selected_track(), Some(id(FIRST)));
    one_undo_step(&mut opened, "Move track", &before);

    let before = mark(&mut opened);
    opened.keys("alt-down");
    assert_eq!(order(&mut opened), names(&[SECOND, AUDIO, FIRST]));
    one_undo_step(&mut opened, "Move track", &before);

    // The last track goes no further down: no step.
    let before = mark(&mut opened);
    opened.keys("alt-down");
    assert_eq!(order(&mut opened), names(&[SECOND, AUDIO, FIRST]));
    assert_eq!(files(opened.folder.path()), before.files);
    assert_eq!(opened.undo_label(), before.undo_label);

    opened.keys("alt-up");
    assert_eq!(order(&mut opened), names(&[SECOND, FIRST, AUDIO]));
    assert_eq!(opened.undo_label().as_deref(), Some("Move track"));

    // With a clip selected the keys are the clip's: a note clip has no gain, and the track
    // stays where it is.
    let before = mark(&mut opened);
    let part = opened.at(BAR + 480, 1);
    opened.click(part);
    assert_eq!(opened.selected_clip(), Some(id(PART)));
    opened.keys("alt-up");
    assert_eq!(order(&mut opened), names(&[SECOND, FIRST, AUDIO]));
    assert_eq!(files(opened.folder.path()), before.files);
}

/// A track written with the same order as another, as an agent may: the tracks show by id, and
/// a move numbers them again in the order people saw.
#[gpui::test]
fn tracks_of_the_same_order_move_in_the_order_they_show(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    opened.edit(|project| {
        let mut changes = Changes::new();
        for track in [FIRST, SECOND, AUDIO] {
            let track = project.resolve::<TrackState>(&id(track)).unwrap();
            let state = project.state(&track).unwrap().clone();
            changes.set(&track, TrackState { order: 0, ..state });
        }
        project.commit("Same order", changes)
    });
    assert_eq!(order(&mut opened), names(&[FIRST, SECOND, AUDIO]));
    let before = mark(&mut opened);
    let (from, to) = (opened.track_header(2), opened.track_header(0));
    opened.drag(from, to);
    opened.settle();
    assert_eq!(order(&mut opened), names(&[AUDIO, FIRST, SECOND]));
    let orders: Vec<u32> = opened.project(|project| {
        [AUDIO, FIRST, SECOND]
            .map(|track| {
                let track = project.resolve::<TrackState>(&id(track)).unwrap();
                project.state(&track).unwrap().order
            })
            .to_vec()
    });
    assert_eq!(orders, [0, 1, 2]);
    one_undo_step(&mut opened, "Move track", &before);
}
