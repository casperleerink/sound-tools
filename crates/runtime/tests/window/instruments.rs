//! Picking the instrument of a track in the track panel, and the plugin's own window.
//!
//! The plugin is the repository's own test instrument, found in a folder inside the project,
//! so no test needs a plugin of this machine. Its left channel is a cosine per key and its
//! right channel is the sustain pedal as a number, so a render says which instrument played.

use gpui::TestAppContext;
use sound_core::Changes;

use crate::support::{self, BAR, Opened, clip, id, note, test_plugin_record};

const TRACK: &str = "arrangement/track-1";
const SLOT: &str = "arrangement/track-1/instrument";
const SLOT_FILE: &str = "state/arrangement/track-1/instrument.json";
const PART: &str = "arrangement/track-1/part";
const PICKER: &str = "instrument-picker";
const SYNTH_ITEM: &str = "menu-instrument.synth";
/// The key of the test plugin in the picker: the format and the plugin's own id.
const PLUGIN_ITEM: &str = "menu-CLAP:sound-tools.test-tone";
const PLUGIN_NAME: &str = "Sound Tools Test Tone";

/// One track with the default synth and a clip of one long note, and the panel of that track
/// open. Whatever is in the slot plays that note, so a render says what is there.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with_test_plugin(cx, |project| {
        let mut changes = Changes::new();
        changes.create(id(PART), clip(0, 4 * BAR, vec![note(0, 4 * BAR, 60)]));
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    assert_eq!(opened.panel_track(), Some(id(TRACK)));
    opened
}

fn card_names(opened: &mut Opened<'_>) -> Vec<String> {
    let panel = opened.track_panel().unwrap();
    opened.cx.read(|cx| {
        let names = panel.read(cx).device_names(cx);
        names.iter().map(ToString::to_string).collect()
    })
}

fn slot_file(opened: &mut Opened<'_>) -> Option<String> {
    std::fs::read_to_string(opened.path(SLOT_FILE)).ok()
}

/// Opens the picker of the one card of the rack.
fn open_picker(opened: &mut Opened<'_>) {
    let trigger = opened.control(PICKER);
    opened.click(trigger);
}

fn pick(opened: &mut Opened<'_>, item: &str) {
    open_picker(opened);
    let row = opened.control(item);
    opened.click(row);
}

/// Plays the note of the clip and renders past the start, so there is a sound to compare.
fn playing(opened: &mut Opened<'_>) -> Vec<f32> {
    opened.settle();
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| {
            session.engine().seek(sound_core::Ticks(0));
            session.engine().play();
        });
    });
    opened.settle();
    opened.render(24_000)
}

fn window_is_open(opened: &mut Opened<'_>) -> bool {
    let plugins = opened.plugins.upgrade().unwrap();
    plugins.window_is_open(&id(SLOT))
}

#[gpui::test]
fn the_picker_offers_the_synth_and_every_clap_instrument_the_scan_found(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(card_names(&mut opened), ["Synth"]);
    // Nothing is open, so no row is on screen.
    assert_eq!(opened.find(PLUGIN_ITEM), None);

    open_picker(&mut opened);
    assert!(opened.find(SYNTH_ITEM).is_some());
    assert!(opened.find(PLUGIN_ITEM).is_some());
    // Looking is no edit.
    assert_eq!(opened.undo_label(), None);
    // Escape closes the menu and leaves the panel open.
    opened.keys("escape");
    assert_eq!(opened.find(PLUGIN_ITEM), None);
    assert!(opened.track_panel().is_some());
}

#[gpui::test]
fn picking_a_plugin_writes_the_record_as_one_undo_step_and_undo_gives_the_synth_back(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let with_synth = playing(&mut opened);
    assert!(support::peak(&with_synth) > 0.0);

    pick(&mut opened, PLUGIN_ITEM);
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    let record = slot_file(&mut opened).unwrap();
    assert!(record.contains(r#""tool": "plugin""#), "{record}");
    assert!(record.contains(test_clap_plugin::PLUGIN_ID), "{record}");
    // A state file of its own, from the plugin's name, that no other record uses.
    assert!(
        record.contains(r#""state_asset": "sound-tools-test-tone""#),
        "{record}"
    );
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Choose Sound Tools Test Tone")
    );
    assert_eq!(opened.project(|project| project.problems().len()), 0);

    // The plugin plays, and it does not sound like the synth.
    let with_plugin = playing(&mut opened);
    assert!(support::peak(&with_plugin) > 0.0);
    assert_ne!(with_plugin, with_synth);

    // One step back: the synth record is what it was, and so is the sound.
    opened.keys("cmd-z");
    assert_eq!(card_names(&mut opened), ["Synth"]);
    assert_eq!(opened.undo_label(), None);
    let record = slot_file(&mut opened).unwrap();
    assert!(record.contains(r#""tool": "instrument.synth""#), "{record}");
    let again = playing(&mut opened);
    assert_eq!(again, with_synth);

    // And forward again.
    opened.keys("shift-cmd-z");
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    let again = playing(&mut opened);
    assert_eq!(again, with_plugin);
}

#[gpui::test]
fn picking_the_synth_again_after_a_plugin_brings_its_controls_back(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    pick(&mut opened, PLUGIN_ITEM);
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    // The plugin has a card without knobs, so the cutoff knob of the synth is gone.
    assert_eq!(opened.find("knob-cutoff_hz"), None);

    pick(&mut opened, SYNTH_ITEM);
    assert_eq!(card_names(&mut opened), ["Synth"]);
    assert_eq!(opened.undo_label().as_deref(), Some("Choose Synth"));
    let knob = opened.control("knob-cutoff_hz");
    opened.click(knob);
    opened.keys("up");
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    // The plugin on another track, and picking it again there: the second pick changes
    // nothing, so the plugin keeps the state file it has and the composer keeps their sound.
    opened.edit(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement)
    });
    let second_track = "state/arrangement/track-2/instrument.json";
    let header = opened.track_header(1);
    opened.click(header);
    pick(&mut opened, PLUGIN_ITEM);
    let written = std::fs::read_to_string(opened.path(second_track)).unwrap();
    assert!(
        written.contains(r#""state_asset": "sound-tools-test-tone""#),
        "{written}"
    );
    let label = opened.undo_label();
    pick(&mut opened, PLUGIN_ITEM);
    assert_eq!(
        std::fs::read_to_string(opened.path(second_track)).unwrap(),
        written
    );
    assert_eq!(opened.undo_label(), label);

    // The same plugin on the first track as well: the name of the second one is taken.
    let header = opened.track_header(0);
    opened.click(header);
    pick(&mut opened, PLUGIN_ITEM);
    let first = slot_file(&mut opened).unwrap();
    assert!(
        first.contains(r#""state_asset": "sound-tools-test-tone-2""#),
        "{first}"
    );
}

#[gpui::test]
fn picking_what_is_already_there_changes_nothing(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let knob = opened.control("knob-cutoff_hz");
    opened.click(knob);
    opened.keys("up");
    let (sound, label) = (slot_file(&mut opened), opened.undo_label());
    assert_eq!(label.as_deref(), Some("Change cutoff"));

    // The menu marks what is in the slot, and picking it writes nothing: a fresh record would
    // throw the sound away, and for a plugin it would make an empty state file.
    open_picker(&mut opened);
    let row = opened.control(SYNTH_ITEM);
    opened.click(row);
    assert_eq!(slot_file(&mut opened), sound);
    assert_eq!(opened.undo_label(), label);
    assert_eq!(card_names(&mut opened), ["Synth"]);
}

#[gpui::test]
fn a_plugin_this_machine_does_not_have_shows_its_id_and_the_rest_of_the_panel_works(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let missing = r#"{"tool": "plugin", "state": {"format": "clap", "plugin_id": "com.example.nowhere", "state_asset": "piano"}}"#;
    let path = opened.path(SLOT_FILE);
    std::fs::write(&path, missing).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));

    // The card is named by the only thing left of the plugin: the id the record names.
    assert_eq!(card_names(&mut opened), ["com.example.nowhere"]);
    let problems = opened.project(|project| {
        let problems = project.problems();
        problems
            .iter()
            .map(|problem| problem.message.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("com.example.nowhere"), "{problems:?}");
    // The record is left exactly as it was written.
    assert_eq!(slot_file(&mut opened).as_deref(), Some(missing));

    // The rest of the panel works: the mixer of the track edits the track record.
    let mute = opened.control("mute-track");
    opened.click(mute);
    assert_eq!(opened.undo_label().as_deref(), Some("Mute track"));
    // And the picker is the way out: pick something this machine has.
    pick(&mut opened, PLUGIN_ITEM);
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    assert_eq!(opened.project(|project| project.problems().len()), 0);
}

#[gpui::test]
fn the_plugins_window_opens_from_its_card_and_goes_when_another_instrument_is_picked(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    pick(&mut opened, PLUGIN_ITEM);
    assert!(!window_is_open(&mut opened));

    let button = opened.control("plugin-window");
    opened.click(button);
    assert!(window_is_open(&mut opened));
    // The same control closes it, and neither is an undo step.
    let button = opened.control("plugin-window");
    opened.click(button);
    assert!(!window_is_open(&mut opened));
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Choose Sound Tools Test Tone")
    );

    // Open again, then pick the synth: the window of the plugin that goes goes with it.
    let button = opened.control("plugin-window");
    opened.click(button);
    assert!(window_is_open(&mut opened));
    pick(&mut opened, SYNTH_ITEM);
    // The host lets go of a plugin no record names at its next poll, which the runtime does
    // every 16 ms, and the window of that plugin goes with it.
    opened.poll_plugins();
    assert!(!window_is_open(&mut opened));
    assert_eq!(opened.notice(), None);
}

#[gpui::test]
fn deleting_the_track_from_outside_closes_the_plugins_window_and_undo_brings_it_back_silent(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    pick(&mut opened, PLUGIN_ITEM);
    let button = opened.control("plugin-window");
    opened.click(button);
    assert!(window_is_open(&mut opened));

    let folder = opened.path("state/arrangement/track-1");
    std::fs::remove_dir_all(&folder).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[folder]));
    opened.poll_plugins();
    assert!(!window_is_open(&mut opened));
    assert!(opened.track_panel().is_none());
    assert_eq!(opened.notice(), None);
    assert_eq!(opened.project(|project| project.problems().len()), 0);

    // Undo brings the track and its plugin back, with no window: opening one is not an edit
    // and is never undone.
    opened.keys("cmd-z");
    let header = opened.track_header(0);
    opened.click(header);
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    assert!(!window_is_open(&mut opened));
    assert!(support::peak(&playing(&mut opened)) > 0.0);
}

#[gpui::test]
fn a_plugin_written_by_hand_gets_the_same_card_and_the_track_can_go_back_to_the_synth(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let path = opened.path(SLOT_FILE);
    std::fs::write(&path, test_plugin_record("piano")).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    assert!(opened.find("plugin-window").is_some());

    // Back to the synth from the window. The file edit and this are two steps.
    pick(&mut opened, SYNTH_ITEM);
    assert_eq!(card_names(&mut opened), ["Synth"]);
    opened.keys("cmd-z");
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    let record = slot_file(&mut opened).unwrap();
    assert!(record.contains(r#""state_asset": "piano""#), "{record}");
}
