//! The automation under a clip goes along when the clip is dragged, nudged, cut, copied, pasted
//! or duplicated, and alt-drag leaves it. Each is one undo step with the clip, which undo gives
//! back byte for byte.

use arrangement::{AutomationLane, AutomationValue, TrackState};
use filter::FilterState;
use gpui::{Modifiers, TestAppContext};
use sound_core::{Changes, Ticks};
use sound_notes::Point;

use crate::support::{self, BAR, Opened, clip, id, mark, note, one_undo_step};

const PART: &str = "arrangement/track-1/part";
const ONE: &str = "arrangement/track-1";
const TWO: &str = "arrangement/track-2";

/// The volume of the first track: -6 dB, and under `part`, bars 2 and 3, a dip to -12 dB and a
/// rise to 0 dB that holds to the end of the clip.
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

/// The volume shape under `part`, from its start.
fn dip(at: u64) -> Vec<(u64, f32)> {
    let shape = [(0, -6.), (960, -12.), (BAR + 960, 0.), (2 * BAR - 1, 0.)];
    shape
        .iter()
        .map(|&(tick, value)| (at + tick, value))
        .collect()
}

/// The cutoff of the filter `dark` of the first track: a sweep from 200 Hz up to 2 kHz at bar
/// 3, through the start of `part`.
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
/// [`volume`] and [`sweep`]. No undo history.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
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
    })
}

/// The points of the lane of `parameter` of a track, by tick and value.
fn points(opened: &mut Opened<'_>, track: &str, parameter: &str) -> Vec<(u64, f32)> {
    opened.project(|project| {
        let track = project.resolve::<TrackState>(&id(track)).unwrap();
        let lanes = &project.state(&track).unwrap().automation;
        let lane = lanes.iter().find(|lane| lane.parameter == parameter);
        let points = lane.map_or(&[][..], |lane| &lane.points[..]).iter();
        points.map(|point| (point.tick.0, point.value.0)).collect()
    })
}

fn ticks(points: &[(u64, f32)]) -> Vec<u64> {
    points.iter().map(|(tick, _)| *tick).collect()
}

#[gpui::test]
fn a_dragged_clip_takes_its_automation_along_in_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(5 * BAR + 960, 0));
    opened.drag(from, to);
    assert_eq!(opened.clip(PART).unwrap().start, Ticks(5 * BAR));

    // The volume was -6 dB around the clip, so where it was is -6 dB again, and the dip is
    // four bars later.
    let mut moved = vec![(0, -6.)];
    moved.extend(dip(5 * BAR));
    moved.push((7 * BAR, -6.));
    assert_eq!(points(&mut opened, ONE, "gain_db"), moved);

    // The sweep: where the clip was, a straight line from its left edge to the 2 kHz after
    // it. Where it lands, the line it took, which starts halfway up the sweep on the knob,
    // at 632 Hz, not halfway in hertz.
    let cutoff = points(&mut opened, ONE, "cutoff_hz");
    assert_eq!(
        ticks(&cutoff),
        [0, BAR - 1, 3 * BAR, 5 * BAR - 1, 5 * BAR, 6 * BAR]
    );
    let (edge, landed) = (cutoff[1].1, cutoff[4].1);
    assert!((edge - 632.).abs() < 1., "{cutoff:?}");
    assert!((landed - (200f32 * 2000.).sqrt()).abs() < 0.1, "{cutoff:?}");
    assert_eq!(cutoff[5].1, 2000.);

    one_undo_step(&mut opened, "Move clip", &before);
    opened.keys("cmd-z");
    assert_eq!(points(&mut opened, ONE, "gain_db"), volume());
    assert_eq!(points(&mut opened, ONE, "cutoff_hz"), sweep());
}

#[gpui::test]
fn an_alt_drag_moves_the_clip_alone(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(5 * BAR + 960, 0));
    let alt = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    opened.drag_with(from, to, alt);
    assert_eq!(opened.clip(PART).unwrap().start, Ticks(5 * BAR));
    assert_eq!(points(&mut opened, ONE, "gain_db"), volume());
    assert_eq!(points(&mut opened, ONE, "cutoff_hz"), sweep());
    one_undo_step(&mut opened, "Move clip", &before);
}

/// Alt pressed during the drag puts the automation back, and let go takes it along again.
#[gpui::test]
fn alt_during_a_drag_flips_whether_the_automation_goes(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(5 * BAR + 960, 0));
    let alt = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    opened.press(from);
    opened.drag_to(to);
    assert_ne!(points(&mut opened, ONE, "gain_db"), volume());
    opened.drag_to_with(to, alt);
    assert_eq!(points(&mut opened, ONE, "gain_db"), volume());
    assert_eq!(points(&mut opened, ONE, "cutoff_hz"), sweep());
    opened.drag_to(to);
    assert_eq!(ticks(&points(&mut opened, ONE, "gain_db"))[1], 5 * BAR);
    opened.release(to);
    assert_eq!(opened.undo_label().as_deref(), Some("Move clip"));
}

/// To another track the volume goes along, around the volume of that track, and the sweep
/// stays: the filter belongs to the first track.
#[gpui::test]
fn to_another_track_the_volume_goes_along_and_the_filter_lane_stays(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(5 * BAR + 960, 1));
    opened.drag(from, to);
    assert!(opened.clip("arrangement/track-2/part").is_some());

    assert_eq!(points(&mut opened, ONE, "gain_db"), [(0, -6.)]);
    assert_eq!(points(&mut opened, ONE, "cutoff_hz"), sweep());
    // The second track plays at 0 dB, which holds before and after the dip.
    let mut landed = vec![(5 * BAR - 1, 0.)];
    landed.extend(dip(5 * BAR));
    landed.pop();
    assert_eq!(points(&mut opened, TWO, "gain_db"), landed);
    assert_eq!(points(&mut opened, TWO, "cutoff_hz"), []);
    one_undo_step(&mut opened, "Move clip", &before);
}

#[gpui::test]
fn an_arrow_nudges_the_automation_with_the_clip(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let part = opened.at(BAR + 960, 0);
    opened.click(part);
    let before = mark(&mut opened);
    opened.keys("right");
    let gain = points(&mut opened, ONE, "gain_db");
    assert_eq!(
        ticks(&gain)[1..5],
        [BAR + 240, BAR + 1200, 2 * BAR + 1200, 3 * BAR + 239]
    );
    one_undo_step(&mut opened, "Nudge clip", &before);
}

#[gpui::test]
fn a_paste_puts_the_automation_down_over_what_was_there(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let part = opened.at(BAR + 960, 0);
    opened.click(part);
    opened.keys("cmd-c");
    let ruler = opened.ruler(5 * BAR);
    opened.click(ruler);
    opened.settle();
    let before = mark(&mut opened);
    opened.keys("cmd-v");
    assert_eq!(
        opened.selected_clip(),
        Some(id("arrangement/track-1/part-2"))
    );
    // The clip that was copied keeps its automation, and the copy gets the same.
    let mut pasted = volume();
    pasted.extend(dip(5 * BAR));
    pasted.push((7 * BAR, -6.));
    assert_eq!(points(&mut opened, ONE, "gain_db"), pasted);
    one_undo_step(&mut opened, "Paste clip", &before);
}

#[gpui::test]
fn a_duplicate_repeats_the_automation_right_after(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let part = opened.at(BAR + 960, 0);
    opened.click(part);
    let before = mark(&mut opened);
    opened.keys("cmd-d");
    let mut repeated = volume();
    repeated.pop();
    repeated.extend(dip(3 * BAR));
    repeated.push((5 * BAR, -6.));
    assert_eq!(points(&mut opened, ONE, "gain_db"), repeated);
    one_undo_step(&mut opened, "Duplicate clip", &before);
}

#[gpui::test]
fn escape_during_a_drag_puts_the_clip_and_its_automation_back(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = support::files(opened.folder.path());
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(5 * BAR + 960, 1));
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(points(&mut opened, ONE, "gain_db"), [(0, -6.)]);
    opened.keys("escape");
    opened.release(to);
    assert_eq!(points(&mut opened, ONE, "gain_db"), volume());
    assert_eq!(points(&mut opened, TWO, "gain_db"), []);
    assert_eq!(opened.undo_label(), None);
    assert_eq!(support::files(opened.folder.path()), before);
}

/// A cut takes the automation as a move does, and a paste puts it back down elsewhere.
#[gpui::test]
fn a_cut_takes_the_automation_and_a_paste_puts_it_down(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let part = opened.at(BAR + 960, 0);
    opened.click(part);
    let before = mark(&mut opened);
    opened.keys("cmd-x");
    assert_eq!(opened.clip(PART), None);
    assert_eq!(points(&mut opened, ONE, "gain_db"), [(0, -6.)]);
    let cutoff = points(&mut opened, ONE, "cutoff_hz");
    assert_eq!(ticks(&cutoff), [0, BAR - 1, 3 * BAR]);
    one_undo_step(&mut opened, "Cut clip", &before);

    let ruler = opened.ruler(5 * BAR);
    opened.click(ruler);
    opened.settle();
    opened.keys("cmd-v");
    let mut pasted = vec![(0, -6.)];
    pasted.extend(dip(5 * BAR));
    pasted.push((7 * BAR, -6.));
    assert_eq!(points(&mut opened, ONE, "gain_db"), pasted);
}
