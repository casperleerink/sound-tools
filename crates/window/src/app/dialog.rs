//! The window's own dialog for a question of code: over everything, Cancel and the yes button,
//! Enter says yes and Escape Cancel.

use rune::El;
use rune_kit::ButtonVariant;

use crate::controls::{Control, Target};
use crate::shell::dialog::{self, Dialog};

use super::State;

pub(crate) const CONFIRM: &str = "dialog-confirm";
pub(crate) const CANCEL: &str = "dialog-cancel";

pub(crate) fn view(state: &State) -> Vec<El> {
    let Some(asked) = state.waiting.asked.front() else {
        return Vec::new();
    };
    let theme = &state.theme;
    let button = |key: &'static str, label: &str, variant, said: bool| {
        dialog::button(
            theme,
            key,
            label,
            variant,
            true,
            move |state: &mut State, _| {
                state.answer_dialog(said);
            },
        )
    };
    Dialog::<State> {
        key: "dialog",
        title: asked.message.clone(),
        description: asked.detail.clone(),
        body: Vec::new(),
        note: None,
        buttons: vec![
            button(CANCEL, "Cancel", ButtonVariant::Neutral, false),
            button(CONFIRM, &asked.confirm, ButtonVariant::Contrast, true).autofocus(),
        ],
        close: |state| state.answer_dialog(false),
    }
    .build(theme)
}

/// The buttons of the dialog, as `--tree` lists them and `--act` presses them.
pub(crate) fn controls(state: &State) -> Vec<Control> {
    let Some(asked) = state.waiting.asked.front() else {
        return Vec::new();
    };
    let button = |key: &str, label: &str, says: bool| Control {
        key: key.to_string(),
        label: label.to_string(),
        role: Some("button".to_string()),
        value: None,
        checked: None,
        expanded: None,
        actions: vec!["press".to_string()],
        target: Target::Dialog(says),
    };
    vec![
        button(CANCEL, "Cancel", false),
        button(CONFIRM, &asked.confirm, true),
    ]
}
