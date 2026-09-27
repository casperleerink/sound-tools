//! The card of the Drum pad in the track panel, with a simulated mouse and keys: picked as the
//! instrument of a track, a press on a pad selects it and plays it, the grid is one tab stop
//! whose arrows move the selection, every edit is one undo step, and a file dropped on a pad or
//! chosen in the file panel makes it a sample pad.

use std::path::{Path, PathBuf};

use drum_pad::view::DrumPadView;
use drum_pad::{DrumPadState, Sound, Source};
use gpui::{ExternalPaths, FileDropEvent, Pixels, Point, TestAppContext, point, px};

use crate::support::{self, Opened, id, mark, one_undo_step};

const SLOT: &str = "arrangement/track-1/instrument";
const SLOT_FILE: &str = "state/arrangement/track-1/instrument.json";

/// The default project with the panel of its track open, and the Drum pad picked as its
/// instrument the way a composer picks it.
fn open_drums(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| project.clear_history());
    let header = opened.track_header(0);
    opened.click(header);
    let picker = opened.control("instrument-picker");
    opened.click(picker);
    let row = opened.control("menu-drum-pad");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Choose Drum pad"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> DrumPadState {
    opened.project(|project| {
        let drums = project.resolve::<DrumPadState>(&id(SLOT)).unwrap();
        project.state(&drums).unwrap().clone()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(SLOT_FILE)).unwrap()
}

fn view(opened: &mut Opened<'_>) -> gpui::Entity<DrumPadView> {
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.next().unwrap().cloned()
    });
    view.unwrap().downcast::<DrumPadView>().ok().unwrap()
}

fn selected(opened: &mut Opened<'_>) -> usize {
    let view = view(opened);
    opened.cx.read(|cx| view.read(cx).selected())
}

fn pad(opened: &mut Opened<'_>, note: u8) -> Point<Pixels> {
    opened.control(&format!("pad-{note}"))
}

/// What the Drum pad plays over the next quarter second, with nothing on the timeline.
fn heard(opened: &mut Opened<'_>) -> f32 {
    support::peak(&opened.render(12_000))
}

#[gpui::test]
fn the_drum_pad_is_picked_as_an_instrument_and_shows_its_pads_and_knobs(cx: &mut TestAppContext) {
    let mut opened = open_drums(cx);
    assert_eq!(state(&mut opened), DrumPadState::default());
    assert_eq!(
        file(&mut opened),
        "{\n  \"tool\": \"drum-pad\",\n  \"state\": {\"pads\": {}}\n}\n"
    );
    for note in 36..=51 {
        assert!(opened.find(&format!("pad-{note}")).is_some(), "{note}");
    }
    // The pads run from the bottom left, row by row.
    let (kick, rim, snare_2, ride) = (
        pad(&mut opened, 36),
        pad(&mut opened, 37),
        pad(&mut opened, 40),
        pad(&mut opened, 51),
    );
    assert_eq!(rim.x - kick.x, px(76.));
    assert_eq!(kick.y - snare_2.y, px(36.));
    assert!(ride.x > kick.x && ride.y < kick.y);
    let grid = opened.bounds("pad-36").unwrap();
    assert_eq!((grid.size.width, grid.size.height), (px(72.), px(32.)));
    for shown in [
        "knob-volume_db",
        "knob-pitch_semitones",
        "knob-decay_ms",
        "knob-pan",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    assert_eq!(opened.find("sound"), None);
    // The card is 452 pt, and 581 pt expanded, with Sound and Choke. Its header is inside
    // its border of 1 pt.
    let card = opened.bounds("card-instrument-header").unwrap();
    assert_eq!(card.size.width, px(452. - 2.));
    let expand = opened.control("card-instrument-expand");
    opened.click(expand);
    assert!(opened.find("sound").is_some());
    assert!(opened.find("toggle-choke").is_some());
    let card = opened.bounds("card-instrument-header").unwrap();
    assert_eq!(card.size.width, px(581. - 2.));
    assert_eq!(opened.undo_label().as_deref(), Some("Choose Drum pad"));
}

#[gpui::test]
fn a_press_on_a_pad_selects_it_and_plays_it_and_is_no_edit(cx: &mut TestAppContext) {
    let mut opened = open_drums(cx);
    opened.settle();
    assert_eq!(heard(&mut opened), 0.0);
    let hat = pad(&mut opened, 42);
    opened.click(hat);
    assert_eq!(selected(&mut opened), 6);
    let played = heard(&mut opened);
    println!("the hat, pressed: peak {played:.3}");
    assert!(played > 0.05, "{played}");
    // The knobs are the hat's.
    let decay = opened.bounds("knob-decay_ms").unwrap();
    assert!(decay.size.width > px(0.));
    assert_eq!(opened.undo_label().as_deref(), Some("Choose Drum pad"));
    // The pad is green while it sounds, and not once it has rung out.
    let hit = pad(&mut opened, 36);
    opened.click(hit);
    opened.render(1_024);
    opened.settle();
    let view = view(&mut opened);
    let sounding = opened.cx.read(|cx| view.read(cx).sounding());
    assert!(sounding[0] > 0.3, "{sounding:?}");
    opened.render(48_000);
    opened.settle();
    opened.settle();
    let sounding = opened.cx.read(|cx| view.read(cx).sounding());
    assert_eq!(sounding, [0.0; 16]);
}

#[gpui::test]
fn the_grid_is_one_tab_stop_whose_arrows_move_the_selection_and_enter_plays(
    cx: &mut TestAppContext,
) {
    let mut opened = open_drums(cx);
    let kick = pad(&mut opened, 36);
    opened.click(kick);
    opened.settle();
    opened.render(48_000);
    assert_eq!(selected(&mut opened), 0);
    opened.keys("right");
    assert_eq!(selected(&mut opened), 1);
    opened.keys("up");
    opened.keys("up");
    assert_eq!(selected(&mut opened), 9);
    // At an edge the selection stays.
    opened.keys("left");
    opened.keys("left");
    assert_eq!(selected(&mut opened), 8);
    opened.keys("down");
    opened.keys("down");
    opened.keys("down");
    assert_eq!(selected(&mut opened), 0);
    opened.keys("right");
    opened.keys("right");
    opened.keys("right");
    opened.keys("right");
    assert_eq!(selected(&mut opened), 3);
    // Enter plays the selected pad, the clap.
    opened.settle();
    assert_eq!(heard(&mut opened), 0.0);
    opened.keys("enter");
    assert!(heard(&mut opened) > 0.05);
    // Tab leaves the grid for the next stop, and shift-tab comes back to the grid, not to a
    // pad: the arrows move the selection again.
    opened.keys("tab");
    opened.keys("shift-tab");
    opened.keys("left");
    assert_eq!(selected(&mut opened), 2);
    assert_eq!(opened.undo_label().as_deref(), Some("Choose Drum pad"));
}

#[gpui::test]
fn a_knob_drag_changes_the_selected_pad_as_one_undo_step_written_once(cx: &mut TestAppContext) {
    let mut opened = open_drums(cx);
    let hat = pad(&mut opened, 42);
    opened.click(hat);
    let before = mark(&mut opened);
    let knob = opened.control("knob-decay_ms");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    let moving = state(&mut opened).pads[6].decay_ms;
    assert!(moving > 180.0);
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    opened.release(point(knob.x, knob.y - px(40.)));
    let after = state(&mut opened).pads[6].decay_ms;
    assert!(after > moving);
    // Only the hat changed, and the file says so.
    let mut expected = DrumPadState::default();
    expected.pads[6].decay_ms = after;
    assert_eq!(state(&mut opened), expected);
    assert!(file(&mut opened).contains("\"42\": {"));
    one_undo_step(&mut opened, "Change decay", &before);
}

#[gpui::test]
fn the_sound_list_and_the_choke_toggle_change_the_selected_pad(cx: &mut TestAppContext) {
    let mut opened = open_drums(cx);
    let tom = pad(&mut opened, 45);
    opened.click(tom);
    let expand = opened.control("card-instrument-expand");
    opened.click(expand);
    let before = mark(&mut opened);
    let select = opened.control("sound");
    opened.click(select);
    for sound in Sound::ALL {
        assert!(
            opened.find(&format!("menu-{}", sound.key())).is_some(),
            "{sound:?}"
        );
    }
    assert!(opened.find("menu-choose").is_some());
    // No file yet, so no row of one.
    assert_eq!(opened.find("menu-sample"), None);
    let clap = opened.control("menu-clap");
    opened.click(clap);
    assert_eq!(
        state(&mut opened).pads[9].source,
        Source::Sound(Sound::Clap)
    );
    one_undo_step(&mut opened, "Change sound", &before);
    opened.edit(|project| project.undo().map(|_| ()));

    // The choke: the open hat leaves the group.
    let open_hat = pad(&mut opened, 46);
    opened.click(open_hat);
    let before = mark(&mut opened);
    let choke = opened.control("toggle-choke");
    opened.click(choke);
    assert!(!state(&mut opened).pads[10].choke);
    one_undo_step(&mut opened, "Take out of choke group", &before);
}

/// A second of a quiet sine, as a file from the Finder.
fn sample_file(folder: &Path, name: &str) -> PathBuf {
    let path = folder.join(name);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..44_100 {
        let sample = 0.3 * (std::f64::consts::TAU * 440.0 * frame as f64 / 44_100.0).sin();
        writer.write_sample((sample * 32_767.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
    path
}

#[gpui::test]
fn a_file_dropped_on_a_pad_makes_it_a_sample_pad_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open_drums(cx);
    let outside = tempfile::tempdir().unwrap();
    let path = sample_file(outside.path(), "Tom Deep.wav");
    let before = mark(&mut opened);
    let target = pad(&mut opened, 50);
    let paths = ExternalPaths(vec![path].into());
    opened.cx.simulate_event(FileDropEvent::Entered {
        position: target - point(px(20.), px(0.)),
        paths,
    });
    for step in [10., 0.] {
        opened.cx.update(|window, _| window.refresh());
        opened.cx.run_until_parked();
        let position = target - point(px(step), px(0.));
        opened
            .cx
            .simulate_event(FileDropEvent::Pending { position });
    }
    opened
        .cx
        .simulate_event(FileDropEvent::Submit { position: target });
    opened.cx.run_until_parked();
    opened.settle();

    assert!(opened.path("assets/audio/tom-deep.wav").exists());
    let pad = state(&mut opened).pads[14].clone();
    assert_eq!(
        pad.source,
        Source::Sample(sound_media::AudioAsset::new("tom-deep.wav").unwrap())
    );
    // At its own pitch, for one and a half times its second.
    assert_eq!((pad.pitch_semitones, pad.decay_ms), (0.0, 1500.0));
    assert_eq!(selected(&mut opened), 14);
    assert_eq!(opened.project(|project| project.problems()), []);
    one_undo_step(&mut opened, "Load sample", &before);

    // It plays the file at its level, 0.3, at the pan of Tom 6 (the left gain 1.176) and the
    // velocity of a press, 100 (0.620): 0.219.
    let tom = self::pad(&mut opened, 50);
    opened.click(tom);
    let played = heard(&mut opened);
    println!("the sample pad, pressed: peak {played:.3}");
    assert!((played - 0.219).abs() < 0.002, "{played}");

    // The list of its sound has the file now.
    let expand = opened.control("card-instrument-expand");
    opened.click(expand);
    let select = opened.control("sound");
    opened.click(select);
    assert!(opened.find("menu-sample").is_some());
}

#[gpui::test]
fn choose_file_in_the_sound_list_opens_the_file_panel_and_loads_the_file(cx: &mut TestAppContext) {
    let mut opened = open_drums(cx);
    let outside = tempfile::tempdir().unwrap();
    let path = sample_file(outside.path(), "Shaker.wav");
    let kick = pad(&mut opened, 36);
    opened.click(kick);
    let expand = opened.control("card-instrument-expand");
    opened.click(expand);
    let before = mark(&mut opened);
    let select = opened.control("sound");
    opened.click(select);
    let choose = opened.control("menu-choose");
    opened.click(choose);
    assert!(opened.cx.did_prompt_for_paths());
    opened
        .cx
        .simulate_path_prompt_response(move |_| Some(vec![path]));
    opened.cx.run_until_parked();
    opened.settle();
    assert_eq!(
        state(&mut opened).pads[0].source,
        Source::Sample(sound_media::AudioAsset::new("shaker.wav").unwrap())
    );
    one_undo_step(&mut opened, "Load sample", &before);

    // Cancelled, the panel loads nothing.
    let select = opened.control("sound");
    opened.click(select);
    let choose = opened.control("menu-choose");
    opened.click(choose);
    opened.cx.simulate_path_prompt_response(|_| None);
    opened.cx.run_until_parked();
    one_undo_step(&mut opened, "Load sample", &before);
}
