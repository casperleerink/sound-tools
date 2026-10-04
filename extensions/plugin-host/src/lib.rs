//! Plugin host: third-party audio plugins as tools of a project, as the instrument of a track
//! and as effects after it.
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
//! It has three ports of the note contract, an event input `notes`, a stereo input `audio` and
//! a stereo output `audio`, so one record fits the `instrument` child of a track like any other
//! instrument and an effect slot after it. Nothing here knows which slot it is in.
//!
//! Why it is built this way, where the code does not show it:
//!
//! - What a plugin says it is decides only which picker offers it. A record may name any
//!   plugin in either slot, because plugins get this wrong: Spectral Freeze calls itself an
//!   instrument and is an effect. A slot whose plugin is missing or failed passes its input
//!   through, so one missing effect never silences a track.
//! - The plugin's own handle belongs to the main thread and only its audio side goes to the
//!   engine, in both formats. So [`Plugins`] lives on the thread the project lives on, and the
//!   behaviour keeps an `Rc` of it.
//! - A plugin's state is opaque bytes in an asset, not project state. It is never an undo step,
//!   and nothing replaces it with a guess: a state the plugin could not give leaves the asset as
//!   it was.
//! - Loading a plugin runs its code, so the scan runs one child process per bundle. A scan of a
//!   real machine takes seconds, most of it VST 3 bundles loading and checking their licence,
//!   so it runs off the thread that draws and a cache of this machine remembers each bundle.
//! - A render tells the plugin it is offline, so a streaming sampler waits for its samples
//!   instead of playing silence. It also does the main-thread work of the host for every block,
//!   because some plugins stay silent until the host answers them.
//!
//! `agent-doc.md` is what an agent reads.

mod backend;
mod clap;
mod host;
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
    InvalidAssetName, MAX_AUTOMATED, OutputEndpoint, Registry, RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT, NOTES_INPUT};

use crate::processor::HostedPlugin;

pub use host::{PluginProblem, Plugins, WeakPlugins};
pub use parameters::{
    MAX_NAMED_STEPS, Parameter, ParameterValue, StepName, Steps, read_parameters,
};
pub use scan::{
    SCAN_ARGUMENT, ScanCache, ScanCommand, ScannedPlugin, default_search_paths, scan_one_bundle,
};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "plugin-host";

/// What Steinberg asks of anyone who writes "VST", from their VST usage guidelines, section
/// 15. It belongs in product credits and documentation, and wherever the VST Compatible Logo
/// does not fit. This program shows it in the picker that offers VST 3 plugins and prints it
/// under `runtime --plugins`; the agent doc carries it as well. A row of a menu is a place
/// where the logo does not fit, so the notice is shown instead. "VST" is never stylized and is
/// not in the product name.
pub const VST_TRADEMARK: &str =
    "VST is a registered trademark of Steinberg Media Technologies GmbH.";

/// The name the behaviour gives its one processor.
const PROCESSOR: &str = "plugin";

/// Where a plugin's own state is kept: `assets/plugin-state/<name>.bin`.
const STATE_FOLDER: &str = "plugin-state";
const STATE_EXTENSION: &str = "bin";

/// The plugin formats this build can host. Each is a backend behind this one record.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginFormat {
    Clap,
    Vst3,
}

impl PluginFormat {
    /// What a person reads. "VST" is Steinberg's trademark and is written as they ask, see
    /// [`VST_TRADEMARK`].
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

    /// The format of a bundle, from its file extension. This is how one list of search folders
    /// covers every format.
    pub fn of_extension(extension: &std::ffi::OsStr) -> Option<Self> {
        Self::of_str(extension.to_str()?)
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

/// A `state_asset` for a plugin that is being put on a track: a name no file of the project
/// has, reserved by making that file.
///
/// It is [`Assets::create`] and its numbering, the rule a raw take follows: `six-sines-1`,
/// `six-sines-2`. The file is never opened, so a new plugin can never come up holding the
/// sound an older one left behind, and undo brings the older one back as it sounded. The file
/// is empty until the plugin saves into it, and an empty one is read as nothing saved yet.
///
/// Whoever writes a record by hand chooses the name themselves, and two records may share one.
/// This is for a record the window writes, where a shared name would be a surprise.
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRecord {
    pub format: PluginFormat,
    /// The plugin's own id, the one its maker gave it, such as `com.u-he.diva`.
    pub plugin_id: String,
    /// Where the plugin's own state is kept. Opaque bytes: nobody edits that file by hand.
    pub state_asset: StateAsset,
    /// The parameters the record holds, by the plugin's own id of each. For these the record
    /// wins over the state asset: they are sent to the plugin after its state is loaded, and
    /// what the plugin changes of them itself is written back here. Every other parameter
    /// stays in the state asset only. Left out of the file when there are none, so a record
    /// from before pins is written as it was.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<u32, Pin>,
}

/// One parameter a record holds, see [`PluginRecord::parameters`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    /// What the plugin calls it, for whoever reads the record. The id is what counts, so a
    /// plugin that renames it still finds it.
    pub name: String,
    /// In the format's own units, as `--plugin-params` prints them: CLAP's plain value, VST
    /// 3's 0 to 1.
    pub value: f64,
}

impl PluginRecord {
    /// How an offer of this plugin is told from every other in a picker: the format and the
    /// plugin's own id. Whoever offers plugins and whoever names what is in a slot build it
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
        // A VST 3 plugin is named by its class id, which is always thirty-two hex digits. An
        // agent that writes anything else is told here and not by a plugin that is not found.
        if self.format == PluginFormat::Vst3 && vst3::class_id_of(&self.plugin_id).is_none() {
            return Err(format!(
                "a vst3 plugin_id is the class id as thirty-two hex digits, not {:?}. `sound-tools --plugins` prints them",
                self.plugin_id
            ));
        }
        // As many as automation lanes can move of one device, which is what pins are for.
        if self.parameters.len() > MAX_AUTOMATED {
            return Err(format!(
                "parameters holds {} pins, and a plugin record holds at most {MAX_AUTOMATED}",
                self.parameters.len()
            ));
        }
        Ok(())
    }
}

/// The doc of the plugin record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "plugins",
    when: "You put a third-party plugin on a track, set its parameters, or a plugin is reported as a problem",
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
    // A processor made in this run has no plugin, whatever the host holds for the record: the
    // record came back, by an undo of its delete, before the host let go of its plugin.
    let mut made = false;
    let node = context.processor(PROCESSOR, || {
        made = true;
        HostedPlugin::silent()
    })?;
    context.input(NOTES_INPUT, InputEndpoint::new(node, HostedPlugin::NOTES));
    // Every hosted plugin has all three ports, whatever the plugin is: one record serves an
    // instrument slot and an effect slot, and this extension knows about neither. An
    // instrument's audio input is connected to nothing and is silent.
    context.input(AUDIO_INPUT, InputEndpoint::new(node, HostedPlugin::INPUT));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(node, HostedPlugin::AUDIO));
    let config = context.prepare_config();
    match plugins.open(context.id(), state, context.assets(), config, !made) {
        Ok(opened) => {
            // A run that loaded a plugin hands it to the engine. Nothing here asks what the
            // engine already has, so an edit the project rejects leaves the engine and this
            // host as they were. A host that only lists opens nothing, and the slot is silent.
            // A run that only changed pins hands nothing: the engine keeps its plugin.
            if let host::ForEngine::Play(started) = opened.engine {
                context.update(node, started)?;
            }
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
