//! What a composer can put in a child of a rack: the devices catalog. The window fills it with
//! the built-in devices, the tools of the project and the plugins of this computer; code reads it
//! as `ctx.catalog` and puts one with `addOffer`.

use std::rc::Rc;

use serde::Serialize;
use sound_core::{Changes, InstanceId, Project, ProjectError};

/// Writes the record of an offer into a child, at its defaults.
pub type Write = Rc<dyn Fn(&Project, &InstanceId, &mut Changes) -> Result<(), ProjectError>>;

/// One thing a composer can choose for a child, as code gets it.
#[derive(Clone, Serialize)]
pub struct Offer {
    /// What `addOffer` names it by. Unique in the catalog.
    pub key: String,
    pub name: String,
    /// A quiet second line, such as who made the plugin.
    pub detail: Option<String>,
    pub group: Group,
    /// The icon a picker shows beside it, from the project's `ui/icons`.
    pub icon: String,
    pub takes: Takes,
    /// Why it cannot be put here, or `None` when it can.
    pub unavailable: Option<String>,
    #[serde(skip)]
    pub write: Option<Write>,
    /// The extension the project must enable to load what it writes, and why it is off else.
    #[serde(skip)]
    pub needs: Option<(&'static str, String)>,
}

/// The groups of a picker, in the order it lists them. Built-in instruments are one group;
/// built-in effects are grouped by what they do to the sound. Plugins come last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Group {
    #[serde(rename = "Built-in")]
    BuiltIn,
    Tone,
    Dynamics,
    Space,
    Mix,
    /// The tools a project wrote for itself.
    #[serde(rename = "This project")]
    Project,
    Plugins,
}

/// What the device of an offer takes in: notes, which it plays into sound, or sound, which it
/// changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Takes {
    Notes,
    Audio,
}

impl Offer {
    /// The offer of a tool known by name, at its defaults.
    pub fn of_tool(tool: &str, name: &str, group: Group, takes: Takes) -> Self {
        let written = tool.to_string();
        Self {
            key: tool.to_string(),
            name: name.to_string(),
            detail: None,
            group,
            icon: "device-plugin".to_string(),
            takes,
            unavailable: None,
            write: Some(Rc::new(move |project, slot, changes| {
                project.set_json(changes, slot.clone(), &written, serde_json::json!({}))
            })),
            needs: None,
        }
    }

    /// An offer that cannot be picked, with why in words, such as a plugin that did not load.
    pub fn unavailable(key: String, name: String, takes: Takes, reason: String) -> Self {
        Self {
            key,
            name,
            detail: None,
            group: Group::Plugins,
            icon: "device-plugin".to_string(),
            takes,
            unavailable: Some(reason),
            write: None,
            needs: None,
        }
    }

    /// Why it cannot be put in `project`, or `None` when it can: one whose extension the project
    /// does not enable shows, and says so.
    fn unavailable_in(&self, project: &Project) -> Option<String> {
        let enabled =
            |extension: &str| (project.project_file().extensions.iter()).any(|it| it == extension);
        (self.unavailable.clone()).or_else(|| match &self.needs {
            Some((extension, reason)) if !enabled(extension) => Some(reason.clone()),
            _ => None,
        })
    }

    /// Puts its record at `slot` in place of what is there, into `changes`, or says why not.
    pub fn write(
        &self,
        project: &Project,
        slot: &InstanceId,
        changes: &mut Changes,
    ) -> Result<(), String> {
        let write = match (self.unavailable_in(project), &self.write) {
            (Some(reason), _) => return Err(reason),
            (None, None) => return Err(format!("{} cannot be put in a device", self.name)),
            (None, Some(write)) => write,
        };
        if project.tool_of(slot).is_some() {
            changes.delete(slot);
        }
        write(project, slot, changes).map_err(|error| error.to_string())
    }
}

/// The catalog as `project` can take it: every offer, unavailable ones with why, by group and
/// in the order of their source within a group.
pub fn in_project(mut offers: Vec<Offer>, project: &Project) -> Vec<Offer> {
    for offer in &mut offers {
        offer.unavailable = offer.unavailable_in(project);
    }
    offers.sort_by_key(|offer| offer.group);
    offers
}

/// The built-in devices, in the order of a picker: the instruments, then the effects group by
/// group.
pub fn built_in() -> Vec<Offer> {
    use Group::{BuiltIn, Dynamics, Mix, Space, Tone};
    const DEVICES: [(&str, &str, &str, Group, &str); 17] = [
        (
            "instrument.synth",
            "instrument",
            "Synth",
            BuiltIn,
            "the synth",
        ),
        (
            "wavetable",
            "wavetable",
            "Wavetable",
            BuiltIn,
            "the Wavetable",
        ),
        ("sampler", "sampler", "Sampler", BuiltIn, "the sampler"),
        ("drum-pad", "drum-pad", "Drum pad", BuiltIn, "the Drum pad"),
        ("eq", "eq", "EQ", Tone, "the EQ"),
        ("filter", "filter", "Filter", Tone, "the filter"),
        ("saturator", "saturator", "Saturator", Tone, "the saturator"),
        ("vocoder", "vocoder", "Vocoder", Tone, "the vocoder"),
        (
            "pitch-correction",
            "pitch-correction",
            "Pitch correction",
            Tone,
            "the pitch correction",
        ),
        (
            "compressor",
            "compressor",
            "Compressor",
            Dynamics,
            "the compressor",
        ),
        ("gate", "gate", "Gate", Dynamics, "the gate"),
        ("limiter", "limiter", "Limiter", Dynamics, "the limiter"),
        (
            "modulation",
            "modulation",
            "Modulation",
            Space,
            "the modulation",
        ),
        ("delay", "delay", "Delay", Space, "the delay"),
        ("reverb", "reverb", "Reverb", Space, "the reverb"),
        ("utility", "utility", "Utility", Mix, "the utility"),
        ("analyzer", "analyzer", "Analyzer", Mix, "the analyzer"),
    ];
    (DEVICES.into_iter())
        .map(|(tool, extension, name, group, what)| {
            let takes = match group {
                BuiltIn => Takes::Notes,
                _ => Takes::Audio,
            };
            let icon = format!("device-{}", tool.trim_start_matches("instrument."));
            Offer {
                icon,
                needs: Some((extension, format!("This project does not load {what}."))),
                ..Offer::of_tool(tool, name, group, takes)
            }
        })
        .collect()
}
