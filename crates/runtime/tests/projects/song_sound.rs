//! The song of the DAW template (`templates/daw`) sounds as main's arrangement did: a muted or
//! soloed-out track sends nothing, before its fader either; a solo silences a track whose
//! volume a lane moves; an audio track sends into a return.

use crate::shared_song::song_files;
use crate::support::{BAR, Harness, write, write_samples};

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
