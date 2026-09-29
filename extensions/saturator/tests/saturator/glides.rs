//! Every change glides: an edit of any field while a tone plays makes no step in the sound
//! larger than the tone itself takes, where a switch would jump by a large part of its level.

use saturator::{Curve, LATENCY, SaturatorState};

use crate::support::{Rig, SAMPLE_RATE, largest_step, sine};

const HZ: f64 = 440.0;
const AMPLITUDE: f32 = 0.5;

/// The largest step of the output around an edit from `before` to `after`, against the largest
/// step of the steady output before and after it. A sine's step is its amplitude times
/// `2π f / sample rate`; a click is many times that.
fn step_ratio(before: SaturatorState, after: SaturatorState) -> f32 {
    let mut rig = Rig::new(before, sine(HZ, AMPLITUDE));
    let [settled, _] = rig.render(SAMPLE_RATE as usize / 2);
    let steady_before = largest_step(&settled[settled.len() - 4_800..]);
    rig.update(after);
    let [around, _] = rig.render(SAMPLE_RATE as usize / 2);
    let steady_after = largest_step(&around[around.len() - 4_800..]);
    let mut edit = settled[settled.len() - 1..].to_vec();
    edit.extend(&around[..4_800]);
    largest_step(&edit) / steady_before.max(steady_after)
}

#[test]
fn no_edit_steps_the_sound() {
    let base = SaturatorState {
        drive_db: 12.0,
        ..SaturatorState::default()
    };
    let edits = [
        (
            "drive up",
            SaturatorState {
                drive_db: 36.0,
                ..base
            },
        ),
        (
            "drive down",
            SaturatorState {
                drive_db: 0.0,
                ..base
            },
        ),
        (
            "tube",
            SaturatorState {
                curve: Curve::Tube,
                ..base
            },
        ),
        (
            "clip",
            SaturatorState {
                curve: Curve::Clip,
                ..base
            },
        ),
        (
            "tone up",
            SaturatorState {
                tone_db: 12.0,
                ..base
            },
        ),
        (
            "tone down",
            SaturatorState {
                tone_db: -12.0,
                ..base
            },
        ),
        (
            "output",
            SaturatorState {
                output_db: 12.0,
                ..base
            },
        ),
        ("mix", SaturatorState { mix: 0.0, ..base }),
    ];
    for (name, after) in edits {
        let ratio = step_ratio(base, after);
        println!("{name}: largest step {ratio:.2} times the steady one");
        assert!(ratio < 1.5, "{name}: {ratio}");
    }
}

/// A glide of the mix is done in 20 ms: after it the output is the dry sound, the latency
/// late, to the bit.
#[test]
fn a_glide_takes_twenty_milliseconds() {
    let base = SaturatorState {
        drive_db: 24.0,
        ..SaturatorState::default()
    };
    let mut rig = Rig::new(base, sine(HZ, AMPLITUDE));
    let settle = SAMPLE_RATE as usize / 2;
    rig.render(settle);
    rig.update(SaturatorState { mix: 0.0, ..base });
    let [output, _] = rig.render(SAMPLE_RATE as usize / 10);
    let mut input = sine(HZ, AMPLITUDE);
    let input: Vec<f32> = (0..settle + output.len()).map(|_| input()[0]).collect();
    let dry = &input[settle - LATENCY as usize..];
    assert_eq!(output[960..], dry[960..output.len()]);
    assert_ne!(output[..480], dry[..480]);
}
