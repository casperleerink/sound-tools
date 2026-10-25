//! A song of the DAW template's tools (`templates/daw`): tracks of clips, as an agent writes
//! them, the same on every machine.

use serde_json::{Value, json};

pub(crate) const BAR: u64 = 3840;

/// The DAW template.
pub(crate) fn daw() -> &'static runtime::Template {
    runtime::templates::named("DAW").unwrap()
}
/// The page's sizes, as `song-page.ts` has them: a track's row and its header, points per tick,
/// and the points left of bar 1.
pub(crate) const ROW: f32 = 64.0;
pub(crate) const HEADER: f32 = 176.0;
pub(crate) const ACROSS: f32 = 0.025;
pub(crate) const LEAD: f32 = 8.0;
/// In a track's header, from its left and its top: the middle of its name.
pub(crate) const NAME: (f32, f32) = (50.0, 18.0);

/// A number from `seed`, the same on every machine.
fn noise(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

/// The files of a song of `tracks` tracks with `clips` clips in all, each of 30 to 150 notes: a
/// synth, a filter in its chain whose cutoff a lane closes, a fader and a recorder of notes per track, each
/// sending -12 dB into the return `ret-a`, a reverb; a return `ret-b` that hears before the
/// faders; a fader `master` into a limiter `limit`, and an audio track `audio-voice` with a recorder
/// of the first channel of the input and no region yet.
pub(crate) fn song_files(tracks: usize, clips: usize) -> Vec<(String, String)> {
    let mut seed = 7;
    song_with(tracks, true, |track| {
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

/// The scale project, as the deleted Rust arrangement measured it: 100 tracks of 100 clips of
/// one bar and four notes, clip `c` of track `t` at bar `8c + t mod 8`.
pub(crate) fn scale_files() -> Vec<(String, String)> {
    song_with(100, false, |track| {
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
/// by its number. With `returns`, the two returns and the sends into the first.
fn song_with(
    tracks: usize,
    returns: bool,
    mut clips: impl FnMut(usize) -> Vec<(usize, Value)>,
) -> Vec<(String, String)> {
    let project = json!({
        "format": 1,
        "extensions": ["instrument", "filter", "utility", "reverb", "limiter", "wavetable"],
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [{ "from": { "instance": "song", "port": "audio" }, "to": { "device_output": 0 } }],
    });
    let colours = [
        "#a9b1ff", "#7ee0a0", "#f0b44c", "#ff8fa3", "#7cc7ff", "#d0a6ff",
    ];
    let mut files = vec![
        ("project.json".to_string(), project.to_string()),
        // The audio track after the tracks, as the song lists tracks in `order`.
        (
            "state/song/instance.json".to_string(),
            json!({ "tool": "song", "state": { "order": (0..tracks).map(|track| format!("t{track:02}")).chain(["audio-voice".to_string()]).collect::<Vec<_>>() } }).to_string(),
        ),
        (
            "state/song/master.json".to_string(),
            r#"{"tool": "fader", "state": {}}"#.to_string(),
        ),
        // A master limiter: full scale, no lookahead.
        (
            "state/song/limit.json".to_string(),
            r#"{"tool": "limiter", "state": {"ceiling_db": 0, "lookahead_ms": 0}}"#.to_string(),
        ),
        // What a solo turns the other tracks down to.
        (
            "state/song/quiet.json".to_string(),
            r#"{"tool": "lane", "state": {"points": [{"tick": 0, "value": 1}]}}"#.to_string(),
        ),
        (
            "state/song/audio-voice/instance.json".to_string(),
            r#"{"tool": "tape", "state": {}}"#.to_string(),
        ),
        (
            "state/song/audio-voice/rec.json".to_string(),
            r#"{"tool": "recorder", "state": {"channels": [0]}}"#.to_string(),
        ),
        (
            "state/song/audio-voice/chain/instance.json".to_string(),
            r#"{"tool": "chain", "state": {}}"#.to_string(),
        ),
        (
            "state/song/audio-voice/mix.json".to_string(),
            r#"{"tool": "fader", "state": {}}"#.to_string(),
        ),
    ];
    // The template's tools, without its records.
    files.extend(
        (daw().files.iter())
            .filter(|(path, _)| path.starts_with("extensions/"))
            .map(|(path, contents)| (path.to_string(), contents.to_string())),
    );
    if returns {
        for (path, record) in [
            (
                "ret-a/instance.json",
                r#"{"tool": "bus", "state": {"title": "Room"}}"#,
            ),
            ("ret-a/mix.json", r#"{"tool": "fader", "state": {}}"#),
            (
                "ret-a/chain/instance.json",
                r#"{"tool": "chain", "state": {"order": ["verb"]}}"#,
            ),
            (
                "ret-a/chain/verb.json",
                r#"{"tool": "reverb", "state": {"mix": 1}}"#,
            ),
            (
                "ret-b/instance.json",
                r#"{"tool": "bus", "state": {"title": "Pre", "pre_fader": true}}"#,
            ),
            ("ret-b/mix.json", r#"{"tool": "fader", "state": {}}"#),
            (
                "ret-b/chain/instance.json",
                r#"{"tool": "chain", "state": {}}"#,
            ),
        ] {
            files.push((format!("state/song/{path}"), record.to_string()));
        }
    }
    for track in 0..tracks {
        let folder = format!("state/song/t{track:02}");
        let colour = colours[track % colours.len()];
        files.push((
            format!("{folder}/instance.json"),
            json!({ "tool": "track", "state": { "colour": colour, "automation": [{ "lane": "lane-cutoff", "target": "chain/filter.cutoff_hz" }] } }).to_string(),
        ));
        files.push((
            format!("{folder}/synth.json"),
            r#"{"tool": "instrument.synth", "state": {}}"#.to_string(),
        ));
        files.push((
            format!("{folder}/chain/filter.json"),
            r#"{"tool": "filter", "state": {}}"#.to_string(),
        ));
        files.push((
            format!("{folder}/mix.json"),
            r#"{"tool": "fader", "state": {}}"#.to_string(),
        ));
        if returns {
            files.push((
                format!("{folder}/send-ret-a.json"),
                r#"{"tool": "fader", "state": {"gain_db": -12}}"#.to_string(),
            ));
        }
        files.push((
            format!("{folder}/chain/instance.json"),
            r#"{"tool": "chain", "state": {"order": ["filter"]}}"#.to_string(),
        ));
        // The cutoff closes from 6 kHz to 1.5 kHz over 64 bars.
        let cutoff = r#"{"tool": "lane", "state": {"points": [{"tick": 0, "value": 6000}, {"tick": 245760, "value": 1500}], "scale": "log"}}"#;
        files.push((format!("{folder}/lane-cutoff.json"), cutoff.to_string()));
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
