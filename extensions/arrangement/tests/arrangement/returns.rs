//! Return tracks and the sends into them, rendered offline with numbers.
//!
//! The piano plays 60 and sends into the return `space`, whose effect multiplies by 1000, so
//! every sample says how much went straight to the master and how much through the return.

use arrangement::decibels::amplitude;
use arrangement::{Colour, TrackState, add_return, remove_return};
use sound_core::Changes;

use crate::support::{Harness, TICK, clip, id, level_changes, note};

const BAR: u64 = 3840;
const PIANO: f32 = 60.0;
const THROUGH_SPACE: f32 = 1000.0;
/// Past the ramp of any change, where the level is steady.
const SETTLED: usize = 4_000;

const PIANO_FILE: &str = "state/arrangement/piano/instance.json";
const SPACE_FILE: &str = "state/arrangement/space/instance.json";

/// The piano with `piano` in its record after its name, and the return `space` with `space`.
fn project(piano: &str, space: &str) -> Harness {
    let mut harness = Harness::new();
    harness.add_track("piano", 1.0);
    let mut changes = Changes::new();
    let long = clip(0, 8 * BAR, vec![note(0, 8 * BAR, PIANO as u8)]);
    changes.create(id("arrangement/piano/long"), long);
    harness.project.commit("Add clip", changes).unwrap();
    harness.write_and_apply(
        "state/arrangement/space/wide.json",
        &format!(r#"{{"tool": "test.trim", "state": {{"gain": {THROUGH_SPACE}}}}}"#),
    );
    write_space(&mut harness, space);
    write_piano(&mut harness, piano);
    harness
}

fn write_piano(harness: &mut Harness, fields: &str) {
    harness.write_and_apply(
        PIANO_FILE,
        &format!(r#"{{"tool": "arrangement.track", "state": {{"name": "piano"{fields}}}}}"#),
    );
}

fn write_space(harness: &mut Harness, fields: &str) {
    harness.write_and_apply(
        SPACE_FILE,
        &format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "space", "kind": "return", "effects": ["wide"]{fields}}}}}"#
        ),
    );
}

/// The level once every ramp is over.
fn settled(harness: &mut Harness) -> f32 {
    let render = harness.play(SETTLED + 1);
    render[SETTLED]
}

fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= expected.abs() * 1e-5
}

#[test]
fn a_send_plays_into_the_return_at_its_level() {
    let mut harness = project(r#", "sends": {"space": -6.0}"#, "");
    assert_eq!(harness.problems(), Vec::<String>::new());
    let level = settled(&mut harness);
    let expected = PIANO + PIANO * amplitude(-6.0) * THROUGH_SPACE;
    assert!(near(level, expected), "{level}, not {expected}");

    // A send taken out leaves no connection behind.
    write_piano(&mut harness, "");
    assert_eq!(settled(&mut harness), PIANO);
}

#[test]
fn a_send_at_minus_inf_or_left_out_sends_nothing() {
    for piano in [r#", "sends": {"space": "-inf"}"#, ""] {
        let mut harness = project(piano, "");
        assert_eq!(harness.problems(), Vec::<String>::new());
        assert_eq!(settled(&mut harness), PIANO, "{piano}");
    }
}

/// With the piano turned down to silence, a send after the fader sends silence and one before
/// it sends the piano as it plays.
#[test]
fn the_volume_of_the_track_turns_down_a_post_fader_send_and_not_a_pre_fader_one() {
    let piano = r#", "gain_db": "-inf", "sends": {"space": 0.0}"#;
    let mut post = project(piano, "");
    assert_eq!(settled(&mut post), 0.0);
    let mut pre = project(piano, r#", "pre_fader": true"#);
    assert_eq!(pre.problems(), Vec::<String>::new());
    assert_eq!(settled(&mut pre), PIANO * THROUGH_SPACE);
}

#[test]
fn a_muted_track_sends_nothing_before_or_after_its_fader() {
    for space in ["", r#", "pre_fader": true"#] {
        let mut harness = project(r#", "mute": true, "sends": {"space": 0.0}"#, space);
        assert_eq!(settled(&mut harness), 0.0, "{space}");
    }
}

/// The bass plays 120 and sends too. Soloing the piano silences the bass and its send, and
/// leaves the return playing what the piano sends it.
#[test]
fn a_solo_leaves_the_return_playing_and_silences_the_sends_of_the_other_tracks() {
    let mut harness = project(r#", "sends": {"space": 0.0}"#, "");
    harness.add_track("bass", 2.0);
    let mut changes = Changes::new();
    let long = clip(0, 8 * BAR, vec![note(0, 8 * BAR, PIANO as u8)]);
    changes.create(id("arrangement/bass/long"), long);
    harness.project.commit("Add clip", changes).unwrap();
    harness.write_and_apply(
        "state/arrangement/bass/instance.json",
        r#"{"tool": "arrangement.track", "state": {"name": "bass", "sends": {"space": 0.0}}}"#,
    );
    let bass = 2.0 * PIANO;
    assert_eq!(
        settled(&mut harness),
        PIANO + bass + (PIANO + bass) * THROUGH_SPACE
    );

    write_piano(&mut harness, r#", "solo": true, "sends": {"space": 0.0}"#);
    assert_eq!(settled(&mut harness), PIANO + PIANO * THROUGH_SPACE);

    // A muted return is silent all the same.
    write_space(&mut harness, r#", "mute": true"#);
    assert_eq!(settled(&mut harness), PIANO);
}

#[test]
fn a_return_that_sends_or_is_soloed_and_pre_fader_on_a_track_are_refused() {
    let mut harness = project(r#", "sends": {"space": 0.0}"#, "");
    let cases = [
        (
            SPACE_FILE,
            r#"{"tool": "arrangement.track", "state": {"name": "space", "kind": "return", "sends": {"hall": 0.0}}}"#,
            "state: a return track must not have `sends`: a return cannot send into another return yet. Send into each return from the tracks that play",
        ),
        (
            SPACE_FILE,
            r#"{"tool": "arrangement.track", "state": {"name": "space", "kind": "return", "solo": true}}"#,
            "state: a return track cannot be soloed: a solo never silences a return, so solo the tracks that send into it instead",
        ),
        (
            PIANO_FILE,
            r#"{"tool": "arrangement.track", "state": {"name": "piano", "pre_fader": true}}"#,
            "state: pre_fader is for a return track, where it says where its sends take the sound. Take it out",
        ),
        (
            PIANO_FILE,
            r#"{"tool": "arrangement.track", "state": {"name": "piano", "sends": {"Space": 0.0}}}"#,
            "state: sends must name each return by its folder name: lowercase letters, digits, `-` and `_`, not \"Space\"",
        ),
        (
            PIANO_FILE,
            r#"{"tool": "arrangement.track", "state": {"name": "piano", "sends": {"space": 7.0}}}"#,
            "state: sends.space must be a number of decibels up to 6, or \"-inf\" for silence, not 7",
        ),
    ];
    for (file, record, message) in cases {
        assert_eq!(harness.write_and_apply(file, record), 0, "{record}");
        assert_eq!(harness.problems(), [format!("{file}: {message}")]);
        // The records as they were still play.
        assert_eq!(settled(&mut harness), PIANO + PIANO * THROUGH_SPACE);
        write_space(&mut harness, "");
        write_piano(&mut harness, r#", "sends": {"space": 0.0}"#);
    }
}

#[test]
fn a_send_to_a_missing_track_or_one_that_is_no_return_is_a_problem_and_the_rest_plays() {
    let mut harness = project(r#", "sends": {"space": 0.0, "hall": 0.0}"#, "");
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/instance.json: track \"piano\" sends to \"hall\", and this arrangement has no hall/instance.json, so it sends nothing there. Name the folder of a return track, or take \"hall\" out of `sends`"
        ]
    );
    assert_eq!(settled(&mut harness), PIANO + PIANO * THROUGH_SPACE);

    harness.add_track("bass", 2.0);
    write_piano(&mut harness, r#", "sends": {"space": 0.0, "bass": 0.0}"#);
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/instance.json: track \"piano\" sends to \"bass\", which is not a return track, so it sends nothing there. Send to a track with \"kind\": \"return\", or take \"bass\" out of `sends`"
        ]
    );
    assert_eq!(settled(&mut harness), PIANO + PIANO * THROUGH_SPACE);
}

#[test]
fn removing_a_return_takes_the_sends_into_it_in_one_undo_step() {
    let mut harness = project(r#", "sends": {"space": 0.0}"#, "");
    let space = harness
        .project
        .resolve::<TrackState>(&id("arrangement/space"))
        .unwrap();
    let mut changes = Changes::new();
    remove_return(&harness.project, &mut changes, &space);
    harness.project.commit("Delete return", changes).unwrap();
    assert_eq!(harness.problems(), Vec::<String>::new());
    let piano = harness
        .project
        .state_json(&id("arrangement/piano"))
        .unwrap();
    assert!(!piano.contains("sends"), "{piano}");
    assert_eq!(settled(&mut harness), PIANO);

    harness.project.undo().unwrap();
    assert_eq!(harness.project.undo_label(), Some("File change"));
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert_eq!(settled(&mut harness), PIANO + PIANO * THROUGH_SPACE);
}

/// A track of before returns gives the same bytes, and a return saves its kind and nothing it
/// does not use.
#[test]
fn a_track_without_sends_and_a_return_save_only_what_they_use() {
    let mut harness = Harness::new();
    harness.add_track("piano", 1.0);
    let mut changes = Changes::new();
    let arrangement = id("arrangement");
    add_return(
        &harness.project,
        &mut changes,
        &arrangement,
        "Space",
        Colour::Teal,
    )
    .unwrap();
    harness.project.commit("Add return", changes).unwrap();
    let read = |file: &str| std::fs::read_to_string(harness.path(file)).unwrap();
    assert_eq!(
        read(PIANO_FILE),
        r#"{
  "tool": "arrangement.track",
  "state": {"name": "piano", "colour": "blue", "order": 0, "gain_db": 0.0, "pan": 0.0, "mute": false}
}
"#
    );
    assert_eq!(
        read(SPACE_FILE),
        r#"{
  "tool": "arrangement.track",
  "state": {
    "name": "Space",
    "kind": "return",
    "colour": "teal",
    "order": 0,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false
  }
}
"#
    );
}

/// The piano sends a short note into a return whose effect plays it 300 frames late. The engine
/// leads the piano by that much and holds back its straight path to the master, so the note
/// through the return and the note straight from the piano start on one frame, and so does the
/// bass, which plays 64 and sends nothing. Without the hold-back, the straight path of the
/// piano would start 300 frames early.
#[test]
fn a_return_with_latency_plays_in_time_with_the_track_that_sends_and_one_that_does_not() {
    const LATE: usize = 300;
    let mut harness = Harness::new();
    harness.add_track("piano", 1.0);
    harness.add_track("bass", 1.0);
    let mut changes = Changes::new();
    changes.create(
        id("arrangement/piano/hit"),
        clip(960, 960, vec![note(0, 4, 60)]),
    );
    changes.create(
        id("arrangement/bass/hit"),
        clip(960, 960, vec![note(0, 4, 64)]),
    );
    harness.project.commit("Add clips", changes).unwrap();
    harness.write_and_apply(
        "state/arrangement/space/late.json",
        &format!(r#"{{"tool": "test.late", "state": {{"frames": {LATE}}}}}"#),
    );
    harness.write_and_apply(
        "state/arrangement/space/wide.json",
        &format!(r#"{{"tool": "test.trim", "state": {{"gain": {THROUGH_SPACE}}}}}"#),
    );
    harness.write_and_apply(
        SPACE_FILE,
        r#"{"tool": "arrangement.track", "state": {"name": "space", "kind": "return", "effects": ["late", "wide"]}}"#,
    );
    write_piano(&mut harness, r#", "sends": {"space": 0.0}"#);
    assert_eq!(harness.problems(), Vec::<String>::new());

    let render = harness.play(LATE + 2 * 960 * TICK);
    assert_eq!(harness.engine.preroll_frames(), LATE as u64);
    let start = LATE + 960 * TICK;
    assert_eq!(
        level_changes(&render),
        [
            (start, PIANO + 64.0 + PIANO * THROUGH_SPACE),
            (start + 4 * TICK, 0.0)
        ]
    );
}
