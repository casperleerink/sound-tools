//! What the window draws around the main view: the title row, the left panel, the list of
//! files that are not live and the notice, and what of them `--tree` and `--act` see.

use anyhow::Result;
use rune::{Cx, El, Tx, col, el, row, text};
use rune_kit::{TooltipAppearance, TooltipStyle};
use serde_json::Value;

use super::State;
use super::panel::{self, Tab};
use super::{notices, problems, transport};
use crate::controls::{Control, Target};
use crate::tree;

const TITLE_ROW: f32 = 48.0;
/// The room the title row leaves at its left for the window's buttons, which macOS draws on it.
const TITLE_LEFT: f32 = if cfg!(target_os = "macos") { 80.0 } else { 8.0 };
/// Where macOS draws the window's buttons: in the middle of the title row.
pub(crate) const WINDOW_BUTTONS: (f32, f32) = (16.0, 17.0);
/// The smallest window: the project's name beside a page with room for a card.
pub(crate) const MIN_SIZE: (f32, f32) = (1100.0, 640.0);

pub(crate) fn view(state: &State, cx: &mut Cx<State>) -> El {
    let theme = &state.theme;
    let body = match state.main() {
        Some(main) => match state.drawn(main) {
            _ if state.code_failed(main) => el()
                .padding(theme.space(4))
                .child(tree::code_failed(state, main)),
            // A device's card on its own has the page's margin.
            Some(drawn) if (state.tools.as_ref()).is_some_and(|tools| tools.draws_card(main)) => {
                el().padding(theme.space(4))
                    .child(tree::view(state, main, "", drawn))
            }
            Some(drawn) => tree::view(state, main, "", drawn),
            None => el(),
        },
        None => super::projects::empty(state),
    };
    cx.focus_ring(2.0, 2.0, theme.color("focus"));
    cx.help_look(TooltipStyle::from_kit(&theme.kit, TooltipAppearance::Compact).help_look());
    // The panel comes before the main view, so Tab reaches it first, as it reads.
    let main = row()
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .children(panel::view(state))
        // Keyed, so what has the keys in it keeps them as the panel comes and goes.
        .child(
            el().key("main")
                .grow(1.0)
                .h_full()
                .clip_soft(0.0)
                .child(body),
        );
    let base = col()
        .w_full()
        .h_full()
        .fill(theme.color("background"))
        .child(title_row(state))
        .child(main)
        .children(notices::view(state));
    let overlays = tree::overlays(state)
        .into_iter()
        .chain(problems::list(state))
        .chain(super::dialog::view(state))
        .chain(super::export::view(state))
        .chain(crate::shell::overlay(state, theme));
    // On the root, which hears the key ups when nothing has the focus.
    rune_kit::root(base, overlays).on_key_up(super::keys::on_key_up)
}

fn title_row(state: &State) -> El {
    let theme = &state.theme;
    // Only the bare parts move the window: a drag that starts on a control is the control's.
    let drag = || el().h_full().window_drag();
    // The transport stands in the middle of the room right of the project's name.
    row()
        .w_full()
        .h(TITLE_ROW)
        .pr(theme.space(4))
        .gap(theme.space(2))
        .items_center()
        .child(drag().w(TITLE_LEFT))
        .child(panel::icon_button(state))
        .child(
            text(state.name.clone())
                .font(theme.text("title"))
                .color(theme.color("text"))
                .px(theme.space(2)),
        )
        .children(problems::sign(state))
        .child(drag().grow(1.0))
        .child(transport::view(state))
        .child(drag().grow(1.0))
}

/// What runs an action of a control of the chrome.
#[derive(Clone, Copy)]
pub(crate) enum Act {
    Transport(transport::Act),
    Dismiss,
    Panel,
    Tab(Tab),
    Problems,
    AskToFix,
    Shell(crate::shell::Act),
    Export(super::export::Act),
}

impl Act {
    /// The action as an agent runs it, with the value it gives: with no window, the keys stay
    /// where they are.
    pub(crate) fn run(self, state: &mut State, value: Option<Value>) -> Result<()> {
        match self {
            Act::Transport(act) => act.run(state, value)?,
            Act::Dismiss => state.session.dismiss_notice(),
            Act::Panel => state.set_panel_open(!state.panel.open),
            Act::Tab(tab) => state.show_tab(tab),
            Act::Problems => state.toggle_problems(),
            Act::AskToFix => {
                state.problems_open = false;
                state.ask_to_fix(None::<&mut Tx>);
            }
            Act::Shell(act) => crate::shell::run(state, act, None, &mut Tx::default()),
            Act::Export(act) => state.export_act(act),
        }
        Ok(())
    }
}

/// The controls of the chrome, for `--tree` and `--act`.
pub(crate) fn controls(state: &State) -> Vec<Control> {
    let control = |key: &str, label: &str, role: &str, act: Act| Control {
        key: key.into(),
        label: label.into(),
        role: Some(role.into()),
        value: None,
        checked: None,
        expanded: None,
        actions: vec!["press".into()],
        target: Target::Chrome(act),
    };
    let panel = &state.panel;
    let mut controls = transport::controls(state);
    controls.extend([Control {
        expanded: Some(panel.open),
        ..control("left-panel-icon", panel::PANEL_LABEL, "button", Act::Panel)
    }]);
    if panel.open {
        controls.extend(panel.tabs().map(|tab| Control {
            checked: Some(panel.tab == tab),
            ..control(tab.key(), tab.label(), "tab", Act::Tab(tab))
        }));
    }
    let files = state.files_not_live();
    if files > 0 {
        controls.push(Control {
            expanded: Some(state.problems_open),
            ..control("problems", &problems::title(files), "button", Act::Problems)
        });
    }
    if state.problems_open && panel.agent.is_some() {
        controls.push(control(
            "problems-ask",
            problems::ask_words(files),
            "button",
            Act::AskToFix,
        ));
    }
    controls.extend(notices::controls(state));
    controls.extend(super::projects::controls(state));
    controls.extend(super::export::controls(state));
    controls
}
