//! What the cards of the built-in devices read as their sound plays: feeds from the code that
//! makes the sound, and facts, through the one read a window makes per binding.

use std::time::{Duration, Instant};

use runtime::files::Files;
use serde_json::Value;
use sound_core::feeds::{self, Feed, FeedRequest, Readings};
use sound_core::{InstanceId, Ticks, facts};

use crate::support::{BAR, Harness, clip, synth, write_samples};

/// A frame of a window at 60 frames a second, in frames of the engine.
const FRAME: usize = 800;

fn request(points: usize, range: Option<[f64; 2]>) -> FeedRequest {
    FeedRequest {
        tick: Ticks(0),
        points,
        range,
    }
}

fn read(harness: &Harness, readings: &Readings, path: &str, name: &str, at: FeedRequest) -> Feed {
    let id = InstanceId::new(path).unwrap();
    feeds::read(&harness.project, readings, &Files::new(), &id, name, at)
        .unwrap_or_else(|| panic!("{path} has no feed {name}"))
}

fn number(feed: Feed) -> f32 {
    match feed {
        Feed::Number(value) => value,
        Feed::List(_) => panic!("a list where a number was read"),
    }
}

fn list(feed: Feed) -> Vec<f32> {
    match feed {
        Feed::List(values) => values,
        Feed::Number(_) => panic!("a number where a list was read"),
    }
}

/// The `y` of each pair of a curve.
fn heights(curve: &[f32]) -> Vec<f32> {
    curve.chunks(2).map(|pair| pair[1]).collect()
}

fn facts_of(harness: &Harness, path: &str) -> Value {
    let id = InstanceId::new(path).unwrap();
    facts::of(&harness.project, &Files::new(), &id, None)
        .unwrap()
        .to_value()
}

fn field<'a>(fields: &'a Value, name: &str) -> &'a Value {
    (fields.as_array().unwrap().iter())
        .find(|field| field["name"] == name)
        .unwrap_or_else(|| panic!("no field {name} in {fields}"))
}

/// A rack that plays a loud chord for four bars into `device`, named `device`.
fn chord_into(device: &str) -> Harness {
    let chord = clip(0, 15360, &[(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)]);
    Harness::rack(&[
        ("instrument", &synth(0.5)),
        ("chord", &chord),
        ("device", device),
    ])
}

/// Plays `frames` a frame of the window at a time, reading `read` after each, as a shown card.
fn play_frames(
    harness: &mut Harness,
    readings: &mut Readings,
    frames: usize,
    read: impl Fn(&Harness, &Readings),
) {
    for _ in 0..frames / FRAME {
        harness.play(FRAME);
        readings.next_frame(1.0 / 60.0);
        read(harness, readings);
    }
}

#[test]
fn the_compressor_curve_is_flat_under_its_threshold_and_a_loud_chord_is_turned_down() {
    let record =
        r#"{"tool": "compressor", "state": {"threshold_db": -40.0, "ratio": 4.0, "knee_db": 0.0}}"#;
    let mut harness = chord_into(record);
    let mut readings = Readings::new();
    let curve = list(read(
        &harness,
        &readings,
        "rack/device",
        "transfer",
        request(61, None),
    ));
    for pair in curve.chunks(2) {
        let (input, output) = (pair[0], pair[1]);
        let expected = match input <= -40.0 {
            true => input,
            false => -40.0 + (input + 40.0) / 4.0,
        };
        assert!((output - expected).abs() < 1e-3, "{input} gave {output}");
    }
    assert_eq!(curve.first(), Some(&-60.0));

    harness.play_from_the_start(BAR / 2);
    readings.next_frame(1.0 / 60.0);
    let reduction = number(read(
        &harness,
        &readings,
        "rack/device",
        "reduction",
        request(0, None),
    ));
    let level = number(read(
        &harness,
        &readings,
        "rack/device",
        "level",
        request(0, None),
    ));
    assert!(
        reduction > 3.0 && level > -40.0,
        "{reduction} dB down at {level} dBFS"
    );
    // Nothing played since: the next frame shows rest.
    readings.next_frame(1.0 / 60.0);
    let reduction = number(read(
        &harness,
        &readings,
        "rack/device",
        "reduction",
        request(0, None),
    ));
    assert_eq!(reduction, 0.0);
}

#[test]
fn the_gate_curve_steps_up_at_its_threshold() {
    let record = r#"{"tool": "gate", "state": {"threshold_db": -30.0, "range_db": 40.0}}"#;
    let harness = chord_into(record);
    let curve = list(read(
        &harness,
        &Readings::new(),
        "rack/device",
        "transfer",
        request(0, None),
    ));
    assert_eq!(curve, [-60.0, -100.0, -30.0, -70.0, -30.0, -30.0, 0.0, 0.0]);
}

#[test]
fn the_limiter_shows_its_reduction_and_the_last_seconds_of_its_output_while_it_plays() {
    let record = r#"{"tool": "limiter", "state": {"gain_db": 12.0, "ceiling_db": -6.0}}"#;
    let mut harness = chord_into(record);
    let mut readings = Readings::new();
    let history = |harness: &Harness, readings: &Readings| {
        list(read(
            harness,
            readings,
            "rack/device",
            "history",
            request(0, None),
        ))
    };
    assert!(history(&harness, &readings).iter().all(|db| *db == -120.0));
    harness.play_from_the_start(64);
    play_frames(&mut harness, &mut readings, BAR / 4, |harness, readings| {
        history(harness, readings);
    });
    let reduction = number(read(
        &harness,
        &readings,
        "rack/device",
        "reduction",
        request(0, None),
    ));
    assert!(reduction > 1.0, "{reduction}");
    readings.next_frame(0.0);
    let history = history(&harness, &readings);
    assert_eq!(history.len(), 50);
    // A quarter of a bar is half a second: the last six columns of 80 ms hold the chord, at
    // the ceiling.
    let last = &history[44..];
    assert!(last.iter().all(|db| (-7.0..=-5.9).contains(db)), "{last:?}");
    assert_eq!(history[0], -120.0);
}

#[test]
fn a_low_pass_is_3_db_down_at_its_cutoff_and_an_eq_bell_has_its_gain_at_its_frequency() {
    let low_pass = r#"{"tool": "filter", "state": {"type": "low_pass", "cutoff_hz": 1000.0, "resonance": 0.0}}"#;
    let harness = chord_into(low_pass);
    let at = |hz: f64| (hz / 20.0).log2();
    let one = |harness: &Harness, hz| {
        let at = request(2, Some([at(hz), at(hz)]));
        heights(&list(read(
            harness,
            &Readings::new(),
            "rack/device",
            "response",
            at,
        )))[0]
    };
    let cutoff = one(&harness, 1000.0);
    assert!((-3.3..=-2.7).contains(&cutoff), "{cutoff}");
    assert!(one(&harness, 50.0).abs() < 0.1);
    assert!(one(&harness, 10_000.0) < -30.0);

    let bell = r#"{"tool": "eq", "state": {"bands": [{"on": true, "shape": "bell", "frequency_hz": 1000.0, "gain_db": 6.0, "q": 1.0}]}}"#;
    let harness = chord_into(bell);
    assert!((one(&harness, 1000.0) - 6.0).abs() < 0.05);
    assert!(one(&harness, 30.0).abs() < 0.05);
    // The whole response by default: 20 Hz to 20 kHz.
    let curve = list(read(
        &harness,
        &Readings::new(),
        "rack/device",
        "response",
        request(157, None),
    ));
    assert_eq!(curve.len(), 2 * 157);
    assert_eq!(curve[0], 0.0);
    assert!((20.0 * curve[curve.len() - 2].exp2() - 20_000.0).abs() < 1.0);
}

#[test]
fn the_analyzer_hears_the_note_that_plays_and_shows_its_spectrum() {
    let a4 = clip(0, 15360, &[(0, 15360, 69)]);
    let mut harness = Harness::rack(&[
        ("instrument", &synth(0.3)),
        ("held", &a4),
        ("device", r#"{"tool": "analyzer", "state": {}}"#),
    ]);
    let mut readings = Readings::new();
    harness.play_from_the_start(64);
    play_frames(&mut harness, &mut readings, BAR / 4, |harness, readings| {
        read(
            harness,
            readings,
            "rack/device",
            "spectrum",
            request(0, None),
        );
    });
    let note = number(read(
        &harness,
        &readings,
        "rack/device",
        "note",
        request(0, None),
    ));
    assert!((note - 69.0).abs() < 0.3, "{note}");
    let loudness = number(read(
        &harness,
        &readings,
        "rack/device",
        "loudness",
        request(0, None),
    ));
    assert!(loudness > -40.0, "{loudness}");
    let spectrum = list(read(
        &harness,
        &readings,
        "rack/device",
        "spectrum",
        request(0, None),
    ));
    assert_eq!(spectrum.len(), 96);
    // The column of 440 Hz is far above the column of 50 Hz.
    let column = |hz: f32| ((hz / 20.0).log2() / 1000_f32.log2() * 96.0) as usize;
    assert!(
        spectrum[column(440.0)] > spectrum[column(50.0)] + 30.0,
        "{spectrum:?}"
    );
}

#[test]
fn a_drum_pad_shows_the_level_of_the_pad_that_plays_and_names_its_pads() {
    let kick = clip(0, 15360, &[(0, 480, 36)]);
    let mut harness = Harness::rack(&[
        ("instrument", r#"{"tool": "drum-pad", "state": {}}"#),
        ("beat", &kick),
    ]);
    let mut readings = Readings::new();
    harness.play_from_the_start(FRAME * 4);
    readings.next_frame(1.0 / 60.0);
    let levels = list(read(
        &harness,
        &readings,
        "rack/instrument",
        "levels",
        request(0, None),
    ));
    assert_eq!(levels.len(), 16);
    assert!(levels[0] > 0.05, "{levels:?}");
    assert!(levels[1..].iter().all(|level| *level == 0.0), "{levels:?}");
    let level = number(read(
        &harness,
        &readings,
        "rack/instrument",
        "levels.0",
        request(0, None),
    ));
    assert_eq!(level, levels[0]);

    let facts = facts_of(&harness, "rack/instrument");
    assert_eq!(facts["pads"][0]["name"], "Kick");
    assert_eq!(facts["pads"][0]["note"], 36);
    assert_eq!(facts["sounds"][5]["value"], "open_hat");
    assert_eq!(field(&facts["fields"], "pads")["kind"], "map");
}

/// The wave of a file comes from a thread of its own: read again until it is there.
fn wave_when_read(harness: &Harness, path: &str, at: FeedRequest) -> Vec<f32> {
    let (files, readings, id) = (
        Files::new(),
        Readings::new(),
        InstanceId::new(path).unwrap(),
    );
    let started = Instant::now();
    loop {
        let wave = feeds::read(&harness.project, &readings, &files, &id, "wave", at).unwrap();
        let wave = list(wave);
        if !wave.is_empty() || started.elapsed() > Duration::from_secs(10) {
            return wave;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_sampler_shows_its_file_and_where_its_last_note_plays() {
    let harness = Harness::new();
    // Quiet for its first half second, then loud.
    write_samples(&harness, "rise.wav", 48_000, 1.0, |second| {
        match second < 0.5 {
            true => 0.1,
            false => 0.8,
        }
    });
    let held = clip(0, 7680, &[(0, 7680, 60)]);
    let sampler = r#"{"tool": "sampler", "state": {"sample": "rise.wav"}}"#;
    let mut harness = harness.with_racks(&[("rack", &[("instrument", sampler), ("held", &held)])]);
    let facts = facts_of(&harness, "rack/instrument");
    assert_eq!(facts["file"]["state"], "plays");
    assert_eq!(facts["file"]["seconds"], 1.0);

    let wave = wave_when_read(&harness, "rack/instrument", request(4, None));
    assert_eq!(wave.len(), 4);
    assert!(
        (wave[0] - 0.1).abs() < 1e-3 && (wave[3] - 0.8).abs() < 1e-3,
        "{wave:?}"
    );
    // The second half alone, as a view zoomed in shows it.
    let zoomed = wave_when_read(&harness, "rack/instrument", request(2, Some([0.5, 1.0])));
    assert!(
        zoomed.iter().all(|peak| (peak - 0.8).abs() < 1e-3),
        "{zoomed:?}"
    );

    let mut readings = Readings::new();
    harness.play_from_the_start(BAR / 8);
    readings.next_frame(1.0 / 60.0);
    let position = number(read(
        &harness,
        &readings,
        "rack/instrument",
        "position",
        request(0, None),
    ));
    assert!((0.2..0.3).contains(&position), "{position}");
}

#[test]
fn a_missing_sample_says_so_in_the_facts_of_its_sampler() {
    let held = clip(0, 7680, &[(0, 7680, 60)]);
    let sampler = r#"{"tool": "sampler", "state": {"sample": "gone.wav"}}"#;
    let harness = Harness::rack(&[("instrument", sampler), ("held", &held)]);
    let facts = facts_of(&harness, "rack/instrument");
    assert_eq!(facts["file"]["name"], "gone.wav");
    assert_eq!(facts["file"]["state"], "missing");
    let wave = read(
        &harness,
        &Readings::new(),
        "rack/instrument",
        "wave",
        request(0, None),
    );
    assert_eq!(list(wave), Vec::<f32>::new());
}

#[test]
fn a_region_shows_its_wave_with_its_gain_and_its_fade_in() {
    // A project with tools of its own, which brings the region.
    let mut harness = Harness::rack(&[]);
    write_samples(&harness, "steady.wav", 48_000, 2.0, |_| 0.5);
    let region = r#"{"tool": "region", "state": {"asset": "steady.wav", "gain_db": -6.0206, "fade_in_ms": 1000.0}}"#;
    harness.write_and_apply("state/take.json", region);
    assert_eq!(harness.project.problems(), []);
    let facts = facts_of(&harness, "take");
    assert_eq!(facts["file"]["state"], "plays");
    assert!(
        (facts["seconds"].as_f64().unwrap() - 2.0).abs() < 1e-3,
        "{facts}"
    );

    // Four columns of half a second: fading in over the first two, then at half the file.
    let wave = wave_when_read(&harness, "take", request(4, None));
    let expected = [0.25 * 0.25, 0.25 * 0.75, 0.25, 0.25];
    for (peak, expected) in wave.iter().zip(expected) {
        assert!((peak - expected).abs() < 2e-3, "{wave:?}");
    }
}

/// An instrument of the project: a sine for each key down.
const PIANO: &str = r#"import { note, sine, tool } from "./sdk";
export const piano = tool({
  name: "piano", title: "Piano", when: "Never", doc: "Keys.", state: {}, voices: 4,
  sound: () => sine(note.freq).times(note.gate),
});
"#;

#[test]
fn the_keys_down_on_a_sound_are_a_feed_of_every_instance() {
    let harness = Harness::new();
    harness.write("extensions/piano.tsx", PIANO);
    let chord = clip(0, 15360, &[(0, 15360, 60), (0, 15360, 64)]);
    let piano = r#"{"tool": "piano", "state": {}}"#;
    let mut harness = harness.with_racks(&[("rack", &[("instrument", piano), ("chord", &chord)])]);
    assert_eq!(harness.project.problems(), []);
    let readings = Readings::new();
    harness.play_from_the_start(FRAME);
    let down = |pitch: u8| {
        let name = format!("sounding.{pitch}");
        number(read(
            &harness,
            &readings,
            "rack/instrument",
            &name,
            request(0, None),
        ))
    };
    assert_eq!((down(60), down(61), down(64)), (1.0, 0.0, 1.0));
}

#[test]
fn the_facts_of_an_eq_and_a_wavetable_show_their_nested_fields() {
    let harness = chord_into(r#"{"tool": "eq", "state": {}}"#);
    let facts = facts_of(&harness, "rack/device");
    assert_eq!(facts["title"], "Eq");
    let bands = field(&facts["fields"], "bands");
    assert_eq!(bands["kind"], "list");
    assert_eq!(bands["max_items"], 4);
    let frequency = field(&bands["item"]["fields"], "frequency_hz");
    assert_eq!(
        (&frequency["scale"], &frequency["unit"]),
        (&"logarithmic".into(), &"hz".into())
    );
    assert_eq!(field(&bands["item"]["fields"], "shape")["kind"], "choice");

    let notes = clip(0, 3840, &[(0, 3840, 60)]);
    let harness = Harness::rack(&[
        ("instrument", r#"{"tool": "wavetable", "state": {}}"#),
        ("notes", &notes),
    ]);
    let facts = facts_of(&harness, "rack/instrument");
    let oscillator = field(&facts["fields"], "osc_1");
    assert_eq!(oscillator["kind"], "group");
    assert_eq!(field(&oscillator["fields"], "position")["kind"], "number");
    assert_eq!(field(&facts["fields"], "matrix")["kind"], "list");
    assert_eq!(facts["tables"][7]["name"], "FM Sweep");
}
