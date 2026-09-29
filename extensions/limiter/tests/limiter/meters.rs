//! What the card shows comes from the audio thread: the peaks of what the limiter sends out,
//! and how much it took, as the largest of each block since the card last looked.

use limiter::{LimiterState, Lookahead};

use crate::support::{Rig, SAMPLE_RATE, amplitude, peak, steady};

#[test]
fn the_meters_give_the_output_and_the_reduction_since_the_last_look() {
    let state = LimiterState {
        lookahead: Lookahead::Off,
        ..LimiterState::default()
    };
    let mut rig = Rig::new(state, steady(2.0));
    let [left, _] = rig.render(SAMPLE_RATE as usize / 10);
    let ceiling = amplitude(state.ceiling_db);
    assert_eq!(rig.meters.output.take(), [peak(&left); 2]);
    // 2 over the ceiling: the factor the sound was above what came out.
    let [reduction, _] = rig.meters.reduction.take();
    assert!((reduction - 2.0 / ceiling).abs() < 1e-4, "{reduction}");
    // Taken: nothing until the next block.
    assert_eq!(rig.meters.output.take(), [0.0; 2]);
    rig.render(480);
    assert_eq!(rig.meters.output.take(), [ceiling; 2]);
}

/// A limiter that hears only silence comes to rest and reports nothing.
#[test]
fn silence_reports_nothing() {
    let mut rig = Rig::new(LimiterState::default(), steady(0.0));
    let [left, _] = rig.render(4_800);
    assert!(left.iter().all(|sample| *sample == 0.0));
    assert_eq!(rig.meters.output.take(), [0.0; 2]);
    assert_eq!(rig.meters.reduction.take(), [0.0; 2]);
}
