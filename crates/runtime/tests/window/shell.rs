//! The keys of the window, the focus, the notices and the project menu, on a project with no
//! view of its own.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    AppContext, Context, Entity, Focusable, IntoElement, Modifiers, Render, TestAppContext,
    VisualTestContext, Window, div, prelude::*, px,
};
use reverb::ReverbState;
use runtime::OFFLINE;
use runtime::window::new_project::TemplatePicker;
use runtime::window::{DeviceMenu, ListDevices, Shell, bind_keys};
use sound_core::{Changes, DeviceChoice, Engine, Instance, InstanceId, Project};
use sound_ui::components::text_input::TextInput;
use sound_ui::{Devices, Session, Views};

use crate::plugin_hosts::open_or_create;
use crate::support::{self, Opened};

/// The height of the row with the project menu, above the main area.
const TOP_ROW: f32 = 48.;

fn reverb_id() -> InstanceId {
    InstanceId::new("room").unwrap()
}

/// Adds a reverb at the top, as one undo step.
fn add_reverb(project: &mut Project) {
    let mut changes = Changes::new();
    changes.create(reverb_id(), ReverbState::default());
    project.commit("Add reverb", changes).unwrap();
}

/// A new project with a reverb added, as one undo step.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_new(cx);
    opened.edit(|project| {
        add_reverb(project);
        Ok(())
    });
    opened
}

fn has_reverb(session: &Entity<Session>, cx: &mut VisualTestContext) -> bool {
    cx.read(|cx| {
        let project = session.read(cx).project();
        project.resolve::<ReverbState>(&reverb_id()).is_some()
    })
}

#[gpui::test]
fn the_standard_keys_undo_and_redo(cx: &mut TestAppContext) {
    let opened = open(cx);
    let session = opened.session.clone();
    assert!(has_reverb(&session, opened.cx));
    opened.cx.simulate_keystrokes("secondary-z");
    assert!(!has_reverb(&session, opened.cx));
    opened.cx.simulate_keystrokes("shift-secondary-z");
    assert!(has_reverb(&session, opened.cx));

    // Nothing left to redo is not an error.
    opened.cx.simulate_keystrokes("shift-secondary-z");
    opened
        .cx
        .read(|cx| assert_eq!(session.read(cx).notice(), None));
}

/// Stands in for a view with a text field, such as a rename or the agent composer.
struct FieldView {
    field: Entity<TextInput>,
}

impl Render for FieldView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p_4().child(self.field.clone())
    }
}

#[gpui::test]
fn a_focused_text_field_gets_cmd_z_before_the_window(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let (control, _engine) = Engine::new(OFFLINE);
    let (mut project, _plugins) = open_or_create(folder.path(), control);
    add_reverb(&mut project);
    cx.update(sound_ui::init);
    cx.update(bind_keys);
    let session = cx.new(|cx| Session::new(project, cx));

    let field: Rc<RefCell<Option<Entity<TextInput>>>> = Rc::default();
    let mut views = Views::new();
    views.register({
        let field = field.clone();
        move |_, _: Instance<ReverbState>, _, cx: &mut Context<FieldView>| {
            let input = cx.new(TextInput::new);
            *field.borrow_mut() = Some(input.clone());
            FieldView { field: input }
        }
    });
    let (_shell, cx) = cx.add_window_view({
        let session = session.clone();
        move |window, cx| {
            Shell::new(
                session,
                (views, Devices::new()),
                "Test device".into(),
                window,
                cx,
            )
        }
    });
    cx.run_until_parked();
    let field = field.borrow().clone().unwrap();
    let undo_label = |cx: &mut VisualTestContext| {
        cx.read(|cx| session.read(cx).project().undo_label().map(str::to_string))
    };

    field.update_in(cx, |field, window, cx| {
        window.focus(&field.focus_handle(cx), cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("a space b secondary-z");
    cx.run_until_parked();
    assert_eq!(
        field.read_with(cx, |field, _| field.text().to_string()),
        "a "
    );
    assert_eq!(undo_label(cx).as_deref(), Some("Add reverb"));

    // Tab leaves the field, and then the keys are the window's again.
    cx.simulate_keystrokes("tab secondary-z");
    cx.run_until_parked();
    assert_eq!(undo_label(cx), None);
}

#[gpui::test]
fn a_long_error_fits_top_right_in_three_lines_at_most(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let session = opened.session.clone();
    let long = "a problem that goes on and on, ".repeat(20);
    opened
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.report(long, cx)));
    let notice = opened.bounds("notice-error").unwrap();
    let window = opened.cx.update(|window, _| window.viewport_size());
    // 24 pt in from the right and under the title row, and 400 pt wide at most.
    assert_eq!(notice.right(), window.width - px(24.), "{notice:?}");
    assert_eq!(notice.top(), px(TOP_ROW + 24.));
    assert!(notice.size.width <= px(400.), "{notice:?}");
    // As tall as the three lines of 20 pt it paints and its padding: the lines stay inside it.
    assert!(
        (px(60.)..=px(74.)).contains(&notice.size.height),
        "{notice:?}"
    );

    // A short one is as wide as its text.
    opened.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.dismiss_notice(cx);
            session.report("short", cx);
        })
    });
    let notice = opened.bounds("notice-error").unwrap();
    assert!(notice.size.width < px(200.), "{notice:?}");
    assert!(notice.size.height < px(40.), "{notice:?}");
}

/// The device menus list the devices of the machine when the window opens and each time the
/// project menu opens, so an interface plugged in since is offered. Never on a timer: listing
/// can take a while, and on Linux it opens each device.
#[gpui::test]
fn the_project_menu_lists_the_devices_each_time_it_opens(cx: &mut TestAppContext) {
    let device = |id: &str, name: &str| DeviceChoice {
        id: id.to_string(),
        name: name.to_string(),
    };
    let outputs = Arc::new(Mutex::new(vec![device("coreaudio:speakers", "Speakers")]));
    let listed = Arc::new(AtomicUsize::new(0));
    let list: ListDevices = Arc::new({
        let (outputs, listed) = (outputs.clone(), listed.clone());
        move || {
            listed.fetch_add(1, Ordering::Relaxed);
            let outputs = outputs.lock().unwrap().clone();
            DeviceMenu {
                outputs,
                ..DeviceMenu::default()
            }
        }
    });
    let mut opened = support::open_with_devices(cx, list);
    let menu = opened.cx.read(|cx| {
        let shell = opened.shell.read(cx);
        shell.project_menu().read(cx).menu().clone()
    });
    let offers = |opened: &mut Opened<'_>, id: &str| {
        let value = format!("output-device:{id}");
        opened.cx.read(|cx| menu.read(cx).item(&value).is_some())
    };
    assert_eq!(listed.load(Ordering::Relaxed), 1);
    assert!(offers(&mut opened, "coreaudio:speakers"));

    outputs
        .lock()
        .unwrap()
        .push(device("coreaudio:interface", "Interface"));
    opened.cx.executor().advance_clock(Duration::from_secs(10));
    opened.cx.run_until_parked();
    assert_eq!(listed.load(Ordering::Relaxed), 1);
    assert!(!offers(&mut opened, "coreaudio:interface"));

    let trigger = opened.control("project-menu");
    opened.click(trigger);
    assert_eq!(listed.load(Ordering::Relaxed), 2);
    assert!(offers(&mut opened, "coreaudio:interface"));
}

#[gpui::test]
fn cmd_z_waits_for_an_open_gesture(cx: &mut TestAppContext) {
    let opened = open(cx);
    let session = opened.session.clone();
    opened
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.begin_gesture("Drag", cx)));
    opened.cx.simulate_keystrokes("secondary-z");
    assert!(has_reverb(&session, opened.cx));
    opened
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.cancel_gesture(cx)));
    opened.cx.simulate_keystrokes("secondary-z");
    assert!(!has_reverb(&session, opened.cx));
}

/// "New project…" opens the templates, one row each; picking one closes them and asks where the
/// project goes. A cancel there makes nothing.
#[gpui::test]
fn new_project_shows_the_templates_and_a_pick_asks_where_it_goes(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    opened.keys("secondary-n");
    let picker = opened.cx.update(|_, cx| {
        (cx.windows().into_iter()).find_map(|window| window.downcast::<TemplatePicker>())
    });
    let picker = picker.expect("the templates did not open");
    let mut picker = VisualTestContext::from_window(picker.into(), &opened.cx.cx);
    picker.update(|window, _| window.refresh());
    picker.run_until_parked();
    let rows: Vec<&str> = (runtime::TEMPLATES.iter())
        .filter(|template| {
            let selector: &'static str = format!("template-{}", template.name).leak();
            picker.debug_bounds(selector).is_some()
        })
        .map(|template| template.name)
        .collect();
    assert_eq!(rows, ["Bare", "DAW"]);

    let daw = picker.debug_bounds("template-DAW").unwrap().center();
    picker.simulate_click(daw, Modifiers::default());
    opened.cx.run_until_parked();
    assert!(opened.cx.did_prompt_for_new_path());
    let windows = opened.cx.update(|_, cx| cx.windows().len());
    assert_eq!(windows, 1, "the templates are still open");
    opened.cx.simulate_new_path_selection(|_| None);
    opened.cx.run_until_parked();
    assert_eq!(opened.notice(), None);
}
