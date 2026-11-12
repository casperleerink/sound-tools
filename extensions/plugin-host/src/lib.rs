//! Plugin host: third-party CLAP and VST 3 plugins as the `plugin` tool, a child like a
//! built-in device, as an instrument or as an effect.
//!
//! A record may name any plugin in either place. What a plugin says it is only decides which
//! picker offers it, because plugins get this wrong: Spectral Freeze calls itself an instrument
//! and is an effect.

mod backend;
#[cfg(unix)]
mod bridge;
mod clap;
mod host;
#[cfg(target_os = "windows")]
mod library;
mod parameters;
mod placements;
mod processor;
mod scan;
pub mod view;
mod vst3;
mod window;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, AssetError, AssetName, Assets, BehaviourContext, BehaviourError, InputEndpoint,
    InvalidAssetName, MAX_AUTOMATED, OutputEndpoint, ParameterInfo, Registry, RegistryError, State,
    ValueRange,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT, NOTES_INPUT};

use crate::parameters::lane_of_pin;
use crate::processor::{AutomatedPin, AutomatedPins, HostedPlugin, HostedUpdate};

#[cfg(unix)]
pub use bridge::{PLAY_ARGUMENT, remember_this_program, serve as serve_plugin};
pub use host::{Isolation, PluginProblem, Plugins, WeakPlugins};
pub use parameters::{
    MAX_NAMED_STEPS, Parameter, ParameterValue, StepName, Steps, read_parameters,
};
pub use scan::{
    SCAN_ARGUMENT, ScanCache, ScanCommand, ScanFailure, ScannedPlugin, default_search_paths,
    one_per_plugin, scan_one_bundle,
};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "plugin-host";

/// Steinberg's VST usage guidelines, section 15, ask for this in documentation and in an app
/// that writes "VST" where the VST Compatible Logo does not fit, such as a menu row: once, in
/// its settings. "VST" is never stylized and is not in the product name.
pub const VST_TRADEMARK: &str =
    "VST is a registered trademark of Steinberg Media Technologies GmbH.";

const PROCESSOR: &str = "plugin";

const STATE_FOLDER: &str = "plugin-state";
const STATE_EXTENSION: &str = "bin";

/// In the order a picker prefers them for a plugin installed in both: CLAP gives a parameter in
/// its own units, so a pin and a lane hold 1000 Hz, where VST 3 gives 0 to 1.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginFormat {
    Clap,
    Vst3,
}

impl PluginFormat {
    /// What a person reads. "VST" is written as Steinberg asks, see [`VST_TRADEMARK`].
    pub fn name(self) -> &'static str {
        match self {
            Self::Clap => "CLAP",
            Self::Vst3 => "VST 3",
        }
    }

    /// What a record holds, and what the scanner takes as an argument.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clap => "clap",
            Self::Vst3 => "vst3",
        }
    }

    pub fn of_str(text: &str) -> Option<Self> {
        match text {
            "clap" => Some(Self::Clap),
            "vst3" => Some(Self::Vst3),
            _ => None,
        }
    }

    /// The format of a bundle, from its file extension. The case does not count, as it does not
    /// for the file systems of Windows and macOS: `Piano.VST3` is a bundle there.
    pub fn of_extension(extension: &std::ffi::OsStr) -> Option<Self> {
        Self::of_str(&extension.to_str()?.to_ascii_lowercase())
    }
}

/// The plain name, such as `piano`, of `assets/plugin-state/<name>.bin`. Checked as it loads, so
/// a record can never point outside the project folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StateAsset(AssetName);

impl StateAsset {
    pub fn new(name: &str) -> Result<Self, InvalidAssetName> {
        Ok(Self(AssetName::new(STATE_FOLDER, name, STATE_EXTENSION)?))
    }

    pub fn name(&self) -> &str {
        self.0.name()
    }
}

/// A `state_asset` for a plugin the window adds: a name no file of the project has, such as
/// `six-sines-2`, reserved by making the file empty. So a new plugin never comes up holding the
/// state an older one left behind, and undo brings the older one back as it sounded.
pub fn new_state_asset(assets: &Assets, wanted: &str) -> Result<StateAsset, AssetError> {
    let base = AssetName::new(STATE_FOLDER, &asset_name_of(wanted), STATE_EXTENSION)?;
    let taken = assets.create(&base, &[])?;
    Ok(StateAsset(taken))
}

/// What an [`AssetName`] may hold, from a display name: lowercase letters, digits and `-`.
fn asset_name_of(display: &str) -> String {
    let mut name = String::new();
    for character in display.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            name.push(character);
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_end_matches('-');
    if name.is_empty() {
        FALLBACK_STATE_ASSET.to_string()
    } else {
        name.to_string()
    }
}

/// For a plugin whose name has nothing an asset name may hold.
const FALLBACK_STATE_ASSET: &str = "plugin";

impl TryFrom<String> for StateAsset {
    type Error = InvalidAssetName;

    fn try_from(name: String) -> Result<Self, InvalidAssetName> {
        Self::new(&name)
    }
}

impl From<StateAsset> for String {
    fn from(asset: StateAsset) -> String {
        asset.0.name().to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRecord {
    pub format: PluginFormat,
    /// The id its maker gave it, such as `com.u-he.diva`; for VST 3 the class id.
    pub plugin_id: String,
    /// The plugin's own state, opaque bytes.
    pub state_asset: StateAsset,
    /// Pins, by the plugin's id of each parameter. A pin wins over the state asset, and what
    /// the plugin changes of it itself is written back here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<u32, Pin>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    /// Only for the reader: the id counts, so a plugin that renames it still finds it.
    pub name: String,
    /// In the format's own units: CLAP's plain value, VST 3's 0 to 1.
    pub value: f64,
}

impl PluginRecord {
    /// The key of this plugin in a picker. The offers and the label of a placed plugin build it
    /// the same way, so a picker can mark the plugin that is already there.
    pub fn offer_key(format: PluginFormat, plugin_id: &str) -> String {
        format!("{}:{plugin_id}", format.name())
    }

    pub fn new(format: PluginFormat, plugin_id: &str, state_asset: &str) -> Option<Self> {
        Some(Self {
            format,
            plugin_id: plugin_id.to_string(),
            state_asset: StateAsset::new(state_asset).ok()?,
            parameters: BTreeMap::new(),
        })
    }

    fn asset(&self) -> AssetName {
        self.state_asset.0.clone()
    }
}

impl State for PluginRecord {
    const TOOL: &'static str = "plugin";

    fn validate(&self) -> Result<(), String> {
        if self.plugin_id.is_empty() {
            return Err("plugin_id must be the id the plugin's maker gave it, not empty".into());
        }
        // The id becomes a C string on the way to the plugin, which cannot hold a zero byte.
        if self.plugin_id.contains('\0') {
            return Err(format!(
                "plugin_id must not hold a zero byte, not {:?}",
                self.plugin_id
            ));
        }
        // So an agent that writes anything else is told here, not by a plugin that is not found.
        if self.format == PluginFormat::Vst3 && vst3::class_id_of(&self.plugin_id).is_none() {
            return Err(format!(
                "a vst3 plugin_id is the class id as thirty-two hex digits, not {:?}. `sound-tools --plugins` prints them",
                self.plugin_id
            ));
        }
        // As many as lanes can move of one device.
        if self.parameters.len() > MAX_AUTOMATED {
            return Err(format!(
                "parameters holds {} pins, and a plugin record holds at most {MAX_AUTOMATED}",
                self.parameters.len()
            ));
        }
        Ok(())
    }
}

pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "plugins",
    group: "Devices",
    when: "A third-party plugin (CLAP, VST 3) on a track: add, set or automate one, or fix its problem",
    markdown: include_str!("../agent-doc.md"),
};

pub fn register(registry: &mut Registry, plugins: Plugins) -> Result<(), RegistryError> {
    registry
        .tool::<PluginRecord>(EXTENSION)?
        .behaviour(move |state, context| apply(&plugins, state, context));
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Never fails for a plugin that does not load: the problem is reported and the rest of the
/// project plays.
fn apply(
    plugins: &Plugins,
    state: &PluginRecord,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    // A processor made in this run has no plugin, whatever the host holds for the record: the
    // record came back, by an undo of its delete, before the host let go of its plugin.
    let mut made = false;
    let node = context.processor(PROCESSOR, || {
        made = true;
        HostedPlugin::silent()
    })?;
    context.input(NOTES_INPUT, InputEndpoint::new(node, HostedPlugin::NOTES));
    // All three ports, whatever the plugin is: one record serves as an instrument and as an
    // effect.
    context.input(AUDIO_INPUT, InputEndpoint::new(node, HostedPlugin::INPUT));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(node, HostedPlugin::AUDIO));
    let config = context.prepare_config();
    match plugins.open(context.id(), state, context.assets(), config, !made) {
        Ok(opened) => {
            let lanes = AutomatedPins::new(&opened.lanes);
            let numbers: Vec<_> = opened.lanes.iter().filter_map(number).collect();
            // With none, the owner says the record takes no automation at all.
            if !numbers.is_empty() {
                let input = InputEndpoint::new(node, HostedPlugin::AUTOMATION);
                context.runtime_automation(input, numbers)?;
            }
            let update = match opened.engine {
                host::ForEngine::Play(started) => HostedUpdate::Plugin(started, lanes),
                host::ForEngine::Same => HostedUpdate::Pins(lanes),
            };
            context.update(node, update)?;
            for note in opened.notes {
                context.problem(note.to_string());
            }
        }
        Err(problem) => {
            // Whatever played here before stops, instead of going on with the old plugin.
            context.update(node, HostedUpdate::Plugin(None, AutomatedPins::NONE))?;
            context.problem(problem.to_string());
        }
    }
    Ok(())
}

fn number(pin: &AutomatedPin) -> Option<(ParameterInfo, f32)> {
    let info = ParameterInfo {
        field: lane_of_pin(pin.id).into(),
        range: ValueRange::linear(pin.minimum as f32, pin.maximum as f32),
    };
    pin.takes_lane.then_some((info, pin.record as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_asset_name_that_could_reach_outside_the_project_does_not_load() {
        let record = r#"{"format": "clap", "plugin_id": "a.b", "state_asset": "../secret"}"#;
        let error = serde_json::from_str::<PluginRecord>(record).expect_err("an error");
        assert!(error.to_string().contains("lowercase"), "{error}");
    }

    #[test]
    fn a_record_keeps_the_plain_name_of_its_state_asset() {
        let record = r#"{"format":"clap","plugin_id":"a.b","state_asset":"piano"}"#;
        let decoded: PluginRecord = serde_json::from_str(record).expect("a record");
        assert_eq!(decoded.state_asset.name(), "piano");
        assert_eq!(decoded.asset().as_str(), "plugin-state/piano.bin");
        assert_eq!(serde_json::to_string(&decoded).expect("json"), record);
    }

    /// The id of a pin is a number, which JSON holds as the text of a key.
    #[test]
    fn a_pin_is_read_by_the_id_of_its_parameter_and_written_back_the_same() {
        let record = r#"{"format":"clap","plugin_id":"a.b","state_asset":"piano","parameters":{"12":{"name":"Cutoff","value":440.0}}}"#;
        let decoded: PluginRecord = serde_json::from_str(record).expect("a record");
        let pin = Pin {
            name: "Cutoff".to_string(),
            value: 440.0,
        };
        assert_eq!(decoded.parameters, BTreeMap::from([(12, pin)]));
        assert_eq!(serde_json::to_string(&decoded).expect("json"), record);
    }
}
