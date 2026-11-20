//! The frame of the window: the side panel and the list of files that are not live, driven by
//! key and label.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use rune::{El, Key, KeyEvent};

use super::super::panel::{LeftPanel, PanelTab, Tab};
use super::*;
use crate::app::MAIN_MODIFIER;

/// An agent that works while `busy`, has a composer, and keeps what it was asked.
#[derive(Default)]
struct Agent {
    busy: bool,
    asked: Rc<RefCell<Vec<String>>>,
}

impl PanelTab for Agent {
    fn view(&self, _: &State) -> El {
        rune::el().key("composer").focusable(true).h(40.0).w(200.0)
    }

    fn focus(&self) -> Option<String> {
        Some("composer".into())
    }

    fn busy(&self) -> bool {
        self.busy
    }

    fn ask(&mut self, message: &str, _: &mut sound_core::Project) -> bool {
        self.asked.borrow_mut().push(message.into());
        true
    }
}

/// The window of a new project, its panel with `agent` and remembered in `file`.
fn framed(
    scratch: &tempfile::TempDir,
    agent: Option<Agent>,
    file: Option<PathBuf>,
) -> rune::Runtime<State> {
    let mut state = opened(&scratch.path().join("song"));
    let agent = agent.map(|agent| Box::new(agent) as Box<dyn PanelTab>);
    state.panel = LeftPanel::new(agent, file);
    let mut window = laid_out(state);
    redraw(&mut window);
    window
}

/// cmd and the key `c`.
fn chord(window: &mut rune::Runtime<State>, c: char) {
    let mods = MAIN_MODIFIER;
    window.key(KeyEvent {
        key: Key::Command(c),
        mods,
    });
    redraw(window);
}

/// The warning sign of the files that are not live, as an agent sees it.
fn sign(state: &State) -> controls::Control {
    let mut controls = chrome_controls(state).into_iter();
    controls.find(|control| control.key == "problems").unwrap()
}

fn open(window: &rune::Runtime<State>) -> bool {
    let open = window.state.panel.open;
    assert_eq!(open, window.find("left-panel").is_some());
    open
}

#[test]
fn the_panel_is_open_until_closed_and_cmd_l_takes_the_keys_to_the_agent_and_back() {
    let (scratch, machine) = (new_project(), tempfile::tempdir().unwrap());
    let file = machine.path().join("left-panel");
    let mut window = framed(&scratch, Some(Agent::default()), Some(file.clone()));
    assert!(open(&window));
    tab_to(&mut window, "track-t00");

    chord(&mut window, 'l');
    assert_eq!(focused(&window).as_deref(), Some("composer"));
    key(&mut window, Key::Escape);
    assert!(open(&window));
    assert_eq!(focused(&window).as_deref(), Some("track-t00"));

    // From the agent, cmd-L closes the panel and gives the keys back.
    chord(&mut window, 'l');
    chord(&mut window, 'l');
    assert!(!open(&window));
    assert_eq!(focused(&window).as_deref(), Some("track-t00"));

    // Another window: closed, as the machine remembers; cmd-L opens it on the composer.
    let mut window = framed(&scratch, Some(Agent::default()), Some(file));
    assert!(!open(&window));
    chord(&mut window, 'l');
    assert!(open(&window));
    assert_eq!(focused(&window).as_deref(), Some("composer"));
}

#[test]
fn cmd_l_goes_to_the_agent_from_history_and_cmd_b_opens_on_the_tab_it_showed() {
    let scratch = new_project();
    let mut window = framed(&scratch, Some(Agent::default()), None);
    chord(&mut window, 'b');
    tab_to(&mut window, "left-panel-history-tab");
    key(&mut window, Key::Text(" ".into()));
    assert_eq!(window.state.panel.tab, Tab::History);

    // On History, cmd-L goes to the agent and leaves the panel open.
    chord(&mut window, 'l');
    assert_eq!(window.state.panel.tab, Tab::Agent);
    assert!(open(&window));
    assert_eq!(focused(&window).as_deref(), Some("composer"));

    click(&mut window, "left-panel-history-tab");
    chord(&mut window, 'b');
    assert!(!open(&window));
    chord(&mut window, 'b');
    assert!(open(&window));
    assert_eq!(window.state.panel.tab, Tab::History);
}

#[test]
fn cmd_b_leaves_the_keys_where_they_are_unless_they_were_in_the_panel() {
    let scratch = new_project();
    let mut window = framed(&scratch, Some(Agent::default()), None);
    tab_to(&mut window, "track-t00");
    chord(&mut window, 'b');
    assert!(open(&window));
    assert_eq!(focused(&window).as_deref(), Some("track-t00"));

    chord(&mut window, 'l');
    chord(&mut window, 'b');
    assert!(!open(&window));
    assert_eq!(focused(&window).as_deref(), Some("track-t00"));
}

#[test]
fn opening_the_panel_widens_a_window_too_narrow_for_it_and_closing_keeps_the_size() {
    let scratch = new_project();
    let mut window = framed(&scratch, Some(Agent::default()), None);
    chord(&mut window, 'b');
    assert_eq!(window.state.panel.widen.take(), Some((1500.0, 920.0)));
    chord(&mut window, 'b');
    assert_eq!(window.state.panel.widen, None);
    window.set_viewport((1600.0, 900.0), 1.0);
    redraw(&mut window);
    window.frame(1.0 / 60.0);
    chord(&mut window, 'b');
    assert!(open(&window));
    assert_eq!(window.state.panel.widen, None);
}

#[test]
fn a_closed_panel_shows_the_agent_working_on_its_icon() {
    let scratch = new_project();
    let busy = Agent {
        busy: true,
        ..Agent::default()
    };
    let mut window = framed(&scratch, Some(busy), None);
    let opacity = |window: &rune::Runtime<State>| {
        let elements = window.elements().into_iter();
        let mut found =
            elements.filter(|element| element.key.as_deref() == Some("left-panel-busy"));
        found.next().unwrap().opacity
    };
    let mut seen = Vec::new();
    for _ in 0..12 {
        window.frame(0.1);
        seen.push(opacity(&window));
    }
    assert!(
        seen.iter().any(|opacity| *opacity < 0.9),
        "it breathes: {seen:?}"
    );
    chord(&mut window, 'b');
    assert!(window.find("left-panel-busy").is_none());
    assert!(window.find("left-panel-agent-busy").is_some());

    let mut idle = framed(&scratch, Some(Agent::default()), None);
    assert!(idle.find("left-panel-busy").is_none());
    chord(&mut idle, 'b');
    assert!(idle.find("left-panel-agent-busy").is_none());
}

#[test]
fn without_an_agent_the_panel_has_history_only_starts_closed_and_cmd_l_does_nothing() {
    let (scratch, machine) = (new_project(), tempfile::tempdir().unwrap());
    let mut window = framed(&scratch, None, Some(machine.path().join("left-panel")));
    assert!(!open(&window));
    chord(&mut window, 'l');
    assert!(!open(&window));
    chord(&mut window, 'b');
    assert!(window.find("left-panel-agent-tab").is_none());
    assert!(window.find("left-panel-history-tab").is_some());
}

#[test]
fn the_list_of_files_not_live_asks_the_agent_to_fix_them() {
    let scratch = new_project();
    let state = scratch.path().join("song/state/song");
    std::fs::write(state.join("quiet.json"), "{").unwrap();
    std::fs::write(state.join("limit.json"), "{").unwrap();
    let agent = Agent::default();
    let asked = agent.asked.clone();
    let mut window = framed(&scratch, Some(agent), None);
    chord(&mut window, 'b');
    let sign = "problems";
    let controls = chrome_controls(&window.state);
    // Two records do not read, and the song names them: six problems in three files.
    assert_eq!(window.state.project().problems().len(), 6);
    assert_eq!(control(&controls, "3 files are not live").key, sign);

    click(&mut window, sign);
    assert!(window.find("problems-list").is_some());
    key(&mut window, Key::Escape);
    assert!(window.find("problems-list").is_none());

    click(&mut window, sign);
    click(&mut window, "problems-ask");
    assert_eq!(*asked.borrow(), [sound_agent::FIX_NOT_LIVE]);
    assert!(window.find("problems-list").is_none());
    assert!(open(&window));
    assert_eq!(window.state.panel.tab, Tab::Agent);
}

#[test]
fn an_agent_opens_the_panel_its_tabs_and_the_list_by_label() {
    let scratch = new_project();
    std::fs::write(scratch.path().join("song/state/song/quiet.json"), "{").unwrap();
    let mut window = framed(&scratch, Some(Agent::default()), None);
    let state = &mut window.state;
    let warning = sign(state).label;
    for label in ["Side panel", "History", &warning] {
        act(state, json!({ "label": label, "action": "press" }));
    }
    assert!(state.panel.open);
    assert_eq!(state.panel.tab, Tab::History);
    assert_eq!(sign(state).expanded, Some(true));
    act(state, json!({ "key": "problems-ask", "action": "press" }));
    assert_eq!(state.panel.tab, Tab::Agent);
}
