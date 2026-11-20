//! What the host reads of a parameter of a plugin a project holds, on the main thread: the
//! value the plugin has now, and the plugin's own text for it.

use plugin_host::{ParameterValue, PluginFormat};

use crate::pins::{level, pinned};
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

/// What the card adds when the composer pins a parameter: what the plugin plays now, so pinning
/// changes no sound. With no plugin loaded, the parameter's default.
#[test]
fn a_new_pin_starts_at_what_the_plugin_plays_now() {
    for format in FORMATS {
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 0.25), Vec::new());
        harness.render(4096);
        let instrument = id("track/instrument");
        let parameters = harness.plugins.parameters(&instrument).unwrap();
        let level = &parameters[&level(format)];
        assert_ne!(level.default, 0.25);
        let pin = harness.plugins.pin_now(&instrument, level);
        assert_eq!(pin.value, 0.25, "{format:?}");
        assert_eq!(pin.name, level.name);
        let unloaded = harness.plugins.pin_now(&id("track"), level);
        assert_eq!(unloaded.value, level.default);
    }
}
