//! The mixer of the TypeScript song of the DAW template (`templates/daw`): a solo silences the other tracks and never a return, a send goes
//! after the track's fader unless its return says before, the master's limiter holds the output
//! under its ceiling, and an audio region counts toward the end of the song.

use sound_core::{Ticks, amplitude};

use crate::shared_song::song_files;
use crate::support::{BAR, Harness, difference, write, write_samples};

/// Two tracks of one clip each from the start, sending into the reverb `ret-a`.
fn song() -> Harness {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(2, 2) {
        write(folder.path(), &path, &text);
    }
    let harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn fader(gain: &str) -> String {
    format!(r#"{{"tool": "fader", "state": {{"gain_db": {gain}}}}}"#)
}

/// Plays the song from its start once every fader has glided to what its record says: a change
/// glides over 20 ms.
fn play(harness: &mut Harness, frames: usize) -> Vec<f32> {
    harness.render(9_600);
    harness.play_from_the_start(frames)
}

fn loudest(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |most, sample| most.max(sample.abs()))
}

/// A solo turns the other track's fader down to silence: once that glide is over, the song
/// plays as with that fader at `-inf`. The reverb return still plays the soloed track's send.
#[test]
fn a_solo_silences_the_other_tracks_and_never_a_return() {
    let mut harness = song();
    let frames = 2 * BAR;
    harness.write_and_apply(
        "state/song/instance.json",
        r#"{"tool": "song", "state": {"solo": ["t00"]}}"#,
    );
    let soloed = play(&mut harness, frames);
    harness.write_and_apply(
        "state/song/instance.json",
        r#"{"tool": "song", "state": {}}"#,
    );
    harness.write_and_apply("state/song/t01/mix.json", &fader(r#""-inf""#));
    let muted = play(&mut harness, frames);
    // What tells them apart is the tail of the reverb from the render before, far under a
    // track's level.
    let apart = (soloed.iter().zip(&muted)).fold(0.0_f32, |most, (a, b)| most.max((a - b).abs()));
    assert!(apart < 1e-3 * loudest(&muted), "{apart}");
    harness.write_and_apply("state/song/ret-a/mix.json", &fader(r#""-inf""#));
    let dry = play(&mut harness, frames);
    assert!(difference(&muted, &dry).is_some(), "the return plays");
}

/// With every track's fader at `-inf`, a send after the fader sends nothing; one before it,
/// as a return with `pre_fader` hears, still sends.
#[test]
fn a_send_goes_after_the_fader_unless_its_return_hears_before_it() {
    let mut harness = song();
    for track in ["t00", "t01"] {
        harness.write_and_apply(&format!("state/song/{track}/mix.json"), &fader(r#""-inf""#));
    }
    let after = play(&mut harness, 2 * BAR);
    assert_eq!(loudest(&after), 0.0);
    harness.write_and_apply(
        "state/song/ret-a/instance.json",
        r#"{"tool": "bus", "state": {"title": "Room", "pre_fader": true}}"#,
    );
    let before = play(&mut harness, 2 * BAR);
    assert!(loudest(&before) > 1e-3, "{}", loudest(&before));
}

/// The master turned up 6 dB into a limiter whose ceiling is -18 dB: no sample goes over it.
#[test]
fn the_limiter_after_the_master_holds_the_output_under_its_ceiling() {
    let mut harness = song();
    let open = play(&mut harness, 2 * BAR);
    let ceiling = amplitude(-18.0);
    assert!(loudest(&open) > ceiling, "{}", loudest(&open));
    harness.write_and_apply("state/song/master.json", &fader("6"));
    harness.write_and_apply(
        "state/song/limit.json",
        r#"{"tool": "limiter", "state": {"ceiling_db": -18, "lookahead_ms": 0}}"#,
    );
    let limited = play(&mut harness, 2 * BAR);
    assert!(loudest(&limited) <= ceiling + 1e-6, "{}", loudest(&limited));
}

/// The song lasts to the end of the last region.
#[test]
fn the_last_region_ends_the_song() {
    let mut harness = song();
    write_samples(&harness, "high.wav", 48_000, 2.0, |_| 0.1);
    // Two seconds are a bar at 120 bpm: from bar 21 the region ends at bar 22.
    let start = 20 * 3840;
    harness.write_and_apply(
        "state/song/audio-voice/high.json",
        &format!(r#"{{"tool": "region", "state": {{"asset": "high.wav", "start": {start}}}}}"#),
    );
    assert_eq!(harness.project.end(), Some(Ticks(start + 3840)));
}

/// A compressor in the chain of `t00` that turns down anything over -60 dB, hard and fast, or
/// passed by when `bypassed`, and `t01` at `t01_gain`, with the song keying the compressor by
/// `t01` at the tap `keyed` names, such as `post_fader`.
fn ducked(keyed: Option<&str>, t01_gain: &str, bypassed: bool) -> Vec<f32> {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(2, 2) {
        let text = match path.as_str() {
            "project.json" => text.replace(r#""wavetable"]"#, r#""wavetable", "compressor"]"#),
            _ => text,
        };
        write(folder.path(), &path, &text);
    }
    let compressor = r#"{"tool": "compressor", "state": {"threshold_db": -60, "ratio": 20, "attack_ms": 0.1, "release_ms": 50}}"#;
    write(folder.path(), "state/song/t00/chain/comp.json", compressor);
    let off = if bypassed { r#"["comp"]"# } else { "[]" };
    let chain = format!(r#"{{"tool": "chain", "state": {{"order": ["comp"], "off": {off}}}}}"#);
    write(folder.path(), "state/song/t00/chain/instance.json", &chain);
    write(folder.path(), "state/song/t01/mix.json", &fader(t01_gain));
    let sidechains = match keyed {
        Some(tap) => format!(r#"[{{"effect": "t00/chain/comp", "from": "t01", "tap": "{tap}"}}]"#),
        None => "[]".to_string(),
    };
    let song = format!(r#"{{"tool": "song", "state": {{"sidechains": {sidechains}}}}}"#);
    write(folder.path(), "state/song/instance.json", &song);
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    play(&mut harness, BAR)
}

fn energy(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample * sample).sum()
}

/// A sidechain from one track keys a compressor of another: its detector hears the key and not
/// the sound it turns down. With the key silent, the compressor leaves `t00` as it is; without a
/// key it hears `t00` and turns it down; keyed by `t01` playing, it turns `t00` down under it.
#[test]
fn a_sidechain_from_one_track_keys_a_compressor_of_another() {
    let silent_key = ducked(Some("post_fader"), r#""-inf""#, false);
    let unkeyed = ducked(None, r#""-inf""#, false);
    let bypassed = ducked(None, r#""-inf""#, true);
    let most =
        (silent_key.iter().zip(&bypassed)).fold(0.0_f32, |most, (a, b)| most.max((a - b).abs()));
    assert!(most < 1e-3 * loudest(&bypassed), "{most}");
    assert!(energy(&unkeyed) < 0.25 * energy(&bypassed));
    let keyed = ducked(Some("post_fader"), "0", false);
    let both_open = ducked(None, "0", true);
    assert!(
        energy(&keyed) < 0.75 * energy(&both_open),
        "{} {}",
        energy(&keyed),
        energy(&both_open)
    );
}

/// A key taken before the fader of its track, into its effects or out of them, keys with the
/// fader all the way down; after the fader, it is silent then.
#[test]
fn a_sidechain_before_the_fader_keys_with_the_fader_down() {
    let silent_key = ducked(Some("post_fader"), r#""-inf""#, false);
    for tap in ["pre_fx", "post_fx"] {
        let keyed = ducked(Some(tap), r#""-inf""#, false);
        assert!(
            energy(&keyed) < 0.75 * energy(&silent_key),
            "{tap}: {} {}",
            energy(&keyed),
            energy(&silent_key)
        );
    }
}

/// A fader tells a lane or a knob what its record holds, which a lane starts from.
#[test]
fn a_fader_says_what_its_record_holds() {
    let mut harness = song();
    harness.write_and_apply("state/song/t01/mix.json", &fader("-6"));
    let mix = sound_core::InstanceId::new("song/t01/mix").unwrap();
    let gain = harness
        .project
        .automation(&mix, "gain_db")
        .and_then(|number| number.record);
    assert_eq!(gain, Some(-6.0));
}

/// A track's `synth` takes any tool that takes notes: a wavetable in its place plays the clip,
/// and an effect there, which takes none, is a problem that says so.
#[test]
fn a_track_plays_any_instrument_and_names_a_tool_that_takes_no_notes() {
    let mut harness = song();
    harness.write_and_apply(
        "state/song/t00/synth.json",
        r#"{"tool": "wavetable", "state": {}}"#,
    );
    assert_eq!(harness.project.problems(), []);
    assert!(loudest(&play(&mut harness, 48_000)) > 0.01);

    harness.write_and_apply(
        "state/song/t00/synth.json",
        r#"{"tool": "utility", "state": {}}"#,
    );
    let problems = harness.project.problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("takes no notes")),
        "{problems:?}"
    );
}
