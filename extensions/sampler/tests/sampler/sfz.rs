//! SFZ instruments: the zone a key plays by key, velocity, round robin and keyswitch, release
//! zones, loops, choke groups, and packs whose files are missing or arrive late.
//!
//! Every sample is a steady level, played at its keycenter with no velocity tracking, so the
//! output names the zone that plays.

use sampler::{SamplerState, SfzPath};

use crate::support::{Harness, SAMPLE_RATE, note, write_wav};

const PACK: &str = "assets/instruments/pack";

/// A Sampler that plays `assets/instruments/pack/pack.sfz`.
fn pack() -> SamplerState {
    SamplerState {
        sfz: Some(SfzPath::new("pack/pack.sfz").unwrap()),
        ..SamplerState::default()
    }
}

/// Writes the pack: its SFZ text and steady samples, each `(name, level, seconds)`.
fn write_pack(harness: &Harness, sfz: &str, samples: &[(&str, f32, f64)]) {
    for (name, level, seconds) in samples {
        let frames = (seconds * f64::from(SAMPLE_RATE)) as usize;
        let path = harness.path(&format!("{PACK}/{name}"));
        write_wav(&path, SAMPLE_RATE, &vec![*level; frames]);
    }
    std::fs::write(harness.path(&format!("{PACK}/pack.sfz")), sfz).unwrap();
}

/// A harness with the pack and one track that plays these notes on it.
fn playing(sfz: &str, samples: &[(&str, f32, f64)], notes: Vec<sound_notes::Note>) -> Harness {
    let mut harness = Harness::with_samples(&[]);
    write_pack(&harness, sfz, samples);
    harness.add_track(notes, pack());
    assert_eq!(harness.project.problems(), []);
    harness
}

#[test]
fn a_key_plays_the_zone_of_its_key_and_velocity() {
    let mut harness = playing(
        "<global> amp_veltrack=0
         <region> sample=samples/soft.wav key=60 hivel=63
         <region> sample=samples/loud.wav key=60 lovel=64
         <region> sample=samples/high.wav key=c5",
        &[
            ("samples/soft.wav", 0.1, 1.0),
            ("samples/loud.wav", 0.2, 1.0),
            ("samples/high.wav", 0.3, 1.0),
        ],
        vec![
            note(0, 4_800, 60, 50),
            note(9_600, 4_800, 60, 100),
            note(19_200, 4_800, 72, 100),
            // No zone has this key: silence.
            note(28_800, 4_800, 61, 100),
        ],
    );
    let left = harness.play(33_600);
    assert_eq!([left[2_400], left[12_000], left[21_600]], [0.1, 0.2, 0.3]);
    assert_eq!(left[31_200], 0.0);
}

#[test]
fn round_robin_zones_take_turns() {
    let mut harness = playing(
        "<group> key=60 amp_veltrack=0 seq_length=2
         <region> sample=a.wav seq_position=1
         <region> sample=b.wav seq_position=2",
        &[("a.wav", 0.1, 1.0), ("b.wav", 0.2, 1.0)],
        (0..3).map(|n| note(n * 9_600, 4_800, 60, 100)).collect(),
    );
    let left = harness.play(28_800);
    assert_eq!([left[2_400], left[12_000], left[21_600]], [0.1, 0.2, 0.1]);
}

#[test]
fn a_keyswitch_picks_the_articulation_and_plays_nothing_itself() {
    let mut harness = playing(
        "<global> amp_veltrack=0 sw_lokey=24 sw_hikey=25 sw_default=24
         <region> sw_last=24 sample=legato.wav key=60
         <region> sw_last=25 sample=pizzicato.wav key=60",
        &[("legato.wav", 0.1, 1.0), ("pizzicato.wav", 0.2, 1.0)],
        vec![
            note(0, 4_800, 60, 100),
            note(9_600, 4_800, 25, 100),
            note(19_200, 4_800, 60, 100),
        ],
    );
    let left = harness.play(28_800);
    assert_eq!([left[2_400], left[12_000], left[21_600]], [0.1, 0.0, 0.2]);
}

#[test]
fn a_release_zone_plays_when_the_key_comes_up() {
    let mut harness = playing(
        "<group> key=60 amp_veltrack=0
         <region> sample=string.wav
         <region> sample=damper.wav trigger=release",
        &[("string.wav", 0.1, 1.0), ("damper.wav", 0.2, 1.0)],
        vec![note(0, 4_800, 60, 100)],
    );
    let left = harness.play(9_600);
    assert_eq!([left[2_400], left[7_200]], [0.1, 0.2]);
}

/// The file is 0.1 s long and the note half a second: the loop holds it.
#[test]
fn a_looped_zone_sounds_past_the_end_of_its_file() {
    let mut harness = playing(
        "<region> sample=a.wav key=60 amp_veltrack=0
           loop_mode=loop_continuous loop_start=1000 loop_end=3999",
        &[("a.wav", 0.25, 0.1)],
        vec![note(0, 24_000, 60, 100)],
    );
    let left = harness.play(24_000);
    assert_eq!(left[20_000], 0.25);
}

/// A closed hi-hat cuts the open one, which would ring on without it.
#[test]
fn a_choke_group_stops_the_zones_it_cuts() {
    let mut harness = playing(
        "<global> amp_veltrack=0
         <region> sample=open.wav key=46 group=2 off_by=1
         <region> sample=closed.wav key=42 group=1",
        &[("open.wav", 0.1, 1.0), ("closed.wav", 0.2, 1.0)],
        vec![note(0, 24_000, 46, 100), note(9_600, 2_400, 42, 100)],
    );
    let left = harness.play(19_200);
    assert_eq!([left[4_800], left[10_800], left[16_800]], [0.1, 0.2, 0.0]);
}

#[test]
fn missing_samples_are_a_problem_and_the_rest_plays() {
    let mut harness = Harness::with_samples(&[]);
    write_pack(
        &harness,
        "<region> sample=here.wav key=60 amp_veltrack=0
         <region> sample=gone.wav key=62",
        &[("here.wav", 0.1, 1.0)],
    );
    harness.add_track(vec![note(0, 4_800, 60, 100)], pack());
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "the Sampler plays assets/instruments/pack/pack.sfz with gaps: 1 sample of assets/instruments/pack/pack.sfz is missing, such as gone.wav"
    );
    assert_eq!(harness.play(4_800)[2_400], 0.1);
}

/// An agent may write the record before it copies the pack in.
#[test]
fn a_pack_that_arrives_after_its_record_plays() {
    let mut harness = Harness::with_samples(&[]);
    harness.add_track(vec![note(0, 4_800, 60, 100)], pack());
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "the instrument assets/instruments/pack/pack.sfz is not there, so the Sampler is silent. Put the SFZ file and its samples under assets/instruments/, or correct `sfz`"
    );
    write_pack(
        &harness,
        "<region> sample=a.wav key=60 amp_veltrack=0",
        &[("a.wav", 0.1, 1.0)],
    );
    let sfz = harness.path(&format!("{PACK}/pack.sfz"));
    harness.project.apply_outside_changes(&[sfz]).unwrap();
    assert_eq!(harness.project.problems(), []);
    assert_eq!(harness.play(4_800)[2_400], 0.1);
}
