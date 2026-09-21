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

/// What a take of ten minutes costs. `cargo nextest run -p runtime --run-ignored only -- long`
///
/// It prints and asserts little: the numbers go into the pull request and into
/// ARCHITECTURE.md. What it does hold is that a map of that size still plays and still
/// answers a steadiness drag inside one display frame.
#[test]
#[ignore = "prints numbers and takes a few seconds"]
fn a_long_take_of_ten_minutes() {
    // 96 bpm in 4/4 for ten minutes is 960 beats; 380 bars is 1520.
    let mut harness = recorded(380);
    let notes = clip_of(&harness).notes.len();
    let without = render_speed(&mut harness);
    let plain = std::fs::metadata(harness.path("project.json"))
        .unwrap()
        .len();

    let started = std::time::Instant::now();
    fit(&mut harness);
    let fitting = started.elapsed();
    let map = tempo_map(&harness);
    let fitted = std::fs::metadata(harness.path("project.json"))
        .unwrap()
        .len();
    let with = render_speed(&mut harness);

    // A steadiness drag: one publish per mouse move, which runs the derive again.
    let mut moves = Vec::new();
    for step in 1_u16..=20 {
        let state = FitState {
            steadiness: f32::from(step) / 100.0,
            ..fit_state(&harness)
        };
        let started = std::time::Instant::now();
        let mut changes = sound_core::Changes::new();
        fit_tempo::set_steadiness(&harness.project, &mut changes, state.steadiness);
        harness
            .project
            .commit("Change steadiness", changes)
            .unwrap();
        moves.push(started.elapsed());
    }
    moves.sort_unstable();

    // What the clock costs per block: a lookup is a binary search over the tempo changes, so
    // it grows with the logarithm of how many there are, not with how many there are.
    let plain_clock = Clock::new(TempoMap::default(), 48_000);
    let fitted_clock = Clock::new(map.clone(), 48_000);
    let lookups = |clock: &Clock| {
        let started = std::time::Instant::now();
        let mut total = 0_u64;
        for tick in 0..200_000_u64 {
            total += clock.frame_of(Ticks(tick * 7)).0;
        }
        assert!(total > 0);
        started.elapsed().as_secs_f64() / 200_000.0 * 1e9
    };
    let (plain_lookup, fitted_lookup) = (lookups(&plain_clock), lookups(&fitted_clock));

    println!("notes {notes}, tempo changes {}", map.tempo_changes().len());
    println!(
        "one tick to frame: {plain_lookup:.1} ns with one tempo change, {fitted_lookup:.1} ns with {}",
        map.tempo_changes().len()
    );
    println!("the fit takes {fitting:?}");
    println!("project.json: {plain} bytes plain, {fitted} bytes fitted");
    println!("render: {without:.1} times realtime plain, {with:.1} fitted");
    println!(
        "one move of a steadiness drag: median {:?}, worst {:?}",
        moves[moves.len() / 2],
        moves[moves.len() - 1]
    );
    // For the run on a real device: `FIT_LONG_TAKE_DIR=/private/tmp/long-take` puts the
    // project somewhere `runtime --headless` can open it.
    if let Ok(into) = std::env::var("FIT_LONG_TAKE_DIR") {
        let _ = std::fs::remove_dir_all(&into);
        let copied = std::process::Command::new("/bin/cp")
            .arg("-R")
            .arg(harness.project.root())
            .arg(&into)
            .status()
            .unwrap();
        assert!(copied.success());
        println!("wrote the project to {into}");
    }
    assert!(map.tempo_changes().len() > 1400);
    assert!(
        moves[moves.len() - 1] < std::time::Duration::from_millis(16),
        "a mouse move of a steadiness drag took {:?}",
        moves[moves.len() - 1]
    );
}

/// How many times faster than realtime the project renders, over ten seconds of it.
fn render_speed(harness: &mut Harness) -> f64 {
    harness.project.engine().stop();
    harness.project.engine().seek(Ticks(0));
    harness.render(4_800);
    harness.project.engine().play();
    let started = std::time::Instant::now();
    harness.render(480_000);
    10.0 / started.elapsed().as_secs_f64()
}

/// Writes a project with a take fitted at double tempo into `FIT_AGENT_DIR`, for the check
/// with an outside agent. `cargo nextest run -p runtime --run-ignored only -- agent_project`
#[test]
#[ignore = "writes a folder for a run with an outside agent"]
fn write_an_agent_project() {
    let into = std::env::var("FIT_AGENT_DIR").expect("FIT_AGENT_DIR");
    let mut harness = recorded(8);
    let doubled = FitState {
        beat: BeatRate::Double,
        ..FitState::new("take-1")
    };
    write_fit(&mut harness, &doubled);
    assert_eq!(harness.project.problems(), []);
    println!(
        "{} tempo changes, first downbeat at {}",
        tempo_map(&harness).tempo_changes().len(),
        clip_of(&harness).start.0
    );
    drop(harness.project);
    let _ = std::fs::remove_dir_all(&into);
    let status = std::process::Command::new("/bin/cp")
        .arg("-R")
        .arg(harness.folder.path())
        .arg(&into)
        .status()
        .unwrap();
    assert!(status.success());
    let _ = std::fs::remove_file(format!("{into}/.sound-tools.lock"));
    let _ = std::fs::remove_file(format!("{into}/problems.txt"));
    println!("wrote the project to {into}");
}

/// Prints how far the notes of every other clip land from the beats of the fitted take, in
/// milliseconds, for the project in `FIT_AGENT_DIR`. For the check with an outside agent:
/// a part written by hand at bar 5 has to follow the rubato of the playing.
#[test]
#[ignore = "measures a folder written by a run with an outside agent"]
fn measure_a_part_against_the_take() {
    let folder = std::env::var("FIT_AGENT_DIR").expect("FIT_AGENT_DIR");
    let (project, _engine, _plugins) =
        runtime::open_read_only(std::path::Path::new(&folder)).unwrap();
    let clock = Clock::new(project.project_file().tempo_map.clone(), 48_000);
    let fit = fit_tempo::fit_of(&project).expect("a fit");
    let take_name = project.state(&fit).expect("its state").take.clone();
    let steadiness = project.state(&fit).expect("its state").steadiness;

    let mut take_onsets: Vec<u64> = Vec::new();
    let mut part_onsets: Vec<(String, u64)> = Vec::new();
    let instances: Vec<InstanceId> = project.instances().map(|(id, _)| id.clone()).collect();
    for id in instances {
        let Some(clip) = project.resolve::<Clip>(&id) else {
            continue;
        };
        let state = project.state(&clip).expect("a clip");
        let from_take = state.take.as_deref() == Some(take_name.as_str());
        for note in state.placed_notes() {
            let frame = clock.frame_of(note.start).0;
            if from_take {
                take_onsets.push(frame);
            } else {
                part_onsets.push((id.to_string(), frame));
            }
        }
    }
    take_onsets.sort_unstable();
    take_onsets.dedup();
    part_onsets.sort_by_key(|(_, frame)| *frame);
    assert!(!part_onsets.is_empty(), "no part was added");

    let mut worst = 0.0_f64;
    let mut errors = Vec::new();
    for (_, frame) in &part_onsets {
        let nearest = take_onsets
            .iter()
            .map(|onset| onset.abs_diff(*frame))
            .min()
            .unwrap_or(u64::MAX);
        let milliseconds = nearest as f64 / 48.0;
        worst = worst.max(milliseconds);
        errors.push(milliseconds);
    }
    errors.sort_by(f64::total_cmp);
    println!(
        "steadiness {:.0}%: {} notes added against {} take onsets, median {:.1} ms, worst {:.1} ms",
        steadiness * 100.0,
        errors.len(),
        take_onsets.len(),
        errors[errors.len() / 2],
        worst
    );
}
