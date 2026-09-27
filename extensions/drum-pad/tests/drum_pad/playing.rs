//! Every pad sounds from its note, as the kit made it, and what a pad is set to is what is
//! measured: volume, pitch, decay, pan and velocity.

use drum_pad::{DrumPadState, KIT, PADS, Pad, Sound, Source, pad_gains};

use crate::support::{
    Harness, SAMPLE_RATE, centroid, decibels, hit, last_above, note, one_hit, peak,
    rising_zero_crossings, spectrum,
};

/// The bands of the fingerprint of a sound, in Hz.
const EDGES: [f32; 7] = [100.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0];

/// The part of the power of a sound in the bands from `from` (an index of the bands) up.
fn from_band(bands: &[f32], from: usize) -> f32 {
    bands[from..].iter().sum()
}

#[test]
fn every_pad_sounds_from_its_note_with_the_level_and_the_spectrum_of_its_sound() {
    println!(
        "pad  note  name        peak L  peak R   <100  -250  -500   -1k   -2k   -4k   -8k   8k+   centroid"
    );
    for pad in 0..PADS {
        let kit = &KIT[pad];
        let seconds = f64::from(kit.decay_ms) / 1000.0 + 0.1;
        let sound = one_hit(pad, 127, DrumPadState::default(), seconds);
        let pad_state = Pad::default_at(pad);
        let [left_gain, right_gain] = pad_gains(&pad_state);

        // The loudest sample of each side is the level of the sound times the gain of the
        // side. The level is set on the loudest sample of both sides of the render, so the
        // louder side of a wide sound has it, less the fade over the decay where it is: within
        // 0.01 dB.
        let level = kit.sound.level();
        let (left, right) = (peak(&sound.left), peak(&sound.right));
        let louder = (left / left_gain).max(right / right_gain);
        assert!(
            (louder / level - 1.0).abs() < 1e-3,
            "{}: {louder} {level}",
            kit.name
        );

        // Silent after its decay, and sounding up to near it.
        let decay_frames = (f64::from(kit.decay_ms) / 1000.0 * f64::from(SAMPLE_RATE)) as usize;
        let mono = sound.sum();
        assert!(
            mono[decay_frames..].iter().all(|sample| *sample == 0.0),
            "{}",
            kit.name
        );
        let heard = last_above(&mono, -60.0);
        assert!(
            heard > decay_frames / 3,
            "{}: heard to {heard} of {decay_frames}",
            kit.name
        );

        let frames = (SAMPLE_RATE / 10) as usize;
        let bands = spectrum(&mono, &EDGES, frames);
        let centre = centroid(&mono, frames);
        println!(
            "{pad:3}  {:4}  {:10}  {left:.3}   {right:.3}   {}   {centre:6.0} Hz",
            note(pad),
            kit.name,
            bands
                .iter()
                .map(|part| format!("{:5.1}", part * 100.0))
                .collect::<Vec<_>>()
                .join(" ")
        );

        // The fingerprint of each sound: where its power is.
        match kit.sound {
            // Almost all of it under 250 Hz, and most of that under 100 Hz.
            Sound::Kick => {
                assert!(bands[0] + bands[1] > 0.95 && bands[0] > 0.5, "{bands:?}");
            }
            // A tone under 500 Hz, the drum, and the snares above 2 kHz.
            Sound::Snare => {
                assert!(bands[1] + bands[2] > 0.5, "{bands:?}");
                assert!(from_band(&bands, 5) > 0.08, "{bands:?}");
            }
            // Noise around 1 kHz, from 500 Hz to 4 kHz.
            Sound::Clap => {
                assert!(bands[3] + bands[4] + bands[5] > 0.75, "{bands:?}");
            }
            // The ring from 250 Hz to 2 kHz.
            Sound::Rim => {
                assert!(bands[2] + bands[3] + bands[4] > 0.8, "{bands:?}");
            }
            // The metal: nearly everything above 4 kHz.
            Sound::Hat | Sound::OpenHat | Sound::Crash => {
                assert!(from_band(&bands, 6) > 0.8, "{}: {bands:?}", kit.name);
            }
            // The wash of metal above 4 kHz, and the ping of the bell from 500 Hz to 1 kHz.
            Sound::Ride => {
                assert!(from_band(&bands, 6) > 0.5, "{bands:?}");
                assert!(bands[3] > 0.1, "{bands:?}");
            }
            // A tone under 250 Hz.
            Sound::Tom => assert!(bands[0] + bands[1] > 0.95, "{bands:?}"),
        }
    }
}

#[test]
fn a_note_outside_the_pads_sounds_nothing() {
    let notes = vec![hit(0, 35, 127), hit(0, 52, 127), hit(0, 60, 127)];
    let mut harness = Harness::with_track(notes, DrumPadState::default());
    let sound = harness.play(24_000);
    assert_eq!(peak(&sound.sum()), 0.0);
}

/// The toms are one sound tuned apart, lowest on Tom 1, and each pitch is what the pad says.
#[test]
fn the_toms_are_tuned_apart_by_their_pitch() {
    let toms: Vec<usize> = (0..PADS)
        .filter(|pad| KIT[*pad].sound == Sound::Tom)
        .collect();
    let hz: Vec<f64> = toms
        .iter()
        .map(|pad| {
            let sound = one_hit(*pad, 127, DrumPadState::default(), 0.5);
            frequency(&sound.left[12_000..20_000])
        })
        .collect();
    println!("toms: {hz:.1?} Hz");
    for (pair, pads) in hz.windows(2).zip(toms.windows(2)) {
        let semitones = 12.0 * (pair[1] / pair[0]).log2();
        let set = KIT[pads[1]].pitch_semitones - KIT[pads[0]].pitch_semitones;
        assert!(
            (semitones - f64::from(set)).abs() < 0.05,
            "{semitones} for {set}"
        );
    }
}

/// The frequency of a steady tone from its rising zero crossings.
fn frequency(samples: &[f32]) -> f64 {
    let crossings = rising_zero_crossings(samples);
    let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
    let cycles = (crossings.len() - 1) as f64;
    cycles * f64::from(SAMPLE_RATE) / (last - first) as f64
}

#[test]
fn volume_velocity_and_pan_measure_as_set() {
    let snare = 2;
    let at = |volume_db: f32, pan: f32, velocity: u8| {
        let mut drums = DrumPadState::default();
        drums.pads[snare].volume_db = volume_db;
        drums.pads[snare].pan = pan;
        one_hit(snare, velocity, drums, 0.4)
    };
    let loud = at(0.0, 0.0, 127);
    let reference = peak(&loud.left);
    for volume in [-24.0, -6.0, 6.0] {
        let measured = decibels(peak(&at(volume, 0.0, 127).left) / reference);
        println!("volume {volume} dB: measured {measured:.4} dB");
        assert!((measured - volume).abs() < 0.001, "{measured}");
    }
    // Velocity is a square: 64 is a quarter of 127 in amplitude, less 0.07 dB.
    let measured = peak(&at(0.0, 0.0, 64).left) / reference;
    let expected = (64.0_f32 / 127.0).powi(2);
    assert!((measured - expected).abs() < 1e-5, "{measured} {expected}");
    // The pan law of a track: hard left is only left, at +3 dB; half right is the sine law.
    let left = at(0.0, -1.0, 127);
    assert_eq!(peak(&left.right), 0.0);
    let measured = decibels(peak(&left.left) / reference);
    assert!((measured - 3.0103).abs() < 0.001, "{measured}");
    let half = at(0.0, 0.5, 127);
    let expected = [0.25_f64, 0.75]
        .map(|part| (std::f64::consts::SQRT_2 * (part * std::f64::consts::FRAC_PI_2).sin()) as f32);
    let measured = [peak(&half.left), peak(&half.right)].map(|side| side / reference);
    println!("pan 0.5: {measured:?}, the law says {expected:?}");
    for (measured, expected) in measured.into_iter().zip(expected) {
        assert!((measured - expected).abs() < 1e-5);
    }
}

#[test]
fn pitch_tunes_a_synthesized_sound_and_keeps_its_length() {
    let tom = 11;
    let at = |semitones: f32| {
        let mut drums = DrumPadState::default();
        drums.pads[tom].pitch_semitones = semitones;
        one_hit(tom, 127, drums, 0.7)
    };
    let (low, high) = (at(0.0), at(12.0));
    let window = 12_000..20_000;
    let ratio = frequency(&high.left[window.clone()]) / frequency(&low.left[window]);
    println!("12 semitones up: {ratio:.4} times the frequency");
    assert!((ratio - 2.0).abs() < 0.005, "{ratio}");
    // Tuned, not sped up: it lasts as long.
    let (low_end, high_end) = (last_above(&low.left, -40.0), last_above(&high.left, -40.0));
    assert!(
        (low_end as f64 / high_end as f64 - 1.0).abs() < 0.1,
        "{low_end} {high_end}"
    );
    // The noise of a sound moves with it too.
    let crash = 13;
    let bright = |semitones: f32| {
        let mut drums = DrumPadState::default();
        drums.pads[crash].pitch_semitones = semitones;
        centroid(&one_hit(crash, 127, drums, 0.3).sum(), 9_600)
    };
    let (down, level) = (bright(-12.0), bright(0.0));
    println!("crash centroid: {down:.0} Hz an octave down, {level:.0} Hz as it is");
    assert!(down < 0.7 * level);
}

#[test]
fn decay_is_how_long_a_pad_sounds() {
    for (pad, decays) in [
        (0, [150.0, 600.0, 1_500.0]),
        (13, [300.0, 1_800.0, 4_000.0]),
    ] {
        for decay_ms in decays {
            let mut drums = DrumPadState::default();
            drums.pads[pad].decay_ms = decay_ms;
            let sound = one_hit(pad, 127, drums, f64::from(decay_ms) / 1000.0 + 0.2);
            let mono = sound.sum();
            let frames = (f64::from(decay_ms) / 1000.0 * f64::from(SAMPLE_RATE)) as usize;
            let heard = last_above(&mono, -60.0);
            let silent_from = mono.iter().rposition(|sample| *sample != 0.0).unwrap() + 1;
            println!(
                "{} at {decay_ms} ms: -60 dB at {:.0} ms, silent from {:.1} ms",
                KIT[pad].name,
                heard as f64 * 1000.0 / f64::from(SAMPLE_RATE),
                silent_from as f64 * 1000.0 / f64::from(SAMPLE_RATE),
            );
            assert!(silent_from <= frames);
            assert!(heard as f64 > 0.6 * frames as f64, "{heard} {frames}");
        }
    }
}

/// The same notes render the same bytes, also when every sound is made again.
#[test]
fn a_render_is_the_same_every_time() {
    let render = || {
        let mut harness = Harness::with_track(crate::listen::beat(), DrumPadState::default());
        harness.play(48_000 * 3)
    };
    let first = render();
    let second = render();
    assert!(first.left == second.left && first.right == second.right);
}

#[test]
fn a_pad_given_another_sound_plays_that_sound() {
    let mut drums = DrumPadState::default();
    drums.pads[0].source = Source::Sound(Sound::Clap);
    drums.pads[0].decay_ms = KIT[3].decay_ms;
    let changed = one_hit(0, 127, drums, 0.5);
    let clap = one_hit(3, 127, DrumPadState::default(), 0.5);
    assert!(changed.left == clap.left && changed.right == clap.right);
}
