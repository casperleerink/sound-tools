//! Audio tracks and clips in the window: selecting, moving, trimming, fading and turning an
//! audio clip up or down, copying, pasting and deleting it, the Clip card, dropping files from
//! the Finder and adding an audio track. Every edit is checked as one undo step on the files.

use std::path::{Path, PathBuf};

use arrangement::view::DropTarget;
use arrangement::view::clip_card::ClipCard;
use arrangement::{AudioClip, Colour, TrackKind, TrackState};
use gpui::{
    Entity, ExternalPaths, FileDropEvent, Modifiers, Pixels, Point, TestAppContext, point, px,
};
use sound_core::{Changes, Project, Ticks};
use sound_media::AudioAsset;

use crate::support::{self, Opened, id, mark, one_undo_step, write_outside};

const VOICE: &str = "arrangement/voice";
const GUITAR: &str = "arrangement/guitar";
const LONG: &str = "arrangement/voice/long";
const STEADY: &str = "arrangement/voice/steady";
const BAR: u64 = 3840;
/// Points per bar at the zoom the window opens with.
const BAR_WIDTH: f32 = 96.;
/// At 120 bpm a second is half a bar.
const SECOND: u64 = BAR / 2;

/// A steady mono WAV file at 48 kHz, 16 bits, of `seconds` at `level`.
fn write_wav(path: &Path, seconds: f64, level: f64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(seconds * 48_000.) as u64 {
        writer.write_sample((level * 32_768.) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// The default project with two audio tracks under its instrument track: `voice` with a clip
/// of a steady level of one second at tick 0 and a clip of four seconds at bar 2, and `guitar`,
/// empty. The long clip is over the steady one.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project: &mut Project| {
        let audio = project.root().join("assets/audio");
        write_wav(&audio.join("steady.wav"), 1., 0.5);
        write_wav(&audio.join("long.wav"), 4., 0.25);
        let arrangement = runtime::main_arrangement(project).unwrap();
        let mut changes = Changes::new();
        let add = |project: &Project, changes: &mut Changes, name, colour| {
            arrangement::add_audio_track(project, changes, arrangement.id(), name, colour).unwrap()
        };
        let voice = add(project, &mut changes, "Voice", Colour::Peach);
        let steady = AudioClip::new(AudioAsset::new("steady.wav").unwrap(), Ticks(0));
        changes.create(voice.id().child("steady").unwrap(), steady);
        let long = AudioClip {
            layer: 1,
            ..AudioClip::new(AudioAsset::new("long.wav").unwrap(), Ticks(BAR))
        };
        changes.create(voice.id().child("long").unwrap(), long);
        project.commit("Add audio", changes).unwrap();
        let mut changes = Changes::new();
        add(project, &mut changes, "Guitar", Colour::Teal);
        project.commit("Add guitar", changes).unwrap();
        project.clear_history();
        assert_eq!(project.problems(), []);
    })
}

fn audio_clip(opened: &mut Opened<'_>, clip: &str) -> Option<AudioClip> {
    let clip = id(clip);
    opened.project(|project| {
        let instance = project.resolve::<AudioClip>(&clip)?;
        project.state(&instance).cloned()
    })
}

/// A place `down` points from the top of the clip at `tick` on a row. A clip starts 4 points
/// into its row, and its handles are 7 points down from its top.
fn in_clip(opened: &mut Opened<'_>, tick: u64, row: usize, down: f32) -> Point<Pixels> {
    let middle = opened.at(tick, row);
    point(middle.x, middle.y - px(32. - 4. - down))
}

fn right(position: Point<Pixels>, points: f32) -> Point<Pixels> {
    position + point(px(points), px(0.))
}

// Keeps the checks of step 1a: the window keeps working around an audio track.

#[gpui::test]
fn the_panel_of_an_audio_track_has_no_instrument(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.track_header(1);
    opened.click(header);
    assert_eq!(opened.panel_track(), Some(id(VOICE)));
    assert!(opened.find("instrument-picker").is_none());
    assert!(opened.find("rack-instrument").is_none());
    // The panel of the instrument track still has its instrument.
    let header = opened.track_header(0);
    opened.click(header);
    assert!(opened.find("instrument-picker").is_some());
}

#[gpui::test]
fn a_double_click_on_an_audio_track_adds_no_note_clip(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let empty = opened.at(8 * BAR, 1);
    opened.double_click(empty);
    let children =
        opened.project(|project| project.children::<sound_notes::Clip>(&id(VOICE)).count());
    assert_eq!(children, 0);
    assert_eq!(opened.undo_label(), None);
    // On the instrument track it still does.
    let empty = opened.at(8 * BAR, 0);
    opened.double_click(empty);
    assert_eq!(opened.undo_label().as_deref(), Some("Add clip"));
}

#[gpui::test]
fn a_keyboard_never_plays_into_an_audio_track(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (selected, first) = opened.project(|project| {
        let voice = project.resolve::<TrackState>(&id(VOICE)).unwrap();
        let selected = runtime::window::recording::target_track(project, Some(voice.id()));
        let first = runtime::window::recording::target_track(project, None);
        (
            selected.map(|track| track.id().clone()),
            first.map(|track| track.id().clone()),
        )
    });
    assert_eq!(selected, None);
    assert_eq!(first, Some(id("arrangement/track-1")));
}

#[gpui::test]
fn the_audio_plays_in_the_window(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    opened.keys("space");
    let output = opened.render(4_800);
    assert!(
        output.iter().any(|sample| (*sample - 0.5).abs() < 1e-6),
        "{:?}",
        &output[..8]
    );
}

// Editing audio clips on the timeline.

#[gpui::test]
fn an_audio_clip_is_selected_and_moved_as_one_undo_step_and_goes_on_top(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let body = opened.at(BAR + BAR / 2, 1);
    opened.drag(body, right(body, BAR_WIDTH));
    assert_eq!(opened.selected_clip(), Some(id(LONG)));
    let long = audio_clip(&mut opened, LONG).unwrap();
    assert_eq!(long.start, Ticks(2 * BAR));
    one_undo_step(&mut opened, "Move clip", &before);

    // The steady clip under it, moved a step with the keys, goes on top of the long one.
    let steady = opened.at(SECOND / 2, 1);
    opened.click(steady);
    opened.keys("right");
    let steady = audio_clip(&mut opened, STEADY).unwrap();
    assert_eq!((steady.start, steady.layer), (Ticks(240), 2));
    assert_eq!(opened.undo_label().as_deref(), Some("Nudge clip"));
}

#[gpui::test]
fn an_audio_clip_goes_to_audio_tracks_only(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // Up over the instrument track: it stays on the voice.
    let body = opened.at(BAR + BAR / 2, 1);
    let above = opened.at(BAR + BAR / 2, 0);
    opened.drag(body, above);
    assert!(audio_clip(&mut opened, LONG).is_some());
    assert_eq!(opened.undo_label(), None);
    // Down onto the guitar: it goes there, as a new file under the guitar.
    let below = opened.at(BAR + BAR / 2, 2);
    opened.drag(body, below);
    let moved = format!("{GUITAR}/long");
    assert!(audio_clip(&mut opened, &moved).is_some());
    assert_eq!(opened.selected_clip(), Some(id(&moved)));
    // The keys: up goes back to the voice, and up again finds no audio track to go to.
    opened.keys("up");
    assert!(audio_clip(&mut opened, LONG).is_some());
    let label = opened.undo_label();
    opened.keys("up");
    assert!(audio_clip(&mut opened, LONG).is_some());
    assert_eq!(opened.undo_label(), label);
}

#[gpui::test]
fn the_edges_trim_the_file_and_keep_the_sound_in_place(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    // The left edge in by a second.
    let left = right(opened.at(BAR, 1), 2.);
    opened.drag(left, right(left, BAR_WIDTH / 2.));
    let long = audio_clip(&mut opened, LONG).unwrap();
    assert_eq!(
        (long.start, long.file_start_seconds),
        (Ticks(BAR + SECOND), 1.0)
    );
    assert_eq!(long.file_end_seconds, None);
    one_undo_step(&mut opened, "Trim clip", &before);
    opened.keys("cmd-z");

    // The right edge in by a second: the clip ends at 3 s of its file.
    let before = mark(&mut opened);
    let end = right(opened.at(BAR + 4 * SECOND, 1), -2.);
    opened.drag(end, right(end, -BAR_WIDTH / 2.));
    let long = audio_clip(&mut opened, LONG).unwrap();
    assert_eq!(long.file_end_seconds, Some(3.0));
    one_undo_step(&mut opened, "Trim clip", &before);
}

#[gpui::test]
fn the_handles_drag_the_fades_and_the_gain(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // The fade in handle, at the top left corner while there is no fade, a second right.
    let before = mark(&mut opened);
    let handle = right(in_clip(&mut opened, BAR, 1, 7.), 5.);
    opened.drag(handle, right(handle, BAR_WIDTH / 2.));
    assert_eq!(audio_clip(&mut opened, LONG).unwrap().fade_in_ms, 1000.);
    one_undo_step(&mut opened, "Change fade in", &before);

    // The fade out handle, half a second left.
    let handle = right(in_clip(&mut opened, BAR + 4 * SECOND, 1, 7.), -5.);
    opened.drag(handle, right(handle, -BAR_WIDTH / 4.));
    assert_eq!(audio_clip(&mut opened, LONG).unwrap().fade_out_ms, 500.);
    assert_eq!(opened.undo_label().as_deref(), Some("Change fade out"));

    // The gain handle in the middle, down by six decibels: 72 dB over 200 points.
    let before = mark(&mut opened);
    let handle = in_clip(&mut opened, BAR + 2 * SECOND, 1, 7.);
    opened.drag(handle, handle + point(px(0.), px(200. / 72. * 6.)));
    assert_eq!(audio_clip(&mut opened, LONG).unwrap().gain_db, -6.);
    one_undo_step(&mut opened, "Change gain", &before);
}

#[gpui::test]
fn alt_up_and_alt_down_change_the_gain_of_the_selected_clips(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let long = opened.at(BAR + BAR / 2, 1);
    opened.click(long);
    let steady = opened.at(SECOND / 2, 1);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    opened.click_with(steady, shift);
    let before = mark(&mut opened);
    opened.keys("alt-up");
    let gains = |opened: &mut Opened<'_>| {
        [LONG, STEADY].map(|clip| audio_clip(opened, clip).unwrap().gain_db)
    };
    assert_eq!(gains(&mut opened), [1., 1.]);
    one_undo_step(&mut opened, "Change gain", &before);
    opened.keys("alt-down alt-down");
    assert_eq!(gains(&mut opened), [-1., -1.]);
}

#[gpui::test]
fn audio_clips_are_copied_pasted_duplicated_and_deleted(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let long = opened.at(BAR + BAR / 2, 1);
    opened.click(long);
    opened.keys("cmd-c");
    // A paste at the playhead, on the track of the selected clip, over the clips there.
    let ruler = opened.ruler(8 * BAR);
    opened.click(ruler);
    opened.settle();
    let before = mark(&mut opened);
    opened.keys("cmd-v");
    let pasted = opened.selected_clip().unwrap();
    assert_eq!(pasted, id("arrangement/voice/long-2"));
    let clip = audio_clip(&mut opened, pasted.as_str()).unwrap();
    assert_eq!((clip.start, clip.layer), (Ticks(8 * BAR), 2));
    one_undo_step(&mut opened, "Paste clip", &before);

    // A duplicate goes right after it: four seconds are two bars.
    let pasted = opened.at(8 * BAR + BAR / 2, 1);
    opened.click(pasted);
    let before = mark(&mut opened);
    opened.keys("cmd-d");
    let duplicate = opened.selected_clip().unwrap();
    let clip = audio_clip(&mut opened, duplicate.as_str()).unwrap();
    assert_eq!(clip.start, Ticks(10 * BAR));
    one_undo_step(&mut opened, "Duplicate clip", &before);

    let copy = opened.at(10 * BAR + BAR / 2, 1);
    opened.click(copy);
    let before = mark(&mut opened);
    opened.keys("backspace");
    assert!(audio_clip(&mut opened, duplicate.as_str()).is_none());
    one_undo_step(&mut opened, "Delete clip", &before);
}

#[gpui::test]
fn a_paste_onto_the_other_kind_of_track_is_refused(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // A note clip, copied, and pasted with the voice selected.
    let empty = opened.at(8 * BAR, 0);
    opened.double_click(empty);
    opened.keys("cmd-c");
    let header = opened.track_header(1);
    opened.click(header);
    let before = mark(&mut opened);
    opened.keys("cmd-v");
    assert_eq!(opened.undo_label(), before.undo_label);
    let notes = opened.project(|project| project.children::<sound_notes::Clip>(&id(VOICE)).count());
    assert_eq!(notes, 0);
    let notice = opened.notice().unwrap();
    assert!(
        notice.contains("a note clip goes on an instrument track"),
        "{notice}"
    );

    // An audio clip, pasted with the instrument track selected.
    let long = opened.at(BAR + BAR / 2, 1);
    opened.click(long);
    opened.keys("cmd-c");
    let header = opened.track_header(0);
    opened.click(header);
    opened.keys("cmd-v");
    assert_eq!(opened.undo_label(), before.undo_label);
    let audio = opened.project(|project| {
        project
            .children::<AudioClip>(&id("arrangement/track-1"))
            .count()
    });
    assert_eq!(audio, 0);
    let notice = opened.notice().unwrap();
    assert!(
        notice.contains("an audio clip goes on an audio track"),
        "{notice}"
    );
}

#[gpui::test]
fn an_outside_edit_of_an_audio_clip_shows_in_the_window(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    write_outside(
        &mut opened,
        "state/arrangement/voice/long.json",
        r#"{"tool": "arrangement.audio_clip", "state": {"asset": "long.wav", "start": 23040, "gain_db": -3.0, "layer": 1}}"#,
    );
    assert_eq!(opened.undo_label().as_deref(), Some("File change"));
    // Where the file puts it now is where a click finds it.
    let there = opened.at(23040 + BAR / 2, 1);
    opened.click(there);
    assert_eq!(opened.selected_clip(), Some(id(LONG)));
    let before = opened.at(BAR + BAR / 2, 1);
    opened.click(before);
    assert_eq!(opened.selected_clip(), None);
}

/// Where the start of the file of a clip is on the timeline, in seconds at 120 bpm: what a
/// trim of its start must not move.
fn sound_place(clip: &AudioClip) -> f64 {
    clip.start.0 as f64 / SECOND as f64 - clip.file_start_seconds
}

const ONE_FRAME: f64 = 1. / 48_000.;

// The Clip card.

fn clip_card(opened: &mut Opened<'_>) -> Option<Entity<ClipCard>> {
    let panel = opened.track_panel()?;
    opened.cx.read(|cx| panel.read(cx).clip_card().cloned())
}

#[gpui::test]
fn the_clip_card_shows_the_selected_clip_of_its_track_and_its_controls_edit_it(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    let header = opened.track_header(1);
    opened.click(header);
    let card = clip_card(&mut opened).unwrap();
    assert!(opened.cx.read(|cx| card.read(cx).clip().is_none()));
    assert!(opened.find("knob-gain").is_none());
    // The instrument track has none.
    let header = opened.track_header(0);
    opened.click(header);
    assert!(clip_card(&mut opened).is_none());

    // A double click on an audio clip opens the panel of its track, the card on the clip.
    let long = opened.at(BAR + BAR / 2, 1);
    opened.double_click(long);
    assert_eq!(opened.panel_track(), Some(id(VOICE)));
    let card = clip_card(&mut opened).unwrap();
    let shown = opened
        .cx
        .read(|cx| card.read(cx).clip().map(|clip| clip.id().clone()));
    assert_eq!(shown, Some(id(LONG)));

    // Gain from its knob, with the keys: a fiftieth of 72 dB.
    let before = mark(&mut opened);
    let knob = opened.control("knob-gain");
    opened.click(knob);
    opened.keys("up");
    assert_eq!(audio_clip(&mut opened, LONG).unwrap().gain_db, 1.44);
    one_undo_step(&mut opened, "Change gain", &before);
    // The fade in from its knob: a fiftieth of the four seconds the clip plays.
    let knob = opened.control("knob-fade-in");
    opened.click(knob);
    opened.keys("up");
    assert_eq!(audio_clip(&mut opened, LONG).unwrap().fade_in_ms, 80.);
    assert_eq!(opened.undo_label().as_deref(), Some("Change fade in"));

    // Start and End behind expand trim the clip, the sound staying in place.
    opened
        .cx
        .update(|_, cx| card.update(cx, |card, cx| card.set_expanded(true, cx)));
    let before = mark(&mut opened);
    let knob = opened.control("knob-start");
    opened.click(knob);
    opened.keys("up");
    // A fiftieth of four seconds is 0.08 s: the clip starts on the tick at or after that, and
    // the start in the file follows from the tick, so the sound is where it was.
    let long = audio_clip(&mut opened, LONG).unwrap();
    assert_eq!(long.start, Ticks(BAR + 154));
    assert!((sound_place(&long) - 2.0).abs() < ONE_FRAME, "{long:?}");
    one_undo_step(&mut opened, "Trim clip", &before);

    // The gain handle of the display, dragged up.
    let handle = opened.control("handle-gain");
    opened.drag(handle, handle + point(px(0.), px(-20.)));
    assert!(audio_clip(&mut opened, LONG).unwrap().gain_db > 1.44);
    assert_eq!(opened.undo_label().as_deref(), Some("Change gain"));

    // Another clip of the track selected: the card follows it.
    let steady = opened.at(SECOND / 2, 1);
    opened.click(steady);
    let shown = opened
        .cx
        .read(|cx| card.read(cx).clip().map(|clip| clip.id().clone()));
    assert_eq!(shown, Some(id(STEADY)));
}

/// A hundred moves of the start line of the display, a point at a time: the clip starts later
/// on the grid of ticks and the sound stays where it was within one frame, and the drag is one
/// step.
#[gpui::test]
fn a_long_drag_of_the_start_line_keeps_the_sound_in_place(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let long = opened.at(BAR + BAR / 2, 1);
    opened.double_click(long);
    let before = mark(&mut opened);
    let handle = opened.control("handle-start");
    opened.press(handle);
    let moves: Vec<_> = (1..=100)
        .map(|step| right(handle, step as f32 * 0.5))
        .collect();
    for position in &moves {
        opened.drag_to(*position);
    }
    let last = *moves.last().unwrap();
    opened.release(last);
    let long = audio_clip(&mut opened, LONG).unwrap();
    assert!(long.file_start_seconds > 0.5, "{long:?}");
    assert!((sound_place(&long) - 2.0).abs() < ONE_FRAME, "{long:?}");
    one_undo_step(&mut opened, "Trim clip", &before);
}

// Dropping files from the Finder.

/// Two files outside the project, of one and two seconds.
fn files() -> (tempfile::TempDir, Vec<PathBuf>) {
    let folder = tempfile::tempdir().unwrap();
    let first = folder.path().join("Strum 2.wav");
    let second = folder.path().join("shaker loop.wav");
    write_wav(&first, 1., 0.3);
    write_wav(&second, 2., 0.2);
    (folder, vec![first, second])
}

/// Files dragged over the window from the Finder, as the platform gives them to GPUI: in, a
/// few moves to `to`, and dropped there.
fn drop_files(opened: &mut Opened<'_>, paths: &[PathBuf], to: Point<Pixels>) {
    let paths = ExternalPaths(paths.iter().cloned().collect());
    let position = to - point(px(30.), px(0.));
    opened
        .cx
        .simulate_event(FileDropEvent::Entered { position, paths });
    for step in [20., 10., 0.] {
        // The screen draws between two moves of a drag, and GPUI gives what a drag carries
        // to the view it draws under it.
        opened.cx.update(|window, _| window.refresh());
        opened.cx.run_until_parked();
        let position = to - point(px(step), px(0.));
        opened
            .cx
            .simulate_event(FileDropEvent::Pending { position });
    }
    opened
        .cx
        .simulate_event(FileDropEvent::Submit { position: to });
    opened.cx.run_until_parked();
    opened.settle();
}

#[gpui::test]
fn files_dropped_on_an_audio_track_become_clips_one_after_another(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (_folder, paths) = files();
    let before = mark(&mut opened);
    // A little into bar 9 of the guitar: the first starts on the step under the pointer.
    let at = right(opened.at(8 * BAR, 2), 3.);
    drop_files(&mut opened, &paths, at);
    let first = audio_clip(&mut opened, "arrangement/guitar/strum-2").unwrap();
    let second = audio_clip(&mut opened, "arrangement/guitar/shaker-loop").unwrap();
    assert_eq!(first.asset.to_string(), "strum-2.wav");
    assert_eq!(first.start, Ticks(8 * BAR));
    assert_eq!(second.start, Ticks(8 * BAR + SECOND));
    assert!(second.layer > first.layer);
    // The files are in the project now, and what the clips play.
    for name in ["strum-2.wav", "shaker-loop.wav"] {
        assert!(opened.path(&format!("assets/audio/{name}")).exists());
    }
    assert_eq!(opened.selected_clips().len(), 2);
    assert_eq!(opened.project(|project| project.problems()), []);
    // One step, and undo leaves the files: assets are never undone.
    one_undo_step(&mut opened, "Add audio clips", &before);
    opened.keys("cmd-z");
    assert!(opened.path("assets/audio/strum-2.wav").exists());
}

#[gpui::test]
fn a_file_dropped_under_the_last_track_makes_an_audio_track_named_after_it(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    let (_folder, paths) = files();
    let before = mark(&mut opened);
    let at = opened.at(2 * BAR, 3);
    drop_files(&mut opened, &paths[..1], at);
    let track = id("arrangement/strum-2");
    let state = opened.project(|project| {
        let track = project.resolve::<TrackState>(&track)?;
        project.state(&track).cloned()
    });
    let state = state.unwrap();
    assert_eq!(
        (state.name.as_str(), state.kind),
        ("Strum 2", TrackKind::Audio)
    );
    assert!(audio_clip(&mut opened, "arrangement/strum-2/strum-2").is_some());
    one_undo_step(&mut opened, "Add audio clip", &before);
}

#[gpui::test]
fn files_dropped_over_an_instrument_track_go_nowhere(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (_folder, paths) = files();
    let at = opened.at(2 * BAR, 0);
    drop_files(&mut opened, &paths, at);
    assert_eq!(opened.undo_label(), None);
    assert!(!opened.path("assets/audio/strum-2.wav").exists());
}

/// The files go where they are let go of, also when the last move of the drag was elsewhere.
#[gpui::test]
fn a_drop_goes_where_the_files_are_let_go_of(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (_folder, paths) = files();
    let over_voice = opened.at(8 * BAR, 1);
    let over_guitar = opened.at(4 * BAR, 2);
    let paths = ExternalPaths(paths[..1].iter().cloned().collect());
    opened.cx.simulate_event(FileDropEvent::Entered {
        position: over_voice,
        paths,
    });
    opened.cx.update(|window, _| window.refresh());
    opened.cx.run_until_parked();
    opened.cx.simulate_event(FileDropEvent::Pending {
        position: over_voice,
    });
    opened.cx.simulate_event(FileDropEvent::Submit {
        position: over_guitar,
    });
    opened.cx.run_until_parked();
    let clip = audio_clip(&mut opened, "arrangement/guitar/strum-2").unwrap();
    assert_eq!(clip.start, Ticks(4 * BAR));
    assert!(audio_clip(&mut opened, "arrangement/voice/strum-2").is_none());
}

/// The drop handler on its own, with no platform drag at all.
#[gpui::test]
fn the_drop_handler_needs_no_display(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (_folder, paths) = files();
    let timeline = opened.timeline.clone();
    let target = DropTarget::Track(id(VOICE), Ticks(6 * BAR));
    opened
        .cx
        .update(|_, cx| timeline.update(cx, |timeline, cx| timeline.drop_files(paths, target, cx)));
    opened.cx.run_until_parked();
    let first = audio_clip(&mut opened, "arrangement/voice/strum-2").unwrap();
    let second = audio_clip(&mut opened, "arrangement/voice/shaker-loop").unwrap();
    assert_eq!(
        (first.start, second.start),
        (Ticks(6 * BAR), Ticks(6 * BAR + SECOND))
    );
    assert_eq!(opened.undo_label().as_deref(), Some("Add audio clips"));
}

#[gpui::test]
fn a_file_that_is_no_audio_is_left_out_and_said(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (folder, mut paths) = files();
    let text = folder.path().join("notes.wav");
    std::fs::write(&text, "not audio").unwrap();
    paths.insert(0, text);
    let at = opened.at(8 * BAR, 2);
    drop_files(&mut opened, &paths, at);
    assert!(audio_clip(&mut opened, "arrangement/guitar/strum-2").is_some());
    assert!(opened.notice().is_some());
    assert!(!opened.path("assets/audio/notes.wav").exists());
}

// Adding an audio track.

#[gpui::test]
fn the_project_menu_adds_an_audio_track(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    opened.keys("tab");
    opened.press_enter();
    opened.keys("down down");
    opened.press_enter();
    let tracks = opened.project(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        let tracks = arrangement::tracks(project, arrangement.id());
        tracks
            .iter()
            .map(|(_, state)| state.kind)
            .collect::<Vec<_>>()
    });
    assert_eq!(tracks.len(), 4);
    assert_eq!(tracks.last(), Some(&TrackKind::Audio));
    one_undo_step(&mut opened, "Add audio track", &before);
}
