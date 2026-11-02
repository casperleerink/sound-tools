//! The History tab of the left panel: a click or the keys on a row write that point's files
//! back, and the tabs with cmd-L and cmd-B, with the agent and without.

use gpui::TestAppContext;
use reverb::ReverbState;
use sound_core::{Changes, InstanceId, PointId, Project, ProjectError};

use crate::agent::install_sidebar;
use crate::support::{self, Opened};

fn room() -> InstanceId {
    InstanceId::new("room").unwrap()
}

fn add_room(project: &mut Project) -> Result<(), ProjectError> {
    let mut changes = Changes::new();
    changes.create(room(), ReverbState::default());
    project.commit("Add reverb", changes)
}

impl Opened<'_> {
    fn current(&mut self) -> PointId {
        self.project(|project| project.history().unwrap().current())
    }

    fn point_count(&mut self) -> usize {
        self.project(|project| project.history().unwrap().points(100).unwrap().len())
    }

    fn history_shown(&mut self) -> bool {
        let shell = self.shell.clone();
        self.cx.read(|cx| shell.read(cx).history_shown())
    }

    fn left_panel_open_on_agent(&mut self) -> bool {
        self.panel_open() && !self.history_shown()
    }

    fn row(&mut self, point: PointId) -> gpui::Point<gpui::Pixels> {
        self.control(&format!("history-row-{}", point.short()))
    }
}

#[gpui::test]
fn a_click_on_a_row_writes_its_files_back_and_makes_no_point(cx: &mut TestAppContext) {
    let mut opened = support::open_new(cx);
    let start = opened.current();
    opened.edit(add_room).unwrap();
    let with_room = opened.current();
    let file = opened.folder.path().join("state/room.json");
    assert!(file.exists());
    // Without the agent the panel starts closed, and opens on History.
    assert!(!opened.history_shown());
    opened.keys("secondary-b");
    assert!(opened.history_shown());
    let points = opened.point_count();

    let row = opened.row(start);
    opened.click(row);
    assert_eq!(opened.current(), start);
    assert!(!file.exists());
    assert!(opened.project(|project| project.resolve::<ReverbState>(&room()).is_none()));

    let row = opened.row(with_room);
    opened.click(row);
    assert_eq!(opened.current(), with_room);
    assert!(file.exists());
    assert_eq!(opened.point_count(), points);
}

#[gpui::test]
fn the_keys_move_down_the_rows_and_enter_goes_to_a_point(cx: &mut TestAppContext) {
    let mut opened = support::open_new(cx);
    let start = opened.current();
    opened.edit(add_room).unwrap();
    let file = opened.folder.path().join("state/room.json");
    opened.keys("secondary-b");
    let history = opened.control("left-panel-history-tab");
    opened.click(history);

    // The highlight starts on the current point, the newest; the one below is the start.
    opened.keys("tab down enter");
    assert_eq!(opened.current(), start);
    assert!(!file.exists());
}

#[gpui::test]
fn cmd_l_shows_the_agent_tab_and_closes_only_from_it(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    assert!(opened.left_panel_open_on_agent());

    let history = opened.control("left-panel-history-tab");
    opened.click(history);
    assert!(opened.history_shown());

    // On History, cmd-L goes to the agent and does not close the panel.
    opened.keys("secondary-l");
    assert!(opened.left_panel_open_on_agent());
    assert!(opened.composer_focused());
    opened.keys("secondary-l");
    assert!(!opened.panel_open());

    // cmd-B closes and opens it again on the tab it showed.
    opened.keys("secondary-b");
    let history = opened.control("left-panel-history-tab");
    opened.click(history);
    opened.keys("secondary-b");
    assert!(!opened.panel_open());
    opened.keys("secondary-b");
    assert!(opened.history_shown());
}

#[gpui::test]
fn without_the_agent_cmd_l_does_nothing(cx: &mut TestAppContext) {
    let mut opened = support::open_new(cx);
    assert!(opened.find("left-panel-agent-tab").is_none());
    opened.keys("secondary-l");
    assert!(!opened.panel_open());
}

impl Opened<'_> {
    fn right_click(&mut self, position: gpui::Point<gpui::Pixels>) {
        let (button, modifiers) = (gpui::MouseButton::Right, gpui::Modifiers::default());
        self.cx.simulate_mouse_move(position, None, modifiers);
        self.cx.simulate_event(gpui::MouseDownEvent {
            position,
            modifiers,
            button,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(gpui::MouseUpEvent {
            position,
            modifiers,
            button,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    fn pick_in_menu(&mut self, row: PointId, item: &str) {
        let row = self.row(row);
        self.right_click(row);
        let item = self.control(&format!("menu-{item}"));
        self.click(item);
    }

    fn line(&mut self, point: PointId) -> sound_core::Line {
        self.project(|project| {
            let history = project.history().unwrap();
            history.line_of(point).unwrap().unwrap().clone()
        })
    }
}

#[gpui::test]
fn the_menu_of_a_row_renames_and_folds_away_and_plus_one_brings_it_back(cx: &mut TestAppContext) {
    let mut opened = support::open_new(cx);
    let start = opened.current();
    opened.edit(add_room).unwrap();
    let first = opened.current();
    opened.edit(|project| project.go_to(start)).unwrap();
    opened.edit(add_room).unwrap();
    let second = opened.current();
    opened.edit(|project| project.go_to(first)).unwrap();
    opened.keys("secondary-b");

    opened.pick_in_menu(first, "rename");
    opened.cx.simulate_input("Longer tail");
    opened.keys("enter");
    assert_eq!(opened.line(first).name.as_deref(), Some("Longer tail"));
    assert!(opened.line(first).named_by_hand);

    opened.pick_in_menu(second, "aside");
    assert!(opened.line(second).set_aside);
    let plus_one = format!("history-versions-{}", start.short());
    let row = format!("history-row-{}", second.short());
    assert!(opened.find(&row).is_none());
    let plus_one = opened.control(&plus_one);
    opened.click(plus_one);
    assert!(opened.find(&row).is_some());
    assert!(
        opened
            .find(&format!("history-hide-{}", start.short()))
            .is_some()
    );
}
