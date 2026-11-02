//! The History tab of the left panel: a click on a row writes that point's files back, and the
//! tabs with cmd-L and cmd-B, with the agent and without.

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
