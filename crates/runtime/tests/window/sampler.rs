//! The card of the Sampler in the track panel, with a simulated mouse and keys: it is picked
//! as the instrument of a track, a file dropped on it from the Finder or chosen in the file
//! panel becomes its sample as one undo step, every knob and handle is one undo step, the
//! green line follows the last note, and a missing file shows on the card.

use std::path::{Path, PathBuf};

use gpui::{Entity, ExternalPaths, FileDropEvent, Pixels, Point, TestAppContext, point, px};
use sampler::SamplerState;
use sampler::view::{LOAD_LABEL, SamplerView};
use sound_core::Changes;
use sound_media::AudioAsset;

use crate::support::{self, BAR, Opened, clip, id, note};

const SLOT: &str = "arrangement/track-1/instrument";
const PART: &str = "arrangement/track-1/part";

/// A mono float WAV of `seconds` of a sine of 440 Hz that dies away, as a plucked sound does.
fn write_wav(path: &Path, seconds: f64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for frame in 0..(seconds * 48_000.) as u64 {
        let time = frame as f64 / 48_000.;
        let sample = (std::f64::consts::TAU * 440. * time).sin() * (-2. * time).exp() * 0.5;
        writer.write_sample(sample as f32).unwrap();
    }
    writer.finalize().unwrap();
}

/// One track whose instrument is a Sampler of `sampler`, with a clip of one long note, and
/// its panel open. `file` is written into `assets/audio/` first, when there is one.
fn open_with<'a>(
    cx: &'a mut TestAppContext,
    sampler: SamplerState,
    file: Option<&'static str>,
) -> Opened<'a> {
    let mut opened = support::open_with(cx, |project| {
        if let Some(name) = file {
            write_wav(&project.root().join("assets/audio").join(name), 2.);
        }
        let mut changes = Changes::new();
        changes.delete(&id(SLOT));
        project.commit("Remove synth", changes).unwrap();
        let mut changes = Changes::new();
        changes.create(id(SLOT), sampler);
        changes.create(id(PART), clip(0, 4 * BAR, vec![note(0, 4 * BAR, 60)]));
        project.commit("Add sampler", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    opened
}

fn with_sample(name: &str) -> SamplerState {
    SamplerState {
        sample: Some(AudioAsset::new(name).unwrap()),
        ..SamplerState::default()
    }
}

fn state(opened: &mut Opened<'_>) -> SamplerState {
    opened.project(|project| {
        let sampler = project.resolve::<SamplerState>(&id(SLOT)).unwrap();
        project.state(&sampler).unwrap().clone()
    })
}

fn view(opened: &mut Opened<'_>) -> Entity<SamplerView> {
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.next().unwrap().cloned()
    });
    view.unwrap().downcast::<SamplerView>().ok().unwrap()
}

/// A file dragged from the Finder over `to` and let go of there.
fn drop_files(opened: &mut Opened<'_>, paths: &[PathBuf], to: Point<Pixels>) {
    let paths = ExternalPaths(paths.iter().cloned().collect());
    let position = to - point(px(30.), px(0.));
    opened
        .cx
        .simulate_event(FileDropEvent::Entered { position, paths });
    for step in [20., 10., 0.] {
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

/// Plays the note of the clip from the start for `frames`.
fn play(opened: &mut Opened<'_>, frames: usize) -> Vec<f32> {
    opened.settle();
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| {
            session.engine().seek(sound_core::Ticks(0));
            session.engine().play();
        });
    });
    opened.render(64);
    opened.render(frames)
}

#[gpui::test]
fn the_picker_puts_an_empty_sampler_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| project.clear_history());
    let header = opened.track_header(0);
    opened.click(header);
    let trigger = opened.control("instrument-picker");
    opened.click(trigger);
    let row = opened.control("menu-sampler");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Choose Sampler"));
    assert_eq!(state(&mut opened), SamplerState::default());
    assert_eq!(opened.project(|project| project.problems()), []);
    view(&mut opened);
    // The shown knobs, `Choose file` in the empty display, and none of the hidden knobs.
    for shown in [
        "knob-root",
        "knob-velocity_to_volume",
        "knob-release_seconds",
        "knob-gain_db",
        "choose-file",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    assert_eq!(opened.find("knob-start"), None);
    let expand = opened.control("card-instrument-expand");
    opened.click(expand);
    for hidden in [
        "knob-start",
        "knob-end",
        "knob-attack_seconds",
        "knob-decay_seconds",
        "knob-sustain",
    ] {
        assert!(opened.find(hidden).is_some(), "{hidden}");
    }
    // The card is 464 pt, and 649 expanded: its header is as wide, inside a border of 1 pt.
    let width =
        |opened: &mut Opened<'_>| opened.bounds("card-instrument-header").unwrap().size.width;
    assert_eq!(width(&mut opened), px(649. - 2.));
    // The icon moved with the right edge.
    let expand = opened.control("card-instrument-expand");
    opened.click(expand);
    assert_eq!(width(&mut opened), px(464. - 2.));
}

#[gpui::test]
fn a_file_dropped_on_the_card_is_copied_in_and_becomes_the_sample_in_one_undo_step(
    cx: &mut TestAppContext,
) {
    let mut opened = open_with(cx, SamplerState::default(), None);
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("My Kalimba.wav");
    write_wav(&source, 1.5);
    let silent = play(&mut opened, 4_800);
    assert_eq!(support::peak(&silent), 0.0);

    let target = opened.control("file-drop");
    drop_files(&mut opened, &[source], target);
    assert_eq!(
        state(&mut opened).sample,
        Some(AudioAsset::new("my-kalimba.wav").unwrap())
    );
    assert!(opened.path("assets/audio/my-kalimba.wav").exists());
    assert_eq!(opened.undo_label().as_deref(), Some(LOAD_LABEL));
    assert_eq!(opened.project(|project| project.problems()), []);
    // It plays, and the card shows it: its handles are there.
    assert!(support::peak(&play(&mut opened, 4_800)) > 0.1);
    assert!(opened.find("handle-attack").is_some());

    // A second file replaces it, from the start of the new file.
    let other = outside.path().join("bell.wav");
    write_wav(&other, 1.);
    let target = opened.control("file-drop");
    drop_files(&mut opened, &[other], target);
    assert_eq!(state(&mut opened).sample.unwrap().to_string(), "bell.wav");

    // A file that does not play is not copied, and the notice says why.
    let text = outside.path().join("notes.wav");
    std::fs::write(&text, "not audio").unwrap();
    let target = opened.control("file-drop");
    drop_files(&mut opened, &[text], target);
    assert_eq!(state(&mut opened).sample.unwrap().to_string(), "bell.wav");
    assert!(!opened.path("assets/audio/notes.wav").exists());
    let notice = opened.notice().unwrap();
    assert!(notice.contains("cannot be played"), "{notice}");

    // Undo, one step per file: an empty Sampler again. The copies stay, as copies of files
    // always do. The keys come last: after a key GPUI takes the pointer to hover nothing until
    // it moves, and a drag from the Finder does not move it (see the known gaps).
    opened.keys("cmd-z");
    assert_eq!(
        state(&mut opened).sample.unwrap().to_string(),
        "my-kalimba.wav"
    );
    opened.keys("cmd-z");
    assert_eq!(state(&mut opened), SamplerState::default());
    assert!(opened.path("assets/audio/my-kalimba.wav").exists());
    opened.keys("shift-cmd-z");
    assert_eq!(
        state(&mut opened).sample.unwrap().to_string(),
        "my-kalimba.wav"
    );
}

#[gpui::test]
fn choose_file_opens_the_file_panel_and_loads_what_is_chosen(cx: &mut TestAppContext) {
    let mut opened = open_with(cx, SamplerState::default(), None);
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("pad.wav");
    write_wav(&source, 1.);
    let button = opened.control("choose-file");
    opened.click(button);
    assert!(opened.cx.did_prompt_for_paths());
    opened.cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![source.clone()])
    });
    opened.cx.run_until_parked();
    opened.settle();
    assert_eq!(state(&mut opened).sample.unwrap().to_string(), "pad.wav");
    assert_eq!(opened.undo_label().as_deref(), Some(LOAD_LABEL));

    // Cancelled: nothing changes.
    opened.keys("cmd-z");
    let button = opened.control("choose-file");
    opened.click(button);
    opened.cx.simulate_path_prompt_response(|_| None);
    opened.cx.run_until_parked();
    assert_eq!(state(&mut opened), SamplerState::default());
}

/// Each handle of the display and each knob is one undo step under the name of its knob.
#[gpui::test]
fn every_handle_and_knob_is_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open_with(cx, with_sample("pluck.wav"), Some("pluck.wav"));
    let before = state(&mut opened);
    let drag = |opened: &mut Opened<'_>, handle: &str, by: Point<Pixels>, label: &str| {
        let from = opened.control(handle);
        opened.drag(from, from + by);
        assert_eq!(opened.undo_label().as_deref(), Some(label), "{handle}");
        assert!(!opened.gesture_open());
        state(opened)
    };
    // The file is 2 s over 312 pt: 31 pt is about 0.2 s.
    let after = drag(
        &mut opened,
        "handle-attack",
        point(px(31.), px(0.)),
        "Change attack",
    );
    assert!((after.attack_seconds - 0.2).abs() < 0.02, "{after:?}");
    let after = drag(
        &mut opened,
        "handle-decay",
        point(px(-20.), px(40.)),
        "Change decay and sustain",
    );
    assert!(after.decay_seconds < before.decay_seconds, "{after:?}");
    assert!(after.sustain < 0.8, "{after:?}");
    let after = drag(
        &mut opened,
        "handle-start",
        point(px(15.6), px(0.)),
        "Change start",
    );
    assert!((after.start_seconds - 0.1).abs() < 0.01, "{after:?}");
    let after = drag(
        &mut opened,
        "handle-end",
        point(px(-78.), px(0.)),
        "Change end",
    );
    let end = after.end_seconds.unwrap();
    assert!((end - 1.5).abs() < 0.01, "{after:?}");

    // The root moves in whole notes: its arrow keys step a semitone.
    let root = opened.control("knob-root");
    opened.click(root);
    opened.keys("up");
    assert_eq!(state(&mut opened).root.number(), 61);
    assert_eq!(opened.undo_label().as_deref(), Some("Change root"));
    opened.keys("down");
    opened.keys("down");
    assert_eq!(state(&mut opened).root.number(), 59);

    // Five steps back to where the card began, one per gesture.
    for _ in 0..7 {
        opened.keys("cmd-z");
    }
    assert_eq!(state(&mut opened), before);
}

/// The green line is where the last note is in the file, and it goes when no note sounds.
#[gpui::test]
fn the_green_line_follows_the_last_note(cx: &mut TestAppContext) {
    let mut opened = open_with(cx, with_sample("pluck.wav"), Some("pluck.wav"));
    let sampler = view(&mut opened);
    assert_eq!(opened.cx.read(|cx| sampler.read(cx).playing_at()), None);
    // Half a second of the note, at the root: half a second into the file.
    play(&mut opened, 24_000);
    opened.settle();
    let at = opened.cx.read(|cx| sampler.read(cx).playing_at()).unwrap();
    println!("after 0.5 s of a note at the root, the green line is at {at:.4} s");
    // A column of the display is 2 s / 312 pt.
    assert!((at - 0.5).abs() < 2. / 312., "{at}");
    // Past the end of the 2 s file the note has ended, and the line is gone.
    opened.render(96_000);
    opened.settle();
    opened.settle();
    assert_eq!(opened.cx.read(|cx| sampler.read(cx).playing_at()), None);
}

#[gpui::test]
fn a_missing_file_shows_on_the_card_and_a_drop_brings_one(cx: &mut TestAppContext) {
    // Trimmed, so the drop shows it keeps the trims of the record.
    let trimmed = SamplerState {
        start_seconds: 0.5,
        end_seconds: Some(0.9),
        ..with_sample("gone.wav")
    };
    let mut opened = open_with(cx, trimmed.clone(), None);
    let problems = opened.project(|project| project.problems());
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0]
            .message
            .starts_with("the sample assets/audio/gone.wav is not there")
    );
    // No waveform and no handles: the line that says so and `Choose file`.
    assert!(opened.find("choose-file").is_some());
    assert_eq!(opened.find("handle-attack"), None);

    // A file of that name: it loads with the record as it was, no edit and no undo step.
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("gone.wav");
    write_wav(&source, 1.);
    let target = opened.control("file-drop");
    drop_files(&mut opened, &[source], target);
    assert_eq!(opened.project(|project| project.problems()), []);
    assert_eq!(state(&mut opened), trimmed);
    assert_eq!(opened.undo_label(), None);
    assert!(opened.find("handle-attack").is_some());
    // And it plays: from half a second into the file.
    assert!(support::peak(&play(&mut opened, 4_800)) > 0.05);
}
