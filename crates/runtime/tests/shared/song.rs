//! A song of the TypeScript example in `tooling/agent-eval/examples`: tracks of clips, as an
//! agent writes them, the same on every machine.

use serde_json::{Value, json};

const SONG: &str = include_str!("../../../../tooling/agent-eval/examples/song.tsx");
const TRACK: &str = include_str!("../../../../tooling/agent-eval/examples/track.ts");
const TAPE: &str = include_str!("../../../../tooling/agent-eval/examples/tape.ts");
const CHAIN: &str = include_str!("../../../../tooling/agent-eval/examples/chain.ts");

pub(crate) const BAR: u64 = 3840;
/// The page's sizes, as `song.tsx` has them: a track's row and its header, and points per tick.
pub(crate) const ROW: f32 = 40.0;
pub(crate) const HEADER: f32 = 200.0;
pub(crate) const ACROSS: f32 = 0.025;

/// A number from `seed`, the same on every machine.
fn noise(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

/// The files of a song of `tracks` tracks with `clips` clips in all, each of 30 to 150 notes: a
/// synth, a filter whose cutoff a lane closes, a utility, a meter and a recorder of notes per
/// track, a utility `master`, and an audio track
/// `voice` with a recorder of the first channel of the input and no region yet.
pub(crate) fn song_files(tracks: usize, clips: usize) -> Vec<(String, String)> {
    let mut seed = 7;
    song_with(tracks, |track| {
        let mine = clips / tracks + usize::from(track < clips % tracks);
        (0..mine)
            .map(|clip| {
                let bars = 1 + noise(&mut seed) % 2;
                let length = bars * BAR;
                let count = 30 + noise(&mut seed) % 121;
                let notes: Vec<Value> = (0..count)
                    .map(|index| {
                        let start = index * length / count;
                        let pitch = 48 + noise(&mut seed) % 36;
                        json!({ "start": start, "length": 120 + noise(&mut seed) % 360, "pitch": pitch, "velocity": 60 + noise(&mut seed) % 60 })
                    })
                    .collect();
                let start = clip as u64 * 3 * BAR;
                (clip, json!({ "start": start, "length": length, "notes": notes }))
            })
            .collect()
    })
}

/// The scale project of the Rust arrangement's measurement as a song: 100 tracks of 100 clips of
/// one bar and four notes, clip `c` of track `t` at bar `8c + t mod 8`.
pub(crate) fn scale_files() -> Vec<(String, String)> {
    song_with(100, |track| {
        let pitch = 36 + (track % 40) as u64;
        let notes: Vec<Value> = (0..4)
            .map(|beat| json!({ "start": beat * 960, "length": 900, "pitch": pitch + beat * 3, "velocity": 100 }))
            .collect();
        (0..100)
            .map(|clip| {
                let start = (clip as u64 * 8 + track as u64 % 8) * BAR;
                (
                    clip,
                    json!({ "start": start, "length": BAR, "notes": notes }),
                )
            })
            .collect()
    })
}

/// The files of a song of `tracks` tracks, each with the clips `clips` gives it: the state of each
/// by its number.
fn song_with(
    tracks: usize,
    mut clips: impl FnMut(usize) -> Vec<(usize, Value)>,
) -> Vec<(String, String)> {
    let project = json!({
        "format": 1,
        "extensions": ["instrument", "filter", "utility", "reverb"],
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [{ "from": { "instance": "song", "port": "audio" }, "to": { "device_output": 0 } }],
    });
    let colours = [
        "#a9b1ff", "#7ee0a0", "#f0b44c", "#ff8fa3", "#7cc7ff", "#d0a6ff",
    ];
    let mut files = vec![
        ("project.json".to_string(), project.to_string()),
        ("extensions/song.tsx".to_string(), SONG.to_string()),
        ("extensions/track.ts".to_string(), TRACK.to_string()),
        ("extensions/tape.ts".to_string(), TAPE.to_string()),
        ("extensions/chain.ts".to_string(), CHAIN.to_string()),
        (
            "state/song/instance.json".to_string(),
            r#"{"tool": "song", "state": {}}"#.to_string(),
        ),
        (
            "state/song/master.json".to_string(),
            r#"{"tool": "utility", "state": {}}"#.to_string(),
        ),
        (
            "state/song/voice/instance.json".to_string(),
            r#"{"tool": "tape", "state": {}}"#.to_string(),
        ),
        (
            "state/song/voice/rec.json".to_string(),
            r#"{"tool": "recorder", "state": {"channels": [0]}}"#.to_string(),
        ),
        (
            "state/song/voice/mix.json".to_string(),
            r#"{"tool": "utility", "state": {}}"#.to_string(),
        ),
        (
            "state/song/voice/level.json".to_string(),
            r#"{"tool": "meter", "state": {}}"#.to_string(),
        ),
    ];
    for track in 0..tracks {
        let folder = format!("state/song/t{track:02}");
        let colour = colours[track % colours.len()];
        files.push((
            format!("{folder}/instance.json"),
            json!({ "tool": "track", "state": { "colour": colour } }).to_string(),
        ));
        files.push((
            format!("{folder}/synth.json"),
            r#"{"tool": "instrument.synth", "state": {}}"#.to_string(),
        ));
        files.push((
            format!("{folder}/fx.json"),
            r#"{"tool": "filter", "state": {}}"#.to_string(),
        ));
        files.push((
            format!("{folder}/mix.json"),
            r#"{"tool": "utility", "state": {}}"#.to_string(),
        ));
        files.push((
            format!("{folder}/chain/instance.json"),
            r#"{"tool": "chain", "state": {}}"#.to_string(),
        ));
        files.push((
            format!("{folder}/level.json"),
            r#"{"tool": "meter", "state": {}}"#.to_string(),
        ));
        // The cutoff closes from 6 kHz to 1.5 kHz over 64 bars.
        let cutoff = r#"{"tool": "lane", "state": {"points": [{"tick": 0, "value": 6000}, {"tick": 245760, "value": 1500}], "scale": "log"}}"#;
        files.push((format!("{folder}/cutoff.json"), cutoff.to_string()));
        files.push((
            format!("{folder}/rec.json"),
            r#"{"tool": "recorder", "state": {}}"#.to_string(),
        ));
        for (clip, state) in clips(track) {
            let record = json!({ "tool": "timeline", "state": state });
            files.push((format!("{folder}/c{clip:02}.json"), record.to_string()));
        }
    }
    files
}
