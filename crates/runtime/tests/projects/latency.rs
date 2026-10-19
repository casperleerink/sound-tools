//! Latency compensation in whole projects, with the repository's own test plugins in both
//! formats: a plugin with latency as the instrument of a rack and in its chain, a latency that
//! changes while the project plays, and the click.
//!
//! The test plugin plays everything `latency` frames late and says so, and its saved state
//! holds that latency (see `tooling/test-plugin-support`). The strongest check there is: a
//! project whose plugins have latency renders exactly, sample for sample, what the same project
//! renders when nothing has latency. Every note of every rack then reaches the device on the
//! frame of its tick, with tick 0 on frame 0.

use metronome::Click;
use plugin_host::PluginFormat;
use test_plugin_support::{LATENCY_KEY, SavedState};

use crate::rack::racks_files;
use crate::support::{Harness, plugin_state_of, test_plugin_of, write};

/// Frames per tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

/// The latency the tests give a plugin, in frames: 14.6 ms, and not a whole number of blocks.
const LATENCY: i32 = 700;

/// One note of a clip: start and length in ticks, the key and its velocity.
type Note = (u64, u64, u8, u8);

/// A rack of these tests: the test plugin as its instrument with a latency, maybe the test
/// plugin again as an effect with a latency of its own, and one timeline.
struct Track<'a> {
    name: &'a str,
    instrument_latency: i32,
    effect_latency: Option<i32>,
    notes: &'a [Note],
}

fn clip(notes: &[Note]) -> String {
    let notes: Vec<String> = notes
        .iter()
        .map(|(start, length, pitch, velocity)| {
            format!(
                r#"{{"start": {start}, "length": {length}, "pitch": {pitch}, "velocity": {velocity}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"tool": "timeline", "state": {{"start": 0, "length": 7680, "notes": [{}]}}}}"#,
        notes.join(", ")
    )
}

/// Writes the state asset of one test plugin with this latency, as the host would have saved it.
fn write_state(folder: &std::path::Path, format: PluginFormat, asset: &str, latency: i32) {
    let path = folder.join(format!("assets/plugin-state/{asset}.bin"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let state = SavedState {
        latency,
        ..SavedState::default()
    };
    std::fs::write(&path, plugin_state_of(format, state)).unwrap();
}

/// A rack for each of these tracks.
fn project(format: PluginFormat, tracks: &[Track<'_>]) -> Harness {
    let folder = tempfile::tempdir().unwrap();
    let records: Vec<[String; 3]> = (tracks.iter())
        .map(|track| {
            let instrument = format!("{}-instrument", track.name);
            write_state(folder.path(), format, &instrument, track.instrument_latency);
            let effect = format!("{}-effect", track.name);
            if let Some(latency) = track.effect_latency {
                write_state(folder.path(), format, &effect, latency);
            }
            let instrument = test_plugin_of(format, &instrument);
            [
                instrument,
                test_plugin_of(format, &effect),
                clip(track.notes),
            ]
        })
        .collect();
    let children: Vec<Vec<(&str, &str)>> = (tracks.iter().zip(&records))
        .map(|(track, [instrument, effect, notes])| {
            let mut children = vec![
                ("instrument", instrument.as_str()),
                ("notes", notes.as_str()),
            ];
            if track.effect_latency.is_some() {
                children.push(("late", effect.as_str()));
            }
            children
        })
        .collect();
    let racks: Vec<(&str, &[(&str, &str)])> = (tracks.iter().zip(&children))
        .map(|(track, children)| (track.name, children.as_slice()))
        .collect();
    for (path, contents) in racks_files(&racks) {
        write(folder.path(), &path, &contents);
    }
    let (harness, _plugins) = Harness::with_test_plugin(folder);
    assert_eq!(harness.project.problems(), [], "{format:?}");
    harness
}

/// The left channel of an interleaved stereo render.
fn left(render: &[f32]) -> Vec<f32> {
    render.iter().step_by(2).copied().collect()
}

/// The first frame from `from` on that is not silent.
fn onset(render: &[f32], from: usize) -> Option<usize> {
    left(render)
        .iter()
        .skip(from)
        .position(|sample| *sample != 0.0)
        .map(|found| found + from)
}

/// The same notes on two tracks, one of them with latency. Two quarter notes each, the first on
/// tick 0.
const NOTES: [Note; 2] = [(0, 240, 60, 100), (480, 240, 67, 50)];

#[test]
fn a_plugin_with_latency_in_an_instrument_slot_plays_in_time_with_one_without() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let tracks = |latency| {
            [
                Track {
                    name: "late",
                    instrument_latency: latency,
                    effect_latency: None,
                    notes: &NOTES,
                },
                Track {
                    name: "plain",
                    instrument_latency: 0,
                    effect_latency: None,
                    notes: &NOTES,
                },
            ]
        };
        let mut with_latency = project(format, &tracks(LATENCY));
        let mut without = project(format, &tracks(0));
        let heard = with_latency.play(24_000);
        assert_eq!(
            with_latency.project.engine().poll().unwrap().latency,
            LATENCY as u64,
            "{format:?}"
        );
        // Sample for sample what the project plays with no latency anywhere.
        assert_eq!(heard, without.play(24_000), "{format:?}");
        // Both notes of tick 0 start on frame 0, together: 100 twice.
        assert_eq!(onset(&heard, 0), Some(0), "{format:?}");
        let both = 2.0 * 100.0 / 127.0;
        assert!((heard[0] - both).abs() < 1e-6, "{format:?}: {}", heard[0]);
        // And the second pair on the frame of tick 480.
        assert_eq!(onset(&heard, 6_000 + 10), Some(480 * TICK), "{format:?}");
    }
}

#[test]
fn a_plugin_with_latency_in_an_effect_slot_plays_in_time_with_one_without() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let tracks = |latency| {
            [
                Track {
                    name: "late",
                    instrument_latency: 0,
                    effect_latency: Some(latency),
                    notes: &NOTES,
                },
                Track {
                    name: "plain",
                    instrument_latency: 0,
                    effect_latency: None,
                    notes: &NOTES,
                },
            ]
        };
        let mut with_latency = project(format, &tracks(LATENCY));
        let mut without = project(format, &tracks(0));
        let heard = with_latency.play(24_000);
        assert_eq!(heard, without.play(24_000), "{format:?}");
        // The effect passes half of what it is played and plays the notes of its track too:
        // 100, half of 100 and 100 again on frame 0.
        let both = 2.5 * 100.0 / 127.0;
        assert!((heard[0] - both).abs() < 1e-6, "{format:?}: {}", heard[0]);
    }
}

/// Latency on both, in the instrument and in the effect of one track, and on the other track:
/// the longest way to the device sets the wait, and each track is led by its own. The notes of
/// the track reach the effect on the frame they reach the instrument, so what the effect plays
/// of them itself is early by the latency of the instrument, and only that.
#[test]
fn every_track_is_led_by_the_latency_of_its_own_chain() {
    let tracks = |instrument, effect, other| {
        [
            Track {
                name: "late",
                instrument_latency: instrument,
                effect_latency: Some(effect),
                notes: &NOTES,
            },
            Track {
                name: "plain",
                instrument_latency: other,
                effect_latency: None,
                notes: &NOTES,
            },
        ]
    };
    let mut with_latency = project(PluginFormat::Clap, &tracks(300, 500, 128));
    let mut without = project(PluginFormat::Clap, &tracks(0, 0, 0));
    let heard = with_latency.play(24_000);
    assert_eq!(with_latency.project.engine().poll().unwrap().latency, 800);
    let notes = Track {
        name: "late",
        instrument_latency: 0,
        effect_latency: None,
        notes: &NOTES,
    };
    let notes = project(PluginFormat::Clap, &[notes]).play(24_000 + 300);
    let expected = without.play(24_000);
    // Interleaved: 300 frames are 600 samples.
    let early = notes.iter().skip(600).zip(&notes);
    let expected = expected
        .iter()
        .zip(early)
        .map(|(all, (early, on_time))| all + early - on_time);
    for (frame, (heard, expected)) in heard.iter().zip(expected).enumerate() {
        assert!(
            (heard - expected).abs() < 1e-6,
            "{frame}: {heard} {expected}"
        );
    }
}

/// A note on the key that asks for a latency, played by the clip on tick 960: the plugin asks
/// its host to start it again, and plays with the new latency from then on.
#[test]
fn a_latency_that_changes_while_the_project_plays_is_in_time_again_afterwards() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        // Velocity 60 on the latency key asks for 600 frames.
        let late: [Note; 5] = [
            (0, 240, 60, 100),
            (480, 240, 67, 50),
            (960, 10, LATENCY_KEY, 60),
            (1920, 240, 60, 100),
            (2880, 240, 67, 50),
        ];
        let plain: [Note; 4] = [late[0], late[1], late[3], late[4]];
        let tracks = |late_latency, late_notes| {
            [
                Track {
                    name: "late",
                    instrument_latency: late_latency,
                    effect_latency: None,
                    notes: late_notes,
                },
                Track {
                    name: "plain",
                    instrument_latency: 0,
                    effect_latency: None,
                    notes: &plain,
                },
            ]
        };
        let mut changing = project(format, &tracks(300, &late));
        let mut without = project(format, &tracks(0, &plain));
        let heard = changing.play(4 * 24_000);
        assert_eq!(
            changing.project.engine().poll().unwrap().latency,
            600,
            "{format:?}: the plugin was not started again with its new latency"
        );
        // Every note before the change and after it is where it is with no latency at all.
        // The restart takes two buffers of the render, while nothing of the clip sounds, so
        // nothing is missing either.
        assert_eq!(heard, without.play(4 * 24_000), "{format:?}");
        // And the plugin says which latency it has now in the state it saved.
        let asset = changing.path("assets/plugin-state/late-instrument.bin");
        let bytes = std::fs::read(asset).unwrap();
        let own = match format {
            PluginFormat::Clap => &bytes[..],
            PluginFormat::Vst3 => {
                let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                &bytes[8..8 + length as usize]
            }
        };
        let saved = test_plugin_support::load_state(own).unwrap();
        assert_eq!(saved.latency, 600, "{format:?}");
    }
}

/// The click goes to the device with no latency after it, and lines up with a track that has.
#[test]
fn the_click_lines_up_with_a_track_with_latency() {
    let tracks = |latency| {
        [Track {
            name: "late",
            instrument_latency: latency,
            effect_latency: None,
            // On the second and the third beat.
            notes: &[(960, 240, 60, 100), (1920, 240, 60, 100)],
        }]
    };
    let mut with_latency = project(PluginFormat::Clap, &tracks(LATENCY));
    let mut without = project(PluginFormat::Clap, &tracks(0));
    for harness in [&mut with_latency, &mut without] {
        let mut click = Click::attach(harness.project.engine()).unwrap();
        click.set_on(harness.project.engine(), true).unwrap();
    }
    let heard = with_latency.play(3 * 24_000);
    assert_eq!(heard, without.play(3 * 24_000));
    // The click of beat 2 and the note on it start on the same frame.
    assert_eq!(onset(&heard, 20_000), Some(24_000));
}
