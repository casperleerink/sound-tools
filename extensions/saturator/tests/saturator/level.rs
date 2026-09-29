//! The drive changes the colour and not the level: a sine at -12 dBFS keeps its level at every
//! drive of every curve. Its peak stays where it was, and the tone grows by at most the
//! 2.1 dB that a square wave's first harmonic has over its peak.

use saturator::{Curve, DRIVE, SaturatorState};

use crate::support::{Rig, SAMPLE_RATE, amplitude_at, peak, sine};

#[test]
fn a_sine_at_minus_twelve_dbfs_keeps_its_level_at_every_drive() {
    let amplitude = 0.25;
    for curve in Curve::ALL {
        for drive_db in [0.0, 6.0, 12.0, 18.0, 24.0, 30.0, DRIVE.max] {
            let state = SaturatorState {
                curve,
                drive_db,
                ..SaturatorState::default()
            };
            let mut rig = Rig::new(state, sine(1_000.0, amplitude));
            let settle = SAMPLE_RATE as usize / 2;
            rig.render(settle);
            let [left, _] = rig.render(4_800);
            let db = |value: f64| 20.0 * (value / f64::from(amplitude)).log10();
            let tone = db(amplitude_at(&left, settle, 1_000.0, SAMPLE_RATE));
            let loudest = db(f64::from(peak(&left)));
            println!("{curve:?} at {drive_db} dB: tone {tone:+.2} dB, peak {loudest:+.2} dB");
            assert!((-0.1..2.2).contains(&tone), "{curve:?} {drive_db}: {tone}");
            assert!(loudest.abs() < 1.0, "{curve:?} {drive_db}: {loudest}");
        }
    }
}
