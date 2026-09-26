//! Every change glides: an edit of any field while a tone plays makes no step in the sound
//! larger than the tone itself takes, where a switch would jump by a large part of its level.

use compressor::{CompressorState, Lookahead};

use crate::support::{Rig, SAMPLE_RATE, largest_step, sine};

const HZ: f64 = 440.0;
const AMPLITUDE: f32 = 0.5;

/// The largest step of the output around an edit from `before` to `after`, against the largest
/// step of the steady output before and after it. A sine's step is its amplitude times
/// `2π f / sample rate`; a click is many times that.
fn step_ratio(before: CompressorState, after: CompressorState) -> f32 {
    let mut rig = Rig::new(before, sine(HZ, AMPLITUDE));
    let [settled, _] = rig.render(SAMPLE_RATE as usize);
    let steady_before = largest_step(&settled[settled.len() - 4_800..]);
    rig.update(after);
    let [around, _] = rig.render(SAMPLE_RATE as usize);
    let steady_after = largest_step(&around[around.len() - 4_800..]);
    let mut edit = settled[settled.len() - 1..].to_vec();
    edit.extend(&around[..4_800]);
    largest_step(&edit) / steady_before.max(steady_after)
}

#[test]
fn no_edit_steps_the_sound() {
    // A fast attack, so the reduction follows the threshold as fast as it can.
    let base = CompressorState {
        threshold_db: -18.0,
        attack_ms: 0.1,
        release_ms: 1.0,
        ..CompressorState::default()
    };
    let edits = [
        (
            "threshold down",
            CompressorState {
                threshold_db: -60.0,
                ..base
            },
        ),
        (
            "threshold up",
            CompressorState {
                threshold_db: 0.0,
                ..base
            },
        ),
        (
            "ratio",
            CompressorState {
                ratio: 100.0,
                ..base
            },
        ),
        (
            "knee",
            CompressorState {
                knee_db: 18.0,
                ..base
            },
        ),
        (
            "makeup",
            CompressorState {
                makeup_db: 24.0,
                ..base
            },
        ),
        ("mix", CompressorState { mix: 0.0, ..base }),
        (
            "attack and release",
            CompressorState {
                attack_ms: 300.0,
                release_ms: 3_000.0,
                ..base
            },
        ),
        (
            "lookahead",
            CompressorState {
                lookahead: Lookahead::Ten,
                ..base
            },
        ),
    ];
    for (name, after) in edits {
        let ratio = step_ratio(base, after);
        println!("{name}: largest step {ratio:.2} times the steady one");
        assert!(ratio < 1.5, "{name}: {ratio}");
    }
}

/// Two changes of the lookahead 10 ms apart, while the first still fades: the second waits for
/// the first to end, so neither jumps.
#[test]
fn a_lookahead_change_during_the_fade_of_another_does_not_step_the_sound() {
    let base = CompressorState::default();
    let mut rig = Rig::new(base, sine(HZ, AMPLITUDE));
    let [settled, _] = rig.render(SAMPLE_RATE as usize);
    let steady = largest_step(&settled[settled.len() - 4_800..]);
    rig.update(CompressorState {
        lookahead: Lookahead::Ten,
        ..base
    });
    let [first, _] = rig.render(480);
    rig.update(CompressorState {
        lookahead: Lookahead::One,
        ..base
    });
    let [second, _] = rig.render(SAMPLE_RATE as usize / 10);
    let mut around = settled[settled.len() - 1..].to_vec();
    around.extend(&first);
    around.extend(&second);
    let ratio = largest_step(&around) / steady;
    println!("two lookahead changes 10 ms apart: largest step {ratio:.2} times the steady one");
    assert!(ratio < 1.5, "{ratio}");
}
