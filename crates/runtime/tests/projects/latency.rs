//! Latency compensation in whole projects, with the repository's own test plugins in both
//! formats: a plugin with latency in an instrument slot and in an effect slot, a latency that
//! changes while the project plays, recording next to a track with latency and through one,
//! and the click.
//!
//! The test plugin plays everything `latency` frames late and says so, and its saved state
//! holds that latency (see `tooling/test-plugin-support`). The strongest check there is: a
//! project whose plugins have latency renders exactly, sample for sample, what the same project
//! renders when nothing has latency. Every note of every track then reaches the device on the
//! frame of its tick, with tick 0 on frame 0.

use metronome::Click;
use midi::{Keyboard, Played, Take};
use plugin_host::PluginFormat;
use sound_core::{Frames, InstanceId, Ticks};
use sound_notes::{Pitch, Velocity};
use test_plugin_support::{LATENCY_KEY, SavedState};

use crate::support::{Harness, plugin_state_of, test_plugin_of};

/// Frames per tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

/// The latency the tests give a plugin, in frames: 14.6 ms, and not a whole number of blocks.
const LATENCY: i32 = 700;

/// One note of a clip: start and length in ticks, the key and its velocity.
type Note = (u64, u64, u8, u8);

/// A track of these tests: the test plugin as its instrument with a latency, maybe the test
/// plugin again as an effect with a latency of its own, and one clip.
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
        r#"{{"tool": "arrangement.clip", "state": {{"start": 0, "length": 7680, "notes": [{}]}}}}"#,
        notes.join(", ")
    )
}

/// Writes the state asset of one test plugin with this latency, as the host would have saved it.
fn write_state(harness: &Harness, format: PluginFormat, asset: &str, latency: i32) {
    let path = harness.path(&format!("assets/plugin-state/{asset}.bin"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let state = SavedState {
        latency,
        ..SavedState::default()
    };
    std::fs::write(&path, plugin_state_of(format, state)).unwrap();
}

/// A project with these tracks next to the silent track of the default project.
fn project(format: PluginFormat, tracks: &[Track<'_>]) -> Harness {
    let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
    // Two tracks at full level sum over full scale, and the numbers below are that sum.
    harness.bypass_limiter();
    for (order, track) in tracks.iter().enumerate() {
        let folder = format!("state/arrangement/{}", track.name);
        let effects = match track.effect_latency {
            Some(_) => r#"["late"]"#,
            None => "[]",
        };
        harness.write(
            &format!("{folder}/instance.json"),
            &format!(
                r#"{{"tool": "arrangement.track", "state": {{"name": "{}", "order": {}, "effects": {effects}}}}}"#,
                track.name,
                order + 1
            ),
        );
        let instrument = format!("{}-instrument", track.name);
        write_state(&harness, format, &instrument, track.instrument_latency);
        harness.write(
            &format!("{folder}/instrument.json"),
            &test_plugin_of(format, &instrument),
        );
        if let Some(latency) = track.effect_latency {
            let effect = format!("{}-effect", track.name);
            write_state(&harness, format, &effect, latency);
            harness.write(
                &format!("{folder}/late.json"),
                &test_plugin_of(format, &effect),
            );
        }
        harness.write(&format!("{folder}/notes.json"), &clip(track.notes));
        let path = harness.path(&folder);
        harness.apply(&[path]);
    }
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
        // The effect passes half of what it is played: 100 and half of 100 on frame 0.
        let both = 1.5 * 100.0 / 127.0;
        assert!((heard[0] - both).abs() < 1e-6, "{format:?}: {}", heard[0]);
    }
}

/// Latency on both, in the instrument and in the effect of one track, and on the other track:
/// the longest way to the device sets the wait, and each track is led by its own.
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
    assert_eq!(heard, without.play(24_000));
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

/// A MIDI input into the instrument of one track, as the window wires it, and a take of what it
/// plays. Every message is sent while the device stands on the frame the test names.
struct Recording {
    harness: Harness,
    keyboard: Keyboard,
}

impl Recording {
    fn into_track(harness: Harness, track: &str) -> Self {
        let mut harness = harness;
        let mut keyboard = Keyboard::attach(harness.project.engine()).unwrap();
        let track = InstanceId::new(track).unwrap();
        let track = harness.project.resolve(&track).unwrap();
        let notes = runtime::window::recording::notes_input(&harness.project, &track).unwrap();
        keyboard
            .play_into(harness.project.engine(), Some(notes))
            .unwrap();
        keyboard.poll(harness.project.engine(), None).unwrap();
        Self { harness, keyboard }
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let output = self.harness.render(frames);
        self.keyboard
            .poll(self.harness.project.engine(), None)
            .unwrap();
        output
    }

    fn record(&mut self, messages: &[(usize, Played)], frames: usize) -> (Take, Vec<f32>) {
        self.harness.project.engine().play();
        let mut heard = self.render(64);
        let from = self.harness.project.engine().poll().unwrap().playhead_tick;
        self.keyboard.start_recording(from);
        let mut at = 64;
        for (frame, played) in messages {
            heard.extend(self.render(frame - at));
            at = *frame;
            assert!(self.keyboard.input().send(*played));
        }
        heard.extend(self.render(frames - at));
        let until = self.harness.project.engine().poll().unwrap().playhead_tick;
        (self.keyboard.finish_recording(until).unwrap(), heard)
    }
}

fn on(pitch: u8) -> Played {
    Played::On {
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    }
}

fn off(pitch: u8) -> Played {
    Played::Off {
        pitch: Pitch::new(pitch).unwrap(),
        velocity: 64,
    }
}

/// Two notes played live, off the grid.
const PLAYED: [(usize, u8); 2] = [(12_010, 72), (30_005, 74)];

fn messages() -> Vec<(usize, Played)> {
    PLAYED
        .iter()
        .flat_map(|(frame, pitch)| [(*frame, on(*pitch)), (frame + 3_000, off(*pitch))])
        .collect()
}

/// The ticks the device played when each message was sent: where the player heard it.
fn ticks_heard(harness: &Harness) -> Vec<Ticks> {
    let clock = harness.project.clock().clone();
    messages()
        .iter()
        .map(|(frame, _)| clock.tick_at(Frames(*frame as u64)))
        .collect()
}

fn take_ticks(take: &Take) -> Vec<Ticks> {
    take.events.iter().map(|event| event.tick).collect()
}

/// The other track plays a note on every beat with latency. The live track has none, and it
/// sounds exactly when a key arrives, as it does with no latency anywhere: 0 frames added.
#[test]
fn a_track_without_latency_plays_live_with_no_delay_and_records_to_the_tick() {
    let tracks = |latency| {
        [
            Track {
                name: "late",
                instrument_latency: latency,
                effect_latency: None,
                notes: &[(0, 120, 48, 30), (960, 120, 48, 30), (1920, 120, 48, 30)],
            },
            Track {
                name: "live",
                instrument_latency: 0,
                effect_latency: None,
                notes: &[],
            },
        ]
    };
    let mut takes = Vec::new();
    let mut renders = Vec::new();
    for latency in [LATENCY, 0] {
        let harness = project(PluginFormat::Clap, &tracks(latency));
        let mut recording = Recording::into_track(harness, "arrangement/live");
        let (take, heard) = recording.record(&messages(), 48_000);
        assert_eq!(
            take_ticks(&take),
            ticks_heard(&recording.harness),
            "{latency}"
        );
        takes.push(take_ticks(&take));
        renders.push(heard);
    }
    // The same take and the same sound, sample for sample, with the other track's latency or
    // without it: the live note starts on the frame its key arrived on.
    assert_eq!(takes[0], takes[1]);
    assert_eq!(renders[0], renders[1]);
    for (frame, _) in PLAYED {
        assert_eq!(onset(&renders[0], frame - 10), Some(frame));
    }
}

/// Recording through the track that has latency. The player hears the key through the plugin,
/// 700 frames late, and nothing more; the take lands where the other tracks were when the key
/// went down.
#[test]
fn a_take_through_a_track_with_latency_lands_where_it_was_played() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let tracks = [
            Track {
                name: "late",
                instrument_latency: LATENCY,
                effect_latency: None,
                notes: &[],
            },
            Track {
                name: "other",
                instrument_latency: 200,
                effect_latency: None,
                notes: &[],
            },
        ];
        let harness = project(format, &tracks);
        let mut recording = Recording::into_track(harness, "arrangement/late");
        let (take, heard) = recording.record(&messages(), 48_000);
        assert_eq!(
            take_ticks(&take),
            ticks_heard(&recording.harness),
            "{format:?}"
        );
        for (frame, _) in PLAYED {
            let latency = LATENCY as usize;
            assert_eq!(
                onset(&heard, frame - 10),
                Some(frame + latency),
                "{format:?}: the live note is late by more than its own plugin"
            );
        }
    }
}
