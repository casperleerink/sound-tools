//! Plugin host: third-party audio plugins as tools of a project. CLAP instruments for now.
//!
//! One tool, `plugin`. Its record says which format, which plugin and where the plugin's own
//! state is kept. A record on disk, usually `instrument.json` inside a track folder:
//!
//! ```json
//! {
//!   "tool": "plugin",
//!   "state": {"format": "clap", "plugin_id": "com.example.piano", "state_asset": "piano"}
//! }
//! ```
//!
//! It has the ports of the note contract, an event input `notes` and a stereo output `audio`,
//! so it fits the `instrument` child of a track like any other instrument.
//!
//! `README.md` in this crate is the guide, and `agent-doc.md` is what an agent reads.

mod host;
mod processor;
pub mod scan;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, AssetName, BehaviourContext, BehaviourError, InputEndpoint, InvalidAssetName,
    OutputEndpoint, Registry, RegistryError, State,
};
use sound_notes::{AUDIO_OUTPUT, NOTES_INPUT};

pub use host::{PluginProblem, Plugins, WeakPlugins};
pub use processor::HostedPlugin;
pub use scan::{SCAN_ARGUMENT, ScanCommand, ScannedPlugin, default_search_paths, scan_one_bundle};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "plugin-host";

/// The name the behaviour gives its one processor.
const PROCESSOR: &str = "plugin";

/// Where a plugin's own state is kept: `assets/plugin-state/<name>.bin`.
const STATE_FOLDER: &str = "plugin-state";
const STATE_EXTENSION: &str = "bin";

/// The plugin formats this build can host. VST3 joins it without changing the record.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginFormat {
    Clap,
}

impl PluginFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Clap => "CLAP",
        }
    }
}

/// The name of the file that holds the plugin's own state, under `assets/plugin-state/`.
///
/// It is saved as a plain name, such as `piano`, and checked while it loads, so a record can
/// never point outside the project folder.
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

/// The saved state of one hosted plugin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRecord {
    pub format: PluginFormat,
    /// The plugin's own id, the one its maker gave it, such as `com.u-he.diva`.
    pub plugin_id: String,
    /// Where the plugin's own state is kept. Opaque bytes: nobody edits that file by hand.
    pub state_asset: StateAsset,
}

impl PluginRecord {
    pub fn new(format: PluginFormat, plugin_id: &str, state_asset: &str) -> Option<Self> {
        Some(Self {
            format,
            plugin_id: plugin_id.to_string(),
            state_asset: StateAsset::new(state_asset).ok()?,
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
        Ok(())
    }
}

/// The doc of the plugin record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "plugins",
    when: "You put a third-party plugin on a track, or a plugin is reported as a problem",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the plugin tool. `plugins` is the host the runtime keeps and polls.
pub fn register(registry: &mut Registry, plugins: Plugins) -> Result<(), RegistryError> {
    registry
        .tool::<PluginRecord>(EXTENSION)?
        .behaviour(move |state, context| apply(&plugins, state, context));
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. It never fails for a plugin this machine
/// does not have: the record stays as it is, the track is silent and the problem is reported,
/// so everything else in the project plays.
fn apply(
    plugins: &Plugins,
    state: &PluginRecord,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let node = context.processor(PROCESSOR, HostedPlugin::silent)?;
    context.input(NOTES_INPUT, InputEndpoint::new(node, HostedPlugin::NOTES));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(node, HostedPlugin::AUDIO));
    let sample_rate = context.prepare_config().sample_rate;
    match plugins.open(context.id(), state, context.assets(), sample_rate) {
        Ok(opened) => {
            // Every run hands the engine a plugin. Nothing here asks what the engine already
            // has, so an edit the project rejects leaves the engine and this host as they were.
            context.update(node, Some(Box::new(opened.started)))?;
            for note in opened.notes {
                context.problem(note.to_string());
            }
        }
        Err(problem) => {
            // Whatever played here before stops, so a record that stops naming a plugin this
            // machine has goes silent instead of going on with the old one.
            context.update(node, None)?;
            context.problem(problem.to_string());
        }
    }
    Ok(())
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
}
