//! The saved form of `project.json`.

use serde::{Deserialize, Serialize};

use super::instance::InstanceId;
use crate::clock::TempoMap;

/// The only project format this runtime reads and writes.
pub const FORMAT: u32 = 1;

/// Everything in `project.json`. Instances are not listed: the runtime finds them in `state/`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    #[serde(deserialize_with = "deserialize_format")]
    pub format: u32,
    /// Enabled extensions by name. Records of tools from other extensions stay on disk
    /// untouched and are reported.
    pub extensions: Vec<String>,
    pub tempo_map: TempoMap,
    /// Core-owned connections. Routing that a tool makes by itself, such as a parent to its
    /// children or to the device, is not listed here.
    pub connections: Vec<SavedConnection>,
}

impl ProjectFile {
    pub(crate) fn new(extensions: Vec<String>) -> Self {
        Self {
            format: FORMAT,
            extensions,
            tempo_map: TempoMap::default(),
            connections: Vec::new(),
        }
    }
}

fn deserialize_format<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let format = u32::deserialize(deserializer)?;
    if format == FORMAT {
        Ok(format)
    } else {
        Err(serde::de::Error::custom(format!(
            "this runtime reads project format {FORMAT}, not {format}"
        )))
    }
}

/// A named port of an instance. Tools name their ports when they expose them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortReference {
    pub instance: InstanceId,
    pub port: String,
}

impl PortReference {
    pub fn new(instance: &InstanceId, port: &str) -> Self {
        Self {
            instance: instance.clone(),
            port: port.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedDestination {
    /// An input port of another instance.
    Input(PortReference),
    /// A channel of the device output, counted from 0.
    DeviceOutput(usize),
}

/// A connection as saved: `{"from": {"instance": "tone-a", "port": "audio"}, "to":
/// {"device_output": 0}}`. It names instances by id, so it is a reference, not ownership. An
/// end that does not exist right now leaves the connection saved, unused and reported.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedConnection {
    pub from: PortReference,
    pub to: SavedDestination,
}

impl SavedConnection {
    pub fn to_device(from: PortReference, channel: usize) -> Self {
        Self {
            from,
            to: SavedDestination::DeviceOutput(channel),
        }
    }

    pub fn to_input(from: PortReference, input: PortReference) -> Self {
        Self {
            from,
            to: SavedDestination::Input(input),
        }
    }

    /// Whether an end is `id` or an instance inside it.
    pub(crate) fn touches(&self, id: &InstanceId) -> bool {
        let names = |port: &PortReference| &port.instance == id || port.instance.is_inside(id);
        names(&self.from) || matches!(&self.to, SavedDestination::Input(input) if names(input))
    }
}
