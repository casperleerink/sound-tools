//! The composer's menu: how much the agent may do without asking, the model, and the account
//! with **Sign out**. Both selects are settings of the machine, see `crate::settings`.

use gpui::SharedString;
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};

use crate::settings::Settings;
use crate::{Account, ApprovalMode, Model};

/// In the order the menu lists them, from the most careful.
const APPROVAL_MODES: [ApprovalMode; 3] = [
    ApprovalMode::AskForEverything,
    ApprovalMode::AskBeforeCommands,
    ApprovalMode::NeverAsk,
];

/// Room for the descriptions of the approval modes on two lines.
pub const WIDTH: f32 = 300.;

/// The groups scroll past this, for a provider with many models.
pub const MAX_HEIGHT: f32 = 480.;

/// What a row of the menu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    ApprovalMode(ApprovalMode),
    /// One of the provider's models, or `None` for its default.
    Model(Option<String>),
    SignOut,
}

impl Choice {
    /// The row's value in the menu, which [`Choice::of`] reads back. A test finds the row as
    /// `menu-<value>`.
    fn value(&self) -> SharedString {
        match self {
            Choice::ApprovalMode(mode) => format!("approval-{}", approval_text(*mode).0).into(),
            Choice::Model(Some(id)) => format!("model-{id}").into(),
            Choice::Model(None) => "model".into(),
            Choice::SignOut => "sign-out".into(),
        }
    }

    pub fn of(value: &str) -> Option<Choice> {
        match value {
            "sign-out" => return Some(Choice::SignOut),
            "model" => return Some(Choice::Model(None)),
            _ => {}
        }
        if let Some(id) = value.strip_prefix("model-") {
            return Some(Choice::Model(Some(id.to_string())));
        }
        let name = value.strip_prefix("approval-")?;
        APPROVAL_MODES
            .into_iter()
            .find(|mode| approval_text(*mode).0 == name)
            .map(Choice::ApprovalMode)
    }
}

/// The name in the menu's values, what the composer reads, and what it means, as in the
/// plan's table.
fn approval_text(mode: ApprovalMode) -> (&'static str, &'static str, &'static str) {
    match mode {
        ApprovalMode::AskForEverything => (
            "ask-for-everything",
            "Ask for everything",
            "Asks before every edit and command, except plain reads like ls.",
        ),
        ApprovalMode::AskBeforeCommands => (
            "ask-before-commands",
            "Ask before commands",
            "Edits freely, asks before commands, except plain reads.",
        ),
        ApprovalMode::NeverAsk => (
            "never-ask",
            "Never ask",
            "Does anything without asking. Undo and git are your safety net.",
        ),
    }
}

/// What the trigger says: the model the agent runs. Until the provider lists its models, the
/// id that is picked, or "Default".
pub fn label(settings: &Settings, models: &[Model]) -> String {
    let picked = match &settings.model {
        Some(id) => models.iter().find(|model| model.id == *id),
        None => models.first(),
    };
    match (picked, &settings.model) {
        (Some(model), _) => model.short_name.clone(),
        (None, Some(id)) => id.clone(),
        (None, None) => "Default".to_string(),
    }
}

/// The approvals, the models, then the account and **Sign out**. `models` are the provider's,
/// its default first, or none while no agent has started yet.
pub fn entries(account: &Account, settings: &Settings, models: &[Model]) -> Vec<MenuEntry> {
    // The menu keeps no pick of its own: the settings say what is checked.
    let row = |choice: Choice, label: &str, checked: bool| {
        MenuItem::new(choice.value(), label.to_string())
            .selectable(false)
            .checked(checked)
    };
    let approvals = APPROVAL_MODES.map(|mode| {
        let (_, label, description) = approval_text(mode);
        row(
            Choice::ApprovalMode(mode),
            label,
            settings.approval_mode == mode,
        )
        .description(description)
    });
    let picked = settings.model.as_deref();
    let models: Vec<MenuItem> = if models.is_empty() {
        // The provider lists its models when the agent starts. Until then only the pick shows.
        vec![row(
            Choice::Model(picked.map(str::to_string)),
            picked.unwrap_or("Default"),
            true,
        )]
    } else {
        models
            .iter()
            .enumerate()
            .map(|(index, model)| {
                let checked = picked.map_or(index == 0, |picked| picked == model.id);
                row(Choice::Model(Some(model.id.clone())), &model.name, checked)
                    .description(model.description.clone())
            })
            .collect()
    };
    let who: Vec<&str> = [&account.email, &account.plan]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let account = MenuGroup::new().item(row(Choice::SignOut, "Sign out", false));
    let account = if who.is_empty() {
        account
    } else {
        account.label(who.join(" · "))
    };
    vec![
        MenuEntry::Group(MenuGroup::new().label("Approvals").items(approvals)),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().label("Model").items(models)),
        MenuEntry::Separator,
        MenuEntry::Group(account),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_reads_back_as_its_choice() {
        let choices = APPROVAL_MODES.map(Choice::ApprovalMode).into_iter().chain([
            Choice::Model(None),
            Choice::Model(Some("claude-opus-5-5".to_string())),
            Choice::SignOut,
        ]);
        for choice in choices {
            assert_eq!(Choice::of(&choice.value()), Some(choice));
        }
        assert_eq!(Choice::of("approval-sometimes"), None);
    }

    #[test]
    fn the_button_says_the_model_it_runs() {
        let model = |id: &str, short_name: &str| Model {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            short_name: short_name.to_string(),
        };
        let models = [model("default", "Opus 5.5"), model("haiku", "Haiku 4.5")];
        let picked = |model: Option<&str>| Settings {
            model: model.map(str::to_string),
            ..Settings::default()
        };
        assert_eq!(label(&picked(None), &models), "Opus 5.5");
        assert_eq!(label(&picked(Some("haiku")), &models), "Haiku 4.5");
        // Before the provider lists its models.
        assert_eq!(label(&picked(None), &[]), "Default");
        assert_eq!(label(&picked(Some("haiku")), &[]), "haiku");
    }
}
