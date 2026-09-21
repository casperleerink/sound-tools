//! The grid a fit builds: the tempo map, the clip and what steadiness does to them.

use fit_tempo::{BeatRate, FIT_SAMPLE_RATE, fit};
use sound_core::{Clock, Frames, TempoMap, Ticks};
use sound_notes::RawTake;

use crate::beats::cases;
use crate::generate::{Case, Curve, Playing};

fn first_case() -> Case {
    cases().into_iter().next().expect("a case").0
}

/// The frame a time in microseconds lands on, at the rate a fit is built for.
fn frame_of(time_us: u64) -> u64 {
    (time_us as f64 * f64::from(FIT_SAMPLE_RATE) / 1_000_000.0).round() as u64
}

/// The clock of a map at the rate a fit is built for.
fn clock(map: &TempoMap) -> Clock {
    Clock::new(map.clone(), FIT_SAMPLE_RATE)
}

/// Every beat of the grid lands on the frame it was built for, and the error of the last beat
/// is no larger than that of the first: a tempo rounded to 0.001 bpm does not add up.
#[test]
fn the_tempo_map_puts_every_beat_where_the_take_had_it() {
    let case = Case {
        name: "long take",
        bars: 380,
        curve: Curve::Rubato {
            base: 100.0,
            depth: 0.08,
            beats: 24.0,
        },
        ..first_case()
    };
    let (take, _) = case.take();
    let signature = case.signature();
    let fitted = fit(&take, signature, 0, BeatRate::Normal).expect("a fit");
    assert!(fitted.beat_count() > 1500, "{} beats", fitted.beat_count());
    let clock = clock(&fitted.map);
    let ticks_per_beat = signature.ticks_per_beat();
    let worst = fitted
        .targets_us
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let tick = Ticks(index as u64 * ticks_per_beat);
            clock.frame_of(tick).0.abs_diff(frame_of(*target))
        })
        .max()
        .unwrap_or(0);
    // One frame at 48 kHz is 21 microseconds. The bound holds for the last beat of a ten
    // minute take as much as for the first.
    assert!(worst <= 2, "the worst beat is {worst} frames out");
}

/// The first downbeat is a bar line, whatever the time signature and wherever the take begins.
#[test]
fn the_first_downbeat_lands_on_a_bar_line() {
    for time_signature in ["4/4", "3/4", "6/8", "7/8"] {
        for downbeat_us in [0, 1_900_000, 3_100_000] {
            let case = Case {
                time_signature,
                ..first_case()
            };
            let (take, _) = case.take();
            let signature = case.signature();
            let fitted = fit(&take, signature, downbeat_us, BeatRate::Normal).expect("a fit");
            let tick = fitted.first_downbeat_tick(signature);
            let position = signature.bar_beat_of(tick);
            assert_eq!(
                (position.beat, position.tick),
                (1, 0),
                "{time_signature} at {downbeat_us} is {position}"
            );
            // The pickup fits in front of it, and there is at least one bar of lead.
            assert!(tick.0 >= signature.ticks_per_bar(), "{position}");
        }
    }
}

/// Double has twice the beats of normal and half has half of them, and the first downbeat
/// stays on the same moment in the take.
#[test]
fn half_and_double_change_the_beats_and_not_the_downbeat() {
    let case = first_case();
    let (take, _) = case.take();
    let signature = case.signature();
    let at = |rate| fit(&take, signature, 0, rate).expect("a fit");
    let (half, normal, double) = (
        at(BeatRate::Half),
        at(BeatRate::Normal),
        at(BeatRate::Double),
    );
    let beats = |fitted: &fit_tempo::Fitted| fitted.targets_us.len() - fitted.first_downbeat;
    assert!(
        beats(&double).abs_diff(beats(&normal) * 2) <= 2,
        "{} against {}",
        beats(&double),
        beats(&normal)
    );
    assert!(
        beats(&half).abs_diff(beats(&normal).div_ceil(2)) <= 2,
        "{} against {}",
        beats(&half),
        beats(&normal)
    );
    // The same moment of the take is the first downbeat in all three.
    let downbeat = |fitted: &fit_tempo::Fitted| fitted.targets_us[fitted.first_downbeat];
    assert_eq!(downbeat(&half), downbeat(&normal));
    assert_eq!(downbeat(&double), downbeat(&normal));
}

/// At 0 % the map is the fitted one, byte for byte, however often it is asked for. At 100 %
/// every beat is the same length. Going back to 0 % gives the same bytes again.
#[test]
fn steadiness_moves_the_beats_and_can_always_be_turned_back() {
    let case = first_case();
    let (take, _) = case.take();
    let signature = case.signature();
    let fitted = fit(&take, signature, 0, BeatRate::Normal).expect("a fit");
    let played = fitted.map_at(signature, 0.0);
    assert_eq!(played, fitted.map);
    assert_eq!(fitted.map_at(signature, 0.0), played);

    // Half way the map still follows the playing, with fewer steps than the take had.
    let half = fitted.map_at(signature, 0.5);
    assert_ne!(half, played);
    assert!(half.tempo_changes().len() > 1);

    // At 100 % every beat is the same length to within a frame, and every step of the map has
    // the same tempo to within a hundredth of a bpm: a beat of about 30000 frames cannot be
    // hit exactly by a tempo held in thousandths, so the last frame is corrected step by step.
    let steady = fitted.map_at(signature, 1.0);
    let tempos: Vec<f64> = steady
        .tempo_changes()
        .iter()
        .map(|change| change.bpm.bpm())
        .collect();
    let spread = tempos.iter().copied().fold(0.0_f64, f64::max)
        - tempos.iter().copied().fold(f64::MAX, f64::min);
    assert!(spread <= 0.01, "the tempo runs from {tempos:?}");
    let steady_clock = clock(&steady);
    let ticks_per_beat = signature.ticks_per_beat();
    let frames: Vec<u64> = (0..=fitted.beat_count())
        .map(|beat| steady_clock.frame_of(Ticks(beat as u64 * ticks_per_beat)).0)
        .collect();
    let steps: Vec<u64> = frames.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let (shortest, longest) = (
        steps.iter().min().copied().unwrap_or(0),
        steps.iter().max().copied().unwrap_or(0),
    );
    // Four frames is 83 microseconds at 48 kHz: the beat is rounded to a frame, the tempo to
    // a thousandth of a bpm, and a step may keep the one before it while it is within a frame.
    assert!(longest - shortest <= 4, "{shortest} to {longest} frames");
    // The take ends where it did, so nothing after it moves.
    let last = Ticks(fitted.beat_count() as u64 * ticks_per_beat);
    let played_end = clock(&played).frame_of(last).0;
    assert!(
        steady_clock.frame_of(last).0.abs_diff(played_end) <= 2,
        "the end moved"
    );

    // And back.
    assert_eq!(fitted.map_at(signature, 0.0), played);
}

/// The clip of a fit renders the take as it was heard: every note within one tick of the
/// moment the engine sounded it, measured through the saved tempo map.
#[test]
fn the_fitted_clip_keeps_every_note_where_it_was_heard() {
    for playing in [Playing::Chords, Playing::Syncopated, Playing::Arpeggiated] {
        let case = Case {
            playing,
            ..first_case()
        };
        let (take, _) = case.take();
        let signature = case.signature();
        let fitted = fit(&take, signature, 0, BeatRate::Normal).expect("a fit");
        let clock = clock(&fitted.map);
        let worst = worst_note_error_ticks(&take, &fitted.clip, &clock);
        assert!(worst <= 1, "{playing:?}: {worst} ticks");
    }
}

/// The largest difference, in ticks, between where a note of the clip sounds and where the
/// take says the engine sounded it.
fn worst_note_error_ticks(take: &RawTake, clip: &sound_notes::Clip, clock: &Clock) -> u64 {
    let mut heard: Vec<u64> = take
        .events
        .iter()
        .filter(|event| matches!(event, sound_notes::RawEvent::On { .. }))
        .map(|event| take.start_us + event.sounded_us())
        .collect();
    heard.sort_unstable();
    let mut played: Vec<Ticks> = clip.placed_notes().map(|note| note.start).collect();
    played.sort_unstable();
    assert_eq!(heard.len(), played.len(), "a note went missing");
    heard
        .iter()
        .zip(&played)
        .map(|(heard, played)| {
            // The tick the engine would sound that moment on, against the tick the clip has.
            let wanted = clock.tick_at(Frames(frame_of(*heard)));
            wanted.0.abs_diff(played.0)
        })
        .max()
        .unwrap_or(0)
}

/// A take is placed where it was recorded, whatever the tempo map was then: the first note of
/// the clip sounds at the same moment before and after the fit.
#[test]
fn the_take_keeps_its_place_in_real_time() {
    let case = Case {
        starts_at_seconds: 7.5,
        ..first_case()
    };
    let (take, _) = case.take();
    let signature = case.signature();
    let fitted = fit(&take, signature, 0, BeatRate::Normal).expect("a fit");
    let clock = clock(&fitted.map);
    let start = clock.frame_of(fitted.clip.start).0;
    assert!(
        start.abs_diff(frame_of(take.start_us)) <= 2,
        "the clip starts at frame {start}, the take at {}",
        frame_of(take.start_us)
    );
    // And nothing of the take reaches before the project starts.
    assert!(clock.tick_at(Frames(0)) == Ticks(0));
}
