//! What the card shows comes from the audio thread: the level the compressor hears and how
//! much it turns down, as the largest of each block since the card last looked.

use compressor::CompressorState;

use crate::support::{Rig, SAMPLE_RATE, sine};

#[test]
fn the_meters_give_the_level_and_the_reduction_since_the_last_look() {
    let state = CompressorState {
        threshold_db: -20.0,
        ratio: 4.0,
        knee_db: 0.0,
        ..CompressorState::default()
    };
    let mut rig = Rig::new(state, sine(1_000.0, 0.5));
    rig.render(SAMPLE_RATE as usize);
    let [level, _] = rig.meters.level.take();
    let [reduction, _] = rig.meters.reduction.take();
    // A sine of 0.5 is -6.02 dB, 13.98 dB over the threshold: three quarters of that off.
    assert_eq!(level, 0.5);
    let expected = 0.75 * (20.0 - 20.0 * 2.0_f32.log10());
    assert!((reduction - expected).abs() < 1e-4, "{reduction}");
    // Taken: nothing until the next block.
    assert_eq!(rig.meters.level.take(), [0.0; 2]);
    rig.render(480);
    assert_eq!(rig.meters.level.take()[0], 0.5);
}
