//! What folds back from above the Nyquist frequency: bright tones at -6 dBFS through an open
//! low pass. Each tone is a prime number of cycles in a window of 0.1 s, see [`Spectrum`].

use filter::{FilterState, FilterType};
use sound_core::testing::Spectrum;

use crate::support::{Rig, SAMPLE_RATE, sine};

const WINDOW: usize = SAMPLE_RATE as usize / 10;
const BIN_HZ: f64 = 10.0;
/// About 5, 10, 15 and 18 kHz.
const BINS: [usize; 4] = [499, 997, 1499, 1801];

fn out_of_tune_db(drive_db: f32, bin: usize) -> f64 {
    let state = FilterState {
        kind: FilterType::LowPass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        drive_db,
        ..FilterState::default()
    };
    let mut rig = Rig::new(state, sine(bin as f64 * BIN_HZ, 0.5));
    rig.render(SAMPLE_RATE as usize);
    let [left, _] = rig.render(WINDOW);
    Spectrum::of(&left, bin, SAMPLE_RATE).out_of_tune_db
}

/// The SVF is linear: without drive it adds nothing out of tune. The drive runs at four times
/// the rate, so most of what it makes above the Nyquist frequency is gone before it can fold
/// back: at the sample rate the same drive folded back -39 to -19 dB at 12 dB, and -18 to -8
/// dB at 24.
#[test]
fn without_drive_nothing_folds_back_and_the_drive_folds_back_little() {
    // The most each tone may fold back at 12 and 24 dB of drive, a few dB over what it does.
    const MOST: [[f64; 2]; 4] = [
        [-90.0, -54.0],
        [-72.0, -40.0],
        [-64.0, -30.0],
        [-46.0, -24.0],
    ];
    for (bin, most) in BINS.into_iter().zip(MOST) {
        let clean = out_of_tune_db(0.0, bin);
        assert!(clean < -100.0, "{bin}: {clean}");
        for (drive_db, most) in [12.0, 24.0].into_iter().zip(most) {
            let driven = out_of_tune_db(drive_db, bin);
            println!(
                "{} Hz at {drive_db} dB: out of tune {driven:.1} dB",
                bin * 10
            );
            assert!(driven < most, "{bin} at {drive_db} dB: {driven}");
        }
    }
}
