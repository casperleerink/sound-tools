//! What the composer changes in a VST 3 plugin's own window reaches the half that makes the
//! sound. `ivsteditcontroller.h`: `IComponentHandler` is to "Allow transfer of parameter editing
//! to component (processor) via host". CLAP has no such split.
//!
//! The VST 3 test plugin's `Level` is read only by its processor, and its controller edits it
//! through the host as soon as it has a component handler.

use plugin_host::PluginFormat;

use crate::support::{
    Harness, Played, peak, record, saved_edit_level, state_asset, tell_the_plugin,
    tell_the_plugin_to_edit_its_level,
};

/// One long note, so a render is one level to read.
fn one_note() -> Vec<Played> {
    vec![Played::On {
        frame: 0,
        pitch: 60,
        velocity: 100,
    }]
}

/// The first blocks are thrown away: an edit made while the plugin loaded reaches the processor
/// at the first poll after it.
fn level_of(harness: &mut Harness) -> f32 {
    harness.play(2048);
    let render = harness.render(4096);
    peak(&render.left())
}

#[test]
fn an_edit_in_the_plugins_window_reaches_its_processor_and_is_heard() {
    tell_the_plugin(None, None);
    let mut plain = Harness::new();
    plain.add_track(record(PluginFormat::Vst3, "piano"), one_note());
    let full = level_of(&mut plain);
    assert!(full > 0.0);
    drop(plain);

    // Four edits, ending on a quarter.
    tell_the_plugin_to_edit_its_level(4);
    let mut edited = Harness::new();
    edited.add_track(record(PluginFormat::Vst3, "piano"), one_note());
    let after = level_of(&mut edited);
    assert_eq!(edited.problems(), Vec::<String>::new());

    let quarter = full / 4.0;
    assert!(
        (after - quarter).abs() < full / 100.0,
        "the plugin plays at {after} of {full}, which is not the quarter the last edit asked for"
    );
    tell_the_plugin_to_edit_its_level(0);
}

#[test]
fn the_state_saved_after_an_edit_holds_what_the_edit_left() {
    tell_the_plugin_to_edit_its_level(4);
    let mut harness = Harness::new();
    harness.add_track(record(PluginFormat::Vst3, "piano"), one_note());
    let edited = level_of(&mut harness);
    harness.plugins.close(&mut harness.project);

    let bytes = harness
        .project
        .assets()
        .read(&state_asset("piano"))
        .expect("the asset reads")
        .expect("the plugin saved its state");
    assert_eq!(saved_edit_level(&bytes), 25);

    // The plugin edits itself again on the way up, to the same value.
    let mut again = harness.reopen();
    again.add_track(record(PluginFormat::Vst3, "piano"), one_note());
    let reopened = level_of(&mut again);
    assert!(
        (reopened - edited).abs() < edited / 100.0,
        "{reopened} {edited}"
    );
    tell_the_plugin_to_edit_its_level(0);
}
