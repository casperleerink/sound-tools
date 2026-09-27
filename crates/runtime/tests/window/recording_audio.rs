//! Recording audio in the window, with a simulated input and no device: arming a track from its
//! header, the input select, the record control and `r` on armed tracks, a take ended by the
//! record control, stop, pause and seek, and an input that goes away while it records.

use arrangement::{AudioClip, Colour, InputChannels, TrackState};
use gpui::TestAppContext;
use sound_core::{Changes, Project, Ticks};
use sound_ui::Recording;

use crate::support::{Opened, SimulatedInput, id, open_with_input};

const VOICE: &str = "arrangement/voice";
const GUITAR: &str = "arrangement/guitar";
const BAR: u64 = 3840;
/// Engine frames per tick at 120 bpm and 48 kHz.
const FRAMES_PER_TICK: u64 = 25;

/// The default project with two audio tracks under its instrument track: the voice records
/// input 1, the guitar inputs 1 and 2 as a stereo take.
fn open(cx: &mut TestAppContext) -> (Opened<'_>, SimulatedInput) {
    open_with_input(cx, |project: &mut Project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        let mut changes = Changes::new();
        let add = |project: &Project, changes: &mut Changes, name, colour| {
            arrangement::add_audio_track(project, changes, arrangement.id(), name, colour).unwrap()
        };
        add(project, &mut changes, "Voice", Colour::Peach);
        project.commit("Add voice", changes).unwrap();
        let mut changes = Changes::new();
        let guitar = add(project, &mut changes, "Guitar", Colour::Teal);
        project.commit("Add guitar", changes).unwrap();
        let mut state = project.state(&guitar).cloned().unwrap();
        state.input = InputChannels::pair(1).unwrap();
        let mut changes = Changes::new();
        changes.set(&guitar, state);
        project.commit("Stereo guitar", changes).unwrap();
        project.clear_history();
    })
}

/// A steady tone on both channels of the input: the left at 0.5, the right at 0.25.
fn tone(_: u64) -> [f32; 2] {
    [0.5, 0.25]
}

fn recording(opened: &mut Opened<'_>) -> gpui::Entity<Recording> {
    let session = opened.session.clone();
    opened.cx.read(|cx| session.read(cx).recording().clone())
}

fn armed(opened: &mut Opened<'_>, track: &str) -> bool {
    let recording = recording(opened);
    let track = id(track);
    opened.cx.read(|cx| recording.read(cx).is_armed(&track))
}

fn arm(opened: &mut Opened<'_>, name: &str) {
    let toggle = opened.control(&format!("toggle-arm-{name}"));
    opened.click(toggle);
    opened.settle();
}

/// The engine and the input run side by side for about `frames`, and the window polls.
fn play(opened: &mut Opened<'_>, input: &SimulatedInput, frames: u64, sample: fn(u64) -> [f32; 2]) {
    for _ in 0..frames.div_ceil(2_048) {
        opened.render(2_048);
        input.write_until(opened.engine.frames(), sample);
        opened.settle();
    }
}

/// After a take ended, the input brings the last of it and the clips are made.
fn until_clips(opened: &mut Opened<'_>, input: &SimulatedInput, sample: fn(u64) -> [f32; 2]) {
    for _ in 0..20 {
        let recording = recording(opened);
        let growing = opened.cx.read(|cx| !recording.read(cx).takes().is_empty());
        if !growing {
            return;
        }
        play(opened, input, 2_048, sample);
    }
    panic!("the take never became clips");
}

fn audio_clips(opened: &mut Opened<'_>, track: &str) -> Vec<AudioClip> {
    let track = id(track);
    opened.project(|project| {
        let clips = project.children::<AudioClip>(&track);
        clips.map(|(_, clip)| clip.clone()).collect()
    })
}

/// The end of a clip on the timeline, as the project hears it.
fn clip_end(opened: &mut Opened<'_>, clip: &AudioClip) -> Ticks {
    opened.project(|project| arrangement::audio_clip_end(project, clip))
}

#[gpui::test]
fn the_arm_toggle_opens_the_input_and_shows_its_level(cx: &mut TestAppContext) {
    let (mut opened, input) = open(cx);
    assert_eq!(
        input.openings.get(),
        0,
        "nothing opens the input before a track is armed"
    );
    arm(&mut opened, "voice");
    assert!(armed(&mut opened, VOICE));
    assert_eq!(input.openings.get(), 1);
    let recording = recording(&mut opened);
    assert_eq!(
        opened.cx.read(|cx| recording.read(cx).input_channels()),
        Some(2)
    );

    play(&mut opened, &input, 4_096, tone);
    let level = opened.cx.read(|cx| recording.read(cx).level(0..2));
    assert_eq!(level, [0.5, 0.25]);
    // Arming is no edit: nothing to undo, and no file changed.
    assert_eq!(opened.undo_label(), None);

    // Disarmed, the input closes, and the level goes.
    arm(&mut opened, "voice");
    assert!(!armed(&mut opened, VOICE));
    assert_eq!(
        opened.cx.read(|cx| recording.read(cx).input_channels()),
        None
    );
}

/// The input select offers the channels of the input, each alone, then pairs, and a pick is
/// one undo step on the track record.
#[gpui::test]
fn the_input_select_writes_the_channels_of_the_track(cx: &mut TestAppContext) {
    let (mut opened, _input) = open(cx);
    let header = opened.track_header(1);
    opened.click(header);
    opened.settle();
    let panel = opened.track_panel().unwrap();
    let select = opened
        .cx
        .read(|cx| panel.read(cx).input_select().cloned().unwrap());
    let shown = opened.cx.read(|cx| select.read(cx).label().clone());
    assert_eq!(shown.as_ref(), "In 1");
    for item in ["In 1", "In 2", "In 1 + 2"] {
        assert!(
            opened.cx.read(|cx| select.read(cx).item(item).is_some()),
            "{item}"
        );
    }

    let trigger = opened.control("input");
    opened.click(trigger);
    opened.settle();
    let pair = opened.control("menu-In 1 + 2");
    opened.click(pair);
    opened.settle();
    let voice = id(VOICE);
    let input = opened.project(|project| {
        let track = project.resolve::<TrackState>(&voice).unwrap();
        project.state(&track).unwrap().input
    });
    assert_eq!(input, InputChannels::pair(1).unwrap());
    assert_eq!(opened.undo_label(), Some("Change input".to_string()));
    let record = std::fs::read_to_string(opened.path("state/arrangement/voice/instance.json"));
    assert!(record.unwrap().contains(r#""input": [1, 2]"#));
    opened.keys("cmd-z");
    let record = std::fs::read_to_string(opened.path("state/arrangement/voice/instance.json"));
    assert!(
        !record.unwrap().contains("input"),
        "the default is left out of the record"
    );
}

/// `r` records every armed audio track, mono and stereo, and the selected instrument track
/// takes the MIDI input as before. The whole recording is one undo step, and undo leaves the
/// files.
#[gpui::test]
fn r_records_every_armed_track_as_one_undo_step(cx: &mut TestAppContext) {
    let (mut opened, input) = open(cx);
    arm(&mut opened, "voice");
    arm(&mut opened, "guitar");
    let header = opened.track_header(0);
    opened.click(header);
    opened.settle();
    opened.keys("r");
    opened.settle();
    assert!(opened.is_recording());
    assert!(opened.playhead().playing);
    let on = midi::Played::On {
        pitch: sound_notes::Pitch::new(60).unwrap(),
        velocity: sound_notes::Velocity::new(90).unwrap(),
    };
    opened.play_midi(on);
    play(&mut opened, &input, 48_000, tone);
    opened.keys("r");
    opened.settle();
    assert!(!opened.is_recording());
    until_clips(&mut opened, &input, tone);

    let voice = audio_clips(&mut opened, VOICE);
    let guitar = audio_clips(&mut opened, GUITAR);
    assert_eq!((voice.len(), guitar.len()), (1, 1));
    assert_eq!(voice[0].asset.to_string(), "voice-take-1.wav");
    assert_eq!(guitar[0].asset.to_string(), "guitar-take-1.wav");
    assert!(opened.clip("arrangement/track-1/take").is_some());
    let files = [
        "assets/audio/voice-take-1.wav",
        "assets/audio/guitar-take-1.wav",
    ];
    let channels: Vec<u16> = files
        .iter()
        .map(|file| {
            hound::WavReader::open(opened.path(file))
                .unwrap()
                .spec()
                .channels
        })
        .collect();
    assert_eq!(channels, [1, 2]);
    // The voice heard input 1, the guitar both.
    let samples = |file: &str, opened: &mut Opened<'_>| -> Vec<f32> {
        let mut reader = hound::WavReader::open(opened.path(file)).unwrap();
        reader
            .samples::<f32>()
            .take(4)
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(samples(files[0], &mut opened), [0.5; 4]);
    assert_eq!(samples(files[1], &mut opened), [0.5, 0.25, 0.5, 0.25]);

    assert_eq!(opened.undo_label(), Some("Record".to_string()));
    opened.keys("cmd-z");
    assert!(audio_clips(&mut opened, VOICE).is_empty());
    assert!(audio_clips(&mut opened, GUITAR).is_empty());
    assert_eq!(opened.clip("arrangement/track-1/take"), None);
    assert_eq!(opened.undo_label(), None);
    assert!(files.iter().all(|file| opened.path(file).exists()));
}

/// With nothing armed and an audio track selected, `r` arms it and records it.
#[gpui::test]
fn r_arms_the_selected_audio_track_when_nothing_is_armed(cx: &mut TestAppContext) {
    let (mut opened, input) = open(cx);
    let header = opened.track_header(2);
    opened.click(header);
    opened.settle();
    opened.keys("r");
    opened.settle();
    assert!(armed(&mut opened, GUITAR));
    assert!(!armed(&mut opened, VOICE));
    play(&mut opened, &input, 24_000, tone);
    opened.keys("r");
    opened.settle();
    until_clips(&mut opened, &input, tone);
    assert_eq!(audio_clips(&mut opened, GUITAR).len(), 1);
    assert!(audio_clips(&mut opened, VOICE).is_empty());
    // The selected track was an audio track: no MIDI take.
    assert_eq!(opened.clip("arrangement/track-1/take"), None);
}

/// A clap the composer played on the second beat is on the second beat of the take, to the
/// frame, through everything the window does.
#[gpui::test]
fn a_take_lands_where_it_was_heard(cx: &mut TestAppContext) {
    let (mut opened, input) = open(cx);
    arm(&mut opened, "voice");
    opened.keys("r");
    opened.settle();
    // Once the project plays steadily, the engine frame at which the second beat sounds.
    play(&mut opened, &input, 4_096, |_| [0.0; 2]);
    let session = opened.session.clone();
    let status = opened.cx.read(|cx| session.read(cx).engine_status());
    let ahead = status.playhead_frame.0 as i64 - status.frames as i64;
    let beat = (960 * FRAMES_PER_TICK) as i64 - ahead;
    assert!(
        beat > opened.engine.frames() as i64,
        "the beat is still to come"
    );
    CLAP.with(|clap| clap.set(beat as u64));
    play(&mut opened, &input, 48_000, clap);
    opened.keys("r");
    opened.settle();
    until_clips(&mut opened, &input, clap);
    let clip = audio_clips(&mut opened, VOICE).remove(0);
    assert_eq!(clip.start, Ticks(0));
    // Where in the file the clap is, and where the clip plays that part of the file.
    let mut reader = hound::WavReader::open(opened.path("assets/audio/voice-take-1.wav")).unwrap();
    let samples: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
    let in_file = samples.iter().position(|sample| *sample > 0.5).unwrap() as f64;
    let heard_at = in_file - clip.file_start_seconds * 48_000.;
    assert_eq!(heard_at, (960 * FRAMES_PER_TICK) as f64);
}

thread_local! {
    static CLAP: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };
}

/// One loud frame on the left, at the engine frame the test chose.
fn clap(frame: u64) -> [f32; 2] {
    match CLAP.with(|clap| clap.get()) == frame {
        true => [0.9, 0.0],
        false => [0.0, 0.0],
    }
}

/// A stop, a pause and a seek each end the take where the playhead was, and the clip ends
/// there too, or where the file ends when the input had not brought that far.
#[gpui::test]
fn stop_pause_and_seek_end_the_take(cx: &mut TestAppContext) {
    let (mut opened, input) = open(cx);
    arm(&mut opened, "voice");
    for (ending, take) in [("stop", 1), ("pause", 2), ("seek", 3)] {
        opened.keys("r");
        opened.settle();
        play(&mut opened, &input, 24_000, tone);
        let before = opened.playhead().tick;
        match ending {
            "stop" => {
                let stop = opened.control("stop");
                opened.click(stop);
            }
            "pause" => opened.keys("space"),
            _ => {
                let at = opened.ruler(BAR * 4);
                opened.click(at);
            }
        }
        opened.settle();
        assert!(!opened.is_recording(), "{ending} ends the take");
        until_clips(&mut opened, &input, tone);
        let clips = audio_clips(&mut opened, VOICE);
        let clip = clips
            .iter()
            .find(|clip| clip.asset.to_string() == format!("voice-take-{take}.wav"))
            .unwrap_or_else(|| panic!("{ending}: no clip"));
        let end = clip_end(&mut opened, clip);
        assert!(
            end <= before + Ticks(2 * 2_048 / FRAMES_PER_TICK),
            "{ending}: {end:?}"
        );
        assert!(
            end > before.saturating_sub(Ticks(100)),
            "{ending}: {end:?} {before:?}"
        );
        // Back to the start and stopped for the next one.
        let stop = opened.control("stop");
        opened.click(stop);
        opened.settle();
    }
    // Three recordings, three undo steps.
    for _ in 0..3 {
        assert_eq!(opened.undo_label(), Some("Record".to_string()));
        opened.keys("cmd-z");
    }
}

/// The input goes away in the middle of a take, as an interface that is unplugged: the take
/// ends and keeps what was recorded, a notice says so, the tracks are disarmed, and nothing
/// crashes. Arming again opens the input again.
#[gpui::test]
fn an_input_that_goes_away_ends_the_take_and_keeps_it(cx: &mut TestAppContext) {
    let (mut opened, input) = open(cx);
    arm(&mut opened, "voice");
    opened.keys("r");
    opened.settle();
    play(&mut opened, &input, 24_000, tone);
    input.unplug();
    play(&mut opened, &input, 8_192, tone);
    assert!(!opened.is_recording());
    until_clips(&mut opened, &input, tone);
    let clips = audio_clips(&mut opened, VOICE);
    assert_eq!(clips.len(), 1);
    let file = opened.path("assets/audio/voice-take-1.wav");
    let frames = hound::WavReader::open(file).unwrap().duration();
    assert!(frames >= 20_000, "{frames} frames kept");
    let notice = opened.notice().unwrap();
    assert!(notice.contains("audio input went away"), "{notice}");
    assert!(!armed(&mut opened, VOICE));
    assert_eq!(opened.undo_label(), Some("Record".to_string()));

    arm(&mut opened, "voice");
    assert_eq!(input.openings.get(), 2);
}

/// A change of the tempo map ends the take where it happened, as a seek does: every tick after
/// it is at another time now. The clip holds what was recorded up to there, placed under the
/// tempo it was heard at, and the clap in it is where it was heard.
#[gpui::test]
fn a_tempo_change_ends_the_take_where_it_happened(cx: &mut TestAppContext) {
    let (mut opened, input) = open(cx);
    arm(&mut opened, "voice");
    opened.keys("r");
    opened.settle();
    play(&mut opened, &input, 4_096, |_| [0.0; 2]);
    let session = opened.session.clone();
    let status = opened.cx.read(|cx| session.read(cx).engine_status());
    let ahead = status.playhead_frame.0 as i64 - status.frames as i64;
    CLAP.with(|clap| clap.set(((960 * FRAMES_PER_TICK) as i64 - ahead) as u64));
    play(&mut opened, &input, 48_000, clap);
    let heard_under = opened.project(|project| project.clock().clone());
    let changed_at = opened.playhead().tick;
    assert!(changed_at > Ticks(1_500), "{changed_at:?}");
    opened.write_tempo_map(
        r#"{"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 90.0}]}"#,
    );
    assert!(!opened.is_recording(), "the change ended the take");
    until_clips(&mut opened, &input, clap);

    let clip = audio_clips(&mut opened, VOICE).remove(0);
    assert_eq!(clip.start, Ticks(0));
    let mut reader = hound::WavReader::open(opened.path("assets/audio/voice-take-1.wav")).unwrap();
    let samples: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
    // It plays what was heard from the start up to the change, under the old tempo: the file
    // holds up to there and not beyond, so the clip plays to its end.
    let file_end = samples.len() as f64 / 48_000.;
    let played = clip.file_end_seconds.unwrap_or(file_end) - clip.file_start_seconds;
    let heard = heard_under.seconds_of(changed_at) - heard_under.seconds_of(Ticks(0));
    assert!(
        (played - heard).abs() < 1. / 48_000.,
        "{played} against {heard}"
    );
    let in_file = samples.iter().position(|sample| *sample > 0.5).unwrap() as f64;
    let heard_at = in_file - clip.file_start_seconds * 48_000.;
    assert_eq!(heard_at, heard_under.frame_of(Ticks(960)).0 as f64);
}

/// The instrument track that takes the MIDI input is deleted while it records. Its performance
/// stays under `assets/takes/`, the notice says so, and the audio of the armed track becomes
/// its clip all the same, as one undo step.
#[gpui::test]
fn deleting_the_midi_track_while_it_records_keeps_the_audio(cx: &mut TestAppContext) {
    let (mut opened, input) = open(cx);
    arm(&mut opened, "voice");
    let header = opened.track_header(0);
    opened.click(header);
    opened.settle();
    opened.keys("r");
    opened.settle();
    let on = midi::Played::On {
        pitch: sound_notes::Pitch::new(60).unwrap(),
        velocity: sound_notes::Velocity::new(90).unwrap(),
    };
    opened.play_midi(on);
    play(&mut opened, &input, 24_000, tone);
    opened.edit(|project| {
        let mut changes = Changes::new();
        changes.delete(&id("arrangement/track-1"));
        project.commit("Delete track", changes)
    });
    opened.keys("r");
    opened.settle();
    until_clips(&mut opened, &input, tone);
    assert_eq!(audio_clips(&mut opened, VOICE).len(), 1);
    assert_eq!(opened.undo_label(), Some("Record".to_string()));
    assert!(opened.path("assets/takes/take-1.json").exists());
    let notice = opened.notice().unwrap();
    assert!(notice.contains("MIDI take went away"), "{notice}");
}
