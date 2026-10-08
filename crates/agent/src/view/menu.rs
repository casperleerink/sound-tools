//! The sidebar's menus. In the composer: the model, with the account and **Sign out** under it,
//! and how much the agent may do without asking, both settings of the machine (see
//! `crate::settings`). In the header: **New**, the recent threads, and the instructions.

use gpui::{Hsla, SharedString, rgb};
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};

use super::instructions::Scope;
use crate::settings::Settings;
use crate::store::RecentThread;
use crate::{Account, ApprovalMode, Model, Provider};

/// In the order the menu lists them, from the most careful.
const APPROVAL_MODES: [ApprovalMode; 3] = [
    ApprovalMode::AskForEverything,
    ApprovalMode::AskBeforeCommands,
    ApprovalMode::NeverAsk,
];

/// Room for the descriptions of the models and approval modes on two lines.
pub(super) const WIDTH: f32 = 300.;

/// The groups scroll past this, for a provider with many models.
pub(super) const MAX_HEIGHT: f32 = 480.;

/// What a row of the menu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    ApprovalMode(ApprovalMode),
    /// One of the provider's models, or `None` for its default.
    Model(Option<String>),
    SignOut,
    NewThread,
    /// A recent thread of the project, by id.
    OpenThread(String),
    Instructions(Scope),
}

impl Choice {
    /// The row's value in the menu, which [`Choice::of`] reads back. A test finds the row as
    /// `menu-<value>`.
    pub(super) fn value(&self) -> SharedString {
        match self {
            Choice::ApprovalMode(mode) => format!("approval-{}", approval_text(*mode).0).into(),
            Choice::Model(Some(id)) => format!("model-{id}").into(),
            Choice::Model(None) => "model".into(),
            Choice::SignOut => "sign-out".into(),
            Choice::NewThread => "new-thread".into(),
            Choice::OpenThread(id) => format!("thread-{id}").into(),
            Choice::Instructions(Scope::Project) => "instructions-project".into(),
            Choice::Instructions(Scope::AllProjects) => "instructions-all-projects".into(),
        }
    }

    pub(super) fn of(value: &str) -> Option<Choice> {
        match value {
            "sign-out" => return Some(Choice::SignOut),
            "model" => return Some(Choice::Model(None)),
            "new-thread" => return Some(Choice::NewThread),
            "instructions-project" => return Some(Choice::Instructions(Scope::Project)),
            "instructions-all-projects" => return Some(Choice::Instructions(Scope::AllProjects)),
            _ => {}
        }
        if let Some(id) = value.strip_prefix("model-") {
            return Some(Choice::Model(Some(id.to_string())));
        }
        if let Some(id) = value.strip_prefix("thread-") {
            return Some(Choice::OpenThread(id.to_string()));
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
            "Ask always",
            "Asks before every edit and command, except plain reads like ls.",
        ),
        ApprovalMode::AskBeforeCommands => (
            "ask-before-commands",
            "Ask for commands",
            "Edits freely, asks before commands, except plain reads.",
        ),
        ApprovalMode::NeverAsk => (
            "never-ask",
            "Full access",
            "Does anything without asking. Undo and git are your safety net.",
        ),
    }
}

/// The icon of an approval mode, from the most closed.
fn approval_icon(mode: ApprovalMode) -> &'static str {
    match mode {
        ApprovalMode::AskForEverything => "shield",
        ApprovalMode::AskBeforeCommands => "lock",
        ApprovalMode::NeverAsk => "lock-open",
    }
}

/// The provider's logo and its brand color, before the model.
pub(super) fn logo(provider: Provider) -> (&'static str, Hsla) {
    match provider {
        Provider::Claude => ("claude", rgb(0xd97757).into()),
    }
}

/// The access menu's trigger: the mode's icon and short name.
pub(super) fn access_label(settings: &Settings) -> (&'static str, &'static str) {
    let mode = settings.approval_mode;
    (approval_icon(mode), approval_text(mode).1)
}

/// The approval modes, each with what it means.
pub(super) fn access_entries(settings: &Settings) -> Vec<MenuEntry> {
    let approvals = APPROVAL_MODES.map(|mode| {
        let (_, label, description) = approval_text(mode);
        row(
            Choice::ApprovalMode(mode),
            label,
            settings.approval_mode == mode,
        )
        .icon(approval_icon(mode))
        .description(description)
    });
    vec![MenuEntry::Group(MenuGroup::new().items(approvals))]
}

/// The menus keep no pick of their own: the settings say what is checked.
fn row(choice: Choice, label: &str, checked: bool) -> MenuItem {
    MenuItem::new(choice.value(), label.to_string())
        .selectable(false)
        .checked(checked)
}

/// **New**'s menu: a new thread, as the button beside it, the recent threads when there are
/// any, and the instructions. Those for all projects need a support folder to be kept in.
pub(super) fn new_entries(recent: &[RecentThread], all_projects: bool) -> Vec<MenuEntry> {
    let new = MenuItem::new(Choice::NewThread.value(), "New thread").selectable(false);
    let recent = (!recent.is_empty()).then(|| {
        let threads = recent.iter().map(|thread| {
            MenuItem::new(Choice::OpenThread(thread.id.clone()).value(), &thread.title)
                .selectable(false)
        });
        MenuItem::new("recent", "Open recent").submenu(threads)
    });
    let instructions = [
        (Scope::Project, "This project", true),
        (Scope::AllProjects, "All projects", all_projects),
    ]
    .map(|(scope, label, enabled)| {
        row(Choice::Instructions(scope), label, false).disabled(!enabled)
    });
    vec![
        MenuEntry::Group(MenuGroup::new().item(new).items(recent)),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().label("Instructions").items(instructions)),
    ]
}

/// What the trigger says: the model the agent runs. Until the provider lists its models, the
/// id that is picked, or "Default".
pub(super) fn label(settings: &Settings, models: &[Model]) -> String {
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

/// The provider's models under its name, then the account and **Sign out**. `models` are the
/// provider's, its default first, or none until it listed them.
pub(super) fn model_entries(
    provider: Provider,
    account: &Account,
    settings: &Settings,
    models: &[Model],
) -> Vec<MenuEntry> {
    let picked = settings.model.as_deref();
    let models: Vec<MenuItem> = if models.is_empty() {
        // The provider lists its models once signed in. Until then only the pick shows.
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
        MenuEntry::Group(MenuGroup::new().label(provider.name()).items(models)),
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
            Choice::NewThread,
            Choice::OpenThread("5f0c7a1e-0000-4000-8000-000000000000".to_string()),
            Choice::Instructions(Scope::Project),
            Choice::Instructions(Scope::AllProjects),
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
