//! The musical clock through the public API: conversions, validation and the saved JSON shape.

#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use std::num::NonZeroU32;

use sound_core::{
    Bar, BarBeat, Clock, ClockError, Frames, MIN_EXACT_SAMPLE_RATE, SignatureRun, Tempo,
    TempoChange, TempoMap, Ticks, TimeSignature, TimeSignatures,
};

fn bpm(value: f64) -> Tempo {
    Tempo::from_bpm(value).unwrap()
}

fn tempo_map(changes: &[(u64, f64)]) -> TempoMap {
    let changes = changes
        .iter()
        .map(|(tick, value)| TempoChange {
            tick: Ticks(*tick),
            bpm: bpm(*value),
        })
        .collect();
    TempoMap::new(TimeSignature::default(), changes).unwrap()
}

/// Every valid tempo in steps of 0.001 bpm, the full range of [`Tempo`].
fn any_tempo() -> impl Strategy<Value = Tempo> {
    (10_000_u32..=1_000_000).prop_map(|milli_bpm| bpm(f64::from(milli_bpm) / 1000.0))
}

/// One to eight tempo changes, from one tick apart to many bars apart.
fn any_tempo_map() -> impl Strategy<Value = TempoMap> {
    let later = prop::collection::vec((1_u64..200_000, any_tempo()), 0..8);
    (any_tempo(), later).prop_map(|(first, later)| {
        let mut tick = 0;
        let mut changes = vec![TempoChange {
            tick: Ticks(0),
            bpm: first,
        }];
        for (gap, bpm) in later {
            tick += gap;
            changes.push(TempoChange {
                tick: Ticks(tick),
                bpm,
            });
        }
        TempoMap::new(TimeSignature::default(), changes).unwrap()
    })
}

fn signature(text: &str) -> TimeSignature {
    text.parse().unwrap()
}

/// Runs of bars, each `(signature, bars)`.
fn time_signatures(runs: &[(&str, u32)]) -> TimeSignatures {
    let runs = runs
        .iter()
        .map(|(text, bars)| SignatureRun {
            signature: signature(text),
            bars: NonZeroU32::new(*bars).unwrap(),
        })
        .collect();
    TimeSignatures::new(runs).unwrap()
}

/// The tick of a position that exists.
fn ticks_of(time_signatures: &TimeSignatures, position: BarBeat) -> Ticks {
    let bar = time_signatures.bar(position.bar).unwrap();
    let beat = bar.signature.ticks_per_beat();
    bar.start + Ticks(u64::from(position.beat - 1) * beat + u64::from(position.tick))
}

fn any_signature() -> impl Strategy<Value = TimeSignature> {
    let denominator = prop::sample::select(vec![1_u32, 2, 4, 8, 16, 32]);
    (1_u32..=32, denominator)
        .prop_map(|(numerator, denominator)| TimeSignature::new(numerator, denominator).unwrap())
}

/// One to twenty runs of one to five bars, often one bar each as in the Danse sacrale.
fn any_time_signatures() -> impl Strategy<Value = TimeSignatures> {
    let run = (any_signature(), 1_u32..=5).prop_map(|(signature, bars)| SignatureRun {
        signature,
        bars: NonZeroU32::new(bars).unwrap(),
    });
    prop::collection::vec(run, 1..20).prop_map(|runs| TimeSignatures::new(runs).unwrap())
}

fn any_sample_rate() -> impl Strategy<Value = u32> {
    prop::sample::select(vec![MIN_EXACT_SAMPLE_RATE, 44_100, 48_000, 96_000])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn tick_to_frame_to_tick_is_exact(
        tempo_map in any_tempo_map(),
        sample_rate in any_sample_rate(),
        tick in 0_u64..2_000_000,
    ) {
        let clock = Clock::new(tempo_map, sample_rate);
        let frame = clock.frame_of(Ticks(tick));
        prop_assert_eq!(clock.tick_at(frame), Ticks(tick));
        // No two ticks share a frame, so a tick is never lost to its neighbour.
        prop_assert!(clock.frame_of(Ticks(tick + 1)) > frame);
    }

    #[test]
    fn far_positions_stay_exact(
        tempo_map in any_tempo_map(),
        sample_rate in any_sample_rate(),
        tick in 0_u64..1_000_000_000_000,
    ) {
        let clock = Clock::new(tempo_map, sample_rate);
        prop_assert_eq!(clock.tick_at(clock.frame_of(Ticks(tick))), Ticks(tick));
    }

    #[test]
    fn the_ticks_of_a_frame_range_are_the_ticks_that_land_in_it(
        tempo_map in any_tempo_map(),
        sample_rate in any_sample_rate(),
        start in 0_u64..50_000_000,
        length in 0_u64..2_000,
    ) {
        let clock = Clock::new(tempo_map, sample_rate);
        let frames = start..start + length;
        let ticks = clock.tick_at(Frames(frames.start)).0..clock.tick_at(Frames(frames.end)).0;
        for tick in ticks.clone() {
            prop_assert!(frames.contains(&clock.frame_of(Ticks(tick)).0));
        }
        if let Some(before) = ticks.start.checked_sub(1) {
            prop_assert!(clock.frame_of(Ticks(before)).0 < frames.start);
        }
        prop_assert!(clock.frame_of(Ticks(ticks.end)).0 >= frames.end);
    }

    #[test]
    fn bar_beat_round_trips(
        time_signatures in any_time_signatures(),
        tick in 0_u64..1_000_000_000_000,
    ) {
        let bar = time_signatures.bar_at(Ticks(tick));
        prop_assert!(bar.start <= Ticks(tick) && Ticks(tick) < bar.end());
        prop_assert_eq!(time_signatures.bar(bar.number), Some(bar));
        let position = time_signatures.bar_beat_of(Ticks(tick));
        prop_assert_eq!(position.bar, bar.number);
        prop_assert!((1..=bar.signature.numerator()).contains(&position.beat));
        prop_assert!(u64::from(position.tick) < bar.signature.ticks_per_beat());
        prop_assert_eq!(ticks_of(&time_signatures, position), Ticks(tick));
    }

    #[test]
    fn every_bar_starts_where_the_one_before_it_ends(
        time_signatures in any_time_signatures(),
        tick in 0_u64..100_000,
    ) {
        let bars: Vec<Bar> = time_signatures.bars_from(Ticks(tick)).take(120).collect();
        prop_assert!(bars[0].start <= Ticks(tick) && Ticks(tick) < bars[0].end());
        for pair in bars.windows(2) {
            prop_assert_eq!(pair[1].start, pair[0].end());
            prop_assert_eq!(pair[1].number, pair[0].number + 1);
            prop_assert_eq!(time_signatures.bar_at(pair[1].start), pair[1]);
        }
    }

    #[test]
    fn the_next_beat_is_the_first_beat_at_or_after_a_tick(
        time_signatures in any_time_signatures(),
        tick in 0_u64..100_000,
    ) {
        let (beat, downbeat) = time_signatures.beat_from(Ticks(tick));
        prop_assert!(beat >= Ticks(tick));
        let bar = time_signatures.bar_at(beat);
        let in_bar = beat.0 - bar.start.0;
        prop_assert!(in_bar.is_multiple_of(bar.signature.ticks_per_beat()));
        prop_assert_eq!(downbeat, in_bar == 0);
        // No beat between the tick and the one found: the beat before it is before the tick.
        if beat > Ticks(0) {
            let bar = time_signatures.bar_at(Ticks(beat.0 - 1));
            let beat_length = bar.signature.ticks_per_beat();
            let previous = bar.start.0 + (beat.0 - 1 - bar.start.0) / beat_length * beat_length;
            prop_assert!(previous < tick);
        }
    }

    #[test]
    fn a_tempo_map_round_trips_through_json(tempo_map in any_tempo_map()) {
        let json = serde_json::to_string(&tempo_map).unwrap();
        prop_assert_eq!(serde_json::from_str::<TempoMap>(&json).unwrap(), tempo_map);
    }
}

#[test]
fn a_quarter_note_at_120_bpm_and_48_khz_is_24000_frames() {
    let clock = Clock::new(TempoMap::default(), 48_000);
    assert_eq!(clock.frame_of(Ticks(0)), Frames(0));
    assert_eq!(clock.frame_of(Ticks(1)), Frames(25));
    assert_eq!(clock.frame_of(Ticks(960)), Frames(24_000));
    assert_eq!(clock.frame_of(Ticks(4 * 960)), Frames(96_000));
    assert_eq!(clock.tick_at(Frames(24_000)), Ticks(960));
    // Frame 24001 is past tick 960, so the first tick at or after it is 961.
    assert_eq!(clock.tick_at(Frames(24_001)), Ticks(961));
    assert_eq!(clock.seconds_of(Ticks(960)), 0.5);
    assert_eq!(clock.tick_at_seconds(0.5), Ticks(960));
    assert_eq!(clock.tick_at_seconds(-1.0), Ticks(0));
}

#[test]
fn a_tick_lands_on_the_frame_that_contains_its_exact_time() {
    // At 44.1 kHz and 120 bpm a tick is 22.96875 frames.
    let clock = Clock::new(TempoMap::default(), 44_100);
    assert_eq!(clock.frame_of(Ticks(1)), Frames(22));
    assert_eq!(clock.frame_of(Ticks(2)), Frames(45));
    assert_eq!(clock.frame_of(Ticks(960)), Frames(22_050));
    assert_eq!(clock.tick_at(Frames(22)), Ticks(1));
    assert_eq!(clock.tick_at(Frames(23)), Ticks(2));
}

#[test]
fn the_tempo_and_sample_rate_bounds_are_tight() {
    // At the fastest tempo and the lowest exact sample rate a tick is exactly one frame.
    let fastest = tempo_map(&[(0, Tempo::MAX_BPM)]);
    let exact = Clock::new(fastest.clone(), MIN_EXACT_SAMPLE_RATE);
    assert_eq!(exact.frame_of(Ticks(12_345)), Frames(12_345));
    assert_eq!(exact.tick_at(Frames(12_345)), Ticks(12_345));

    // Below it two ticks share a frame, and the second one does not come back.
    let too_low = Clock::new(fastest, MIN_EXACT_SAMPLE_RATE / 2);
    assert_eq!(too_low.frame_of(Ticks(1)), too_low.frame_of(Ticks(0)));
    assert_eq!(too_low.tick_at(too_low.frame_of(Ticks(1))), Ticks(0));
}

#[test]
fn a_tempo_change_moves_later_ticks_by_the_expected_frames() {
    let steady = Clock::new(tempo_map(&[(0, 120.0)]), 48_000);
    let slowing = Clock::new(
        tempo_map(&[(0, 120.0), (1920, 60.0), (3840, 240.0)]),
        48_000,
    );

    assert_eq!(slowing.frame_of(Ticks(1920)), steady.frame_of(Ticks(1920)));
    // A quarter note at 60 bpm is 48000 frames, twice as long as at 120 bpm.
    assert_eq!(slowing.frame_of(Ticks(2880)), Frames(48_000 + 48_000));
    assert_eq!(steady.frame_of(Ticks(2880)), Frames(48_000 + 24_000));
    // And 12000 frames at 240 bpm.
    assert_eq!(slowing.frame_of(Ticks(3840)), Frames(144_000));
    assert_eq!(slowing.frame_of(Ticks(4800)), Frames(156_000));
    assert_eq!(slowing.tick_at(Frames(156_000)), Ticks(4800));

    assert_eq!(slowing.tempo_at(Ticks(1919)), bpm(120.0));
    assert_eq!(slowing.tempo_at(Ticks(1920)), bpm(60.0));
    assert_eq!(slowing.tempo_at(Ticks(1_000_000)), bpm(240.0));
    assert_eq!(slowing.seconds_of(Ticks(3840)), 3.0);
}

#[test]
fn bars_and_beats_count_from_one() {
    let four_four = TimeSignatures::default();
    let first = BarBeat {
        bar: 1,
        beat: 1,
        tick: 0,
    };
    assert_eq!(four_four.bar_beat_of(Ticks(0)), first);
    let position = four_four.bar_beat_of(Ticks(3 * 3840 + 2 * 960 + 5));
    assert_eq!(position.to_string(), "4:3:005");

    let six_eight = TimeSignature::new(6, 8).unwrap();
    assert_eq!(six_eight.ticks_per_beat(), 480);
    assert_eq!(six_eight.ticks_per_bar(), 2880);
    let six_eight = TimeSignatures::constant(six_eight);
    let position = BarBeat {
        bar: 2,
        beat: 6,
        tick: 479,
    };
    assert_eq!(six_eight.bar_beat_of(Ticks(2880 + 5 * 480 + 479)), position);
}

/// The opening of the Danse sacrale: a new time signature almost every bar.
#[test]
fn the_time_signature_can_change_every_bar() {
    let sacrale = time_signatures(&[
        ("3/16", 1),
        ("2/16", 1),
        ("3/16", 2),
        ("2/8", 1),
        ("5/16", 1),
        ("4/4", 1),
    ]);
    let starts: Vec<(u64, u64, String)> = sacrale
        .bars_from(Ticks(0))
        .take(8)
        .map(|bar| (bar.number, bar.start.0, bar.signature.to_string()))
        .collect();
    let expected = [
        (1, 0, "3/16"),
        (2, 720, "2/16"),
        (3, 1200, "3/16"),
        (4, 1920, "3/16"),
        (5, 2640, "2/8"),
        (6, 3600, "5/16"),
        (7, 4800, "4/4"),
        // The last run goes on.
        (8, 8640, "4/4"),
    ]
    .map(|(bar, tick, text)| (bar, tick, text.to_string()));
    assert_eq!(starts, expected);
    let changes: Vec<(u64, u64)> = sacrale
        .changes()
        .map(|bar| (bar.number, bar.start.0))
        .collect();
    assert_eq!(
        changes,
        [(1, 0), (2, 720), (3, 1200), (5, 2640), (6, 3600), (7, 4800)]
    );
    assert_eq!(sacrale.first(), signature("3/16"));

    assert_eq!(
        sacrale.bar_beat_of(Ticks(1200 + 480)).to_string(),
        "3:3:000"
    );
    assert_eq!(
        sacrale.bar_beat_of(Ticks(8640 + 3840 + 961)).to_string(),
        "9:2:001"
    );
    assert_eq!(sacrale.bar(9).map(|bar| bar.start), Some(Ticks(12_480)));
    assert_eq!(sacrale.bar(0), None);

    // The click: every sixteenth in the sixteenth bars, the downbeats on the bar lines.
    assert_eq!(sacrale.beat_from(Ticks(0)), (Ticks(0), true));
    assert_eq!(sacrale.beat_from(Ticks(1)), (Ticks(240), false));
    assert_eq!(sacrale.beat_from(Ticks(481)), (Ticks(720), true));
    assert_eq!(sacrale.beat_from(Ticks(2641)), (Ticks(2640 + 480), false));
    assert_eq!(sacrale.beat_from(Ticks(3121)), (Ticks(3600), true));
}

#[test]
fn time_signatures_need_a_run_and_whole_bars() {
    assert_eq!(
        TimeSignatures::new(Vec::new()),
        Err(ClockError::NoTimeSignature)
    );
    let error = |json: &str| {
        serde_json::from_str::<TimeSignatures>(json)
            .unwrap_err()
            .to_string()
    };
    assert!(error("[]").contains("time_signatures needs at least one entry"));
    assert!(error(r#"[{"signature": "4/4", "bars": 0}]"#).contains("nonzero"));
    assert!(error(r#"[{"signature": "4/4"}]"#).contains("missing field `bars`"));
    assert!(error(r#"[{"signature": "4/4", "bars": 1, "tempo": 3}]"#).contains("unknown field"));
}

#[test]
fn invalid_tempos_and_time_signatures_cannot_be_built() {
    for value in [0.0, 9.999, 1000.001, -120.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            Tempo::from_bpm(value),
            Err(ClockError::TempoOutOfRange(_))
        ));
    }
    assert_eq!(bpm(10.0).bpm(), 10.0);
    assert_eq!(bpm(1000.0).bpm(), 1000.0);
    assert_eq!(bpm(120.000_4).bpm(), 120.0);

    for (numerator, denominator) in [(0, 4), (33, 4), (4, 0), (4, 3), (4, 64)] {
        assert_eq!(
            TimeSignature::new(numerator, denominator),
            Err(ClockError::UnsupportedTimeSignature {
                numerator,
                denominator
            })
        );
    }
    assert_eq!("7/8".parse(), TimeSignature::new(7, 8));
    assert!(matches!(
        "waltz".parse::<TimeSignature>(),
        Err(ClockError::TimeSignatureSyntax(_))
    ));
}

#[test]
fn tempo_changes_must_start_at_zero_and_go_up() {
    let change = |tick, value| TempoChange {
        tick: Ticks(tick),
        bpm: bpm(value),
    };
    let new = |changes| TempoMap::new(TimeSignature::default(), changes);
    assert_eq!(new(vec![]), Err(ClockError::NoTempoAtStart));
    assert_eq!(new(vec![change(1, 120.0)]), Err(ClockError::NoTempoAtStart));
    assert_eq!(
        new(vec![change(0, 120.0), change(960, 90.0), change(960, 80.0)]),
        Err(ClockError::TempoChangesNotSorted(Ticks(960)))
    );
    assert_eq!(
        new(vec![change(0, 120.0), change(960, 90.0), change(480, 80.0)]),
        Err(ClockError::TempoChangesNotSorted(Ticks(480)))
    );
}

#[test]
fn the_saved_json_is_plain_bpm_and_ticks() {
    let tempo_map = TempoMap::new(
        TimeSignature::new(6, 8).unwrap(),
        vec![
            TempoChange {
                tick: Ticks(0),
                bpm: bpm(120.0),
            },
            TempoChange {
                tick: Ticks(15_360),
                bpm: bpm(93.5),
            },
        ],
    )
    .unwrap();
    let json = r#"{"time_signatures":[{"signature":"6/8","bars":1}],"tempo_changes":[{"tick":0,"bpm":120.0},{"tick":15360,"bpm":93.5}]}"#;
    assert_eq!(serde_json::to_string(&tempo_map).unwrap(), json);
    assert_eq!(serde_json::from_str::<TempoMap>(json).unwrap(), tempo_map);

    // What an agent may write by hand: whole numbers and spaces.
    let by_hand = r#"{ "time_signatures": [{ "signature": "6 / 8", "bars": 1 }], "tempo_changes": [ { "tick": 0, "bpm": 120 }, { "tick": 15360, "bpm": 93.5 } ] }"#;
    assert_eq!(
        serde_json::from_str::<TempoMap>(by_hand).unwrap(),
        tempo_map
    );

    // A project from before time signatures could change: one for the whole piece.
    let one = r#"{"time_signature":"6/8","tempo_changes":[{"tick":0,"bpm":120.0},{"tick":15360,"bpm":93.5}]}"#;
    assert_eq!(serde_json::from_str::<TempoMap>(one).unwrap(), tempo_map);
}

#[test]
fn invalid_json_is_rejected_with_the_reason() {
    let error = |json: &str| {
        serde_json::from_str::<TempoMap>(json)
            .unwrap_err()
            .to_string()
    };
    let too_fast = r#"{"time_signature":"4/4","tempo_changes":[{"tick":0,"bpm":5000}]}"#;
    assert!(error(too_fast).contains("5000 bpm is outside 10 to 1000 bpm"));
    let no_start = r#"{"time_signature":"4/4","tempo_changes":[{"tick":960,"bpm":120}]}"#;
    assert!(error(no_start).contains("the first tempo change must be at tick 0"));
    let odd_signature = r#"{"time_signature":"4/5","tempo_changes":[{"tick":0,"bpm":120}]}"#;
    assert!(error(odd_signature).contains("time signature 4/5 is not supported"));
    let odd_run = r#"{"time_signatures":[{"signature":"4/5","bars":1}],"tempo_changes":[{"tick":0,"bpm":120}]}"#;
    assert!(error(odd_run).contains("time signature 4/5 is not supported"));
    let both = r#"{"time_signature":"4/4","time_signatures":[{"signature":"4/4","bars":1}],"tempo_changes":[{"tick":0,"bpm":120}]}"#;
    assert!(error(both).contains("not as \"time_signature\" as well"));
    let neither = r#"{"tempo_changes":[{"tick":0,"bpm":120}]}"#;
    assert!(error(neither).contains("the tempo map needs \"time_signatures\""));
}

#[test]
fn one_tempo_change_of_a_map_can_be_set_by_its_tick() {
    let map = tempo_map(&[(0, 120.0), (3840, 60.0), (7680, 93.5)]);
    let changed = map.with_tempo_at(Ticks(3840), bpm(140.0)).unwrap();
    assert_eq!(changed.time_signatures(), map.time_signatures());
    assert_eq!(changed.tempo_changes()[0], map.tempo_changes()[0]);
    assert_eq!(changed.tempo_changes()[2], map.tempo_changes()[2]);
    assert_eq!(changed.tempo_changes()[1].tick, Ticks(3840));
    assert_eq!(changed.tempo_changes()[1].bpm, bpm(140.0));

    // A tick that no tempo change starts on has nothing to set.
    assert_eq!(map.with_tempo_at(Ticks(3839), bpm(140.0)), None);
    assert_eq!(map.with_tempo_at(Ticks(99_999), bpm(140.0)), None);
    // The result loads and validates like any other map.
    let text = serde_json::to_string(&changed).unwrap();
    assert_eq!(serde_json::from_str::<TempoMap>(&text).unwrap(), changed);
}

#[test]
fn a_tempo_change_is_added_with_the_tempo_in_effect_and_removed_by_its_tick() {
    let map = tempo_map(&[(0, 120.0), (3840, 60.0)]);
    assert_eq!(map.change_at(Ticks(3839)).tick, Ticks(0));
    assert_eq!(map.change_at(Ticks(99_999)).bpm, bpm(60.0));

    // Added in its place, with the tempo that played there, so nothing sounds different.
    let added = map.with_change_at(Ticks(1920)).unwrap();
    assert_eq!(added, tempo_map(&[(0, 120.0), (1920, 120.0), (3840, 60.0)]));
    let later = map.with_change_at(Ticks(7680)).unwrap();
    assert_eq!(later, tempo_map(&[(0, 120.0), (3840, 60.0), (7680, 60.0)]));
    // A tick that has a change already gets no second one.
    assert_eq!(map.with_change_at(Ticks(3840)), None);
    assert_eq!(map.with_change_at(Ticks(0)), None);

    // Removed by its tick. The change at tick 0 stays, and a tick without one has nothing.
    assert_eq!(added.without_change_at(Ticks(1920)), Some(map.clone()));
    assert_eq!(map.without_change_at(Ticks(0)), None);
    assert_eq!(map.without_change_at(Ticks(1920)), None);
    let text = serde_json::to_string(&added).unwrap();
    assert_eq!(serde_json::from_str::<TempoMap>(&text).unwrap(), added);
}

/// A map with a tempo change on every beat is the same piece at every sample rate.
///
/// Each change starts a segment at the exact moment its tick falls on, fraction of a frame and
/// all. A segment that began on a whole frame would throw a part of a frame away at every
/// change, and a thousand changes would then be a piece of a different length at 44.1 kHz than
/// at 96 kHz. This is what a fitted tempo map is made of, so it is not a corner case.
#[test]
fn a_tempo_change_per_beat_gives_the_same_piece_at_every_sample_rate() {
    let beats = 1500;
    let changes: Vec<TempoChange> = (0..beats)
        .map(|beat| TempoChange {
            tick: Ticks(beat * 960),
            // A tempo that gives and takes, and never a whole number of frames per beat.
            bpm: Tempo::from_bpm(96.0 + f64::from((beat % 37) as u32) * 0.137).unwrap(),
        })
        .collect();
    let map = TempoMap::new(signature("4/4"), changes).unwrap();
    let last = Ticks(beats * 960);

    let seconds = |rate: u32| {
        let clock = Clock::new(map.clone(), rate);
        clock.frame_of(last).0 as f64 / f64::from(rate)
    };
    let at_48 = seconds(48_000);
    for rate in [16_000, 44_100, 48_000, 88_200, 96_000, 192_000] {
        let difference = (seconds(rate) - at_48).abs();
        assert!(
            difference < 0.001,
            "{rate} Hz ends {difference} s from 48 kHz over {beats} beats"
        );
    }
    // And every tick still lands on the frame that holds it, and comes back.
    for rate in [16_000, 44_100, 96_000] {
        let clock = Clock::new(map.clone(), rate);
        for beat in [0_u64, 1, 2, 749, 1499, 1500] {
            let tick = Ticks(beat * 960 + 37);
            assert_eq!(clock.tick_at(clock.frame_of(tick)), tick, "{rate} Hz");
        }
    }
}

#[test]
fn a_tempo_reads_short() {
    assert_eq!(bpm(120.0).to_string(), "120");
    assert_eq!(bpm(93.5).to_string(), "93.5");
    assert_eq!(bpm(120.125).to_string(), "120.125");
    assert_eq!(bpm(10.0).to_string(), "10");
    assert_eq!(bpm(1000.0).to_string(), "1000");
}
