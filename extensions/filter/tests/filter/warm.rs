//! The warm character: a quiet sound passes as through the clean filter, a hot one bends at
//! the cutoff, which compresses the peak and adds odd harmonics, and full resonance rings on
//! its own at a steady level and in tune wherever the cutoff is.

use filter::{FilterCharacter, FilterState, Slope};

use crate::support::{Rig, SAMPLE_RATE, Signal, amplitude_at, noise, peak, sine};

/// A tone at the cutoff at resonance 0.5, once it has settled: its gain and its third harmonic
/// against it, in dB.
fn tone_at_the_cutoff(character: FilterCharacter, slope: Slope, amplitude: f32) -> (f64, f64) {
    let state = FilterState {
        cutoff_hz: 1_000.0,
        resonance: 0.5,
        slope,
        character,
        ..FilterState::default()
    };
    let mut rig = Rig::new(state, sine(1_000.0, amplitude));
    rig.render(SAMPLE_RATE as usize / 4);
    // A tenth of a second: whole cycles of the tone and of its harmonics.
    let [left, _] = rig.render(SAMPLE_RATE as usize / 10);
    let level = |hz| amplitude_at(&left, 0, hz, SAMPLE_RATE);
    let tone = level(1_000.0);
    let db = |ratio: f64| 20.0 * ratio.log10();
    (db(tone / f64::from(amplitude)), db(level(3_000.0) / tone))
}

/// At -24 dBFS warm is the clean filter. At -6 dBFS the clean filter is as linear as ever, and
/// the warm one gives the peak less gain and adds a third harmonic you hear.
#[test]
fn warm_is_clean_when_quiet_and_bends_a_hot_peak_at_moderate_resonance() {
    for slope in Slope::ALL {
        let quiet = |character| tone_at_the_cutoff(character, slope, 0.063);
        let hot = |character| tone_at_the_cutoff(character, slope, 0.5);
        let ((clean, _), (warm, warm_third)) =
            (quiet(FilterCharacter::Clean), quiet(FilterCharacter::Warm));
        println!(
            "{slope:?} at -24 dBFS: clean {clean:.2} dB, warm {warm:.2} dB, third {warm_third:.0} dB"
        );
        assert!((warm - clean).abs() < 0.6, "{slope:?}: {clean} {warm}");
        assert!(warm_third < -50.0, "{slope:?}: {warm_third}");
        let ((clean, clean_third), (warm, warm_third)) =
            (hot(FilterCharacter::Clean), hot(FilterCharacter::Warm));
        println!(
            "{slope:?} at -6 dBFS: gain clean {clean:.2} dB, warm {warm:.2} dB; third clean {clean_third:.0} dB, warm {warm_third:.0} dB"
        );
        assert!(clean_third < -90.0, "{slope:?}: {clean_third}");
        assert!(warm < clean - 4.0, "{slope:?}: {clean} {warm}");
        assert!(warm_third > -55.0, "{slope:?}: {warm_third}");
    }
}

/// A short burst of noise, then silence.
fn burst() -> Signal {
    let mut frames = 0_usize;
    let mut sound = noise(0.5);
    Box::new(move || {
        frames += 1;
        if frames <= 480 { sound() } else { [0.0; 2] }
    })
}

/// Full resonance after a burst: warm rings on at one level while the LFO sweeps the cutoff
/// over four octaves, from 250 Hz to 4 kHz; clean dies away.
#[test]
fn full_warm_resonance_rings_on_at_a_steady_level_while_the_cutoff_sweeps() {
    for slope in Slope::ALL {
        let state = |character| FilterState {
            cutoff_hz: 1_000.0,
            resonance: 1.0,
            slope,
            character,
            lfo_rate_hz: 0.25,
            lfo_depth_octaves: 2.0,
            ..FilterState::default()
        };
        let mut warm = Rig::new(state(FilterCharacter::Warm), burst());
        warm.render(SAMPLE_RATE as usize);
        let [ring, _] = warm.render(4 * SAMPLE_RATE as usize);
        // The peak of every 50 ms over a whole cycle of the LFO.
        let peaks: Vec<f32> = ring.chunks(SAMPLE_RATE as usize / 20).map(peak).collect();
        let (lowest, highest) = peaks.iter().fold((f32::MAX, 0.0_f32), |(low, high), peak| {
            (low.min(*peak), high.max(*peak))
        });
        let swing = 20.0 * (highest / lowest).log10();
        println!("{slope:?}: warm rings at {lowest:.3} to {highest:.3}, {swing:.2} dB apart");
        assert!(lowest > 0.1, "{slope:?}: {lowest}");
        assert!(swing < 0.5, "{slope:?}: {swing} dB");
        let mut clean = Rig::new(state(FilterCharacter::Clean), burst());
        clean.render(SAMPLE_RATE as usize);
        let [ring, _] = clean.render(SAMPLE_RATE as usize);
        assert!(peak(&ring) < 1e-3, "{slope:?}: {}", peak(&ring));
    }
}

/// The bend slows the ring a little, the more the louder it rings: it stays within half a
/// semitone of the cutoff, from the bass to the top.
#[test]
fn full_warm_resonance_rings_in_tune_with_the_cutoff() {
    for cutoff_hz in [100.0, 1_000.0, 5_000.0] {
        let state = FilterState {
            cutoff_hz,
            resonance: 1.0,
            character: FilterCharacter::Warm,
            ..FilterState::default()
        };
        let mut rig = Rig::new(state, burst());
        rig.render(2 * SAMPLE_RATE as usize);
        let [ring, _] = rig.render(SAMPLE_RATE as usize);
        let rising: Vec<usize> = (1..ring.len())
            .filter(|frame| ring[frame - 1] < 0.0 && ring[*frame] >= 0.0)
            .collect();
        let cycles = (rising.len() - 1) as f32;
        let hz = cycles * SAMPLE_RATE as f32 / (rising[rising.len() - 1] - rising[0]) as f32;
        let semitones = 12.0 * (hz / cutoff_hz).log2();
        println!(
            "{cutoff_hz} Hz: rings at {hz:.1} Hz, {semitones:+.2} semitones, peak {:.3}",
            peak(&ring)
        );
        assert!(semitones.abs() < 0.5, "{cutoff_hz}: {semitones}");
    }
}
