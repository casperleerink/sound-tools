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

/// The ceiling of a limiter at its defaults, full scale.
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
    // At the ceiling, to the last bit of rounding under it.
    assert!(ceiling() - render[step] < 1e-6, "{}", render[step]);
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

/// A stereo project at `sample_rate` with this master: a loud track whose one note ends at
/// tick 960, and a quiet track that holds a note all the time.
fn burst_then_quiet(sample_rate: u32, arrangement: ArrangementState) -> Harness {
    let mut harness = Harness::with_config(EngineConfig::new(sample_rate, 2));
    let mut changes = Changes::new();
    let master = harness
        .project
        .resolve::<ArrangementState>(&id("arrangement"));
    changes.set(&master.unwrap(), arrangement);
    harness.project.commit("Set the master", changes).unwrap();
    harness.add_track("loud", 1.0);
    harness.add_track("quiet", 0.001);
    let mut changes = Changes::new();
    changes.create(
        id("arrangement/loud/hit"),
        clip(0, BAR, vec![note(0, 960, 60)]),
    );
    let long = clip(0, 64 * BAR, vec![note(0, 64 * BAR, 60)]);
    changes.create(id("arrangement/quiet/long"), long);
    harness.project.commit("Add clips", changes).unwrap();
    harness
}

/// After a peak the gain comes back to exactly 1: the quiet track then comes out bit for bit
/// as it went in, and the limiter reports no reduction. An f32 envelope stopped just under 1
/// for good, which scaled every sample for the rest of the session.
#[test]
fn after_a_peak_the_gain_comes_back_to_exactly_one() {
    let quiet = 0.001_f32 * 60.0;
    for sample_rate in [44_100, 48_000, 96_000] {
        for release_ms in [10.0, 100.0, 1000.0] {
            for lookahead_ms in [0.0, 5.0] {
                let arrangement = with_limiter(|limiter| {
                    limiter.release_ms = release_ms;
                    limiter.lookahead_ms = lookahead_ms;
                });
                let mut harness = burst_then_quiet(sample_rate, arrangement);
                let case =
                    format!("{sample_rate} Hz, {release_ms} ms, lookahead {lookahead_ms} ms");
                // The hit, half a second, and ten times the release after it.
                let rate = sample_rate as f32;
                let frames = (rate * (0.5 + 0.1 + 10.0 * release_ms / 1000.0)) as usize;
                let hit = left(&harness.play(2 * frames));
                assert!(
                    peak(&hit) > 0.9,
                    "{case}: the hit was not limited: {}",
                    peak(&hit)
                );
                let reduction =
                    arrangement::reduction_peaks(&harness.project, &id("arrangement")).unwrap();
                assert!(reduction.take()[0] > 1.0, "{case}: no reduction at the hit");
                let after = left(&harness.render(2 * 4_800));
                assert!(
                    after.iter().all(|sample| *sample == quiet),
                    "{case}: {:?}",
                    after.iter().find(|sample| **sample != quiet)
                );
                assert_eq!(reduction.take(), [0.0, 0.0], "{case}");
            }
        }
    }
}

/// Not a number and infinity are no sound: they come out as silence, never as full scale, and
/// the limiter plays on as before once they are gone.
#[test]
fn not_a_number_and_infinity_come_out_as_silence() {
    let mut harness = burst_then_quiet(SAMPLE_RATE, ArrangementState::default());
    let quiet = 0.001_f32 * 60.0;
    // An effect that makes the quiet track infinite, and then not a number: the largest gain
    // plus the largest offset overflows, and the tail of infinity times 0 is not a number.
    let mut changes = Changes::new();
    let track = harness
        .project
        .resolve::<arrangement::TrackState>(&id("arrangement/quiet"))
        .unwrap();
    let slot = arrangement::add_effect(&harness.project, &mut changes, &track, "poison").unwrap();
    changes.create(slot, crate::support::Trim::new(f32::MAX, f32::MAX));
    harness.project.commit("Add poison", changes).unwrap();
    let render = left(&harness.play(2 * 48_000));
    let late = &render[30_000..];
    assert!(
        late.iter().all(|sample| *sample == 0.0),
        "{:?}",
        late.iter().find(|s| **s != 0.0)
    );
    // Gone: the quiet track bit for bit.
    let record = r#"{"tool": "arrangement.track", "state": {"name": "quiet", "order": 1}}"#;
    harness.write_and_apply("state/arrangement/quiet/instance.json", record);
    let render = left(&harness.render(2 * 4_800));
    assert!(render[1_000..].iter().all(|sample| *sample == quiet));
}

/// The ceiling holds while the ceiling, the bypass and the lookahead change during playback,
/// at two sample rates and three lookaheads.
#[test]
fn the_ceiling_holds_through_edits_while_the_project_plays() {
    for sample_rate in [44_100, 96_000] {
        for lookahead_ms in [0.0, 5.0, 10.0] {
            let case = format!("{sample_rate} Hz, lookahead {lookahead_ms} ms");
            let ahead = with_limiter(|limiter| limiter.lookahead_ms = lookahead_ms);
            let mut harness = Harness::with_config(EngineConfig::new(sample_rate, 2));
            let mut changes = Changes::new();
            let master = harness
                .project
                .resolve::<ArrangementState>(&id("arrangement"));
            changes.set(&master.unwrap(), ahead.clone());
            harness.project.commit("Set the master", changes).unwrap();
            harness.add_track("piano", 0.02);
            // A level that jumps every eighth: 1.2, 2.5, 1.2, ...
            let notes = (0..32)
                .map(|step| note(step * 480, 480, if step % 2 == 0 { 60 } else { 125 }))
                .collect();
            let mut changes = Changes::new();
            changes.create(id("arrangement/piano/steps"), clip(0, 8 * BAR, notes));
            harness.project.commit("Add clip", changes).unwrap();
            let chunk = 2 * sample_rate as usize / 5;
            let first = harness.play(chunk);
            assert!(peak(&first) <= 1.0, "{case}: {}", peak(&first));

            let edit = |harness: &mut Harness, change: &dyn Fn(&mut LimiterState)| {
                let mut arrangement = ahead.clone();
                change(&mut arrangement.master.limiter);
                let state = serde_json::to_string(&arrangement).unwrap();
                let record = format!(r#"{{"tool": "arrangement", "state": {state}}}"#);
                harness.write_and_apply(ARRANGEMENT_FILE, &record);
                harness.render(chunk)
            };
            let lower = edit(&mut harness, &|limiter| limiter.ceiling_db = -6.0);
            let six_down = decibels::amplitude(-6.0);
            assert!(peak(&lower) <= six_down, "{case}: {}", peak(&lower));
            let off = edit(&mut harness, &|limiter| {
                limiter.ceiling_db = -6.0;
                limiter.bypass = true;
            });
            assert!(peak(&off) > 2.0, "{case}: bypassed, the level passes");
            let on = edit(&mut harness, &|limiter| limiter.ceiling_db = -6.0);
            assert!(peak(&on) <= six_down, "{case}: {}", peak(&on));
            let raised = edit(&mut harness, &|limiter| limiter.ceiling_db = -1.0);
            assert!(peak(&raised) <= decibels::amplitude(-1.0), "{case}");
            assert_eq!(harness.problems(), Vec::<String>::new(), "{case}");
        }
    }
}
