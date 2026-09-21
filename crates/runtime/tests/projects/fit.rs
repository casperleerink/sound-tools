//! Fitting the project tempo to a recorded take, and correcting a fit from a file.
//!
//! The take is made here, not recorded, so the numbers are known: a hand that plays a chord on
//! every beat with a tempo that gives and takes, and the jitter of a real hand. It is written
//! into the project exactly as a recording writes one, so what these tests exercise is the
//! whole path from the file to the tempo map and the clip.

use fit_tempo::{BeatRate, FitState};
use sound_core::{Clock, Frames, InstanceId, TempoMap, Ticks};
use sound_notes::Clip;

use crate::generated_take::{STARTS_AT_US, generated_take};
use crate::support::Harness;

/// The clip a recording of this take would make.
const CLIP: &str = "arrangement/piano/take";
const FIT: &str = "fit-tempo";

/// A project with a track and the clip of a generated take, as a recording leaves it: the take
/// under `assets/takes/`, the clip naming it, and no fit yet.
fn recorded(bars: usize) -> Harness {
    let mut harness = Harness::new();
    harness.write_track("piano", 1, 0.15, &[]);
    let take = generated_take(bars);
    let name = take.write(harness.project.assets()).unwrap();
    assert_eq!(name, "take-1");
    let clock = harness.project.clock().clone();
    let mut clip = take.clip(|time_us| clock.tick_at_micros(time_us)).unwrap();
    clip.take = Some(name);
    let record = serde_json::json!({"tool": "arrangement.clip", "state": clip});
    harness.write_and_apply(
        &format!("state/{CLIP}.json"),
        &serde_json::to_string(&record).unwrap(),
    );
    assert_eq!(harness.project.problems(), []);
    harness
}

fn clip_id() -> InstanceId {
    InstanceId::new(CLIP).unwrap()
}

fn fit_id() -> InstanceId {
    InstanceId::new(FIT).unwrap()
}

fn clip_of(harness: &Harness) -> Clip {
    let clip = harness.project.resolve::<Clip>(&clip_id()).unwrap();
    harness.project.state(&clip).unwrap().clone()
}

fn tempo_map(harness: &Harness) -> TempoMap {
    harness.project.project_file().tempo_map.clone()
}

/// The frame every note of the clip sounds on, and how long one tick is there, under the
/// tempo map the project has now. Both in frames at 48 kHz, in order.
fn note_frames(harness: &Harness) -> Vec<(u64, u64)> {
    let clock = Clock::new(tempo_map(harness), 48_000);
    let mut frames: Vec<(u64, u64)> = clip_of(harness)
        .placed_notes()
        .map(|note| {
            let frame = clock.frame_of(note.start).0;
            (frame, clock.frame_of(Ticks(note.start.0 + 1)).0 - frame)
        })
        .collect();
    frames.sort_unstable();
    frames
}

/// Fits the project to the take, from the window, as one undo step.
fn fit(harness: &mut Harness) {
    let clip = clip_of(harness);
    let mut changes = sound_core::Changes::new();
    fit_tempo::fit_take(&harness.project, &mut changes, &clip).unwrap();
    harness
        .project
        .commit(fit_tempo::FIT_LABEL, changes)
        .unwrap();
}

/// The fit record as a file an agent writes.
fn fit_record(state: &FitState) -> String {
    let record = serde_json::json!({"tool": "fit-tempo", "state": state});
    serde_json::to_string_pretty(&record).unwrap()
}

/// Writes the fit record from outside and applies it, as an agent's edit arrives.
fn write_fit(harness: &mut Harness, state: &FitState) {
    let path = harness.write(&format!("state/{FIT}.json"), &fit_record(state));
    harness.apply(std::slice::from_ref(&path));
}

fn fit_state(harness: &Harness) -> FitState {
    let fit = harness.project.resolve::<FitState>(&fit_id()).unwrap();
    harness.project.state(&fit).unwrap().clone()
}

fn project_json(harness: &Harness) -> String {
    std::fs::read_to_string(harness.path("project.json")).unwrap()
}

fn clip_json(harness: &Harness) -> String {
    std::fs::read_to_string(harness.path(&format!("state/{CLIP}.json"))).unwrap()
}

/// At 0 % steadiness the fitted project plays the take exactly where it was heard: every note
/// within one tick of the frame it had before the fit, measured through the two saved tempo
/// maps and not against an ideal grid.
#[test]
fn a_fitted_take_sounds_where_it_was_heard() {
    let mut harness = recorded(16);
    let before = note_frames(&harness);
    let map_before = tempo_map(&harness);
    fit(&mut harness);
    let after = note_frames(&harness);
    let map_after = tempo_map(&harness);
    assert_ne!(map_before, map_after, "the tempo map did not change");
    assert_eq!(before.len(), after.len(), "a note went missing");

    // Every note within one tick of where it was, which is what "within one tick per note"
    // means: a tick is the finest a clip can hold, and both maps round a moment up to one.
    let errors: Vec<(u64, u64)> = before
        .iter()
        .zip(&after)
        .map(|((before, before_tick), (after, after_tick))| {
            (before.abs_diff(*after), (*before_tick).max(*after_tick))
        })
        .collect();
    let worst = errors.iter().map(|(error, _)| *error).max().unwrap_or(0);
    for (error, tick) in &errors {
        assert!(error <= tick, "{error} frames, one tick is {tick} frames");
    }
    // And the error does not grow over the take: the last note is as close as the first.
    let (first, last) = (errors[0].0, errors[errors.len() - 1].0);
    assert!(last <= first + worst, "{first} then {last} frames");
    assert_eq!(harness.project.problems(), []);
}

/// The whole fit is one undo step: the fit record, the tempo map and the clip go back together.
#[test]
fn a_fit_is_one_undo_step_and_gives_everything_back() {
    let mut harness = recorded(8);
    let map_before = tempo_map(&harness);
    let clip_before = clip_of(&harness);
    let file_before = project_json(&harness);
    fit(&mut harness);
    assert_ne!(clip_of(&harness), clip_before);
    assert_eq!(harness.project.undo_label(), Some(fit_tempo::FIT_LABEL));

    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some(fit_tempo::FIT_LABEL)
    );
    assert_eq!(tempo_map(&harness), map_before);
    assert_eq!(clip_of(&harness), clip_before);
    assert!(harness.project.resolve::<FitState>(&fit_id()).is_none());
    // The files hold it too, byte for byte.
    assert_eq!(project_json(&harness), file_before);
    assert!(!harness.path(&format!("state/{FIT}.json")).exists());

    harness.project.redo().unwrap();
    assert_ne!(clip_of(&harness), clip_before);
    assert!(harness.project.resolve::<FitState>(&fit_id()).is_some());
}

/// Steadiness rewrites the tempo map and never the clip, so turning it back to 0 gives the
/// fitted map again, byte for byte.
#[test]
fn steadiness_moves_the_tempo_and_leaves_the_notes() {
    let mut harness = recorded(16);
    fit(&mut harness);
    let fitted_file = project_json(&harness);
    let fitted_clip = clip_json(&harness);
    let fitted_map = tempo_map(&harness);

    let steady = FitState {
        steadiness: 1.0,
        ..fit_state(&harness)
    };
    write_fit(&mut harness, &steady);
    assert_ne!(tempo_map(&harness), fitted_map);
    assert_eq!(clip_json(&harness), fitted_clip, "a note moved");

    // Evenly spaced: every beat the same length to within four frames.
    let clock = Clock::new(tempo_map(&harness), 48_000);
    let beats = tempo_map(&harness).tempo_changes().len();
    let frames: Vec<u64> = (0..beats)
        .map(|beat| clock.frame_of(Ticks(beat as u64 * 960)).0)
        .collect();
    let steps: Vec<u64> = frames.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let spread = steps.iter().max().unwrap() - steps.iter().min().unwrap();
    assert!(spread <= 4, "beats of {:?} frames", steps.len());

    // And back to 0 %.
    let played = FitState {
        steadiness: 0.0,
        ..fit_state(&harness)
    };
    write_fit(&mut harness, &played);
    assert_eq!(tempo_map(&harness), fitted_map);
    assert_eq!(project_json(&harness), fitted_file);
    assert_eq!(clip_json(&harness), fitted_clip);
    assert_eq!(harness.project.problems(), []);
}

/// An agent corrects a fit by editing one file. Every correction rewrites the tempo map and the
/// clip in the same undo step, and correcting twice then undoing twice gives the first state
/// back, byte for byte.
#[test]
fn correcting_a_fit_from_a_file_is_one_undo_step_each() {
    let mut harness = recorded(12);
    // A fit made at double tempo on purpose, as the check of the milestone does.
    let doubled = FitState {
        beat: BeatRate::Double,
        ..FitState::new("take-1")
    };
    write_fit(&mut harness, &doubled);
    let beats = |harness: &Harness| tempo_map(harness).tempo_changes().len();
    let doubled_beats = beats(&harness);
    let first_file = project_json(&harness);
    let first_clip = clip_json(&harness);

    // Double to normal: half the beats.
    let normal = FitState {
        beat: BeatRate::Normal,
        ..doubled.clone()
    };
    write_fit(&mut harness, &normal);
    let normal_beats = beats(&harness);
    assert!(
        normal_beats * 2 > doubled_beats && normal_beats < doubled_beats,
        "{normal_beats} against {doubled_beats}"
    );
    let normal_file = project_json(&harness);
    let normal_clip = clip_json(&harness);
    assert_ne!(normal_clip, first_clip, "the clip was not made again");

    // Normal to half: half again.
    let half = FitState {
        beat: BeatRate::Half,
        ..doubled.clone()
    };
    write_fit(&mut harness, &half);
    assert!(beats(&harness) < normal_beats);

    // Two undos, and the two states come back exactly.
    harness.project.undo().unwrap();
    assert_eq!(project_json(&harness), normal_file);
    assert_eq!(clip_json(&harness), normal_clip);
    harness.project.undo().unwrap();
    assert_eq!(project_json(&harness), first_file);
    assert_eq!(clip_json(&harness), first_clip);
    assert_eq!(fit_state(&harness).beat, BeatRate::Double);
    assert_eq!(harness.project.problems(), []);
}

/// Another first downbeat moves the bar lines and nothing else, and another time signature
/// rewrites the grid from the same take.
#[test]
fn another_downbeat_and_another_time_signature_rebuild_the_grid() {
    let mut harness = recorded(12);
    write_fit(&mut harness, &FitState::new("take-1"));
    let start = clip_of(&harness).start;

    // The first downbeat a beat and a bit later: the clip starts a bar further on, because the
    // pickup needs room in front of the downbeat.
    let later = FitState {
        first_downbeat_us: 2_600_000,
        ..fit_state(&harness)
    };
    write_fit(&mut harness, &later);
    let moved = clip_of(&harness).start;
    assert_ne!(moved, start, "the bar lines did not move");
    // The take still sounds where it was: the clip's start in real time is unchanged.
    let clock = Clock::new(tempo_map(&harness), 48_000);
    let frame = clock.frame_of(moved).0;
    let wanted = STARTS_AT_US * 48_000 / 1_000_000;
    // Within one tick of the lead, whose beats are long because one bar covers the silence in
    // front of the take. Every note keeps its own moment exactly.
    let tick = clock.frame_of(Ticks(moved.0 + 1)).0 - frame;
    assert!(frame.abs_diff(wanted) <= tick, "{frame} against {wanted}");

    // The time signature is the project's. Changing it rebuilds the map in the same group.
    let before = tempo_map(&harness);
    let file = r#"{"format": 1, "extensions": ["arrangement", "fit-tempo", "instrument", "plugin-host", "tone"], "tempo_map": {"time_signature": "3/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]}, "connections": []}"#;
    harness.write_and_apply("project.json", file);
    let after = tempo_map(&harness);
    assert_eq!(after.time_signature().to_string(), "3/4");
    assert!(
        after.tempo_changes().len() > 1,
        "the fit did not rewrite the map: {:?}",
        after.tempo_changes()
    );
    assert_ne!(after, before);
    // One undo gives the whole group back.
    harness.project.undo().unwrap();
    assert_eq!(tempo_map(&harness), before);
    assert_eq!(harness.project.problems(), []);
}

/// A fit whose take is not there says so, and changes nothing.
#[test]
fn a_fit_that_cannot_be_made_says_why() {
    let mut harness = recorded(8);
    let map = tempo_map(&harness);
    write_fit(&mut harness, &FitState::new("take-9"));
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "state/fit-tempo.json");
    assert!(
        problems[0].message.contains("does not exist"),
        "{}",
        problems[0].message
    );
    assert_eq!(tempo_map(&harness), map);

    // Naming the take that is there takes the problem away.
    write_fit(&mut harness, &FitState::new("take-1"));
    assert_eq!(harness.project.problems(), []);
    assert_ne!(tempo_map(&harness), map);
}

/// A clip that no longer names the take is reported, and the grid still follows the take.
#[test]
fn a_fit_without_a_clip_still_fits_the_grid() {
    let mut harness = recorded(8);
    fit(&mut harness);
    let map = tempo_map(&harness);
    let mut clip = clip_of(&harness);
    clip.take = None;
    let record = serde_json::json!({"tool": "arrangement.clip", "state": clip});
    harness.write_and_apply(
        &format!("state/{CLIP}.json"),
        &serde_json::to_string(&record).unwrap(),
    );
    // The clip change alone does not run the fit again; the next fit edit does.
    let nudged = FitState {
        steadiness: 0.25,
        ..fit_state(&harness)
    };
    write_fit(&mut harness, &nudged);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].message.contains("no clip names the take"),
        "{}",
        problems[0].message
    );
    assert_ne!(tempo_map(&harness), map, "the grid still follows the take");
}

/// A read-only open never derives and never writes, so `--render` and `--inspect` see exactly
/// what the files hold.
#[test]
fn a_read_only_open_writes_nothing() {
    let mut harness = recorded(8);
    fit(&mut harness);
    let file = project_json(&harness);
    let clip = clip_json(&harness);
    let folder = harness.project.root().to_path_buf();
    let (project, _engine, _plugins) = runtime::open_read_only(&folder).unwrap();
    assert!(project.resolve::<FitState>(&fit_id()).is_some());
    drop(project);
    assert_eq!(project_json(&harness), file);
    assert_eq!(clip_json(&harness), clip);
}

/// The grid a fit builds is the same on every run, so two machines that open one project get
/// the same bytes.
#[test]
fn the_same_take_gives_the_same_project_file_every_time() {
    let mut first = recorded(8);
    fit(&mut first);
    let mut second = recorded(8);
    fit(&mut second);
    assert_eq!(project_json(&first), project_json(&second));
    assert_eq!(clip_json(&first), clip_json(&second));
    let clock = Clock::new(tempo_map(&first), 48_000);
    assert_eq!(clock.tick_at(Frames(0)), Ticks(0));
}
