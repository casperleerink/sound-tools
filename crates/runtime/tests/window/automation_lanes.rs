//! The automation lanes under a track: the toggle on the second line of its header shows them,
//! the select under them adds one, a click adds a point, a drag on a point moves it (with shift
//! along one axis), delete removes the selected point, and alt and a drag erase points. Each
//! edit is one undo step that undo gives back byte for byte. A clip dragged with its automation
//! shows what the drop will be. `a` on the selected track shows the lanes too, and tab reaches
//! the select.

use arrangement::view::layout::{LANES_MIDDLE, NAME_LEFT, TRACK_HEIGHT};
use arrangement::view::track_lanes::LANE_BOX;
use arrangement::{AutomationLane, AutomationValue, LaneMove, TrackState, moved, travel_in};
use filter::FilterState;
use gpui::{Modifiers, TestAppContext, point, px};
use sound_core::{Changes, Ticks};
use sound_notes::Point;

use crate::support::{self, BAR, Opened, clip, files, id, mark, note, one_undo_step};

const PART: &str = "arrangement/track-1/part";
const ONE: &str = "arrangement/track-1";

/// The volume of the first track: -6 dB, and under `part`, bars 2 and 3, a dip to -12 dB and a
/// rise to 0 dB.
fn volume() -> Vec<(u64, f32)> {
    vec![
        (0, -6.),
        (BAR, -6.),
        (BAR + 960, -12.),
        (2 * BAR + 960, 0.),
        (3 * BAR - 1, 0.),
        (3 * BAR, -6.),
    ]
}

/// The cutoff of the filter `dark`: a sweep from 200 Hz up to 2 kHz at bar 3.
fn sweep() -> Vec<(u64, f32)> {
    vec![(0, 200.), (2 * BAR, 2000.), (3 * BAR - 1, 2000.)]
}

fn lane(device: Option<&str>, parameter: &str, points: &[(u64, f32)]) -> AutomationLane {
    AutomationLane {
        device: device.map(str::to_string),
        parameter: parameter.to_string(),
        points: points
            .iter()
            .map(|&(tick, value)| Point {
                tick: Ticks(tick),
                value: AutomationValue(value),
            })
            .collect(),
    }
}

/// Two tracks. On the first, `part` in bars 2 and 3, a filter `dark`, and the lanes of
/// [`volume`] and [`sweep`], shown when `shown`. No undo history.
fn open(cx: &mut TestAppContext, shown: bool) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        let track = project.resolve::<TrackState>(&id(ONE)).unwrap();
        let mut changes = Changes::new();
        changes.create(id(PART), clip(BAR, 2 * BAR, vec![note(960, 480, 60)]));
        let dark = arrangement::add_effect(project, &mut changes, &track, "dark").unwrap();
        changes.create(dark, FilterState::default());
        project.commit("Add clip", changes).unwrap();
        let mut state = project.state(&track).unwrap().clone();
        state.automation = vec![
            lane(None, "gain_db", &volume()),
            lane(Some("dark"), "cutoff_hz", &sweep()),
        ];
        let mut changes = Changes::new();
        changes.set(&track, state);
        project.commit("Automate", changes).unwrap();
        project.clear_history();
        assert_eq!(project.problems(), []);
    });
    if shown {
        let toggle = toggle(&mut opened);
        opened.click(toggle);
        assert!(shows_lanes(&mut opened));
    }
    opened
}

/// The toggle of the lanes of the first track, its words on the second line of its header.
fn toggle(opened: &mut Opened<'_>) -> gpui::Point<gpui::Pixels> {
    let header = opened.track_header(0);
    point(
        px(NAME_LEFT + 8.),
        header.y + px(LANES_MIDDLE - TRACK_HEIGHT / 2.),
    )
}

fn shows_lanes(opened: &mut Opened<'_>) -> bool {
    let timeline = opened.timeline.clone();
    opened.cx.read(|cx| timeline.read(cx).shows_lanes(&id(ONE)))
}

fn lanes(opened: &mut Opened<'_>) -> Vec<AutomationLane> {
    opened.project(|project| {
        let track = project.resolve::<TrackState>(&id(ONE)).unwrap();
        project.state(&track).unwrap().automation.clone()
    })
}

/// The points of the lane of `parameter` of the first track, by tick and value.
fn points(opened: &mut Opened<'_>, parameter: &str) -> Vec<(u64, f32)> {
    let lanes = lanes(opened);
    let lane = lanes.iter().find(|lane| lane.parameter == parameter);
    let points = lane.map_or(&[][..], |lane| &lane.points[..]).iter();
    points.map(|point| (point.tick.0, point.value.0)).collect()
}

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Modifiers::default()
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

/// The middle of the dot of a point of a lane of the first track.
fn dot(opened: &mut Opened<'_>, lane: usize, point: usize) -> gpui::Point<gpui::Pixels> {
    let (tick, y) = opened.project(|project| {
        let track = project.resolve::<TrackState>(&id(ONE)).unwrap();
        let state = project.state(&track).unwrap();
        let lane = &state.automation[lane];
        let number = lane.number(track.id(), state, &travel_in(project));
        let range = number.unwrap().range;
        let point = lane.points[point];
        (point.tick.0, LANE_BOX.y_of(range.position(point.value.0)))
    });
    opened.in_track_lane(tick, 0, lane, y)
}

/// The toggle shows the lanes and folds them away again. It is no edit: no file changes and
/// there is no undo step.
#[gpui::test]
fn the_toggle_shows_and_folds_the_lanes(cx: &mut TestAppContext) {
    let mut opened = open(cx, false);
    let before = mark(&mut opened);
    let place = toggle(&mut opened);
    opened.click(place);
    assert!(shows_lanes(&mut opened));
    // The select that adds a lane is under them.
    assert!(opened.find("add-lane-track-1").is_some());
    opened.click(place);
    assert!(!shows_lanes(&mut opened));
    assert!(opened.find("add-lane-track-1").is_none());
    assert_eq!(opened.undo_label(), before.undo_label);
    assert_eq!(files(opened.folder.path()), before.files);
    // The toggle did not select the track.
    assert_eq!(opened.selected_track(), None);
}

/// The select offers the numbers of the track and of its devices that have no lane yet, and a
/// pick adds a lane that holds the value of the record: one undo step.
#[gpui::test]
fn the_select_adds_a_lane_in_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let before = mark(&mut opened);
    let select = opened.control("add-lane-track-1");
    opened.click(select);
    assert!(opened.find("menu-dark/resonance").is_some());
    assert!(
        opened.find("menu-/gain_db").is_none(),
        "the volume has a lane"
    );
    assert!(
        opened.find("menu-dark/cutoff_hz").is_none(),
        "the cutoff has one"
    );
    let pan = opened.control("menu-/pan");
    opened.click(pan);
    assert_eq!(points(&mut opened, "pan"), [(0, 0.)]);
    assert_eq!(lanes(&mut opened).len(), 3);
    one_undo_step(&mut opened, "Add automation", &before);
}

/// A click in a lane away from the dots adds a point there, on the grid, at the height of the
/// click: one undo step. It is selected, and delete takes it away again.
#[gpui::test]
fn a_click_adds_a_point_and_delete_removes_it(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let before = mark(&mut opened);
    let top = opened.in_track_lane(5 * BAR + 20, 0, 0, LANE_BOX.y_of(1.));
    opened.click(top);
    let mut added = volume();
    added.push((5 * BAR, 6.));
    assert_eq!(points(&mut opened, "gain_db"), added);
    one_undo_step(&mut opened, "Add automation point", &before);

    let before = mark(&mut opened);
    opened.keys("backspace");
    assert_eq!(points(&mut opened, "gain_db"), volume());
    one_undo_step(&mut opened, "Delete automation point", &before);
}

/// A click on a dot selects its point and changes nothing. A drag moves it on the grid, and
/// with shift only the way the pointer went furthest.
#[gpui::test]
fn a_drag_on_a_point_moves_it_and_shift_keeps_one_axis(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let before = mark(&mut opened);
    let dip = dot(&mut opened, 0, 2);
    opened.click(dip);
    assert_eq!(points(&mut opened, "gain_db"), volume());
    assert!(!opened.gesture_open());
    assert_eq!(opened.undo_label(), before.undo_label);

    let to = opened.in_track_lane(BAR + 1440 + 30, 0, 0, LANE_BOX.y_of(1.));
    opened.drag(dip, to);
    let mut moved = volume();
    moved[2] = (BAR + 1440, 6.);
    assert_eq!(points(&mut opened, "gain_db"), moved);
    one_undo_step(&mut opened, "Move automation point", &before);

    // Mostly down with shift: the point keeps its tick.
    let peak = dot(&mut opened, 0, 2);
    let down = point(peak.x + px(8.), peak.y + px(30.));
    opened.drag_with(peak, down, shift());
    let (tick, value) = points(&mut opened, "gain_db")[2];
    assert_eq!(tick, BAR + 1440);
    assert!(value < 6., "{value}");
    // Mostly sideways with shift: it keeps its value.
    let point_now = dot(&mut opened, 0, 2);
    let side = opened.in_track_lane(BAR + 1920, 0, 0, 0.);
    let side = point(side.x, point_now.y + px(6.));
    opened.drag_with(point_now, side, shift());
    assert_eq!(points(&mut opened, "gain_db")[2], (BAR + 1920, value));
}

/// Escape during a drag of a point puts it back and lets go of it: delete then does nothing.
#[gpui::test]
fn escape_puts_a_dragged_point_back_and_lets_go_of_it(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let before = mark(&mut opened);
    let dip = dot(&mut opened, 0, 2);
    opened.press(dip);
    opened.drag_to(point(dip.x + px(60.), dip.y - px(20.)));
    assert_ne!(points(&mut opened, "gain_db"), volume());
    opened.keys("escape");
    opened.release(dip);
    opened.keys("backspace");
    assert_eq!(points(&mut opened, "gain_db"), volume());
    assert_eq!(opened.undo_label(), before.undo_label);
}

/// An undo between the press on a point and the first move takes the lane the drag started
/// from: the drag ends and does not write it back.
#[gpui::test]
fn an_undo_before_the_first_move_ends_the_drag(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let added = opened.in_track_lane(5 * BAR, 0, 0, LANE_BOX.y_of(1.));
    opened.click(added);
    let dip = dot(&mut opened, 0, 2);
    opened.press(dip);
    opened.keys("cmd-z");
    assert_eq!(points(&mut opened, "gain_db"), volume());
    let to = point(dip.x + px(60.), dip.y - px(20.));
    opened.drag_to(to);
    opened.release(to);
    assert_eq!(points(&mut opened, "gain_db"), volume());
}

/// Alt and a drag erase the points between the press and the pointer, on the grid.
#[gpui::test]
fn an_alt_drag_erases_points_in_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let before = mark(&mut opened);
    let from = opened.in_track_lane(BAR + 240, 0, 1, 20.);
    let to = opened.in_track_lane(3 * BAR + 240, 0, 1, 20.);
    opened.drag_with(from, to, alt());
    assert_eq!(points(&mut opened, "cutoff_hz"), [(0, 200.)]);
    one_undo_step(&mut opened, "Erase automation", &before);
}

/// An erase of every point takes the lane away, as a clear does: the number plays its record.
#[gpui::test]
fn erasing_every_point_takes_the_lane_away(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let before = mark(&mut opened);
    let from = opened.in_track_lane(0, 0, 1, 20.);
    let to = opened.in_track_lane(4 * BAR, 0, 1, 20.);
    opened.drag_with(from, to, alt());
    assert_eq!(points(&mut opened, "cutoff_hz"), []);
    assert_eq!(lanes(&mut opened).len(), 1);
    one_undo_step(&mut opened, "Erase automation", &before);
}

/// What a drag of a clip shows is what it drops: the lanes while the button is down are those
/// `arrangement::moved` gives from the tracks before the drag, and they stay after it. The
/// hint shows only while the automation goes along, so not with alt.
#[gpui::test]
fn the_ghost_of_a_drag_is_the_drop(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let tracks = opened.project(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        let tracks = arrangement::tracks(project, arrangement.id()).into_iter();
        let tracks = tracks.map(|(track, state)| (track.id().clone(), state.clone()));
        tracks.collect::<std::collections::BTreeMap<_, _>>()
    });
    let step = LaneMove {
        from: id(ONE),
        range: Ticks(BAR)..Ticks(3 * BAR),
        to: id(ONE),
        start: Ticks(5 * BAR),
    };
    let expected = opened.project(|project| moved(&tracks, &[step], &travel_in(project)).lanes);
    let expected = expected.get(&id(ONE)).cloned().unwrap();
    let automation_moves = |opened: &mut Opened<'_>| {
        let timeline = opened.timeline.clone();
        opened.cx.read(|cx| timeline.read(cx).automation_moves())
    };

    let (from, to) = (opened.at(BAR + 960, 0), opened.at(5 * BAR + 960, 0));
    opened.press(from);
    assert!(!automation_moves(&mut opened), "nothing moved yet");
    opened.drag_to(to);
    assert!(automation_moves(&mut opened));
    assert_eq!(lanes(&mut opened), expected);
    // Alt leaves the automation, and the hint goes.
    opened.drag_to_with(to, alt());
    assert!(!automation_moves(&mut opened));
    opened.drag_to(to);
    assert!(automation_moves(&mut opened));
    opened.release(to);
    assert!(!automation_moves(&mut opened));
    assert_eq!(lanes(&mut opened), expected);

    // Folded away, a drag takes it along the same way and says so.
    let place = toggle(&mut opened);
    opened.click(place);
    let (from, to) = (opened.at(5 * BAR + 960, 0), opened.at(7 * BAR + 960, 0));
    opened.press(from);
    opened.drag_to(to);
    assert!(automation_moves(&mut opened));
    opened.release(to);
}

/// The order of the tracks of the arrangement, by name.
fn order(opened: &mut Opened<'_>) -> Vec<String> {
    opened.project(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        let tracks = arrangement::tracks(project, arrangement.id()).into_iter();
        let names = tracks.map(|(track, _)| track.id().name().to_string());
        names.collect()
    })
}

/// A drag of a track header past a track that shows its lanes goes where the pointer is and
/// stays there while the pointer stays: the rows of mouse down decide, not those the last move
/// made, in which the taller track is somewhere else.
#[gpui::test]
fn a_track_dragged_past_open_lanes_stays_where_the_pointer_is(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    assert_eq!(order(&mut opened), ["track-1", "track-2"]);
    // The second header up into the lanes of the first track, and on a little.
    let from = opened.track_header(1);
    let into_lanes = opened.in_track_lane(0, 0, 1, 20.);
    let to = point(from.x, into_lanes.y);
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(order(&mut opened), ["track-2", "track-1"]);
    for step in [1., 2., 3.] {
        opened.drag_to(point(to.x, to.y + px(step)));
        assert_eq!(order(&mut opened), ["track-2", "track-1"], "{step}");
    }
    opened.release(to);
    assert_eq!(opened.undo_label().as_deref(), Some("Move track"));
}

/// A lane just added holds one value, so a clip over it takes nothing along and no hint shows,
/// also to another track, where only the volume and the pan would go.
#[gpui::test]
fn a_lane_that_holds_one_value_does_not_move(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        let mut changes = Changes::new();
        changes.create(id(PART), clip(0, BAR, vec![note(0, 480, 60)]));
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let toggle = toggle(&mut opened);
    opened.click(toggle);
    let select = opened.control("add-lane-track-1");
    opened.click(select);
    let pan = opened.control("menu-/pan");
    opened.click(pan);
    assert_eq!(points(&mut opened, "pan"), [(0, 0.)]);
    let timeline = opened.timeline.clone();
    let (from, to) = (opened.at(240, 0), opened.at(4 * BAR + 240, 1));
    opened.press(from);
    opened.drag_to(to);
    assert!(!opened.cx.read(|cx| timeline.read(cx).automation_moves()));
    opened.release(to);
    assert_eq!(points(&mut opened, "pan"), [(0, 0.)]);
    let second = opened.project(|project| {
        let track = project.resolve::<TrackState>(&id("arrangement/track-2"));
        project.state(&track.unwrap()).unwrap().automation.clone()
    });
    assert_eq!(second, []);
}

/// `a` on the selected track shows its lanes and folds them away, and tab reaches the select
/// that adds one.
#[gpui::test]
fn a_shows_the_lanes_of_the_selected_track_and_tab_reaches_the_select(cx: &mut TestAppContext) {
    let mut opened = open(cx, false);
    let header = opened.track_header(0);
    opened.click(header);
    opened.keys("a");
    assert!(shows_lanes(&mut opened));
    let timeline = opened.timeline.clone();
    let lane_menu = opened
        .cx
        .read(|cx| timeline.read(cx).lane_menu(&id(ONE)).cloned());
    let menu = lane_menu.unwrap();
    let mut reached = false;
    for _ in 0..4 {
        opened.keys("tab");
        let menu = menu.clone();
        reached |= opened
            .cx
            .update(|window, cx| menu.read(cx).trigger_is_focused(window));
    }
    assert!(reached, "tab does not reach the select under the lanes");
    // Back on the timeline, `a` folds them away.
    let header = opened.track_header(0);
    opened.click(header);
    opened.keys("a");
    assert!(!shows_lanes(&mut opened));
}
