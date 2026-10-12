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

/// The SVF is linear: without drive it adds nothing out of tune. The driven numbers are
/// printed, for comparing an oversampled drive with this one.
#[test]
fn without_drive_nothing_folds_back() {
    for bin in BINS {
        let clean = out_of_tune_db(0.0, bin);
        assert!(clean < -100.0, "{bin}: {clean}");
        for drive_db in [12.0, 24.0] {
            let driven = out_of_tune_db(drive_db, bin);
            println!(
                "{} Hz at {drive_db} dB: out of tune {driven:.1} dB",
                bin * 10
            );
        }
    }
}
