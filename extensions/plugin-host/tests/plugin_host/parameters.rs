//! What the host reads of a parameter of a plugin a project holds, on the main thread: the
//! value the plugin has now, and the plugin's own text for it.

use plugin_host::{ParameterValue, PluginFormat, PluginRecord};

use crate::pins::{level, pinned, pinned_value};
use crate::support::{FORMATS, Harness, id, record};

#[test]
fn a_loaded_plugin_says_what_a_parameter_is_now_in_its_own_words() {
    for format in FORMATS {
        let mut harness = Harness::new();
        harness.add_track(record(format, "piano"), Vec::new());
        let wave = match format {
            PluginFormat::Clap => test_clap_plugin::WAVE,
            PluginFormat::Vst3 => test_vst3_plugin::WAVE,
        };
        let first = ParameterValue {
            value: 0.0,
            text: Some("Sine".to_string()),
        };
        let instrument = id("track/instrument");
        assert_eq!(
            harness.plugins.parameter_value(&instrument, wave),
            Some(first),
            "{format:?}"
        );
        assert_eq!(harness.plugins.parameter_value(&id("track"), wave), None);
    }
}

/// CLAP says when it has no such parameter. VST 3 cannot: its controller answers any id.
#[test]
fn a_clap_plugin_has_no_value_for_a_parameter_it_does_not_have() {
    let mut harness = Harness::new();
    harness.add_track(record(PluginFormat::Clap, "piano"), Vec::new());
    let instrument = id("track/instrument");
    assert_eq!(harness.plugins.parameter_value(&instrument, 999), None);
    let cutoff = harness
        .plugins
        .parameter_value(&instrument, test_clap_plugin::CUTOFF);
    assert_eq!(
        cutoff.and_then(|cutoff| cutoff.text).as_deref(),
        Some("1000 Hz")
    );
}

/// What the card adds when the composer pins a parameter, sending `pin` with its id: what the
/// plugin plays now, so pinning changes no sound. With no plugin loaded, the parameter's default.
#[test]
fn a_new_pin_starts_at_what_the_plugin_plays_now() {
    for format in FORMATS {
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 0.25), Vec::new());
        harness.render(4096);
        let instrument = id("track/instrument");
        let level = level(format);
        let unpin = |harness: &mut Harness| {
            let instance = harness
                .project
                .resolve::<PluginRecord>(&instrument)
                .unwrap();
            let mut edit = harness.project.begin("Remove Level");
            (harness
                .project
                .update(&mut edit, &instance, |record| record.parameters.clear()))
            .unwrap();
            harness.project.finish(edit).unwrap();
        };
        unpin(&mut harness);
        harness.render(1024);
        let default = harness.plugins.parameters(&instrument).unwrap()[&level].default;
        assert_ne!(default, 0.25);
        let pinned = harness
            .project
            .command(&instrument, "pin", Some(f64::from(level)));
        assert_eq!(pinned.ok(), Some(true));
        assert_eq!(pinned_value(&harness, level), 0.25, "{format:?}");
        let unloaded = harness.plugins.pin_now(
            &id("track"),
            &harness.plugins.parameters(&instrument).unwrap()[&level],
        );
        assert_eq!(unloaded.value, default);
    }
}

/// A card shows a pinned value in the plugin's own words, as its facts give them, and the words
/// follow the value.
#[test]
fn the_facts_give_each_pin_in_the_plugins_own_words() {
    let mut harness = Harness::new();
    let mut record = record(PluginFormat::Clap, "piano");
    let cutoff = test_clap_plugin::CUTOFF;
    record
        .parameters
        .insert(cutoff, crate::pins::pin("Cutoff", 3200.0));
    harness.add_track(record, Vec::new());
    harness.render(1024);
    let instrument = id("track/instrument");
    let words = |harness: &Harness| {
        let facts = sound_core::facts::of(
            &harness.project,
            &sound_core::feeds::NoSources,
            &instrument,
            None,
        );
        facts.unwrap().to_value()["texts"][cutoff.to_string()].clone()
    };
    assert_eq!(words(&harness), "3200 Hz");
    let instance = harness
        .project
        .resolve::<PluginRecord>(&instrument)
        .unwrap();
    let mut edit = harness.project.begin("Change Cutoff");
    (harness.project.update(&mut edit, &instance, |record| {
        record.parameters.get_mut(&cutoff).unwrap().value = 440.0;
    }))
    .unwrap();
    harness.project.finish(edit).unwrap();
    assert_eq!(words(&harness), "440 Hz");
}
