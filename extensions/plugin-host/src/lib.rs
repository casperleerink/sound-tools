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
pub mod view;
mod window;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, AssetName, BehaviourContext, BehaviourError, InputEndpoint, InvalidAssetName,
    OutputEndpoint, Project, Registry, RegistryError, State,
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

/// A `state_asset` name that no plugin record of the project uses, from a display name such as
/// the plugin's own. Everything an asset name may not hold becomes `-`, and a name another
/// record already has gets a number: `six-sines`, `six-sines-2`.
///
/// Whoever writes a record by hand chooses the name themselves, and two records may share one.
/// This is for a record the window writes, where a shared name would be a surprise.
pub fn free_state_asset(project: &Project, wanted: &str) -> Result<StateAsset, InvalidAssetName> {
    let mut taken: Vec<&str> = Vec::new();
    for (id, tool) in project.instances() {
        if tool != PluginRecord::TOOL {
            continue;
        }
        let Some(instance) = project.resolve::<PluginRecord>(id) else {
            continue;
        };
        if let Some(record) = project.state(&instance) {
            taken.push(record.state_asset.name());
        }
    }
    let base = asset_name_of(wanted);
    // A project holds a finite number of records, so one of these names is free.
    let free = (1..=taken.len() + 1).find_map(|number| {
        let name = match number {
            1 => base.clone(),
            number => format!("{base}-{number}"),
        };
        (!taken.iter().any(|used| *used == name)).then_some(name)
    });
    StateAsset::new(free.as_deref().unwrap_or(&base))
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

/// The name a plugin whose own name has nothing an asset name may hold gets.
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
