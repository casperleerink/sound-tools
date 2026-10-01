//! The automation lanes under a track: the toggle in its header shows them, the select under
//! them adds one, a drag draws its line, alt and a drag erase points, and a double click clears
//! it. Each edit is one undo step that undo gives back byte for byte. A clip dragged with its
//! automation shows what the drop will be.

use arrangement::view::track_lanes::y_of;
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

/// The toggle of the lanes of the first track, left of its dot.
fn toggle(opened: &mut Opened<'_>) -> gpui::Point<gpui::Pixels> {
    let header = opened.track_header(0);
    point(px(14.), header.y)
}

fn shows_lanes(opened: &mut Opened<'_>) -> bool {
    let timeline = opened.timeline.clone();
    opened
        .cx
        .read(|cx| timeline.read(cx).shows_lanes(&id(ONE)))
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
    assert!(opened.find("menu-/gain_db").is_none(), "the volume has a lane");
    assert!(opened.find("menu-dark/cutoff_hz").is_none(), "the cutoff has one");
    let pan = opened.control("menu-/pan");
    opened.click(pan);
    assert_eq!(points(&mut opened, "pan"), [(0, 0.)]);
    assert_eq!(lanes(&mut opened).len(), 3);
    one_undo_step(&mut opened, "Add automation", &before);
}

/// A drag across a lane draws on the sixteenths it passes, over the points that were there. A
/// level line is its two ends, and above the lane is the top of the range.
#[gpui::test]
fn a_drag_draws_the_line_in_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let before = mark(&mut opened);
    let (from, to) = (
        opened.in_track_lane(5 * BAR, 0, 0, y_of(1.)),
        opened.in_track_lane(6 * BAR, 0, 0, -20.),
    );
    opened.press(from);
    // A press is no edit.
    assert!(!opened.gesture_open());
    opened.drag_to(point(from.x + px(48.), from.y));
    opened.drag_to(to);
    opened.release(to);
    let mut drawn = volume();
    drawn.extend([(5 * BAR, 6.), (6 * BAR, 6.)]);
    assert_eq!(points(&mut opened, "gain_db"), drawn);
    one_undo_step(&mut opened, "Draw automation", &before);
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

/// A double click clears a lane: it goes, the number plays its record again, and the lanes
/// under it move up.
#[gpui::test]
fn a_double_click_clears_the_lane_in_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx, true);
    let before = mark(&mut opened);
    let volume_lane = opened.in_track_lane(4 * BAR, 0, 0, 20.);
    opened.double_click(volume_lane);
    let left: Vec<String> = lanes(&mut opened)
        .into_iter()
        .map(|lane| lane.parameter)
        .collect();
    assert_eq!(left, ["cutoff_hz"]);
    assert_eq!(opened.project(|project| project.problems().len()), 0);
    one_undo_step(&mut opened, "Clear automation", &before);
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
    let expected = opened.project(|project| moved(&tracks, &[step], &travel_in(project)));
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
