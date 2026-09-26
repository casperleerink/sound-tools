//! Every setting is stable: no blow-up at the most extreme Q and gain, also at the top of the
//! range at 44.1, 48 and 96 kHz, nothing that is not a number, the EQ comes to rest, and a
//! render is the same every time.

use eq::{BANDS, Band, EqState, FREQUENCIES, GAIN, OUTPUT_GAIN, Q, Shape, band_response};
use proptest::prelude::*;
use sound_core::Processor;

use crate::support::{Rig, SAMPLE_RATE, Signal, band, noise, peak, sine, with_bands};

/// The most a band can make of any input, by its linear bound: its input part plus twice its
/// loudest gain, which covers the ring of its filter part. A generous bound, and still a
/// bound: a filter that runs away passes any number.
fn band_bound(band: &Band, sample_rate: f32) -> f32 {
    // From 1 Hz, under the range: a shelf at 20 Hz with a high Q peaks below 20 Hz.
    let top = 0.49 * sample_rate;
    let loudest = (0..=1_000)
        .map(|step| top.powf(step as f32 / 1_000.0))
        .map(|hz| {
            let (real, imaginary) = band_response(band, hz, sample_rate);
            real.hypot(imaginary) as f32
        })
        .fold(0.0, f32::max);
    1.0 + 2.0 * loudest
}

fn bound(state: &EqState, sample_rate: f32) -> f32 {
    let output = 10_f32.powf(state.output_gain_db / 20.0);
    state
        .bands
        .iter()
        .map(|band| band_bound(band, sample_rate))
        .product::<f32>()
        * output
}

fn assert_bounded(label: &str, bound: f32, [left, right]: &[Vec<f32>; 2]) {
    for sample in left.iter().chain(right) {
        assert!(sample.is_finite(), "{label}: {sample}");
    }
    let loudest = peak(left).max(peak(right));
    assert!(loudest < bound, "{label}: peak {loudest}, bound {bound}");
}

/// A square at full scale, whose every edge rings a resonant band.
fn square(hz: f64, sample_rate: u32) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(sample_rate);
    Box::new(move || {
        phase = (phase + step).fract();
        [if phase < 0.5 { 1.0 } else { -1.0 }; 2]
    })
}

/// Every shape at the most extreme Q and gain, at both ends of the range, with all four bands
/// on the same place so they add up: full scale noise and a square stay under the linear bound
/// and are numbers, at 44.1, 48 and 96 kHz.
#[test]
fn every_shape_at_its_extremes_stays_bounded_at_every_sample_rate() {
    for sample_rate in [44_100, 48_000, 96_000] {
        let rate = sample_rate as f32;
        for shape in Shape::ALL {
            for frequency_hz in [FREQUENCIES[0].min, 1_000.0, FREQUENCIES[0].max] {
                for q in [Q.min, Q.max] {
                    for gain_db in [GAIN.min, GAIN.max] {
                        let one = band(shape, frequency_hz, gain_db, q);
                        let mut state = with_bands(&[one; BANDS]);
                        state.output_gain_db = OUTPUT_GAIN.max;
                        let label =
                            format!("{sample_rate} Hz: {shape:?} {frequency_hz} Hz Q {q} {gain_db} dB");
                        let bound = bound(&state, rate);
                        let frames = sample_rate as usize / 2;
                        let square = square(f64::from(frequency_hz).min(5_000.0) / 3.0, sample_rate);
                        let mut rig = Rig::at_rate(state, square, sample_rate);
                        assert_bounded(&label, bound, &rig.render(frames));
                        let mut rig = Rig::at_rate(state, noise(1.0), sample_rate);
                        assert_bounded(&label, bound, &rig.render(frames));
                    }
                }
            }
        }
    }
}

/// A full scale sine that sweeps through the peak of the most resonant bands: it stays under
/// the bound, and it does reach the peak, which the steady response says.
#[test]
fn a_full_scale_sweep_through_the_most_resonant_bands_stays_bounded() {
    for (shape, frequency_hz) in [
        (Shape::Bell, 1_000.0),
        (Shape::LowCut, 100.0),
        (Shape::HighCut, 10_000.0),
        (Shape::HighShelf, 5_000.0),
    ] {
        let one = band(shape, frequency_hz, GAIN.max, Q.max);
        let state = with_bands(&[one]);
        let (from, octaves, seconds) = (f64::from(frequency_hz) / 4.0, 4.0, 2.0);
        let mut phase = 0.0_f64;
        let mut frame = 0_u64;
        let sweep: Signal = Box::new(move || {
            let time = frame as f64 / f64::from(SAMPLE_RATE);
            let hz = from * 2_f64.powf(octaves * (time / seconds).min(1.0));
            phase = (phase + hz / f64::from(SAMPLE_RATE)).fract();
            frame += 1;
            [(std::f64::consts::TAU * phase).sin() as f32; 2]
        });
        let mut rig = Rig::new(state, sweep);
        let output = rig.render(2 * SAMPLE_RATE as usize);
        let loudest = peak(&output[0]);
        let steady = eq::response(&state, frequency_hz, SAMPLE_RATE as f32);
        println!("{shape:?} at {frequency_hz} Hz: peak {loudest:.2}, steady peak {steady:.2}");
        assert_bounded(&format!("{shape:?}"), bound(&state, SAMPLE_RATE as f32), &output);
        assert!(loudest > steady * 0.3, "{shape:?}: {loudest}");
    }
}

/// A sample that is not a number, or is infinite, is silence or the limit to the EQ. The EQ
/// goes on as if nothing had happened.
#[test]
fn input_that_is_not_a_number_or_infinite_does_not_reach_the_output() {
    let mut frame = 0_usize;
    let mut clean = sine(440.0, 0.5);
    let broken: Signal = Box::new(move || {
        frame += 1;
        match frame {
            1_000 => [f32::NAN, f32::INFINITY],
            2_000 => [f32::NEG_INFINITY, f32::NAN],
            3_000 => [f32::MAX, -f32::MAX],
            _ => clean(),
        }
    });
    let state = with_bands(&[band(Shape::Bell, 440.0, 15.0, 18.0)]);
    let output = Rig::new(state, broken).render(SAMPLE_RATE as usize);
    for sample in output.iter().flatten() {
        assert!(sample.is_finite());
    }
    // The limit rings the band, and that ring dies away like any other: half a second later
    // the tone is back at its steady level.
    let [left, _] = &output;
    let tail = &left[SAMPLE_RATE as usize / 2..];
    let steady = 0.5 * eq::response(&state, 440.0, SAMPLE_RATE as f32);
    assert!(peak(tail) < steady * 1.01, "{} {steady}", peak(tail));
}

/// After the sound ends the EQ rings out and then is exactly silent: its memory lets go, and
/// it does no work.
#[test]
fn after_the_sound_the_eq_comes_to_rest_and_is_silent() {
    let mut frames = 0_usize;
    let mut sound = noise(0.5);
    let burst: Signal = Box::new(move || {
        frames += 1;
        if frames <= 4_800 { sound() } else { [0.0; 2] }
    });
    // The slowest ring there is: every band at 20 Hz and Q 18.
    let state = with_bands(&[band(Shape::Bell, 20.0, 15.0, 18.0); BANDS]);
    let mut rig = Rig::new(state, burst);
    let [ringing, _] = rig.render(SAMPLE_RATE as usize);
    assert!(peak(&ringing[4_800..]) > 0.0);
    rig.render(20 * SAMPLE_RATE as usize);
    let [rest, other] = rig.render(SAMPLE_RATE as usize);
    assert!(rest.iter().chain(&other).all(|sample| *sample == 0.0));
}

#[test]
fn renders_are_the_same_every_time_to_the_byte() {
    let state = with_bands(&[
        band(Shape::LowCut, 80.0, 0.0, 2.0),
        band(Shape::Bell, 350.0, -6.0, 3.0),
        band(Shape::Notch, 2_500.0, 0.0, 5.0),
        band(Shape::HighShelf, 7_000.0, 4.0, 0.71),
    ]);
    let render = || {
        let mut rig = Rig::new(state, noise(0.8));
        let first = rig.render(SAMPLE_RATE as usize);
        let mut next = state;
        next.bands[1].shape = Shape::HighCut;
        next.bands[2].on = false;
        next.output_gain_db = -3.0;
        rig.update(next);
        let [left, right] = rig.render(SAMPLE_RATE as usize);
        let bytes = |samples: &[f32]| -> Vec<u8> {
            samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect()
        };
        [first[0].clone(), first[1].clone(), left, right].map(|samples| bytes(&samples))
    };
    assert_eq!(render(), render());
}

#[test]
fn the_eq_has_no_latency() {
    assert_eq!(eq::Eq::new(EqState::default()).latency(), 0);
}

fn any_band() -> impl Strategy<Value = Band> {
    (
        any::<bool>(),
        0..Shape::ALL.len(),
        20.0_f32..=20_000.0,
        -15.0_f32..=15.0,
        0.1_f32..=18.0,
    )
        .prop_map(|(on, shape, frequency_hz, gain_db, q)| Band {
            on,
            shape: Shape::ALL[shape],
            frequency_hz,
            gain_db,
            q,
        })
}

fn any_state() -> impl Strategy<Value = EqState> {
    (
        proptest::array::uniform4(any_band()),
        -12.0_f32..=12.0,
    )
        .prop_map(|(bands, output_gain_db)| EqState {
            bands,
            output_gain_db,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any valid records, one after the other every 10 ms while full scale noise plays, at any
    /// of the three sample rates: the output is always a number and under the bound of the
    /// louder of the two records.
    #[test]
    fn any_edits_while_it_plays_keep_the_output_bounded(
        states in proptest::collection::vec(any_state(), 2..12),
        rate in 0..3_usize,
    ) {
        let sample_rate = [44_100, 48_000, 96_000][rate];
        let mut rig = Rig::at_rate(states[0], noise(1.0), sample_rate);
        let mut previous = states[0];
        for state in &states {
            rig.update(*state);
            let output = rig.render(sample_rate as usize / 100);
            // During a glide the EQ is between the two records.
            let limit = bound(state, sample_rate as f32).max(bound(&previous, sample_rate as f32));
            for sample in output.iter().flatten() {
                prop_assert!(sample.is_finite());
                prop_assert!(sample.abs() < limit * 4.0, "{sample} after {state:?}");
            }
            previous = *state;
        }
    }
}
