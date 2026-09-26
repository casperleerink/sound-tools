//! Freeze holds the tail as it is: for a minute, without growing, without letting new sound in,
//! and with every sample a number. Off again, the tail dies away in the decay time.

use reverb::ReverbState;

use crate::support::{Rig, SAMPLE_RATE, db, noise, plain, rms};

const SECOND: usize = SAMPLE_RATE as usize;

/// Every sample is a number and none is subnormal, which would cost time on some processors
/// and says that something decays where nothing should.
fn assert_clean(samples: &[f32]) {
    for sample in samples {
        assert!(*sample == 0.0 || sample.is_normal(), "{sample}");
    }
}

#[test]
fn freeze_holds_the_tail_for_a_minute_without_growing_or_letting_sound_in() {
    let state = plain(2.0);
    // Loud noise goes on playing the whole time: freeze keeps it out.
    let mut rig = Rig::new(state, noise(0.5));
    rig.render(SECOND);
    let frozen = ReverbState {
        freeze: true,
        ..state
    };
    rig.update(frozen);
    // The first 100 ms hold the glide into freeze.
    let [left, right] = rig.render(SECOND);
    let held = rms(&left[SECOND / 10..]).hypot(rms(&right[SECOND / 10..]));
    let mut levels = Vec::new();
    for second in 2..=60 {
        let [left, right] = rig.render(SECOND);
        assert_clean(&left);
        assert_clean(&right);
        let level = db(rms(&left).hypot(rms(&right)) / held);
        levels.push(level);
        if second == 30 {
            // A change of size while frozen fades to other taps of the same lines: it may
            // lose a little, and never adds.
            rig.update(ReverbState {
                size: 1.0,
                ..frozen
            });
        }
    }
    let (lowest, highest) = levels
        .iter()
        .fold((f64::MAX, f64::MIN), |(low, high), level| (low.min(*level), high.max(*level)));
    println!("held {held:.4}: from {lowest:+.2} dB to {highest:+.2} dB over a minute");
    assert!(highest < 0.5, "{levels:?}");
    assert!(lowest > -3.0, "{levels:?}");
    assert!(levels[..28].iter().all(|level| level.abs() < 0.5), "{levels:?}");
}

/// A frozen reverb with silence coming in holds a tail for a minute as well: freeze does not
/// depend on sound coming in, and the tail does not fade to subnormal numbers.
#[test]
fn a_frozen_tail_with_silence_coming_in_stays_where_it_was() {
    let mut rig = Rig::new(plain(5.0), crate::support::burst(0.5, SECOND / 2));
    rig.render(SECOND);
    rig.update(ReverbState {
        freeze: true,
        ..plain(5.0)
    });
    let [first, _] = rig.render(SECOND);
    let held = rms(&first[SECOND / 10..]);
    rig.render(58 * SECOND);
    let [last, _] = rig.render(SECOND);
    assert_clean(&last);
    let change = db(rms(&last) / held);
    println!("frozen with silence in: {change:+.3} dB after a minute");
    assert!(change.abs() < 0.5, "{change}");

    // Off again: the tail falls by 60 dB in the decay time, 5 s, and on to silence.
    rig.update(plain(5.0));
    rig.render(5 * SECOND);
    let [after, _] = rig.render(SECOND);
    let fallen = db(rms(&after) / held);
    println!("5 to 6 s after freeze: {fallen:.1} dB");
    assert!(fallen < -58.0, "{fallen}");
    assert_clean(&after);
}
