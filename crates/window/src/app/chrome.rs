//! What the window draws around the main view: the title row and the notice.

use rune::{Cx, El, col, el, row, text};

use crate::tree;

use super::State;

const TITLE_ROW: f32 = 44.0;
/// The room the title row leaves at its left for the window's buttons, which macOS draws on it.
const TITLE_LEFT: f32 = if cfg!(target_os = "macos") {
    88.0
} else {
    16.0
};
/// Where macOS draws the window's buttons: in the middle of the title row.
pub(crate) const WINDOW_BUTTONS: (f32, f32) = (16.0, 15.0);

pub(crate) fn view(state: &State, cx: &mut Cx<State>) -> El {
    let theme = &state.theme;
    let body = match state.main() {
        Some(main) => match state.drawn(main) {
            _ if state.code_failed(main) => el()
                .padding(theme.space(4))
                .child(tree::code_failed(state, main)),
            // A device's card on its own has the page's margin.
            Some(drawn) if state.cards.contains_key(main) => el()
                .padding(theme.space(4))
                .child(tree::view(state, main, "", drawn)),
            Some(drawn) => tree::view(state, main, "", drawn),
            None => el(),
        },
        None => text("This project has no main tool.")
            .font(theme.text("body"))
            .color(theme.color("text-muted"))
            .padding(theme.space(4)),
    };
    let notice = (state.notice.iter()).map(|notice| {
        text(notice.clone())
            .font(theme.text("label"))
            .color(theme.color("peach"))
            .absolute(theme.space(3), 0.0)
            .pin(None, None, Some(theme.space(3)), Some(theme.space(2)))
    });
    cx.focus_ring(2.0, 2.0, theme.color("focus"));
    col()
        .w_full()
        .h_full()
        .fill(theme.color("background"))
        .child(title_row(state))
        .child(el().grow(1.0).w_full().clip_soft(0.0).child(body))
        .children(notice)
        .children(tree::overlays(state))
}

fn title_row(state: &State) -> El {
    let theme = &state.theme;
    let label = if state.playing { "Stop" } else { "Play" };
    let play = row()
        .key("play")
        .role("button")
        .label(label)
        .focusable(true)
        .h(28.0)
        .px(theme.space(3))
        .items_center()
        .radius(theme.radius("md").unwrap_or(6.0))
        .fill(theme.color(if state.playing { "green" } else { "fill" }))
        .on_press(|s: &mut State| {
            s.play_or_stop();
            s.handled();
        })
        .child(
            text(if state.playing { "■" } else { "▶" })
                .font(theme.text("label"))
                .color(theme.color(if state.playing {
                    "text-inverse"
                } else {
                    "text"
                })),
        );
    // The tools' problems are the project's too.
    let problems = state.project.problems().len();
    let warning = (problems > 0).then(|| {
        text(format!("{problems} not live"))
            .font(theme.text("label"))
            .color(theme.color("peach"))
    });
    row()
        .w_full()
        .h(TITLE_ROW)
        .px(theme.space(4))
        .pl(TITLE_LEFT)
        .window_drag()
        .gap(theme.space(3))
        .items_center()
        .fill(theme.color("surface"))
        .border_sides([0.0, 0.0, 1.0, 0.0], theme.color("border-strong"))
        .child(
            text(state.name.clone())
                .font(theme.text("title"))
                .color(theme.color("text")),
        )
        .children(warning)
        .child(el().grow(1.0))
        .child(
            text("1.1")
                .key("time")
                .font(theme.text("number"))
                .color(theme.color("text-secondary"))
                .w(48.0)
                .live_text(&state.lives.time),
        )
        .child(play)
}
