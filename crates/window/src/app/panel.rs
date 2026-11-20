//! The left panel: one width, left of the main view, with a tab for the agent and one for the
//! history. What a tab shows is its own ([`PanelTab`]); the panel opens and closes, remembers
//! that for the machine, and gives the keys back to where they were.

use std::any::Any;
use std::path::PathBuf;

use rune::{Color, El, Icon, KeyEvent, Tx, col, el, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::State;

/// One width, so the main view keeps the width it was designed for.
const WIDTH: f32 = 400.0;
const HEADER: f32 = 56.0;
/// What a key focuses in the panel when its tab names nothing.
const BODY: &str = "left-panel-body";
/// What the icon that opens and closes the panel is called.
pub(super) const PANEL_LABEL: &str = "Side panel";

static PANEL_LEFT: Icon = Icon {
    name: "panel-left",
    view: 24.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/></g>"#,
};

/// What a tab of the panel shows, and what the window needs of it. The agent and the history
/// each implement it, and a [`LeftPanel`] holds them.
pub(crate) trait PanelTab: Any {
    fn view(&self, state: &State) -> El;
    /// What it puts right of the tabs while it shows, such as New.
    fn header(&self, _: &State) -> Option<El> {
        None
    }
    /// The key of what cmd-L focuses, such as the composer. Else the panel itself.
    fn focus(&self) -> Option<String> {
        None
    }
    /// One of its text fields has the keys: none of them is a shortcut then.
    fn typing(&self) -> bool {
        false
    }
    /// It works, or waits on the composer: the panel says so, also while closed.
    fn busy(&self) -> bool {
        false
    }
    /// Puts `words` into its composer, such as a merge to ask for. Whether it could.
    fn compose(&mut self, _words: &str) -> bool {
        false
    }
    /// Sends `message` as if typed. Whether it went.
    fn ask(&mut self, _message: &str, _project: &mut sound_core::Project) -> bool {
        false
    }
    /// cmd-.: stops what it does.
    fn stop(&mut self) {}
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tab {
    Agent,
    History,
}

impl Tab {
    pub(super) fn key(self) -> &'static str {
        match self {
            Tab::Agent => "left-panel-agent-tab",
            Tab::History => "left-panel-history-tab",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Tab::Agent => "Agent",
            Tab::History => "History",
        }
    }
}

pub(crate) struct LeftPanel {
    /// None without an agent: then the panel has History only, and starts closed.
    pub(crate) agent: Option<Box<dyn PanelTab>>,
    pub(crate) history: super::history::History,
    pub(crate) open: bool,
    pub(crate) tab: Tab,
    /// Remembers open or closed for every project, in the machine's support folder, never in a
    /// project. None remembers nothing, as in a run without a window.
    file: Option<PathBuf>,
    /// What had the keys before the panel took them, None for the window.
    back: Option<String>,
    /// The window's size, as the last layout found it.
    pub(super) window: (f32, f32),
    /// The size the window grows to as the panel opens, for the next frame to ask for.
    pub(super) widen: Option<(f32, f32)>,
}

impl LeftPanel {
    /// Open the first time, and as the machine remembers it after.
    pub(crate) fn new(agent: Option<Box<dyn PanelTab>>, file: Option<PathBuf>) -> Self {
        let open =
            agent.is_some() && (file.as_deref()).is_some_and(runtime::app::left_panel_was_open);
        Self {
            tab: if agent.is_some() {
                Tab::Agent
            } else {
                Tab::History
            },
            agent,
            history: super::history::History::default(),
            open,
            file,
            back: None,
            window: (0.0, 0.0),
            widen: None,
        }
    }

    /// The agent's tab as what it is, for its own handlers.
    pub(crate) fn agent_mut<T: PanelTab>(&mut self) -> Option<&mut T> {
        let agent: &mut dyn Any = self.agent.as_deref_mut()?;
        agent.downcast_mut()
    }

    fn shown(&self) -> &dyn PanelTab {
        match (self.tab, &self.agent) {
            (Tab::Agent, Some(agent)) => agent.as_ref(),
            _ => &self.history,
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.agent.as_ref().is_some_and(|agent| agent.busy())
    }

    pub(crate) fn tabs(&self) -> impl Iterator<Item = Tab> {
        (self.agent.is_some().then_some(Tab::Agent).into_iter()).chain([Tab::History])
    }
}

impl State {
    /// Opens or closes the panel, and remembers it for the machine. Opening widens a window
    /// too narrow for the panel and the main view; closing leaves its size.
    pub(crate) fn set_panel_open(&mut self, open: bool) {
        let wanted = super::MIN_SIZE.0 + WIDTH;
        let (width, height) = self.panel.window;
        // Before the first layout there is no window to widen yet.
        if open && !self.panel.open && width > 0.0 && width < wanted {
            self.panel.widen = Some((wanted, height));
        }
        self.panel.open = open;
        if let Some(file) = &self.panel.file
            && let Err(error) = runtime::app::remember_left_panel(file, open)
        {
            self.session.report(format!("{error:#}"));
        }
    }

    /// cmd-B and the icon: open or close, the keys staying where they are, unless they were in
    /// the panel, which gives them back.
    pub(super) fn open_or_close_panel(&mut self, inside: bool, tx: &mut Tx) {
        if self.panel.open && inside {
            self.give_keys_back(tx);
        }
        self.set_panel_open(!self.panel.open);
    }

    /// cmd-L: the Agent tab with the keys, or closed when it has them already. Nothing
    /// without an agent.
    pub(crate) fn agent_keys(&mut self, inside: bool, tx: &mut Tx) {
        let Some(agent) = &self.panel.agent else {
            return;
        };
        if self.panel.open && self.panel.tab == Tab::Agent && inside {
            self.set_panel_open(false);
            self.give_keys_back(tx);
            return;
        }
        let focus = agent.focus().unwrap_or_else(|| BODY.to_string());
        self.show_tab(Tab::Agent);
        if !inside {
            self.panel.back = tx.focused().map(str::to_string);
        }
        if !self.panel.open {
            self.set_panel_open(true);
        }
        tx.focus_when_shown(focus, true);
    }

    /// Escape in the panel, or closing it: the keys go back to where they were before the
    /// panel took them, else to the window, whose keys then work again.
    pub(super) fn give_keys_back(&mut self, tx: &mut Tx) {
        match self.panel.back.take() {
            Some(back) => tx.focus(back, false),
            None => tx.blur(),
        }
    }

    pub(crate) fn show_tab(&mut self, tab: Tab) {
        if self.panel.tabs().any(|shown| shown == tab) {
            self.panel.tab = tab;
        }
    }

    /// Asks the agent to fix the files that are not live: the panel opens on the agent, the
    /// message goes when the agent can take one, and its composer gets the keys.
    pub(crate) fn ask_to_fix(&mut self, tx: Option<&mut Tx>) {
        self.show_tab(Tab::Agent);
        if !self.panel.open {
            self.set_panel_open(true);
        }
        if let Some(agent) = self.panel.agent.as_mut() {
            (self.session).background(|project| agent.ask(sound_agent::FIX_NOT_LIVE, project));
        }
        if let Some(tx) = tx {
            self.agent_keys(false, tx);
        }
    }
}

/// The icon that opens and closes the panel. While the agent works, a lavender dot breathes.
pub(super) fn icon_button(state: &State) -> El {
    let kit = &state.theme.kit;
    let button = IconButton::new(&PANEL_LEFT)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label(PANEL_LABEL)
        .shortcut(super::keys::app_key_of(state, "left_panel").unwrap_or_default())
        .on_press_tx(|s: &mut State, tx: &mut Tx| {
            s.open_or_close_panel(false, tx);
            s.handled();
        })
        .build(kit)
        .key("left-panel-icon")
        .expanded(state.panel.open);
    let busy = (state.panel.busy() && !state.panel.open).then(|| {
        dot(state.theme.color("focus"))
            .key("left-panel-busy")
            .cycle(crate::sidebar::pulse())
            .absolute(20.0, 4.0)
    });
    el().child(button).children(busy)
}

fn dot(colour: Color) -> El {
    el().size(6.0, 6.0).radius(3.0).fill(colour).a11y_hidden()
}

/// The panel while it is open: its tabs, then what the tab shows.
pub(super) fn view(state: &State) -> Option<El> {
    let panel = &state.panel;
    if !panel.open {
        return None;
    }
    let theme = &state.theme;
    let shown = panel.shown();
    let header = (panel.tab == Tab::Agent)
        .then(|| shown.header(state))
        .flatten();
    let tabs = row()
        .key("left-panel-tabs")
        .role("tablist")
        .h(HEADER)
        .px(theme.space(4))
        .gap(theme.space(1))
        .items_center()
        .children(panel.tabs().map(|tab| tab_button(state, tab)))
        .child(el().grow(1.0))
        .children(header);
    let body = col()
        .key(BODY)
        .focusable(true)
        .no_focus_ring()
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(shown.view(state));
    Some(
        col()
            .key("left-panel")
            .w(WIDTH)
            .h_full()
            .border_sides([1.0, 1.0, 0.0, 0.0], theme.color("border"))
            .on_key(|s: &mut State, key: &KeyEvent, tx: &mut Tx| {
                let done = super::keys::panel_key(s, key, true, tx);
                if done {
                    s.handled();
                }
                done
            })
            .child(tabs)
            .child(body),
    )
}

fn tab_button(state: &State, tab: Tab) -> El {
    let kit = &state.theme.kit;
    let chosen = state.panel.tab == tab;
    let ink = if chosen {
        kit.c.text_base
    } else {
        kit.c.text_muted
    };
    let busy = (tab == Tab::Agent && state.panel.busy())
        .then(|| dot(state.theme.color("focus")).key("left-panel-agent-busy"));
    let e = row()
        .key(tab.key())
        .role("tab")
        .label(tab.label())
        .checked(chosen)
        .h(28.0)
        .px(state.theme.space(3))
        .gap(state.theme.space(2))
        .items_center()
        .radius(kit.radius.md)
        .fill(if chosen {
            kit.c.hover
        } else {
            Color::TRANSPARENT
        })
        .focusable(true)
        .on_press(move |s: &mut State| {
            s.show_tab(tab);
            s.handled();
        })
        .child(text(tab.label()).font(kit.ty.ui_medium).color(ink))
        .children(busy);
    kit.interactive(e)
}
