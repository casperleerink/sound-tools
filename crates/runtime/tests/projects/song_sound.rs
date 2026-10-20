//! The song of the DAW template (`templates/daw`) sounds as main's arrangement did: a muted or
//! soloed-out track sends nothing, before its fader either; a solo silences a track whose
//! volume a lane moves; an audio track sends into a return; a pitch two clips hold sounds until
//! both end; regions that meet hand over with no dip, and of two that overlap one is heard.

use crate::shared_song::song_files;
use crate::support::{BAR, Harness, clip, write, write_samples};

const SILENT: &str = r#"{"tool": "fader", "state": {"gain_db": "-inf"}}"#;
const PRE_FADER: &str = r#"{"tool": "bus", "state": {"title": "Room", "pre_fader": true}}"#;

/// The song of two tracks with a clip each, sending into the reverb `ret-a`, with `extra`
/// written over it.
fn song(extra: &[(&str, &str)]) -> Harness {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(2, 2) {
        write(folder.path(), &path, &text);
    }
    for (path, text) in extra {
        write(folder.path(), path, text);
    }
    let harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    harness
}

/// Plays the song from its start once every fader has glided to what its record says.
fn play(harness: &mut Harness, frames: usize) -> Vec<f32> {
    harness.render(9_600);
    harness.play_from_the_start(frames)
}

fn loudest(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |most, sample| most.max(sample.abs()))
}

/// Into a return that hears before the faders, a muted track sends nothing, nor does one that
/// another track's solo silences, and the soloed one still does.
#[test]
fn a_silent_track_sends_nothing_before_its_fader() {
    let muted = r#"{"tool": "fader", "state": {"mute": true}}"#;
    let mut harness = song(&[
        ("state/song/ret-a/instance.json", PRE_FADER),
        ("state/song/t00/mix.json", muted),
        ("state/song/t01/mix.json", muted),
    ]);
    assert_eq!(loudest(&play(&mut harness, BAR)), 0.0);

    // `t00` soloed plays only through its send, before its fader; `t01` sends nothing.
    let soloed = |t00_sends: &str| {
        let mut harness = song(&[
            ("state/song/ret-a/instance.json", PRE_FADER),
            (
                "state/song/instance.json",
                r#"{"tool": "song", "state": {"solo": ["t00"]}}"#,
            ),
            ("state/song/t00/mix.json", SILENT),
            ("state/song/t00/send-ret-a.json", t00_sends),
        ]);
        loudest(&play(&mut harness, BAR))
    };
    assert!(soloed(r#"{"tool": "fader", "state": {}}"#) > 1e-3);
    assert_eq!(soloed(SILENT), 0.0);
}

/// A lane that moves a track's volume does not keep it playing through another track's solo.
#[test]
fn a_solo_silences_a_track_whose_volume_a_lane_moves() {
    let automated = r#"{"tool": "track", "state": {"automation": [{"lane": "lane-vol", "target": "mix.gain_db"}]}}"#;
    let mut harness = song(&[
        ("state/song/t00/instance.json", automated),
        (
            "state/song/t00/lane-vol.json",
            r#"{"tool": "lane", "state": {"points": [{"tick": 0, "value": 0}]}}"#,
        ),
        ("state/song/t01/mix.json", SILENT),
        ("state/song/ret-a/mix.json", SILENT),
    ]);
    assert!(loudest(&play(&mut harness, BAR)) > 1e-3);
    harness.write_and_apply(
        "state/song/instance.json",
        r#"{"tool": "song", "state": {"solo": ["t01"]}}"#,
    );
    assert_eq!(loudest(&play(&mut harness, BAR)), 0.0);
}

/// An audio track sends into a return as a track does: here before its fader, which is down.
#[test]
fn an_audio_track_sends_into_a_return() {
    let mut harness = song(&[
        ("state/song/ret-a/instance.json", PRE_FADER),
        ("state/song/t00/mix.json", SILENT),
        ("state/song/t01/mix.json", SILENT),
        ("state/song/t00/send-ret-a.json", SILENT),
        ("state/song/t01/send-ret-a.json", SILENT),
        ("state/song/audio-voice/mix.json", SILENT),
    ]);
    write_samples(&harness, "tone.wav", 48_000, 1.0, |seconds| {
        (0.2 * (seconds * 220.0 * std::f64::consts::TAU).sin()) as f32
    });
    harness.write_and_apply(
        "state/song/audio-voice/tone.json",
        r#"{"tool": "region", "state": {"asset": "tone.wav", "start": 0}}"#,
    );
    assert_eq!(loudest(&play(&mut harness, BAR)), 0.0);
    harness.write_and_apply(
        "state/song/audio-voice/send-ret-a.json",
        r#"{"tool": "fader", "state": {}}"#,
    );
    assert_eq!(harness.project.problems(), []);
    let sent = play(&mut harness, BAR);
    assert!(loudest(&sent) > 1e-3, "{}", loudest(&sent));
}

/// A volume lane of scale `"fader"` moves along the fader's travel, as main's lanes moved a
/// volume and as the lane is drawn: halfway from -100 dB to 0 dB is about -18.6 dB, not -50.
#[test]
fn a_volume_lane_moves_along_the_faders_travel() {
    let automated = r#"{"tool": "track", "state": {"automation": [{"lane": "lane-vol", "target": "mix.gain_db"}]}}"#;
    let render = |lane: &str| {
        let mut harness = song(&[
            ("state/song/t00/instance.json", automated),
            ("state/song/t00/lane-vol.json", lane),
            ("state/song/t01/mix.json", SILENT),
            ("state/song/ret-a/mix.json", SILENT),
        ]);
        play(&mut harness, BAR)
    };
    let steady = render(r#"{"tool": "lane", "state": {"points": [{"tick": 0, "value": 0}]}}"#);
    let fade = render(
        r#"{"tool": "lane", "state": {"points": [{"tick": 0, "value": -100}, {"tick": 3840, "value": 0}], "scale": "fader"}}"#,
    );
    // Half a bar in, over 10 ms.
    let middle = 2 * (BAR / 2 - 240)..2 * (BAR / 2 + 240);
    let energy = |samples: &[f32]| samples.iter().map(|sample| sample * sample).sum::<f32>();
    let ratio = (energy(&fade[middle.clone()]) / energy(&steady[middle])).sqrt();
    let decibels = 20.0 * ratio.log10();
    assert!((-20.0..-17.0).contains(&decibels), "{decibels} dB");
}

/// Two clips of a track that hold one pitch at once hold it until the later note ends, as main's
/// arrangement did: the end of the first note does not cut the second.
#[test]
fn a_pitch_two_clips_hold_sounds_until_both_notes_end() {
    let mut harness = song(&[
        ("state/song/t00/c00.json", &clip(0, 3840, &[(0, 1920, 60)])),
        (
            "state/song/t00/c01.json",
            &clip(960, 2880, &[(0, 2880, 60)]),
        ),
        ("state/song/t01/mix.json", SILENT),
        ("state/song/ret-a/mix.json", SILENT),
    ]);
    let samples = play(&mut harness, BAR);
    // From three quarters of the bar on only the second clip's note holds the pitch.
    let held = loudest(&samples[2 * BAR * 3 / 4..2 * BAR * 15 / 16]);
    assert!(held > 1e-2, "{held}");
}

/// The regions of the audio track, of a file that holds 0.25 from start to end, with the other
/// tracks and the returns down. What the song plays over the first second.
fn regions(records: &[(&str, &str)]) -> Vec<f32> {
    let mut harness = song(&[
        ("state/song/t00/mix.json", SILENT),
        ("state/song/t01/mix.json", SILENT),
        ("state/song/ret-a/mix.json", SILENT),
        ("state/song/ret-b/mix.json", SILENT),
    ]);
    write_samples(&harness, "level.wav", 48_000, 2.0, |_| 0.25);
    for (name, record) in records {
        harness.write_and_apply(&format!("state/song/audio-voice/{name}.json"), record);
    }
    assert_eq!(harness.project.problems(), []);
    let samples = play(&mut harness, 48_000);
    // The left channel.
    samples.iter().step_by(2).copied().collect()
}

/// Where one region ends and the next starts, the level holds, as main's audio clips handed over:
/// one player per track crossfades the two.
#[test]
fn two_regions_that_meet_hand_over_with_no_dip() {
    let left = regions(&[
        (
            "a",
            r#"{"tool": "region", "state": {"asset": "level.wav", "file_end_seconds": 0.5}}"#,
        ),
        (
            "b",
            r#"{"tool": "region", "state": {"asset": "level.wav", "start": 960, "file_start_seconds": 0.5}}"#,
        ),
    ]);
    let level = left[12_000];
    assert!(level > 0.1, "{level}");
    let lowest = left[22_000..26_000]
        .iter()
        .fold(f32::MAX, |low, sample| low.min(*sample));
    assert!(
        lowest > level * 0.95,
        "{lowest} at the join, {level} before it"
    );
}

/// Where two regions overlap, the last by name is heard, as on main's audio tracks, and not the
/// two summed.
#[test]
fn of_two_regions_that_overlap_the_last_by_name_is_heard() {
    let left = regions(&[
        (
            "a",
            r#"{"tool": "region", "state": {"asset": "level.wav", "file_end_seconds": 1.0}}"#,
        ),
        (
            "b",
            r#"{"tool": "region", "state": {"asset": "level.wav", "start": 960, "file_end_seconds": 0.25}}"#,
        ),
    ]);
    let (alone, over) = (left[12_000], left[30_000]);
    assert!(alone > 0.1, "{alone}");
    assert!(
        (over - alone).abs() < 1e-3,
        "{over} where they overlap, {alone} before"
    );
}
