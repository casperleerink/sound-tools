//! The audio states of the window, as drawn in `docs/mockups/`:
//!
//! - `audio-window.png`: the piece with two audio tracks, Voice and Guitar, playing, a clip of
//!   the voice selected with fades and gain, and the panel of the voice: the Clip card, the
//!   hairline, a Compressor and a Reverb. `window-audio.png` of the mockups.
//! - `audio-hover.png`: the pointer on a clip that is not selected: its three handles.
//! - `audio-trim.png`: its left edge dragged in, the button still down: the part of the file it
//!   hides shows faint past the edge.
//! - `audio-fade.png`: its fade in handle dragged, with its label.
//! - `audio-gain.png`: its gain handle dragged down, the waveform smaller, with its label.
//! - `audio-missing-muted.png`: a clip whose file is not there, and the guitar muted.
//! - `audio-card-empty.png`: the panel of an audio track with no clip of it selected.
//! - `audio-card-expanded.png`: the Clip card expanded: Start and End.
//! - `audio-drop-new-track.png`: a file from the Finder over the space under the last track:
//!   its ghost, and `New audio track` in the header column.
//! - `audio-drop-track.png`: two files over the guitar, one after the other.
//! - `audio-add-track.png`: the add track button under the guitar, its menu open: an
//!   instrument or an audio track.
//! - `audio-armed.png`: the voice armed, its input level in its header and on the meter of its
//!   panel, the guitar not armed, and the input select under M and S. `audio-clip.png`.
//! - `audio-input-select.png`: the input select open: each channel alone, then the pair.
//! - `audio-recording.png`: both armed and recording from bar 5 over their clips, the takes
//!   growing to the playhead at 7.3 with their waveforms, and the panel of the voice with no
//!   clip selected. `recording.png` of the mockups.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result};
use arrangement::view::ArrangementView;
use arrangement::{AudioClip, Colour, InputChannels, TrackState};
use gpui::{
    Entity, ExternalPaths, FileDropEvent, HeadlessAppContext, Pixels, PlatformInput, Point, point,
    px,
};
use runtime::window::audio_input::{OpenInput, OpenedInput};
use sound_core::{CaptureWriter, Changes, Instance, InstanceId, Project, Ticks};
use sound_media::AudioAsset;
use sound_ui::{LiveBody, Waveforms};

use super::{
    BAR, HEADER_WIDTH, Opened, RULER_HEIGHT, TRACK_HEIGHT, add_compressor, add_reverb, piece,
};
use compressor::CompressorState;

/// A 16-bit mono WAV file at 48 kHz, `seconds` long, of what `sound` gives at each time.
pub(crate) fn write_wav(path: &Path, seconds: f64, sound: impl Fn(f64) -> f64) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("a folder")?)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for frame in 0..(seconds * 48_000.) as u64 {
        let time = frame as f64 / 48_000.;
        let sample = (sound(time).clamp(-1., 1.) * 32_000.) as i16;
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

/// A noise that is the same every time.
fn noise(time: f64) -> f64 {
    let x = (time * 48_000.).floor();
    ((x * 12.9898).sin() * 43_758.545).fract() * 2. - 1.
}

/// Something like a voice: syllables in phrases, with gaps between the phrases.
fn voice(time: f64) -> f64 {
    let phrase = (time * 0.5).fract();
    let open = if phrase < 0.72 { 1. } else { 0. };
    let syllable = (time * std::f64::consts::PI * 4.3).sin().abs().powf(1.5);
    let swell = 0.55 + 0.45 * (time * 1.3).sin().abs();
    let tone = (time * 2. * std::f64::consts::PI * 180.).sin() * 0.6 + noise(time) * 0.4;
    tone * syllable * swell * open * 0.9
}

/// Something like a guitar: a strum on every eighth at 120 bpm, each dying away.
fn strum(time: f64) -> f64 {
    let since = time % 0.25;
    let accent = if ((time / 0.25) as u64).is_multiple_of(2) {
        1.
    } else {
        0.7
    };
    let decay = (-since * 14.).exp();
    let tone = (time * 2. * std::f64::consts::PI * 220.).sin() * 0.5 + noise(time) * 0.5;
    tone * decay * accent * 0.8
}

/// Puts the audio files in `assets/audio/` and an audio track of `clips`: each a file, where it
/// starts in bars, where it starts and ends in the file, and its gain and fades.
fn add_audio_track(
    project: &mut Project,
    name: &str,
    colour: Colour,
    clips: &[(&str, f64, f64, Option<f64>, f32, (f32, f32))],
) -> Result<Instance<TrackState>> {
    let arrangement = runtime::main_arrangement(project).context("no arrangement")?;
    let mut changes = Changes::new();
    let track =
        arrangement::add_audio_track(project, &mut changes, arrangement.id(), name, colour)?;
    for (index, (file, bar, from, to, gain_db, (fade_in_ms, fade_out_ms))) in
        clips.iter().enumerate()
    {
        let start = Ticks((bar * BAR as f64) as u64);
        let clip = AudioClip {
            file_start_seconds: *from,
            file_end_seconds: *to,
            gain_db: *gain_db,
            fade_in_ms: *fade_in_ms,
            fade_out_ms: *fade_out_ms,
            layer: index as u32,
            ..AudioClip::new(AudioAsset::new(file)?, start)
        };
        let id = track.id().child(&format!("take-{index}"))?;
        changes.create(id, clip);
    }
    project.commit("Add audio track", changes)?;
    Ok(track)
}

/// The piece, and a voice and a guitar as audio: three takes of the voice, the second trimmed
/// and faded, and the guitar in two clips of one file.
fn audio_piece(project: &mut Project) -> Result<()> {
    piece(project)?;
    let audio = project.root().join("assets/audio");
    write_wav(&audio.join("voice-take-1.wav"), 4.6, voice)?;
    write_wav(&audio.join("voice-take-3.wav"), 14.6, |time| {
        voice(time + 3.)
    })?;
    write_wav(&audio.join("voice-take-4.wav"), 6.5, |time| {
        voice(time + 7.)
    })?;
    write_wav(&audio.join("guitar.wav"), 24., strum)?;
    add_audio_track(
        project,
        "Voice",
        Colour::Mauve,
        &[
            ("voice-take-1.wav", 1.5, 0., None, 0., (0., 0.)),
            ("voice-take-3.wav", 4., 2.1, Some(10.1), -0.9, (150., 260.)),
            ("voice-take-4.wav", 8.5, 0., None, 0., (0., 0.)),
        ],
    )?;
    add_audio_track(
        project,
        "Guitar",
        Colour::Teal,
        &[
            ("guitar.wav", 0., 0., Some(16.), 0., (0., 0.)),
            ("guitar.wav", 8., 16., None, 0., (0., 0.)),
        ],
    )?;
    add_compressor(
        project,
        "voice",
        CompressorState {
            threshold_db: -18.,
            ratio: 4.,
            ..CompressorState::default()
        },
    )?;
    add_reverb(project, "voice")?;
    Ok(())
}

/// Lets the background threads make every waveform the window asked for, and draws again.
pub(crate) fn wait_for_waveforms(cx: &mut HeadlessAppContext, opened: &Opened) -> Result<()> {
    for _ in 0..200 {
        cx.run_until_parked();
        let asking = cx.update(|cx| {
            let waveforms = Waveforms::entity(cx);
            waveforms.read(cx).making()
        });
        if asking == 0 {
            cx.update(|cx| opened.session.update(cx, |_, cx| cx.notify()));
            cx.run_until_parked();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    anyhow::bail!("the waveforms were not made")
}

/// The place of a tick on a track row, in the middle of the row.
fn at(tick: f64, row: f32) -> Point<Pixels> {
    let viewport = arrangement::view::layout::Viewport::default();
    point(
        px(HEADER_WIDTH + viewport.x_of(Ticks(tick as u64))),
        px(48. + RULER_HEIGHT + TRACK_HEIGHT * (row + 0.5)),
    )
}

/// The same place, `down` points from the top of the clip there.
fn in_clip(tick: f64, row: f32, down: f32) -> Point<Pixels> {
    let middle = at(tick, row);
    point(middle.x, middle.y - px(TRACK_HEIGHT / 2. - 4. - down))
}

fn view(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<Entity<ArrangementView>> {
    opened.arrangement_view(cx)
}

/// Selects a clip and opens the panel of its track, where its Clip card is.
fn select_and_open(opened: &Opened, clip: &str, cx: &mut HeadlessAppContext) -> Result<()> {
    let id = InstanceId::new(clip)?;
    let view = view(opened, cx)?;
    let timeline = opened.timeline_view(cx)?;
    let track = cx.update(|cx| {
        let project = opened.session.read(cx).project();
        project
            .resolve::<TrackState>(&id.parent().context("a track")?)
            .context("no track")
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        timeline.update(cx, |timeline, cx| {
            timeline.select_clip(Some(id.clone()), cx)
        });
        view.update(cx, |view, cx| view.open_track_panel(track, window, cx));
    })?;
    cx.run_until_parked();
    Ok(())
}

/// A drag of files from the Finder, entered and moved to `to`, not dropped.
fn drag_files(
    opened: &Opened,
    paths: &[PathBuf],
    to: Point<Pixels>,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    let paths = ExternalPaths(paths.iter().cloned().collect());
    let entered = FileDropEvent::Entered {
        position: to - point(px(40.), px(0.)),
        paths,
    };
    opened.mouse(PlatformInput::FileDrop(entered), cx)?;
    for step in 0..3 {
        let position = to - point(px(10. * (2 - step) as f32), px(0.));
        let pending = FileDropEvent::Pending { position };
        opened.mouse(PlatformInput::FileDrop(pending), cx)?;
        // The background thread reads what each file is, for the length of its ghost.
        cx.run_until_parked();
    }
    Ok(())
}

/// Nine tracks of audio, every one full of clips of one file of two seconds, on the screen at
/// once: the most audio a window of this size shows.
fn audio_tracks(project: &mut Project) -> Result<()> {
    let audio = project.root().join("assets/audio");
    write_wav(&audio.join("phrase.wav"), 2., voice)?;
    for track in 0..9 {
        let clips: Vec<_> = (0..24)
            .map(|index| ("phrase.wav", f64::from(index), 0., None, 0., (20., 40.)))
            .collect();
        let colour = Colour::ALL[track % Colour::ALL.len()];
        add_audio_track(project, &format!("Take {track}"), colour, &clips)?;
    }
    Ok(())
}

/// Frame times with audio clips on screen, as the scale project measures them with notes: one
/// update and the frame it causes, while playing and while scrolling, and one mouse move of a
/// drag of an audio clip.
fn frame_times(cx: &mut HeadlessAppContext) -> Result<()> {
    let mut opened = Opened::new(cx, audio_tracks)?;
    wait_for_waveforms(cx, &opened)?;
    let timeline = opened.timeline_view(cx)?;
    let clips = cx.update(|cx| {
        let project = opened.session.read(cx).project();
        let arrangement = InstanceId::new("arrangement")?;
        let tracks = project.children::<TrackState>(&arrangement);
        let clips = tracks.map(|(track, _)| project.children::<AudioClip>(track.id()).count());
        anyhow::Ok(clips.sum::<usize>())
    })?;
    println!("audio frame times: {clips} audio clips on 9 tracks, 126 of them on screen");
    opened.play_from(Ticks(0), cx)?;
    let mut playing = Vec::new();
    for _ in 0..200 {
        playing.push(opened.advance(800, cx));
    }
    let start = arrangement::view::layout::Viewport::default();
    let mut scrolling = Vec::new();
    for step in 0..200 {
        let viewport = start.scrolled(-((step % 40) as f32) * 7.0, 0.0);
        let started = std::time::Instant::now();
        cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.set_viewport(viewport, cx)));
        cx.run_until_parked();
        scrolling.push(started.elapsed());
    }
    let from = at(4.5 * BAR as f64, 2.);
    let moves = opened.drag(from, point(px(3.), px(0.)), 100, cx)?;
    super::print_times("audio clips: frame while playing", playing);
    super::print_times("audio clips: frame while scrolling", scrolling);
    super::print_times("audio clips: clip drag in time, per mouse move", moves);
    Ok(())
}

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    // The window of the mockup: playing at 6.3, inside the selected clip of the voice.
    let mut opened = Opened::new(cx, audio_piece)?;
    select_and_open(&opened, "arrangement/voice/take-1", cx)?;
    opened.play_from(Ticks(5 * BAR + 2 * 960), cx)?;
    opened.listen(0.3, cx)?;
    wait_for_waveforms(cx, &opened)?;
    save(cx, &opened, "audio-window")?;
    let view = view(&opened, cx)?;
    let card = cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned().context("no panel")?;
        let card = panel.read(cx).clip_card().cloned();
        card.context("no clip card")
    })?;
    cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(true, cx)));
    cx.run_until_parked();
    save(cx, &opened, "audio-card-expanded")?;
    cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(false, cx)));
    drop(opened);

    // One clip under the pointer, and dragged three ways, the button still down.
    // Where the pointer is on the last clip of the voice, and where a drag takes it: the body,
    // the left edge in by half a bar, the fade in handle out by 0.42 of a bar, and the gain
    // handle down by six decibels, 72 dB over 200 points.
    let (row, start) = (3., 8.5 * BAR as f64);
    let bar = 96.;
    let poses = [
        ("audio-hover", at(start + BAR as f64, row), None),
        (
            "audio-trim",
            at(start + 20., row),
            Some(point(px(bar / 2.), px(0.))),
        ),
        (
            "audio-fade",
            in_clip(start + 60., row, 7.),
            Some(point(px(bar * 0.42), px(0.))),
        ),
        (
            "audio-gain",
            in_clip(start + 1.625 * BAR as f64, row, 7.),
            Some(point(px(0.), px(200. / 72. * 6.))),
        ),
    ];
    for (name, pointer, drag) in poses {
        let opened = Opened::new(cx, audio_piece)?;
        let hover = gpui::MouseMoveEvent {
            position: pointer,
            pressed_button: None,
            modifiers: gpui::Modifiers::default(),
        };
        opened.mouse(PlatformInput::MouseMove(hover), cx)?;
        if let Some(by) = drag {
            opened.press_and_move(pointer, pointer + by, cx)?;
        }
        wait_for_waveforms(cx, &opened)?;
        save(cx, &opened, name)?;
    }

    // A clip whose file is not there, and the guitar muted.
    let opened = Opened::new(cx, |project| {
        audio_piece(project)?;
        std::fs::remove_file(project.root().join("assets/audio/voice-take-4.wav"))?;
        let guitar = project
            .resolve::<TrackState>(&InstanceId::new("arrangement/guitar")?)
            .context("no guitar")?;
        let mut state = project.state(&guitar).context("no guitar")?.clone();
        state.mute = true;
        let mut changes = Changes::new();
        changes.set(&guitar, state);
        project.commit("Mute", changes)?;
        Ok(())
    })?;
    wait_for_waveforms(cx, &opened)?;
    save(cx, &opened, "audio-missing-muted")?;
    drop(opened);

    // The panel of an audio track with no clip of it selected.
    let opened = Opened::new(cx, audio_piece)?;
    opened.click_track_header(4., cx)?;
    wait_for_waveforms(cx, &opened)?;
    save(cx, &opened, "audio-card-empty")?;

    // Files from the Finder: over the space under the last track, and over the guitar.
    let folder = tempfile::tempdir()?;
    let shaker = folder.path().join("shaker-loop.wav");
    write_wav(&shaker, 4., |time| {
        noise(time) * (-(time % 0.125) * 30.).exp() * 0.6
    })?;
    let strum_2 = folder.path().join("strum-2.wav");
    write_wav(&strum_2, 3., |time| strum(time + 1.))?;
    drag_files(
        &opened,
        std::slice::from_ref(&shaker),
        at(4.25 * BAR as f64, 5.),
        cx,
    )?;
    save(cx, &opened, "audio-drop-new-track")?;
    opened.mouse(PlatformInput::FileDrop(FileDropEvent::Exited), cx)?;
    drag_files(&opened, &[strum_2, shaker], at(9. * BAR as f64, 4.), cx)?;
    save(cx, &opened, "audio-drop-track")?;
    opened.mouse(PlatformInput::FileDrop(FileDropEvent::Exited), cx)?;

    frame_times(cx)?;

    // The menu of the add track button, open.
    let shown = opened.arrangement_view(cx)?;
    let menu = cx.update(|cx| shown.read(cx).add_track_menu(cx));
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.open(window, cx));
    })?;
    cx.run_until_parked();
    save(cx, &opened, "audio-add-track")?;
    drop(opened);
    armed_frame_times(cx)?;
    recording(cx, save)
}

/// An audio input the window opens as its default input, with no device: the voice on input 1
/// and the guitar on input 2, as the engine runs. With no device the window has no timing, so
/// an input frame is heard at the engine frame of the same number.
#[derive(Clone, Default)]
struct SimulatedInput {
    writer: Arc<Mutex<Option<CaptureWriter>>>,
    written: Arc<AtomicU64>,
}

impl SimulatedInput {
    fn opener(&self) -> OpenInput {
        let input = self.clone();
        Arc::new(move || {
            let (writer, reader) = sound_core::capture(48_000, 2);
            if let Ok(mut slot) = input.writer.lock() {
                *slot = Some(writer);
            }
            input.written.store(0, Ordering::Relaxed);
            Ok(OpenedInput {
                stream: None,
                reader,
            })
        })
    }

    fn write_until(&self, until: u64) {
        let Ok(mut writer) = self.writer.lock() else {
            return;
        };
        let Some(writer) = writer.as_mut() else {
            return;
        };
        let first = self.written.load(Ordering::Relaxed);
        let frames = first..until.max(first);
        let samples: Vec<f32> = frames
            .flat_map(|frame| {
                let time = frame as f64 / 48_000.;
                [(voice(time) * 0.5) as f32, (strum(time) * 0.5) as f32]
            })
            .collect();
        writer.write(&samples, first * 1_000_000_000 / 48_000, 0);
        self.written.store(until.max(first), Ordering::Relaxed);
    }
}

/// The piece with audio, the guitar recording input 2.
fn studio(project: &mut Project) -> Result<()> {
    audio_piece(project)?;
    let guitar = project
        .resolve::<TrackState>(&InstanceId::new("arrangement/guitar")?)
        .context("no guitar")?;
    let mut state = project.state(&guitar).context("no guitar")?.clone();
    state.input = InputChannels::mono(2).context("input 2")?;
    let mut changes = Changes::new();
    changes.set(&guitar, state);
    project.commit("Guitar on input 2", changes)?;
    Ok(())
}

fn arm(opened: &Opened, tracks: &[&str], cx: &mut HeadlessAppContext) -> Result<()> {
    for track in tracks {
        let track = InstanceId::new(track)?;
        cx.update(|cx| {
            let recording = opened.session.read(cx).recording().clone();
            recording.update(cx, |recording, cx| recording.set_armed(track, true, cx));
        });
    }
    cx.run_until_parked();
    Ok(())
}

/// Runs the engine and the input for `seconds` in polls of 16 ms, and what the timers of the
/// window do after each: the poll of the transport, which reads the input and runs the
/// recorder, and the meters.
fn run_input(
    opened: &mut Opened,
    input: &SimulatedInput,
    seconds: f32,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    let poll = (48_000. * sound_ui::POLL_INTERVAL.as_secs_f32()) as usize;
    for _ in 0..(seconds * 1000. / 16.) as usize {
        opened.advance(poll, cx);
        input.write_until(opened.engine.frames());
        poll_window(opened, cx)?;
    }
    Ok(())
}

fn poll_window(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<()> {
    let view = view(opened, cx)?;
    cx.update(|cx| {
        let transport = opened.window.read(cx)?.transport().clone();
        transport.update(cx, |pill, cx| {
            pill.poll_input(cx);
            pill.read_meter(cx);
        });
        if let Some(panel) = view.read(cx).track_panel().cloned() {
            panel.update(cx, |panel, cx| panel.read_meter(cx));
        }
        anyhow::Ok(())
    })?;
    cx.run_until_parked();
    Ok(())
}

/// Lets the recorder, which runs on a background thread, write what came in, and the window
/// line the takes up with the timeline.
fn wait_for_takes(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<()> {
    for _ in 0..200 {
        poll_window(opened, cx)?;
        let lined_up = cx.update(|cx| {
            let recording = opened.session.read(cx).recording().read(cx);
            let takes = recording.takes();
            !takes.is_empty()
                && takes
                    .iter()
                    .all(|take| !matches!(take.body, LiveBody::Audio(None)))
        });
        if lined_up {
            std::thread::sleep(Duration::from_millis(50));
            poll_window(opened, cx)?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    anyhow::bail!("the takes were not lined up")
}

/// The frame one poll of the window causes on the nine tracks of audio while one of them is
/// armed and the project is stopped: the input level moves every poll.
fn armed_frame_times(cx: &mut HeadlessAppContext) -> Result<()> {
    let input = SimulatedInput::default();
    let mut opened = Opened::with_input(cx, audio_tracks, Some(input.opener()))?;
    wait_for_waveforms(cx, &opened)?;
    arm(&opened, &["arrangement/take-0"], cx)?;
    let poll = (48_000. * sound_ui::POLL_INTERVAL.as_secs_f32()) as usize;
    let mut times = Vec::new();
    for _ in 0..200 {
        let mut buffer = vec![0.0_f32; poll * 2];
        opened.engine.process_block(&mut buffer);
        input.write_until(opened.engine.frames());
        let started = std::time::Instant::now();
        poll_window(&opened, cx)?;
        times.push(started.elapsed());
    }
    super::print_times(
        "audio clips: frame per poll, one track armed, stopped",
        times,
    );
    Ok(())
}

/// Arming, the input select and a recording, from a simulated input.
fn recording(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    // The voice armed, stopped, with its panel open: the level of its input.
    let input = SimulatedInput::default();
    let mut opened = Opened::with_input(cx, studio, Some(input.opener()))?;
    arm(&opened, &["arrangement/voice"], cx)?;
    opened.click_track_header(3., cx)?;
    run_input(&mut opened, &input, 0.5, cx)?;
    wait_for_waveforms(cx, &opened)?;
    save(cx, &opened, "audio-armed")?;
    let view = view(&opened, cx)?;
    let select = cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned().context("no panel")?;
        let select = panel.read(cx).input_select().cloned();
        select.context("no input select")
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        select.update(cx, |select, cx| select.open(window, cx));
    })?;
    cx.run_until_parked();
    save(cx, &opened, "audio-input-select")?;
    drop(opened);

    // Both armed, recording from bar 5 to 7.3 over their clips.
    let input = SimulatedInput::default();
    let mut opened = Opened::with_input(cx, studio, Some(input.opener()))?;
    arm(&opened, &["arrangement/voice", "arrangement/guitar"], cx)?;
    opened.click_track_header(3., cx)?;
    cx.update(|cx| {
        opened
            .session
            .update(cx, |session, _| session.engine().seek(Ticks(4 * BAR)))
    });
    opened.advance(64, cx);
    cx.update(|cx| {
        let transport = opened.window.read(cx)?.transport().clone();
        transport.update(cx, |pill, cx| pill.toggle_recording(cx));
        anyhow::Ok(())
    })?;
    run_input(&mut opened, &input, 4.6, cx)?;
    wait_for_takes(&opened, cx)?;
    wait_for_waveforms(cx, &opened)?;
    save(cx, &opened, "audio-recording")?;
    Ok(())
}
