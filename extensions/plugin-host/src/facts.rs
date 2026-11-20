//! What a card of a plugin knows that is no record: whether it plays, its window and its
//! parameters. Read every frame a card shows, so it calls into no plugin but to list the
//! parameters once.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;
use sound_core::InstanceId;

use crate::{PluginRecord, Plugins};

#[derive(Serialize, JsonSchema)]
pub(crate) struct Facts {
    /// The plugin's own name; null when this computer does not have it.
    name: Option<String>,
    status: Status,
    window: Window,
    /// Its parameters in the order of their ids; empty while it does not play.
    parameters: Vec<ParameterFacts>,
    /// The plugin's own words for the value of each pin, such as `3.2 kHz`, by its id.
    texts: BTreeMap<u32, String>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Status {
    /// Not on this computer.
    Missing,
    /// It crashed or hung in its own process and waits to be started again.
    Stopped,
    /// On this computer, but it did not load.
    NotLoaded,
    Plays,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Window {
    /// It has no window of its own, or this system opens none (Linux, for now).
    None,
    Closed,
    Open,
}

#[derive(Serialize, JsonSchema)]
struct ParameterFacts {
    id: u32,
    name: String,
    min: f64,
    max: f64,
    default: f64,
    /// How many values it takes, evenly from min to max; null for any value in its range.
    steps: Option<u32>,
    /// The values of its steps that have names, for a list of them.
    names: Vec<StepFacts>,
    /// A lane may move it.
    automatable: bool,
}

#[derive(Serialize, JsonSchema)]
struct StepFacts {
    value: f64,
    name: String,
}

impl Facts {
    pub(crate) fn of(plugins: &Plugins, record: &PluginRecord, instance: &InstanceId) -> Self {
        let name = plugins.installed_name(record.format, &record.plugin_id);
        let offered = plugins.window_offered(instance);
        let status = match (&name, plugins.has_stopped(instance), offered) {
            (None, ..) => Status::Missing,
            (_, true, _) => Status::Stopped,
            (_, _, None) => Status::NotLoaded,
            _ => Status::Plays,
        };
        let opens = cfg!(any(target_os = "macos", target_os = "windows"));
        let window = match (
            opens && offered == Some(true),
            plugins.window_is_open(instance),
        ) {
            (false, _) => Window::None,
            (true, false) => Window::Closed,
            (true, true) => Window::Open,
        };
        let parameters = match status {
            Status::Plays => plugins.parameters(instance),
            _ => None,
        };
        let parameters = (parameters.iter().flat_map(|parameters| parameters.values()))
            .map(|parameter| ParameterFacts {
                id: parameter.id,
                name: parameter.name.clone(),
                min: parameter.minimum,
                max: parameter.maximum,
                default: parameter.default,
                steps: parameter.steps.as_ref().map(|steps| steps.count),
                names: (parameter.steps.iter().flat_map(|steps| &steps.names))
                    .map(|step| StepFacts {
                        value: step.value,
                        name: step.name.clone(),
                    })
                    .collect(),
                automatable: parameter.automatable,
            })
            .collect();
        let texts = match status {
            Status::Plays => (record.parameters.iter())
                .filter_map(|(id, pin)| {
                    Some((*id, plugins.parameter_text(instance, *id, pin.value)?))
                })
                .collect(),
            _ => BTreeMap::new(),
        };
        Self {
            name,
            status,
            window,
            parameters,
            texts,
        }
    }
}
