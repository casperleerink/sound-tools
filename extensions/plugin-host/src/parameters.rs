//! The parameters of a plugin, whatever its format, as the plugin itself describes them.
//!
//! Every number is in the format's own units: the plain value for CLAP, such as `440` for a
//! frequency, and the normalized value from 0 to 1 for VST 3. Nothing converts between them, so
//! a value read from a plugin is a value that can be sent back to it unchanged.
//!
//! All of it is read on the main thread, where both formats put these calls.

use std::collections::BTreeMap;

use crate::scan::ScannedPlugin;
use crate::{Pin, PluginProblem};

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

impl Parameter {
    /// Whether `value` is in its range.
    pub(crate) fn takes(&self, value: f64) -> bool {
        (self.minimum..=self.maximum).contains(&value)
    }
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

/// Why the pin `id` of a record moves nothing, when it does not: the plugin has no such
/// parameter a host may set, or the value is outside its range. Such a pin is not sent, and the
/// rest of the record plays.
pub(crate) fn pin_problem(
    plugin_id: &str,
    parameters: &BTreeMap<u32, Parameter>,
    id: u32,
    pin: &Pin,
) -> Option<PluginProblem> {
    let Some(parameter) = parameters.get(&id) else {
        return Some(PluginProblem::NoSuchParameter {
            plugin_id: plugin_id.to_string(),
            id,
        });
    };
    (!parameter.takes(pin.value)).then(|| PluginProblem::OutOfRange {
        plugin_id: plugin_id.to_string(),
        id,
        name: parameter.name.clone(),
        value: pin.value,
        minimum: parameter.minimum,
        maximum: parameter.maximum,
    })
}

/// Every parameter of `plugin` a host may set, read from the plugin itself. Read-only and
/// hidden parameters are left out.
///
/// It makes the plugin in this process and lets it go again, which runs the plugin's own code
/// here. That is for a command that runs once and ends, `sound-tools --plugin-params`. The
/// plugin is initialized and never activated: nothing is prepared for audio.
pub fn read_parameters(plugin: &ScannedPlugin) -> Result<Vec<Parameter>, PluginProblem> {
    match plugin.format {
        crate::PluginFormat::Clap => crate::clap::read_parameters(plugin),
        crate::PluginFormat::Vst3 => crate::vst3::read_parameters(plugin),
    }
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
