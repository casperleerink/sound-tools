//! The window's own dialog for a question of code: over everything, Cancel and the yes button,
//! Enter says yes and Escape Cancel.

use rune::{El, col, el, row, text};

use crate::controls::{Control, Target};

use super::State;

pub(crate) const CONFIRM: &str = "dialog-confirm";
pub(crate) const CANCEL: &str = "dialog-cancel";

pub(crate) fn view(state: &State) -> Option<El> {
    let asked = state.waiting.asked.front()?;
    let theme = &state.theme;
    let button = |key: &'static str, label: &str, primary: bool| {
        row()
            .key(key)
            .role("button")
            .label(label.to_string())
            .focusable(true)
            .h(28.0)
            .px(theme.space(3))
            .items_center()
            .radius(theme.radius("md").unwrap_or(6.0))
            .fill(theme.color(if primary { "on" } else { "fill" }))
            .on_press(move |state: &mut State| state.answer_dialog(primary))
            .child(
                text(label.to_string())
                    .font(theme.text("label"))
                    .color(theme.color(if primary { "text-inverse" } else { "text" })),
            )
    };
    let detail = (asked.detail.iter()).map(|detail| {
        text(detail.clone())
            .font(theme.text("body"))
            .color(theme.color("text-secondary"))
    });
    let card = col()
        .key("dialog")
        .role("alertdialog")
        .label(asked.message.clone())
        .w(400.0)
        .padding(theme.space(4))
        .gap(theme.space(3))
        .fill(theme.color("raised"))
        .radius(theme.radius("lg").unwrap_or(10.0))
        .border(1.0, theme.color("border-strong"))
        .modal(false, |state: &mut State| state.answer_dialog(false))
        .child(
            text(asked.message.clone())
                .font(theme.text("heading"))
                .color(theme.color("text")),
        )
        .children(detail)
        .child(
            row()
                .w_full()
                .justify_end()
                .gap(theme.space(2))
                .child(button(CANCEL, "Cancel", false))
                .child(
                    button(CONFIRM, &asked.confirm, true)
                        .default_button()
                        .autofocus(),
                ),
        );
    Some(
        el().absolute(0.0, 0.0)
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .items_center()
            .justify_center()
            .child(card),
    )
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
