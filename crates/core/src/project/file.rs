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

/// Where a saved connection starts: an output of an instance, `{"instance": "drone", "port":
/// "audio"}`, or the device input, `{"device_input": 0}`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "SourceFields", into = "SourceFields")]
pub enum SavedSource {
    Output(PortReference),
    /// A channel of the device input, counted from 0: the first of a stereo port, as for
    /// [`SavedDestination::DeviceOutput`].
    DeviceInput(usize),
}

impl From<PortReference> for SavedSource {
    fn from(port: PortReference) -> Self {
        Self::Output(port)
    }
}

/// [`SavedSource`] as it is written. Both forms in one struct, so a wrong mix of fields gets a
/// message that names both.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<InstanceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_input: Option<usize>,
}

impl TryFrom<SourceFields> for SavedSource {
    type Error = &'static str;

    fn try_from(fields: SourceFields) -> Result<Self, Self::Error> {
        match fields {
            SourceFields {
                instance: Some(instance),
                port: Some(port),
                device_input: None,
            } => Ok(Self::Output(PortReference { instance, port })),
            SourceFields {
                instance: None,
                port: None,
                device_input: Some(channel),
            } => Ok(Self::DeviceInput(channel)),
            _ => Err(
                r#"a connection starts at an output, {"instance": ..., "port": ...}, or at the device input, {"device_input": 0}"#,
            ),
        }
    }
}

impl From<SavedSource> for SourceFields {
    fn from(source: SavedSource) -> Self {
        match source {
            SavedSource::Output(PortReference { instance, port }) => Self {
                instance: Some(instance),
                port: Some(port),
                device_input: None,
            },
            SavedSource::DeviceInput(channel) => Self {
                instance: None,
                port: None,
                device_input: Some(channel),
            },
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
    pub from: SavedSource,
    pub to: SavedDestination,
}

impl SavedConnection {
    pub fn to_device(from: PortReference, channel: usize) -> Self {
        Self {
            from: from.into(),
            to: SavedDestination::DeviceOutput(channel),
        }
    }

    /// Whether an end is `id` or an instance inside it.
    pub(crate) fn touches(&self, id: &InstanceId) -> bool {
        let names = |port: &PortReference| &port.instance == id || port.instance.is_inside(id);
        matches!(&self.from, SavedSource::Output(output) if names(output))
            || matches!(&self.to, SavedDestination::Input(input) if names(input))
    }
}

impl ProjectFile {
    /// Whether a connection starts at the device input, so the input has to be open.
    pub fn hears_device_input(&self) -> bool {
        (self.connections.iter())
            .any(|connection| matches!(connection.from, SavedSource::DeviceInput(_)))
    }
}
