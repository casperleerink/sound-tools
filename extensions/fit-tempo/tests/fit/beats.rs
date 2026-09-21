//! How close the fitted beats land to the true beats of a generated take.
//!
//! The generator in `generate.rs` makes every take from a tempo curve, so the true beat of
//! every moment is a number and not an opinion. The bound below is what this build reaches.

use fit_tempo::{BeatRate, fit};

use crate::generate::{Case, Curve, Playing};

/// How far a beat may be from the true beat and still count as found.
const BOUND_MS: f64 = 30.0;

/// How far a fitted beat is from the true beat nearest to it, in milliseconds, for every true
/// beat of the take.
pub fn errors_ms(case: &Case) -> Vec<f64> {
    let (take, truth) = case.take();
    let fitted = fit(&take, case.signature(), 0, BeatRate::Normal).expect("a fit");
    let grid = &fitted.targets_us[fitted.first_downbeat..];
    // The last true beat is the one after the last note. Nothing was played on it, so the grid
    // has no reason to reach it.
    let truth = &truth[..truth.len() - 1];
    truth
        .iter()
        .map(|beat| {
            let nearest = grid
                .iter()
                .map(|fitted| fitted.abs_diff(*beat))
                .min()
                .unwrap_or(u64::MAX);
            nearest as f64 / 1000.0
        })
        .collect()
}

/// Sixteen bars of every kind of playing this build is meant to follow, with the timing jitter
/// of a hand. `off` is how many beats may be further than [`BOUND_MS`] from the truth.
pub fn cases() -> Vec<(Case, usize)> {
    let base = Case {
        name: "",
        time_signature: "4/4",
        curve: Curve::Steady(96.0),
        playing: Playing::Chords,
        jitter_ms: 18.0,
        bars: 16,
        starts_at_seconds: 2.0,
        silence_seconds: 0.7,
    };
    vec![
        (
            Case {
                name: "steady 4/4 chords",
                ..base
            },
            0,
        ),
        (
            Case {
                name: "steady 3/4 chords",
                time_signature: "3/4",
                ..base
            },
            0,
        ),
        (
            Case {
                name: "steady 6/8 chords",
                time_signature: "6/8",
                curve: Curve::Steady(60.0),
                ..base
            },
            0,
        ),
        (
            Case {
                name: "slow rubato",
                curve: Curve::Rubato {
                    base: 96.0,
                    depth: 0.12,
                    beats: 16.0,
                },
                ..base
            },
            0,
        ),
        (
            Case {
                name: "ritardando",
                curve: Curve::Ritardando {
                    from: 110.0,
                    to: 70.0,
                },
                ..base
            },
            0,
        ),
        // The one kind this build is measurably worse at: the beat at the change itself lands
        // about a fifth of a beat out, because the period is measured over a window that holds
        // both tempos. Every other beat of the take is inside the bound.
        (
            Case {
                name: "sudden tempo change",
                curve: Curve::Sudden {
                    from: 92.0,
                    to: 128.0,
                    at: 32,
                },
                ..base
            },
            1,
        ),
        (
            Case {
                name: "syncopated",
                playing: Playing::Syncopated,
                ..base
            },
            0,
        ),
        (
            Case {
                name: "arpeggiated",
                playing: Playing::Arpeggiated,
                ..base
            },
            0,
        ),
        (
            Case {
                name: "no jitter",
                jitter_ms: 0.0,
                ..base
            },
            0,
        ),
        (
            Case {
                name: "loose hand",
                jitter_ms: 35.0,
                ..base
            },
            0,
        ),
    ]
}

/// The bound this build reaches, and the test that holds it.
///
/// Every beat of every kind of playing lands within 30 ms of the true beat, except one beat at
/// a sudden tempo change, which lands within 200 ms. The allowance is one beat and not a
/// fraction, so a change that loses a beat anywhere fails here.
#[test]
fn the_fitted_beats_land_close_to_the_true_beats() {
    for (case, allowed) in cases() {
        let errors = errors_ms(&case);
        let off: Vec<(usize, f64)> = errors
            .iter()
            .enumerate()
            .filter(|(_, error)| **error > BOUND_MS)
            .map(|(index, error)| (index, *error))
            .collect();
        assert!(
            off.len() <= allowed,
            "{}: {} of {} beats are more than {BOUND_MS} ms out, at most {allowed} may be: {off:?}",
            case.name,
            off.len(),
            errors.len()
        );
        let worst = errors.iter().copied().fold(0.0_f64, f64::max);
        assert!(worst < 200.0, "{}: worst beat {worst:.1} ms out", case.name);
    }
}

/// The same take fits to the same grid on every run: no time, no randomness, no order that
/// depends on a hash.
#[test]
fn a_fit_of_one_take_is_the_same_every_time() {
    let (case, _) = cases().into_iter().next().expect("a case");
    let (take, _) = case.take();
    let once = fit(&take, case.signature(), 0, BeatRate::Normal).expect("a fit");
    for _ in 0..3 {
        let again = fit(&take, case.signature(), 0, BeatRate::Normal).expect("a fit");
        assert_eq!(once, again);
    }
}

/// A take with almost nothing in it has no beat to find, and says so instead of guessing.
#[test]
fn a_take_with_too_few_notes_is_not_fitted() {
    let (case, _) = cases().into_iter().next().expect("a case");
    let (mut take, _) = case.take();
    take.events.truncate(6);
    let error = fit(&take, case.signature(), 0, BeatRate::Normal).expect_err("no fit");
    assert!(error.to_string().contains("to find a beat in"), "{error}");
}

/// Prints the bound of every case. `cargo nextest run -p fit-tempo --run-ignored only`
#[test]
#[ignore = "prints numbers, it asserts nothing"]
fn measure() {
    for (case, _) in cases() {
        let errors = errors_ms(&case);
        let within = errors.iter().filter(|error| **error <= BOUND_MS).count();
        let worst = errors.iter().copied().fold(0.0_f64, f64::max);
        let mut sorted = errors.clone();
        sorted.sort_by(f64::total_cmp);
        println!(
            "{:22} beats {:3}  within {BOUND_MS} ms {:3}  median {:5.1} ms  worst {:7.1} ms",
            case.name,
            errors.len(),
            within,
            sorted[sorted.len() / 2],
            worst
        );
    }
}
