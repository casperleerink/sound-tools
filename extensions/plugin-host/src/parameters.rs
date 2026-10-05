//! The parameters of a plugin, whatever its format, as the plugin itself describes them.
//!
//! Every number is in the format's own units: the plain value for CLAP, such as `440` for a
//! frequency, and the normalized value from 0 to 1 for VST 3. Nothing converts between them, so
//! a value read from a plugin is a value that can be sent back to it unchanged.
//!
//! All of it is read on the main thread, where both formats put these calls.

use std::collections::BTreeMap;

use crate::backend::ParameterChange;
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

    /// Whether an automation lane may move it: the plugin says a host may automate it, and it
    /// takes any value in its range, as a whole number of a built-in device takes no lane.
    pub fn takes_lane(&self) -> bool {
        self.automatable && self.steps.is_none()
    }

    /// The value of the step `index` of a stepped parameter, inside its range. A CLAP plugin
    /// cuts a value to its whole step, so its first step may lie just below the minimum it
    /// gives, and the minimum is what reaches it.
    pub(crate) fn step_value(&self, steps: &Steps, index: u32) -> f64 {
        steps.value(index).max(self.minimum).min(self.maximum)
    }
}

/// A parameter that takes only some values, evenly spaced from its minimum to its maximum.
#[derive(Clone, Debug, PartialEq)]
pub struct Steps {
    /// How many values it takes, the minimum and the maximum among them.
    pub count: u32,
    /// The values of the first and the last step. The ones between are worked out from them
    /// as the plugin does, so a step written to a record is the plugin's own value of it.
    first: f64,
    last: f64,
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
    /// The steps of a parameter with `count` values evenly from `first` to `last`, named by
    /// `text` when the plugin says they are a list.
    pub(crate) fn new(
        count: u32,
        first: f64,
        last: f64,
        is_list: bool,
        mut text: impl FnMut(f64) -> Option<String>,
    ) -> Self {
        let mut steps = Self {
            count,
            first,
            last,
            names: Vec::new(),
        };
        if is_list && count <= MAX_NAMED_STEPS {
            steps.names = (0..count)
                .filter_map(|index| {
                    let value = steps.value(index);
                    Some(StepName {
                        value,
                        name: text(value)?,
                    })
                })
                .collect();
        }
        steps
    }

    /// The value of the step `index`. Multiplied before it is divided, so a whole step of
    /// CLAP and `index / steps` of VST 3 come out exact.
    pub fn value(&self, index: u32) -> f64 {
        match self.count {
            0 | 1 => self.first,
            count => {
                self.first + (self.last - self.first) * f64::from(index) / f64::from(count - 1)
            }
        }
    }

    /// The step nearest `value`.
    pub fn index(&self, value: f64) -> u32 {
        let gaps = f64::from(self.count.saturating_sub(1));
        let index = ((value - self.first) * gaps / (self.last - self.first)).round();
        // Not a number is the first step, as `as` makes it.
        index.clamp(0.0, gaps) as u32
    }

    /// Whether every step has a name, which is what a list a composer picks from needs.
    pub fn all_named(&self) -> bool {
        self.names.len() == self.count as usize
    }
}

/// Parameters by their id, as the host keeps them.
pub(crate) fn by_id(parameters: Vec<Parameter>) -> BTreeMap<u32, Parameter> {
    let parameters = parameters.into_iter();
    parameters
        .map(|parameter| (parameter.id, parameter))
        .collect()
}

/// The pins of a record that a plugin with `parameters` takes: one it has a parameter for, with
/// a value in its range. Only these are sent; the others move nothing and are reported.
pub(crate) fn playable<'a>(
    parameters: &'a BTreeMap<u32, Parameter>,
    pins: &'a BTreeMap<u32, Pin>,
) -> impl Iterator<Item = ParameterChange> + 'a {
    let pins = pins.iter();
    pins.filter(|(id, pin)| parameters.get(id).is_some_and(|it| it.takes(pin.value)))
        .map(|(id, pin)| ParameterChange {
            id: *id,
            value: pin.value,
        })
}

/// The name an automation lane gives the pin `id`: its path in the record, as a lane names a
/// number of a built-in device by its path.
pub(crate) fn lane_of_pin(id: u32) -> String {
    format!("parameters.{id}.value")
}

/// The pin an automation lane of this name moves, when it names one.
pub(crate) fn pin_of_lane(name: &str) -> Option<u32> {
    let id = name.strip_prefix("parameters.")?.strip_suffix(".value")?;
    id.parse().ok()
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
        let long = Steps::new(
            MAX_NAMED_STEPS + 1,
            0.0,
            f64::from(MAX_NAMED_STEPS),
            true,
            |value| Some(value.to_string()),
        );
        assert!(long.names.is_empty());
        // A step the plugin gives no text for is left out, not named with a guess.
        let short = Steps::new(3, 0.0, 2.0, true, |value| {
            (value != 1.0).then(|| format!("step {value}"))
        });
        let names: Vec<_> = short.names.iter().map(|step| step.name.as_str()).collect();
        assert_eq!(names, ["step 0", "step 2"]);
        assert!(!short.all_named());
    }

    /// The steps of VST 3 are fractions, which a value in single precision is not quite.
    #[test]
    fn a_step_is_found_from_a_value_near_it_and_its_value_is_exact() {
        let thirds = Steps::new(4, 0.0, 1.0, false, |_| None);
        let near = f64::from(1.0_f32 / 3.0);
        assert_eq!(thirds.index(near), 1);
        assert_eq!(thirds.value(1), 1.0 / 3.0);
        assert_eq!(thirds.index(2.0), 3);
        assert_eq!(thirds.index(-1.0), 0);
        // Multiplying by a forty-ninth would make the last 0.9999999999999999.
        let fine = Steps::new(50, 0.0, 1.0, false, |_| None);
        assert_eq!(fine.value(49), 1.0);
    }
}
