//! A song of the TypeScript example in `tooling/agent-eval/examples`: tracks of clips, as an
//! agent writes them, the same on every machine.

use serde_json::{Value, json};

const SONG: &str = include_str!("../../../../tooling/agent-eval/examples/song.tsx");
const TRACK: &str = include_str!("../../../../tooling/agent-eval/examples/track.ts");

pub(crate) const BAR: u64 = 3840;
/// The page's sizes, as `song.tsx` has them: a track's row and its header, and points per tick.
pub(crate) const ROW: f32 = 40.0;
pub(crate) const HEADER: f32 = 168.0;
pub(crate) const ACROSS: f32 = 0.025;

/// A number from `seed`, the same on every machine.
fn noise(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

/// The files of a song of `tracks` tracks with `clips` clips in all, each of 30 to 150 notes: a
/// synth, a filter and a utility per track, and a utility `master`.
pub(crate) fn song_files(tracks: usize, clips: usize) -> Vec<(String, String)> {
    let project = json!({
        "format": 1,
        "extensions": ["instrument", "filter", "utility"],
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
        (
            "state/song/instance.json".to_string(),
            r#"{"tool": "song", "state": {}}"#.to_string(),
        ),
        (
            "state/song/master.json".to_string(),
            r#"{"tool": "utility", "state": {}}"#.to_string(),
        ),
    ];
    let mut seed = 7;
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
        let mine = clips / tracks + usize::from(track < clips % tracks);
        for clip in 0..mine {
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
            let record = json!({ "tool": "timeline", "state": { "start": start, "length": length, "notes": notes } });
            files.push((format!("{folder}/c{clip:02}.json"), record.to_string()));
        }
    }
    files
}
