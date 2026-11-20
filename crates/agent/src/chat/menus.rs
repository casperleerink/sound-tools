//! The composer's two pills: the model the agent runs, and how much it may do without asking.
//! Both are settings of the machine (see `crate::settings`); the menus keep no pick of their
//! own.

use super::Chat;
use crate::{ApprovalAnswer, ApprovalMode, Model, Provider};

pub const SEARCH_MODELS: &str = "Search models";
/// The model pill with no agent signed in.
pub const NO_AGENT: &str = "No agent";

/// The answers to a question of the agent, in the order they show: solid, subtle, ghost.
pub const APPROVAL_ANSWERS: [(ApprovalAnswer, &str); 3] = [
    (ApprovalAnswer::Allow, "Allow"),
    (ApprovalAnswer::AllowForThread, "Allow for this thread"),
    (ApprovalAnswer::Deny, "Deny"),
];

/// In the order the menu lists them, from the most careful.
const APPROVAL_MODES: [ApprovalMode; 3] = [
    ApprovalMode::AskForEverything,
    ApprovalMode::AskBeforeCommands,
    ApprovalMode::NeverAsk,
];

impl ApprovalMode {
    /// What the composer reads, also on the pill, which always says the mode so it is never a
    /// surprise.
    pub fn label(self) -> &'static str {
        match self {
            ApprovalMode::AskForEverything => "Ask always",
            ApprovalMode::AskBeforeCommands => "Ask for commands",
            ApprovalMode::NeverAsk => "Full access",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ApprovalMode::AskForEverything => {
                "Asks before every edit and command, except reading the project."
            }
            ApprovalMode::AskBeforeCommands => {
                "Edits freely, asks before commands, except reading the project."
            }
            ApprovalMode::NeverAsk => "Does anything without asking. Undo is your safety net.",
        }
    }

    /// From the most closed.
    pub fn icon(self) -> &'static str {
        match self {
            ApprovalMode::AskForEverything => "shield",
            ApprovalMode::AskBeforeCommands => "lock",
            ApprovalMode::NeverAsk => "lock-open",
        }
    }
}

/// A row of the access menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessChoice {
    pub mode: ApprovalMode,
    pub checked: bool,
}

/// The models of one provider that is signed in, in the picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelGroup<'a> {
    pub provider: Provider,
    /// Why its models are off: a thread keeps its provider.
    pub off: Option<String>,
    /// The ones the search finds; none while it is off.
    pub models: Vec<ModelRow<'a>>,
    /// In place of the models when there are none.
    pub empty: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    /// Who serves it, else what it is.
    pub second: Option<&'a str>,
    /// The check: the composer's pick, or the default, which is first.
    pub picked: bool,
}

impl Chat {
    /// The rows of the access menu, the current one checked.
    pub fn access_choices(&self) -> [AccessChoice; 3] {
        let current = self.agents.settings().settings().approval_mode;
        APPROVAL_MODES.map(|mode| AccessChoice {
            mode,
            checked: mode == current,
        })
    }

    pub fn approval_mode(&self) -> ApprovalMode {
        self.agents.settings().settings().approval_mode
    }

    /// The provider the model pill shows the logo of, or `None` with no agent signed in.
    pub fn model_provider(&self) -> Option<Provider> {
        let provider = self.provider();
        self.agents.ready(provider).map(|_| provider)
    }

    /// What the model pill says: the model the agent runs. Until the provider lists its
    /// models, the id that is picked, or "Default".
    pub fn model_label(&self) -> String {
        let Some(provider) = self.model_provider() else {
            return NO_AGENT.to_string();
        };
        let settings = self.agents.settings();
        label(
            settings.settings().model(provider),
            settings.models(provider),
        )
    }

    /// The picker: each provider signed in, with its models whose name or source holds
    /// `query`.
    pub fn model_groups(&self, query: &str) -> Vec<ModelGroup<'_>> {
        let search = query.trim().to_lowercase();
        let locked = self.thread_provider();
        let settings = self.agents.settings();
        (self.agents.programs().iter())
            .filter(|program| program.is_ready())
            .map(|program| {
                let provider = program.provider;
                let off = locked
                    .filter(|locked| *locked != provider)
                    .map(|_| format!("{}: start a new thread to switch", provider.name()));
                let picked = settings.settings().model(provider);
                let models: Vec<ModelRow<'_>> = match off {
                    Some(_) => Vec::new(),
                    None => (settings.models(provider).iter().enumerate())
                        .filter(|(_, model)| found(model, &search))
                        .map(|(index, model)| ModelRow {
                            id: &model.id,
                            name: &model.name,
                            second: (model.source.as_deref()).or_else(|| {
                                (!model.description.is_empty()).then_some(&*model.description)
                            }),
                            picked: match picked {
                                Some(picked) => picked == model.id,
                                None => index == 0 && search.is_empty(),
                            },
                        })
                        .collect(),
                };
                let empty = match (&off, models.is_empty(), search.is_empty()) {
                    (None, true, true) => Some("No models yet."),
                    (None, true, false) => Some("No model matches."),
                    _ => None,
                };
                ModelGroup {
                    provider,
                    off,
                    models,
                    empty,
                }
            })
            .collect()
    }
}

fn found(model: &Model, search: &str) -> bool {
    let source = model.source.as_deref().unwrap_or_default();
    search.is_empty()
        || model.name.to_lowercase().contains(search)
        || source.to_lowercase().contains(search)
}

/// What the pill says for the models of one provider and its pick.
fn label(picked: Option<&str>, models: &[Model]) -> String {
    let model = match picked {
        Some(id) => models.iter().find(|model| model.id == *id),
        None => models.first(),
    };
    match (model, picked) {
        (Some(model), _) => model.short_name.clone(),
        (None, Some(id)) => id.to_string(),
        (None, None) => "Default".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_says_the_model_it_runs() {
        let model = |id: &str, short_name: &str| Model {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            short_name: short_name.to_string(),
            source: None,
        };
        let models = [model("default", "Opus 5.5"), model("haiku", "Haiku 4.5")];
        assert_eq!(label(None, &models), "Opus 5.5");
        assert_eq!(label(Some("haiku"), &models), "Haiku 4.5");
        // Before the provider lists its models.
        assert_eq!(label(None, &[]), "Default");
        assert_eq!(label(Some("haiku"), &[]), "haiku");
    }
}
