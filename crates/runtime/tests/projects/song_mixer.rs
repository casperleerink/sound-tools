//! The mixer of the TypeScript song of the DAW template (`templates/daw`), played as the Rust
//! arrangement's mixer plays: a solo silences the other tracks and never a return, a send goes
//! after the track's fader unless its return says before, the master's limiter holds the output
//! under its ceiling, and an audio region counts toward the end of the song.

use sound_core::{Ticks, amplitude};

use crate::shared_song::song_files;
use crate::support::{BAR, Harness, difference, write, write_samples};

/// Two tracks of one clip each from the start, sending into the reverb `ret-a`.
fn song() -> Option<Harness> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(2, 2) {
        write(folder.path(), &path, &text);
    }
    let harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    Some(harness)
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
    let Some(mut harness) = song() else {
        return;
    };
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
    let Some(mut harness) = song() else {
        return;
    };
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
    let Some(mut harness) = song() else {
        return;
    };
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

/// Two regions that overlap are heard together, and the song lasts to the end of the last one.
#[test]
fn overlapping_regions_sum_and_the_last_one_ends_the_song() {
    let Some(mut harness) = song() else {
        return;
    };
    write_samples(&harness, "low.wav", 48_000, 2.0, |seconds| {
        (0.2 * (seconds * 220.0 * std::f64::consts::TAU).sin()) as f32
    });
    write_samples(&harness, "high.wav", 48_000, 2.0, |seconds| {
        (0.1 * (seconds * 330.0 * std::f64::consts::TAU).sin()) as f32
    });
    let region = |asset: &str, start: u64| {
        format!(r#"{{"tool": "region", "state": {{"asset": "{asset}", "start": {start}}}}}"#)
    };
    // The tracks silent, so only the regions play.
    for track in ["t00", "t01"] {
        harness.write_and_apply(&format!("state/song/{track}/mix.json"), &fader(r#""-inf""#));
    }
    harness.write_and_apply("state/song/voice/low.json", &region("low.wav", 0));
    let low = play(&mut harness, BAR);
    harness.write_and_apply("state/song/voice/high.json", &region("high.wav", 0));
    let both = play(&mut harness, BAR);
    std::fs::remove_file(harness.path("state/song/voice/low.json")).unwrap();
    harness.apply(&[harness.path("state/song/voice/low.json")]);
    let high = play(&mut harness, BAR);
    let summed = (low.iter().zip(&high)).map(|(low, high)| low + high);
    let most =
        (both.iter().zip(summed)).fold(0.0_f32, |most, (both, sum)| most.max((both - sum).abs()));
    assert!(most < 1e-6, "{most}");
    // Two seconds are a bar at 120 bpm: from bar 21 the region ends at bar 22.
    let start = 20 * 3840;
    harness.write_and_apply("state/song/voice/high.json", &region("high.wav", start));
    assert_eq!(harness.project.end(), Some(Ticks(start + 3840)));
}

/// A compressor in the chain of `t00` that turns down anything over -60 dB, hard and fast, or
/// passed by when `bypassed`, and `t01` at `t01_gain`, with the song keying the compressor by
/// `t01` when `keyed`.
fn ducked(keyed: bool, t01_gain: &str, bypassed: bool) -> Option<Vec<f32>> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(2, 2) {
        let text = match path.as_str() {
            "project.json" => text.replace(r#""limiter"]"#, r#""limiter", "compressor"]"#),
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
        true => r#"[{"effect": "t00/chain/comp", "from": "t01"}]"#,
        false => "[]",
    };
    let song = format!(r#"{{"tool": "song", "state": {{"sidechains": {sidechains}}}}}"#);
    write(folder.path(), "state/song/instance.json", &song);
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    Some(play(&mut harness, BAR))
}

fn energy(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample * sample).sum()
}

/// A sidechain from one track keys a compressor of another: its detector hears the key and not
/// the sound it turns down. With the key silent, the compressor leaves `t00` as it is; without a
/// key it hears `t00` and turns it down; keyed by `t01` playing, it turns `t00` down under it.
#[test]
fn a_sidechain_from_one_track_keys_a_compressor_of_another() {
    let Some(silent_key) = ducked(true, r#""-inf""#, false) else {
        return;
    };
    let unkeyed = ducked(false, r#""-inf""#, false).unwrap();
    let bypassed = ducked(false, r#""-inf""#, true).unwrap();
    let most =
        (silent_key.iter().zip(&bypassed)).fold(0.0_f32, |most, (a, b)| most.max((a - b).abs()));
    assert!(most < 1e-3 * loudest(&bypassed), "{most}");
    assert!(energy(&unkeyed) < 0.25 * energy(&bypassed));
    let keyed = ducked(true, "0", false).unwrap();
    let both_open = ducked(false, "0", true).unwrap();
    assert!(
        energy(&keyed) < 0.75 * energy(&both_open),
        "{} {}",
        energy(&keyed),
        energy(&both_open)
    );
}
