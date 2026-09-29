//! A change of the gain glides over 20 ms: it makes no step in the sound larger than the tone
//! itself takes, where a switch would jump by a large part of its level.

use limiter::{GAIN, LimiterState, Lookahead};

use crate::support::{Rig, SAMPLE_RATE, largest_step, sine};

#[test]
fn a_change_of_the_gain_glides() {
    // A quiet tone, so the whole change of the gain is heard and none of it is limited.
    let before = LimiterState {
        lookahead: Lookahead::Off,
        ..LimiterState::default()
    };
    let mut rig = Rig::new(before, sine(440.0, 0.05));
    let [settled, _] = rig.render(SAMPLE_RATE as usize / 10);
    rig.update(LimiterState {
        gain_db: GAIN.max,
        ..before
    });
    let [around, _] = rig.render(SAMPLE_RATE as usize / 10);
    let steady = largest_step(&around[around.len() - 4_800..]);
    let mut edit = settled[settled.len() - 1..].to_vec();
    edit.extend(&around[..4_800]);
    // Up by 24 dB, and no step of it larger than the loud tone takes by itself.
    assert!(
        largest_step(&edit) <= steady * 1.01,
        "{} {steady}",
        largest_step(&edit)
    );
    // Arrived after 20 ms: the tone is 24 dB louder.
    let ratio = steady / largest_step(&settled);
    assert!((20.0 * ratio.log10() - 24.0).abs() < 0.1, "{ratio}");
}
