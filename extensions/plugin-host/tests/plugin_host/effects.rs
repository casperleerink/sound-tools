//! A hosted plugin in an effect slot: the audio that reaches it, the audio that comes out, and
//! what a slot whose plugin is missing does to the sound that was going through it.
//!
//! One record and one wrapper serve both slots, so what is checked here is the wrapper: the
//! stereo input it gives the plugin, the pass-through when there is no plugin, and that neither
//! allocates on the audio thread.

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

/// What the track plays without any effect, for what every test here compares against.
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
        // A state of its own: the effect half adds 0.25 to every sample.
        harness.write_offset(format, "trim", 25);
        harness.add_effect(record(format, "trim"));
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");

        let through = harness.play(4096).samples().to_vec();
        let expected: Vec<f32> = plain.iter().map(|sample| sample * 0.5 + 0.25).collect();
        assert_eq!(through, expected, "{format:?}");
    }
}

/// The rule that keeps one missing plugin from silencing a track: a slot with no plugin passes
/// what it is given straight through.
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

/// An effect that is taken off the chain while it plays. The sound goes back to what the
/// instrument makes, and nothing is left behind.
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

/// The audio thread allocates nothing with a chain, which is one buffer more to copy per block
/// in each direction. The realtime sanitizer cannot see inside a plugin's own call; this
/// counts every allocation of the process instead.
#[test]
fn a_block_through_an_effect_allocates_nothing() {
    for format in FORMATS {
        let mut harness = Harness::new();
        harness.add_track(record(format, "piano"), played());
        harness.add_effect(record(format, "trim"));
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
        // The first blocks are what a plugin sets up in; the count is of the ones after.
        harness.play(2048);
        let (_render, allocations) = harness.render_counting_allocations(2048);
        assert_eq!(allocations, 0, "{format:?}");
    }
}
