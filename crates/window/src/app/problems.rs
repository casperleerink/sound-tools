//! The files that are not live: a warning sign with their count beside the project's name, for
//! as long as there are any. A press lists them, with what is wrong in each.

use std::collections::BTreeSet;

use rune::{Align, El, Icon, ScrollSpec, Side, Tx, col, icon, row, text};
use rune_kit::{Button, ButtonVariant};

use super::State;

const LIST_WIDTH: f32 = 400.0;
const LIST_HEIGHT: f32 = 360.0;

static TRIANGLE_ALERT: Icon = Icon {
    name: "triangle-alert",
    view: 24.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/></g>"#,
};

impl State {
    /// How many files are not live: a file with two problems is one.
    pub(crate) fn files_not_live(&self) -> usize {
        let problems = self.project().problems();
        let files: BTreeSet<&str> = problems
            .iter()
            .map(|problem| problem.path.as_str())
            .collect();
        files.len()
    }

    pub(crate) fn toggle_problems(&mut self) {
        self.problems_open = !self.problems_open && self.files_not_live() > 0;
    }
}

/// "1 file is not live", "3 files are not live".
pub(super) fn title(files: usize) -> String {
    match files {
        1 => "1 file is not live".to_string(),
        n => format!("{n} files are not live"),
    }
}

pub(super) fn ask_words(files: usize) -> &'static str {
    match files {
        1 => "Ask the agent to fix it",
        _ => "Ask the agent to fix them",
    }
}

/// The sign, while any file is not live.
pub(super) fn sign(state: &State) -> Option<El> {
    let files = state.files_not_live();
    if files == 0 {
        return None;
    }
    let theme = &state.theme;
    let peach = theme.color("warning");
    let e = row()
        .key("problems")
        .role("button")
        .label(title(files))
        .expanded(state.problems_open)
        .h(28.0)
        .px(theme.space(2))
        .gap(theme.space(1))
        .items_center()
        .radius(theme.kit.radius.md)
        .focusable(true)
        .on_press(|s: &mut State| {
            s.toggle_problems();
            s.handled();
        })
        .child(icon(&TRIANGLE_ALERT, 16.0).color(peach))
        .child(
            text(files.to_string())
                .font(theme.text("label"))
                .color(peach),
        );
    Some(theme.kit.interactive(e))
}

/// The list under the sign while it is open: a press outside or Escape closes it.
pub(super) fn list(state: &State) -> Option<El> {
    let files = state.files_not_live();
    if !state.problems_open || files == 0 {
        return None;
    }
    let theme = &state.theme;
    let kit = &theme.kit;
    let muted = theme.color("text-secondary");
    let ask = match state.panel.agent.is_some() {
        true => Button::new(ask_words(files))
            .variant(ButtonVariant::Neutral)
            .on_press_tx(|s: &mut State, tx: &mut Tx| {
                s.problems_open = false;
                s.ask_to_fix(Some(tx));
                s.handled();
            })
            .build(kit)
            .key("problems-ask"),
        false => text(ask_words(files))
            .font(theme.text("label"))
            .color(muted),
    };
    let header = row()
        .items_center()
        .justify_between()
        .gap(theme.space(2))
        .pb(theme.space(1))
        .child(
            text(title(files))
                .font(theme.text("body"))
                .color(theme.color("text")),
        )
        .child(ask);
    let problems = state.project().problems();
    let rows = problems.iter().map(|problem| {
        col()
            .py(6.0)
            .child(
                text(sound_agent::file_name(state.project(), &problem.path))
                    .font(theme.text("label"))
                    .color(muted),
            )
            .child(
                text(problem.message.clone())
                    .font(theme.text("body"))
                    .color(theme.color("text"))
                    .wrap(),
            )
    });
    let list = col()
        .key("problems-list")
        .role("dialog")
        .label(title(files))
        .w(LIST_WIDTH)
        .max_h(LIST_HEIGHT)
        .padding(theme.space(3))
        .radius(kit.radius.lg)
        .fill(theme.color("raised"))
        .border(1.0, theme.color("border-strong"))
        .child(header)
        .child(
            col()
                .min_h(0.0)
                .shrink()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .children(rows),
        );
    Some(
        kit.elevate(list, &kit.elevation.floating)
            .anchored("problems", Side::Bottom, Align::Start, 4.0)
            .modal(true, |s: &mut State| s.problems_open = false)
            .focus_return("problems"),
    )
}
