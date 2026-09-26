//! The master: the limiter at the end of every project, its volume, and the meters. The probe
//! puts out the pitches it holds as a level, so a level far over full scale is easy to make and
//! every sample is a number a test can check.

use arrangement::{ArrangementState, LimiterState, decibels};
use sound_core::{Changes, EngineConfig};

use crate::support::{Harness, SAMPLE_RATE, clip, id, note, probe_arrangement};

/// Ticks in a bar of 4/4.
const BAR: u64 = 3840;
/// Frames per tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

const ARRANGEMENT_FILE: &str = "state/arrangement/instance.json";

/// The ceiling of a limiter at its defaults, -0.3 dBFS.
fn ceiling() -> f32 {
    decibels::amplitude(LimiterState::default().ceiling_db)
}

/// A stereo project with a probe track of this scale that holds middle C for eight bars, and
/// then a G on top of it from `step` on: a level that jumps. The master is `master`.
fn project(scale: f32, step: u64, arrangement: ArrangementState) -> Harness {
    let mut harness = Harness::with_config(EngineConfig::new(SAMPLE_RATE, 2));
    let mut changes = Changes::new();
    changes.set(
        &harness
            .project
            .resolve::<ArrangementState>(&id("arrangement"))
            .unwrap(),
        arrangement,
    );
    harness.project.commit("Set the master", changes).unwrap();
    harness.add_track("piano", scale);
    let mut changes = Changes::new();
    let notes = vec![note(0, 8 * BAR, 60), note(step, 8 * BAR - step, 67)];
    changes.create(id("arrangement/piano/long"), clip(0, 8 * BAR, notes));
    harness.project.commit("Add clip", changes).unwrap();
    harness
}

fn with_limiter(change: impl FnOnce(&mut LimiterState)) -> ArrangementState {
    let mut arrangement = ArrangementState::default();
    change(&mut arrangement.master.limiter);
    arrangement
}

/// What the probe plays at a scale of 0.01: C, then C and G.
const QUIET: f32 = 0.01 * 60.0;
const LOUD: f32 = 0.01 * 127.0;

fn left(interleaved: &[f32]) -> Vec<f32> {
    interleaved.iter().step_by(2).copied().collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()))
}

#[test]
fn a_level_far_over_full_scale_comes_out_at_the_ceiling_and_never_above_it() {
    // 60, then 127: 35 and 41 dB over full scale.
    let mut harness = project(1.0, BAR, ArrangementState::default());
    let render = harness.play(2 * 2 * BAR as usize * TICK);
    assert!(peak(&render) <= ceiling(), "{}", peak(&render));
    // Held there, not pushed further down.
    assert_eq!(left(&render)[1_000], ceiling());
    assert_eq!(*left(&render).last().unwrap(), ceiling());
    // The meter of the master says the same as the render, exactly.
    let master = arrangement::master_peaks(&harness.project, &id("arrangement")).unwrap();
    assert_eq!(master.take(), [peak(&render); 2]);
    // The track meter shows what the track sends to the master, before the limiter.
    let track = arrangement::track_peaks(&harness.project, &id("arrangement/piano")).unwrap();
    assert_eq!(track.take(), [127.0; 2]);
    // And the reduction: 127 over the ceiling, as a factor.
    let reduction = arrangement::reduction_peaks(&harness.project, &id("arrangement")).unwrap();
    let factor = reduction.take()[0];
    assert!((factor - 127.0 / ceiling()).abs() < 1e-3, "{factor}");
}

#[test]
fn under_the_ceiling_the_limiter_leaves_every_sample_as_it_was() {
    // 0.6, then 1.27 over the first bar: under the ceiling until the G comes.
    let quiet = |arrangement| {
        let mut harness = project(0.01, 4 * BAR, arrangement);
        harness.play(2 * 2 * BAR as usize * TICK)
    };
    let on = quiet(ArrangementState::default());
    let off = quiet(probe_arrangement());
    let bar = 2 * BAR as usize * TICK;
    assert_eq!(on[..bar], off[..bar]);
    assert_eq!(left(&on)[100], QUIET);
    // With a lookahead the same, one lookahead later: the tracks are led by it, so after the
    // wait of a play the first frame is where it was.
    let ahead = with_limiter(|limiter| limiter.lookahead_ms = 5.0);
    let mut harness = project(0.01, 4 * BAR, ahead);
    let with_lookahead = harness.play(2 * bar);
    let wait = harness.engine.preroll_frames() as usize;
    assert_eq!(wait, 240);
    assert_eq!(with_lookahead[2 * wait..2 * wait + bar], off[..bar]);
}

#[test]
fn a_lookahead_lowers_the_gain_before_the_peak_and_says_it_as_latency() {
    let ahead = with_limiter(|limiter| limiter.lookahead_ms = 5.0);
    // 0.6, then 1.27 from bar 2.
    let mut harness = project(0.01, BAR, ahead);
    let render = left(&harness.play(2 * 2 * BAR as usize * TICK));
    let status = harness.project.engine().poll().unwrap();
    assert_eq!(status.latency, 240);
    assert!(peak(&render) <= ceiling(), "{}", peak(&render));
    let wait = harness.engine.preroll_frames() as usize;
    let step = wait + BAR as usize * TICK;
    // Untouched until one lookahead before the peak, then down along a straight line, so the
    // G arrives at the ceiling and the wave has no corner.
    assert_eq!(render[step - 241], QUIET);
    assert!(
        render[step - 1] < QUIET * (ceiling() / LOUD) + 1e-3,
        "{}",
        render[step - 1]
    );
    let steps = render[step - 240..step]
        .windows(2)
        .map(|pair| pair[0] - pair[1]);
    let largest = steps.fold(0.0_f32, f32::max);
    assert!(largest < QUIET / 240.0 * 1.01, "{largest}");
    assert!((render[step] - ceiling()).abs() < 1e-3, "{}", render[step]);

    // Without a lookahead, nothing is ahead and no latency: the G is cut down at its frame.
    let mut plain = project(0.01, BAR, ArrangementState::default());
    let render = left(&plain.play(2 * 2 * BAR as usize * TICK));
    assert_eq!(plain.project.engine().poll().unwrap().latency, 0);
    let step = BAR as usize * TICK;
    assert_eq!(render[step - 1], QUIET);
    assert_eq!(render[step], ceiling());
}

#[test]
fn a_bypassed_limiter_keeps_its_latency_and_lets_everything_through() {
    let bypassed = with_limiter(|limiter| {
        limiter.bypass = true;
        limiter.lookahead_ms = 5.0;
    });
    let mut harness = project(1.0, BAR, bypassed);
    let render = left(&harness.play(2 * BAR as usize * TICK));
    assert_eq!(harness.project.engine().poll().unwrap().latency, 240);
    let wait = harness.engine.preroll_frames() as usize;
    assert_eq!(render[wait], 60.0);
    assert_eq!(render[wait - 1], 0.0);
}

#[test]
fn the_ceiling_and_the_master_volume_from_a_file_apply_live_as_one_undo_step() {
    let mut harness = project(1.0, 7 * BAR, ArrangementState::default());
    harness.play(4_800);
    let record =
        r#"{"tool": "arrangement", "state": {"master": {"limiter": {"ceiling_db": -6.0}}}}"#;
    assert_eq!(harness.write_and_apply(ARRANGEMENT_FILE, record), 1);
    assert_eq!(harness.problems(), Vec::<String>::new());
    let render = left(&harness.render(2 * 4_800));
    let six_down = decibels::amplitude(-6.0);
    assert!(peak(&render) <= six_down);
    assert_eq!(*render.last().unwrap(), six_down);
    assert_eq!(harness.project.undo_label(), Some("File change"));

    // The volume of the master is before the limiter, so it cannot push the output over the
    // ceiling, and its bottom is silence.
    let record = r#"{"tool": "arrangement", "state": {"master": {"gain_db": "-inf"}}}"#;
    harness.write_and_apply(ARRANGEMENT_FILE, record);
    let render = left(&harness.render(2 * 4_800));
    assert_eq!(peak(&render[2_000..]), 0.0);

    harness.project.undo().unwrap();
    harness.project.undo().unwrap();
    let render = left(&harness.render(2 * 4_800));
    assert_eq!(*render.last().unwrap(), ceiling());
}

#[test]
fn a_master_value_out_of_range_names_the_field() {
    let mut harness = project(1.0, 7 * BAR, ArrangementState::default());
    let record =
        r#"{"tool": "arrangement", "state": {"master": {"limiter": {"ceiling_db": 3.0}}}}"#;
    harness.write_and_apply(ARRANGEMENT_FILE, record);
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/instance.json: state: master.limiter.ceiling_db must be from -24 to 0, not 3"
        ]
    );
}
