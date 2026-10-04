//! The parameters of a plugin, whatever its format, as the plugin itself describes them.
//!
//! Every number is in the format's own units: the plain value for CLAP, such as `440` for a
//! frequency, and the normalized value from 0 to 1 for VST 3. Nothing converts between them, so
//! a value read from a plugin is a value that can be sent back to it unchanged.
//!
//! All of it is read on the main thread, where both formats put these calls.

use sound_core::PrepareConfig;

use crate::PluginProblem;
use crate::scan::ScannedPlugin;

/// The most steps a parameter has names for. A list longer than this is a knob with many
/// values, not a choice a composer reads through, and asking the plugin for every name would
/// be thousands of calls.
pub const MAX_NAMED_STEPS: u32 = 128;

/// One parameter a host may set. A read-only one, such as a meter, is the plugin's to set and
/// is never listed.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    /// The plugin's own id of it, which stays the same across versions of the plugin.
    pub id: u32,
    pub name: String,
    pub minimum: f64,
    pub maximum: f64,
    pub default: f64,
    /// `None` for a parameter that takes any value in its range.
    pub steps: Option<Steps>,
    /// Whether the plugin says a host may move it while it plays.
    pub automatable: bool,
}

/// A parameter that takes only some values, evenly spaced from its minimum to its maximum.
#[derive(Clone, Debug, PartialEq)]
pub struct Steps {
    /// How many values it takes, the minimum and the maximum among them.
    pub count: u32,
    /// The plugin's name of each value, in order, when it says the steps are a list of names
    /// (CLAP `IS_ENUM`, VST 3 `kIsList`) and has no more than [`MAX_NAMED_STEPS`]. A value the
    /// plugin gives no text for has no name here.
    pub names: Vec<StepName>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StepName {
    pub value: f64,
    pub name: String,
}

/// What a parameter is now, as the plugin says.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterValue {
    pub value: f64,
    /// The plugin's own text for the value, such as `1.2 kHz`, when it gives one.
    pub text: Option<String>,
}

impl Steps {
    /// The steps of a parameter with `count` values, `value(index)` the value of each, named by
    /// `text` when the plugin says they are a list.
    pub(crate) fn new(
        count: u32,
        is_list: bool,
        value: impl Fn(u32) -> f64,
        mut text: impl FnMut(f64) -> Option<String>,
    ) -> Self {
        let names = match is_list && count <= MAX_NAMED_STEPS {
            true => (0..count)
                .filter_map(|index| {
                    let value = value(index);
                    Some(StepName {
                        value,
                        name: text(value)?,
                    })
                })
                .collect(),
            false => Vec::new(),
        };
        Self { count, names }
    }
}

/// Every parameter of `plugin`, read from the plugin itself.
///
/// It loads the plugin in this process and lets it go again, which runs the plugin's own code
/// here. That is for a command that runs once and ends, `sound-tools --plugin-params`; a
/// project reads the parameters of the plugins it already holds.
pub fn read_parameters(plugin: &ScannedPlugin) -> Result<Vec<Parameter>, PluginProblem> {
    // Offline, so a plugin that streams from disk starts nothing for a render that never comes.
    let config = PrepareConfig {
        sample_rate: 48_000,
        offline: true,
    };
    let opening = match plugin.format {
        crate::PluginFormat::Clap => crate::clap::load(plugin, None, config),
        crate::PluginFormat::Vst3 => crate::vst3::load(plugin, None, config),
    }?;
    let crate::backend::Opening {
        mut plugin,
        started,
        ..
    } = opening;
    let parameters = plugin.parameters();
    // The audio side goes first, and then the plugin lets go of itself, in the order the
    // engine gives a plugin back.
    // Nothing else holds the audio side, so the plugin always lets go here.
    drop(started);
    let _released = plugin.released();
    Ok(parameters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_longer_than_the_most_names_has_none_and_a_short_one_has_one_per_text() {
        let long = Steps::new(MAX_NAMED_STEPS + 1, true, f64::from, |value| {
            Some(value.to_string())
        });
        assert!(long.names.is_empty());
        // A step the plugin gives no text for is left out, not named with a guess.
        let short = Steps::new(3, true, f64::from, |value| {
            (value != 1.0).then(|| format!("step {value}"))
        });
        let names: Vec<_> = short.names.iter().map(|step| step.name.as_str()).collect();
        assert_eq!(names, ["step 0", "step 2"]);
    }
}
