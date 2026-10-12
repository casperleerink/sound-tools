//! The warm character: a resonance that compresses as the sound gets louder, where the clean
//! filter's grows with it, and full resonance that rings on its own at a steady level wherever
//! the cutoff is.

use filter::{FilterCharacter, FilterState, Slope};

use crate::support::{Rig, SAMPLE_RATE, Signal, noise, peak, sine};

/// The peak of the steady output for a sine at the cutoff, at a resonance near the top.
fn peak_at_the_cutoff(character: FilterCharacter, slope: Slope, amplitude: f32) -> f32 {
    let state = FilterState {
        cutoff_hz: 1_000.0,
        resonance: 0.9,
        slope,
        character,
        ..FilterState::default()
    };
    let mut rig = Rig::new(state, sine(1_000.0, amplitude));
    let [left, _] = rig.render(SAMPLE_RATE as usize / 2);
    peak(&left[SAMPLE_RATE as usize / 4..])
}

/// 20 dB more into a ringing peak: the clean filter gives 20 dB more, the warm one far less.
#[test]
fn a_loud_resonance_compresses_when_warm_and_not_when_clean() {
    let db = |ratio: f32| 20.0 * ratio.log10();
    for slope in Slope::ALL {
        let rise = |character| {
            let quiet = peak_at_the_cutoff(character, slope, 0.05);
            let loud = peak_at_the_cutoff(character, slope, 0.5);
            (db(loud / quiet), loud)
        };
        let ((clean, clean_peak), (warm, warm_peak)) =
            (rise(FilterCharacter::Clean), rise(FilterCharacter::Warm));
        println!(
            "{slope:?}: clean rises {clean:.1} dB to {clean_peak:.2}, warm {warm:.1} dB to {warm_peak:.2}"
        );
        assert!((clean - 20.0).abs() < 0.1, "{slope:?}: {clean}");
        assert!(warm < 12.0, "{slope:?}: {warm}");
        assert!(
            warm_peak < 0.6 * clean_peak,
            "{slope:?}: {warm_peak} {clean_peak}"
        );
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
        assert!(lowest > 0.15, "{slope:?}: {lowest}");
        assert!(swing < 0.5, "{slope:?}: {swing} dB");
        let mut clean = Rig::new(state(FilterCharacter::Clean), burst());
        clean.render(SAMPLE_RATE as usize);
        let [ring, _] = clean.render(SAMPLE_RATE as usize);
        assert!(peak(&ring) < 1e-3, "{slope:?}: {}", peak(&ring));
    }
}
