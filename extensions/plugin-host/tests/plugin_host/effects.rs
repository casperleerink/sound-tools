//! A hosted plugin as an effect: the audio that reaches it, the audio that comes out, and the
//! pass-through when its plugin is missing.

use plugin_host::PluginFormat;
use sound_core::Changes;

use crate::support::{EFFECT, FORMATS, Harness, Played, id, peak, record};

/// One note held from the first frame, loud enough to read and quiet enough that the effect
/// half of the test plugin learns nothing from it.
fn played() -> Vec<Played> {
    vec![Played::On {
        frame: 0,
        pitch: 60,
        velocity: 40,
    }]
}

fn without_effect(format: PluginFormat) -> Vec<f32> {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), played());
    harness.play(4096).samples().to_vec()
}

#[test]
fn an_effect_plays_what_it_is_given_and_adds_what_its_state_says() {
    for format in FORMATS {
        let plain = without_effect(format);
        let mut harness = Harness::new();
        harness.add_track(record(format, "piano"), played());
        // The effect half adds 0.25 to every sample.
        harness.write_offset(format, "trim", 25);
        harness.add_effect(record(format, "trim"));
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");

        let through = harness.play(4096).samples().to_vec();
        let expected: Vec<f32> = plain.iter().map(|sample| sample * 0.5 + 0.25).collect();
        assert_eq!(through, expected, "{format:?}");
    }
}

#[test]
fn an_effect_slot_whose_plugin_is_missing_passes_the_sound_through_unchanged() {
    for format in FORMATS {
        let plain = without_effect(format);
        let mut harness = Harness::new();
        harness.add_track(record(format, "piano"), played());
        let missing = plugin_host::PluginRecord::new(
            format,
            match format {
                PluginFormat::Clap => "com.example.nowhere",
                PluginFormat::Vst3 => "00000000000000000000000000000000",
            },
            "trim",
        )
        .expect("a plugin record");
        harness.add_effect(missing);

        let problems = harness.problems();
        assert_eq!(problems.len(), 1, "{format:?}: {problems:?}");
        assert!(
            problems[0].contains("state/track/effect.json"),
            "{problems:?}"
        );
        let through = harness.play(4096).samples().to_vec();
        assert_eq!(through, plain, "{format:?}");
        assert!(peak(&through) > 0.0, "{format:?}");
    }
}

#[test]
fn taking_an_effect_off_gives_the_sound_of_the_instrument_back() {
    for format in FORMATS {
        let mut harness = Harness::new();
        harness.add_track(record(format, "piano"), played());
        harness.write_offset(format, "trim", 25);
        harness.add_effect(record(format, "trim"));
        let through = harness.play(4096).samples().to_vec();

        let mut changes = Changes::new();
        changes.delete(&id(&format!("track/{EFFECT}")));
        harness
            .project
            .commit("Remove the effect", changes)
            .expect("the delete applies");
        harness.plugins.poll(&harness.project);
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");

        let plain = harness.play(4096).samples().to_vec();
        assert!(peak(&plain) > 0.0, "{format:?}: the track went silent");
        assert_ne!(plain, through, "{format:?}");
    }
}

/// The realtime sanitizer cannot see inside a plugin's own call, so this counts allocations.
#[test]
fn a_block_through_an_effect_allocates_nothing() {
    for format in FORMATS {
        let mut harness = Harness::new();
        harness.add_track(record(format, "piano"), played());
        harness.add_effect(record(format, "trim"));
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
        // A plugin sets up in its first blocks.
        harness.play(2048);
        let (_render, allocations) = harness.render_counting_allocations(2048);
        assert_eq!(allocations, 0, "{format:?}");
    }
}

/// A backend writes nothing into its output when it fails, so a host that only passed through
/// from the next block would leave one block of silence.
#[test]
fn the_block_a_plugin_fails_on_is_passed_through_and_is_no_gap() {
    for format in FORMATS {
        tell_the_plugin_to_fail_from(Some(FAILS_FROM));
        let mut harness = Harness::new();
        harness.write_offset(format, "trim", 25);
        harness.add_level_track(DRY, record(format, "trim"));
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");

        let render = harness.play(BLOCK * 8);
        tell_the_plugin_to_fail_from(None);
        let left = render.left();
        // The effect plays `input * 0.5 + offset`.
        let wet = DRY * 0.5 + 0.25;
        let played = BLOCK * FAILS_FROM as usize;
        assert!(
            left[..played].iter().all(|sample| *sample == wet),
            "{format:?}: {:?}",
            &left[..4]
        );
        assert!(
            left[played..].iter().all(|sample| *sample == DRY),
            "{format:?}: {:?}",
            &left[played..played + 4]
        );
    }
}

const DRY: f32 = 0.4;
const FAILS_FROM: u64 = 4;
const BLOCK: usize = sound_core::MAX_BLOCK;

fn tell_the_plugin_to_fail_from(block: Option<u64>) {
    // SAFETY: nextest runs one test per process and the audio of this test is driven from the
    // test thread, so no other thread reads the environment.
    unsafe {
        match block {
            Some(block) => {
                std::env::set_var(test_plugin_support::FAIL_FROM_VARIABLE, block.to_string())
            }
            None => std::env::remove_var(test_plugin_support::FAIL_FROM_VARIABLE),
        }
    }
}

/// An effect that takes no notes is missing nothing about the sustain pedal.
#[test]
fn an_audio_only_effect_reports_nothing_about_the_pedal() {
    for format in FORMATS {
        tell_the_plugin_to_be_audio_only(true);
        let mut harness = Harness::new();
        harness.add_level_track(DRY, record(format, "trim"));
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
        // And it is in the chain.
        let render = harness.play(BLOCK * 4);
        assert!(
            render.left().iter().all(|sample| *sample == DRY * 0.5),
            "{format:?}"
        );
        tell_the_plugin_to_be_audio_only(false);
    }
}

/// A plugin with a sidechain input and an aux output gets the track on its main ports and
/// silence on the others.
#[test]
fn a_clap_plugin_with_extra_ports_gets_a_buffer_for_every_port() {
    // SAFETY: as `tell_the_plugin_to_fail_from`.
    unsafe { std::env::set_var(test_plugin_support::EXTRA_PORTS_VARIABLE, "1") };
    let mut harness = Harness::new();
    harness.add_level_track(DRY, record(PluginFormat::Clap, "trim"));
    assert_eq!(harness.problems(), Vec::<String>::new());
    let render = harness.play(BLOCK * 4);
    assert!(
        render.left().iter().all(|sample| *sample == DRY * 0.5),
        "{:?}",
        &render.left()[..4]
    );
    let (_render, allocations) = harness.render_counting_allocations(BLOCK * 4);
    assert_eq!(allocations, 0);
}

#[test]
fn a_plugin_that_takes_notes_and_no_pedal_is_still_reported() {
    for format in FORMATS {
        tell_the_plugin_to_take_no_pedal(true);
        let mut harness = Harness::new();
        harness.add_level_track(DRY, record(format, "trim"));
        let problems = harness.problems();
        assert_eq!(problems.len(), 1, "{format:?}: {problems:?}");
        assert!(problems[0].contains("sustain pedal"), "{problems:?}");
        tell_the_plugin_to_take_no_pedal(false);
    }
}

fn tell_the_plugin_to_take_no_pedal(without: bool) {
    // SAFETY: as `tell_the_plugin_to_fail_from`.
    unsafe {
        match without {
            true => std::env::set_var(test_plugin_support::NO_PEDAL_VARIABLE, "1"),
            false => std::env::remove_var(test_plugin_support::NO_PEDAL_VARIABLE),
        }
    }
}

fn tell_the_plugin_to_be_audio_only(audio_only: bool) {
    // SAFETY: as `tell_the_plugin_to_fail_from`.
    unsafe {
        match audio_only {
            true => std::env::set_var(test_plugin_support::AUDIO_ONLY_VARIABLE, "1"),
            false => std::env::remove_var(test_plugin_support::AUDIO_ONLY_VARIABLE),
        }
    }
}
