//! The sidebar's menus. In the composer: how much the agent may do without asking, a setting
//! of the machine (see
//! `crate::settings`). In the header: **New**, the recent threads, and the instructions.

use gpui::SharedString;
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};

use super::instructions::Scope;
use crate::ApprovalMode;
use crate::settings::Settings;
use crate::store::RecentThread;

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
            Choice::NewThread => "new-thread".into(),
            Choice::OpenThread(id) => format!("thread-{id}").into(),
            Choice::Instructions(Scope::Project) => "instructions-project".into(),
            Choice::Instructions(Scope::AllProjects) => "instructions-all-projects".into(),
        }
    }

    pub(super) fn of(value: &str) -> Option<Choice> {
        match value {
            "new-thread" => return Some(Choice::NewThread),
            "instructions-project" => return Some(Choice::Instructions(Scope::Project)),
            "instructions-all-projects" => return Some(Choice::Instructions(Scope::AllProjects)),
            _ => {}
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

/// The name in the menu's values, what the composer reads, and what it means.
fn approval_text(mode: ApprovalMode) -> (&'static str, &'static str, &'static str) {
    match mode {
        ApprovalMode::AskForEverything => (
            "ask-for-everything",
            "Ask always",
            "Asks before every edit and command, except reading the project.",
        ),
        ApprovalMode::AskBeforeCommands => (
            "ask-before-commands",
            "Ask for commands",
            "Edits freely, asks before commands, except reading the project.",
        ),
        ApprovalMode::NeverAsk => (
            "never-ask",
            "Full access",
            "Does anything without asking. Undo is your safety net.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_reads_back_as_its_choice() {
        let choices = APPROVAL_MODES.map(Choice::ApprovalMode).into_iter().chain([
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
}
