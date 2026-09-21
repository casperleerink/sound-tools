//! Adding and removing effects in the track rack, with a simulated mouse.
//!
//! The effect is the repository's own test plugin, which is an instrument and an effect in one,
//! so it is offered in both pickers. In an effect slot it gets no notes: what comes out of it
//! is what it is played times a half, plus the offset it has learned and saved.

use arrangement::view::track_panel::remove_control;
use gpui::TestAppContext;
use plugin_host::{PluginFormat, PluginRecord};

use crate::support::{self, BAR, Opened, clip, id, note, test_plugin_id};

const TRACK: &str = "arrangement/track-1";
const TRACK_FILE: &str = "state/arrangement/track-1/instance.json";
const PART: &str = "arrangement/track-1/part";
const ADD_EFFECT: &str = "add-effect";
const PLUGIN_NAME: &str = "Sound Tools Test Tone";

/// The row of the test plugin in a picker, by the key that tells one offer from another.
fn plugin_item() -> String {
    format!(
        "menu-{}",
        PluginRecord::offer_key(PluginFormat::Clap, test_plugin_id(PluginFormat::Clap))
    )
}

/// One track whose instrument is the test plugin, with a clip of one note at full velocity,
/// and its panel open. Full velocity so that an effect hears 1.0 and learns an offset of its
/// own, which is how a test makes an effect change its own state.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with_test_plugin(cx, |project| {
        let mut changes = sound_core::Changes::new();
        let note = note(0, 4 * BAR, 60);
        changes.create(id(PART), clip(0, 4 * BAR, vec![note]));
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    let path = opened.path("state/arrangement/track-1/part.json");
    let loud = format!(
        r#"{{"tool": "arrangement.clip", "state": {{"start": 0, "length": {}, "notes": [{{"start": 0, "length": {}, "pitch": 60, "velocity": 127}}]}}}}"#,
        4 * BAR,
        4 * BAR
    );
    std::fs::write(&path, loud).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    // The instrument first, so the card of the rack is the plugin and the sound is its cosine.
    let trigger = opened.control("instrument-picker");
    opened.click(trigger);
    let row = opened.control(&plugin_item());
    opened.click(row);
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn card_names(opened: &mut Opened<'_>) -> Vec<String> {
    let panel = opened.track_panel().unwrap();
    opened.cx.read(|cx| {
        let names = panel.read(cx).device_names(cx);
        names.iter().map(ToString::to_string).collect()
    })
}

fn track_file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(TRACK_FILE)).unwrap()
}

/// Plays the note of the clip from the start and renders, as `instruments.rs` does.
fn playing(opened: &mut Opened<'_>) -> Vec<f32> {
    opened.settle();
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| {
            session.engine().stop();
            session.engine().seek(sound_core::Ticks(0));
            session.engine().play();
        });
    });
    opened.settle();
    opened.render(12_000)
}

/// The sound the track has settled on: an effect learns its offset from the first loud block,
/// so the second render is what it plays from then on.
fn settled(opened: &mut Opened<'_>) -> Vec<f32> {
    playing(opened);
    playing(opened)
}

fn add_effect(opened: &mut Opened<'_>) {
    let trigger = opened.control(ADD_EFFECT);
    opened.click(trigger);
    let row = opened.control(&plugin_item());
    opened.click(row);
}

#[gpui::test]
fn the_add_control_offers_every_effect_and_looking_is_no_edit(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    assert_eq!(opened.find(&plugin_item()), None);

    let trigger = opened.control(ADD_EFFECT);
    opened.click(trigger);
    assert!(opened.find(&plugin_item()).is_some());
    let label = opened.undo_label();
    // Escape closes the menu and leaves the panel and the track record alone.
    opened.keys("escape");
    assert_eq!(opened.find(&plugin_item()), None);
    assert!(opened.track_panel().is_some());
    assert_eq!(opened.undo_label(), label);
    assert!(!track_file(&mut opened).contains("effects"));
}

#[gpui::test]
fn adding_an_effect_is_one_undo_step_and_the_track_plays_through_it(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let plain = settled(&mut opened);
    assert!(support::peak(&plain) > 0.0);

    add_effect(&mut opened);
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME, PLUGIN_NAME]);
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Add Sound Tools Test Tone")
    );
    opened.project(|project| assert_eq!(project.problems(), []));

    // One record for the effect, and the track record that names it in its chain.
    let record = track_file(&mut opened);
    assert!(
        record.contains(r#""effects": ["sound-tools-test-tone"]"#),
        "{record}"
    );
    let effect = opened.path("state/arrangement/track-1/sound-tools-test-tone.json");
    let effect = std::fs::read_to_string(effect).unwrap();
    assert!(effect.contains(r#""tool": "plugin""#), "{effect}");

    // It is in the path of the sound, and the sound is not what it was.
    let through = settled(&mut opened);
    assert!(support::peak(&through) > 0.0);
    assert_ne!(through, plain);

    // One step back takes the record and the name in the list together.
    opened.keys("cmd-z");
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    // What is left is the step before: picking the instrument of the track.
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Choose Sound Tools Test Tone")
    );
    assert!(!track_file(&mut opened).contains("effects"));
    assert_eq!(settled(&mut opened), plain);
}

#[gpui::test]
fn removing_an_effect_is_one_undo_step_and_undo_brings_it_back_as_it_sounded(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let plain = settled(&mut opened);
    add_effect(&mut opened);
    let through = settled(&mut opened);
    assert_ne!(through, plain);
    // The effect learned an offset from the loud note and the host saved it, which is what
    // undo has to bring back.
    opened.poll_plugins();
    let asset = opened.path("assets/plugin-state/sound-tools-test-tone-2.bin");
    assert!(asset.exists(), "the effect saved no state");

    let remove = opened.control(&remove_control(&id(&format!(
        "{TRACK}/sound-tools-test-tone"
    ))));
    opened.click(remove);
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME]);
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Remove Sound Tools Test Tone")
    );
    assert!(!track_file(&mut opened).contains("effects"));
    assert_eq!(settled(&mut opened), plain);

    // One step back: the effect is in the chain again, with the sound it had.
    opened.keys("cmd-z");
    assert_eq!(card_names(&mut opened), [PLUGIN_NAME, PLUGIN_NAME]);
    opened.project(|project| assert_eq!(project.problems(), []));
    assert_eq!(settled(&mut opened), through);
}

/// Two effects of the same plugin, and the control of the second one. Every card of the rack
/// is drawn by one view, so two controls of one element id would be one control to GPUI and
/// the wrong effect would go. Found by hand with three effects on a track.
#[gpui::test]
fn the_control_of_the_second_effect_takes_that_one_off_and_not_the_first(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    add_effect(&mut opened);
    add_effect(&mut opened);
    let names = ["sound-tools-test-tone", "sound-tools-test-tone-2"];
    assert_eq!(card_names(&mut opened).len(), 3);

    let remove = opened.control(&remove_control(&id(&format!("{TRACK}/{}", names[1]))));
    opened.click(remove);
    assert_eq!(card_names(&mut opened).len(), 2);
    let record = track_file(&mut opened);
    assert!(record.contains(&format!(r#"["{}"]"#, names[0])), "{record}");
    assert!(
        !opened
            .path(&format!("state/arrangement/track-1/{}.json", names[1]))
            .exists()
    );
    assert!(
        opened
            .path(&format!("state/arrangement/track-1/{}.json", names[0]))
            .exists()
    );
}

/// A file edit is the way to reorder, and the window follows it.
#[gpui::test]
fn a_reorder_written_from_outside_shows_in_the_rack(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    add_effect(&mut opened);
    add_effect(&mut opened);
    let names = ["sound-tools-test-tone", "sound-tools-test-tone-2"];
    let record = track_file(&mut opened);
    assert!(
        record.contains(&format!(r#"["{}", "{}"]"#, names[0], names[1])),
        "{record}"
    );
    assert_eq!(card_names(&mut opened).len(), 3);

    // The other way round, as an agent writes it: one record and one undo step.
    let path = opened.path(TRACK_FILE);
    let swapped = format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "Track 1", "effects": ["{}", "{}"]}}}}"#,
        names[1], names[0]
    );
    std::fs::write(&path, swapped).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    opened.project(|project| assert_eq!(project.problems(), []));
    assert_eq!(card_names(&mut opened).len(), 3);
    let panel = opened.track_panel().unwrap();
    let slots: Vec<String> = opened.cx.read(|cx| {
        let track = panel.read(cx).track().id().clone();
        let project = opened.session.read(cx).project();
        let track = project.resolve(&track).unwrap();
        arrangement::device_slots(project, &track)
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect()
    });
    assert_eq!(
        slots,
        [
            format!("{TRACK}/instrument"),
            format!("{TRACK}/{}", names[1]),
            format!("{TRACK}/{}", names[0]),
        ]
    );
}
