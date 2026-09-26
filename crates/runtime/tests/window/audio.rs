//! A project with an audio track in the window. Audio clips are not drawn yet (step 1b); what
//! is checked here is that the window keeps working around them: the panel of an audio track
//! has no instrument, a double click adds no note clip to it, a keyboard does not play into
//! it, and the audio still plays.

use arrangement::{AudioClip, Colour, TrackState};
use gpui::TestAppContext;
use sound_core::{Changes, Project, Ticks};
use sound_media::AudioAsset;

use crate::support::{self, Opened, id};

const VOICE: &str = "arrangement/voice";

/// The default project and an audio track `voice` with a clip of a steady level on bar 1.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project: &mut Project| {
        let path = project.root().join("assets/audio/steady.wav");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..48_000 {
            writer.write_sample(16_384_i16).unwrap();
        }
        writer.finalize().unwrap();

        let arrangement = runtime::main_arrangement(project).unwrap();
        let mut changes = Changes::new();
        let track = arrangement::add_audio_track(
            project,
            &mut changes,
            arrangement.id(),
            "Voice",
            Colour::Peach,
        )
        .unwrap();
        let clip = AudioClip::new(AudioAsset::new("steady.wav").unwrap(), Ticks(0));
        changes.create(track.id().child("steady").unwrap(), clip);
        project.commit("Add audio", changes).unwrap();
        project.clear_history();
        assert_eq!(project.problems(), []);
    })
}

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
    let empty = opened.at(8 * 960, 1);
    opened.double_click(empty);
    let children =
        opened.project(|project| project.children::<sound_notes::Clip>(&id(VOICE)).count());
    assert_eq!(children, 0);
    assert_eq!(opened.undo_label(), None);
    // On the instrument track it still does.
    let empty = opened.at(8 * 960, 0);
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
