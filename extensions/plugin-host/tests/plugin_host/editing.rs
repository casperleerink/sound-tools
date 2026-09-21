//! What the composer changes in a plugin's own window reaches the half that makes the sound.
//!
//! `ivsteditcontroller.h` says what `IComponentHandler` is for: "Allow transfer of parameter
//! editing to component (processor) via host and support automation." A VST 3 plugin is up to
//! two objects, and the one with the knobs is not the one with the audio, so a host that only
//! notes an edit leaves the plugin sounding the way it did before the composer touched it.
//!
//! The repository's VST 3 test plugin has a `Level` parameter that only its processor reads.
//! Its controller edits that parameter through the host as soon as it has a component handler,
//! which is what its own window would do under the composer's hand. CLAP has no such split:
//! a CLAP plugin's parameters are its own business and nothing of the host is in between.

use plugin_host::PluginFormat;

use crate::support::{
    Harness, Played, peak, record, saved_edit_level, state_asset, tell_the_plugin,
    tell_the_plugin_to_edit_its_level,
};

/// A clip of one long note, so a render is one level to read.
fn one_note() -> Vec<Played> {
    vec![Played::On {
        frame: 0,
        pitch: 60,
        velocity: 100,
    }]
}

/// How loud the plugin plays, once whatever the host had to carry has arrived.
///
/// The first blocks are played and thrown away: an edit made while the plugin loaded reaches
/// the processor at the first poll after it, which is one block later, and the level of this
/// plugin is the level of the block it is in.
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

    // Four edits of one parameter, ending on a quarter. The host has to carry them to the
    // processor: it is the only half that reads the parameter.
    tell_the_plugin_to_edit_its_level(4);
    let mut edited = Harness::new();
    edited.add_track(record(PluginFormat::Vst3, "piano"), one_note());
    let after = level_of(&mut edited);
    assert_eq!(edited.problems(), Vec::<String>::new());

    // The last value of the burst and not the first, an average, or none of them.
    let quarter = full / 4.0;
    assert!(
        (after - quarter).abs() < full / 100.0,
        "the plugin plays at {after} of {full}, which is not the quarter the last edit asked for"
    );
    tell_the_plugin_to_edit_its_level(0);
}

/// The state the host saves afterwards holds what the edit left the plugin on, so a close and
/// a reopen bring back the sound the composer made and not the one the plugin starts with.
#[test]
fn the_state_saved_after_an_edit_holds_what_the_edit_left() {
    tell_the_plugin_to_edit_its_level(4);
    let mut harness = Harness::new();
    harness.add_track(record(PluginFormat::Vst3, "piano"), one_note());
    let edited = level_of(&mut harness);
    harness.plugins.close(&harness.project);

    let bytes = harness
        .project
        .assets()
        .read(&state_asset("piano"))
        .expect("the asset reads")
        .expect("the plugin saved its state");
    assert_eq!(saved_edit_level(&bytes), 25);

    // And the same project, opened again, plays at the level the state holds. The plugin
    // edits itself again on the way up, which is the same value, so this is the sound either
    // way round: what a close and a reopen may not do is lose it.
    let mut again = harness.reopen();
    again.add_track(record(PluginFormat::Vst3, "piano"), one_note());
    let reopened = level_of(&mut again);
    assert!(
        (reopened - edited).abs() < edited / 100.0,
        "{reopened} {edited}"
    );
    tell_the_plugin_to_edit_its_level(0);
}

/// A plugin that moves the parameter its sustain pedal is mapped to, after the host has looked
/// that mapping up. VST 3 has no MIDI controller event, so a pedal reaches a plugin only
/// through the parameter `IMidiMapping` names, and `kMidiCCAssignmentChanged` is how a plugin
/// says that name has changed, after a MIDI learn or a loaded preset.
///
/// This build looks the mapping up once, while the plugin loads, and keeps it. So the pedal
/// goes on reaching the parameter it reached before, and the composer is told that it may now
/// be the wrong one instead of being left to wonder why his pedal does nothing.
#[test]
fn a_plugin_that_moves_its_pedal_mapping_is_reported_and_the_pedal_still_reaches_the_old_one() {
    tell_the_plugin(None, None);
    crate::support::tell_the_plugin_to_move_its_pedal();
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![
            Played::On {
                frame: 0,
                pitch: 60,
                velocity: 100,
            },
            Played::Pedal {
                frame: 64,
                value: 100,
            },
        ],
    );
    // Nothing the behaviour reported: the record is fine and the plugin plays.
    assert_eq!(harness.problems(), Vec::<String>::new());

    harness.project.engine().play();
    let render = harness.render_without_polling(512);
    // The pedal still reaches the parameter the host found, which the plugin writes into its
    // right channel. That is what the message says happens.
    assert_eq!(render.right()[64], 100.0 / 127.0);

    // The first poll after the load is where the host says so, once.
    let reported = harness.plugins.poll(&harness.project);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(
        reported[0]
            .to_string()
            .contains("moved the parameter its sustain pedal is mapped to"),
        "{reported:?}"
    );
    // And not again at the next poll: the plugin said it once.
    assert_eq!(harness.plugins.poll(&harness.project), Vec::new());
}
