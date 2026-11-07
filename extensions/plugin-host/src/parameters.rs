//! The parameters of a plugin, whatever its format.
//!
//! Every number is in the format's own units: CLAP's plain value, such as `440`, VST 3's 0 to 1.
//! Nothing converts, so a value read from a plugin can be sent back to it unchanged.

use std::collections::BTreeMap;

use crate::backend::ParameterChange;
use crate::scan::ScannedPlugin;
use crate::{Pin, PluginProblem};

/// A longer list is a knob, not a choice a composer reads through, and asking the plugin for
/// every name would be thousands of calls.
pub const MAX_NAMED_STEPS: u32 = 128;

/// One parameter a host may set. Read-only and hidden ones are never listed.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    /// Stays the same across versions of the plugin.
    pub id: u32,
    pub name: String,
    pub minimum: f64,
    pub maximum: f64,
    pub default: f64,
    /// `None` for a parameter that takes any value in its range.
    pub steps: Option<Steps>,
    pub automatable: bool,
}

impl Parameter {
    pub(crate) fn takes(&self, value: f64) -> bool {
        (self.minimum..=self.maximum).contains(&value)
    }

    /// A stepped parameter takes no lane, as a whole number of a built-in device takes none.
    pub fn takes_lane(&self) -> bool {
        self.automatable && self.steps.is_none()
    }

    /// A pin outside the range moves nothing, and neither would its lane.
    pub fn takes_lane_at(&self, pin: &Pin) -> bool {
        self.takes_lane() && self.takes(pin.value)
    }

    /// Inside the range: a CLAP plugin cuts a value to its whole step, so its first step may lie
    /// just below the minimum it gives.
    pub(crate) fn step_value(&self, steps: &Steps, index: u32) -> f64 {
        steps.value(index).max(self.minimum).min(self.maximum)
    }
}

/// A parameter that takes only some values, evenly spaced from its minimum to its maximum.
#[derive(Clone, Debug, PartialEq)]
pub struct Steps {
    /// The minimum and the maximum among them.
    pub count: u32,
    /// The steps between are worked out from these as the plugin does, so a step written to a
    /// record is the plugin's own value of it.
    first: f64,
    last: f64,
    /// When the plugin says the steps are a list (CLAP `IS_ENUM`, VST 3 `kIsList`) of at most
    /// [`MAX_NAMED_STEPS`]. A value the plugin gives no text for has no name here.
    pub names: Vec<StepName>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StepName {
    pub value: f64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterValue {
    pub value: f64,
    /// Such as `1.2 kHz`.
    pub text: Option<String>,
}

impl Steps {
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

    /// Multiplied before it is divided, so a whole step of CLAP and `index / steps` of VST 3
    /// come out exact.
    pub fn value(&self, index: u32) -> f64 {
        match self.count {
            0 | 1 => self.first,
            count => {
                self.first + (self.last - self.first) * f64::from(index) / f64::from(count - 1)
            }
        }
    }

    pub fn index(&self, value: f64) -> u32 {
        let gaps = f64::from(self.count.saturating_sub(1));
        let index = ((value - self.first) * gaps / (self.last - self.first)).round();
        // Not a number is the first step, as `as` makes it.
        index.clamp(0.0, gaps) as u32
    }

    pub fn all_named(&self) -> bool {
        self.names.len() == self.count as usize
    }
}

pub(crate) fn by_id(parameters: Vec<Parameter>) -> BTreeMap<u32, Parameter> {
    let parameters = parameters.into_iter();
    parameters
        .map(|parameter| (parameter.id, parameter))
        .collect()
}

/// The pins a plugin with `parameters` takes. The others move nothing and are reported.
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

/// A lane names a pin by its path in the record, as a number of a built-in device.
pub(crate) fn lane_of_pin(id: u32) -> String {
    format!("parameters.{id}.value")
}

/// Why the pin `id` moves nothing, when it does not.
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

/// Makes the plugin in this process, never activated, and lets it go again. Running the
/// plugin's own code here is only for a command that runs once and ends, `--plugin-params`.
pub fn read_parameters(plugin: &ScannedPlugin) -> Result<Vec<Parameter>, PluginProblem> {
    match plugin.format {
        crate::PluginFormat::Clap => crate::clap::read_parameters(plugin),
        crate::PluginFormat::Vst3 => crate::vst3::read_parameters(plugin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin that narrows a range leaves a pin outside it, which takes no lane.
    #[test]
    fn a_pin_outside_the_range_takes_no_lane() {
        let parameter = Parameter {
            id: 0,
            name: "Cutoff".to_string(),
            minimum: 20.0,
            maximum: 20_000.0,
            default: 1000.0,
            steps: None,
            automatable: true,
        };
        let pin = |value| Pin {
            name: "Cutoff".to_string(),
            value,
        };
        assert!(parameter.takes_lane_at(&pin(1000.0)));
        assert!(!parameter.takes_lane_at(&pin(30_000.0)));
    }

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
