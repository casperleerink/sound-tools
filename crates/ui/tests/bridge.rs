//! The session bridge and the view registry, on a real project folder with an offline engine.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them, and
// it does not know `#[gpui::test]`.
#![allow(clippy::unwrap_used)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{Context, Entity, IntoElement, Render, TestAppContext, Window, div, prelude::*};
use serde::{Deserialize, Serialize};
use sound_core::{
    Changes, Engine, EngineConfig, Instance, InstanceId, Project, ProjectEvent, Registry, State,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::{ControlEdit, POLL_INTERVAL, Session, Views};
use tempfile::TempDir;

/// A tool of plain data. The bridge knows nothing about what a tool means.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    value: u32,
}

impl State for Marker {
    const TOOL: &'static str = "marker";

    fn validate(&self) -> Result<(), String> {
        if self.value > 100 {
            return Err(format!("value must be at most 100, not {}", self.value));
        }
        Ok(())
    }
}

struct Opened {
    folder: TempDir,
    engine: Engine,
    session: Entity<Session>,
}

fn open(cx: &mut TestAppContext) -> Opened {
    let folder = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    registry.tool::<Marker>("markers").unwrap();
    let (control, engine) = Engine::new(EngineConfig::new(48_000, 2));
    let mut project = Project::open(folder.path(), registry, control).unwrap();
    project.watch().unwrap();
    let session = cx.new(|cx| Session::new(project, cx));
    Opened {
        folder,
        engine,
        session,
    }
}

fn marker_id() -> InstanceId {
    InstanceId::new("marker-a").unwrap()
}

fn create_marker(session: &Entity<Session>, cx: &mut TestAppContext) -> Instance<Marker> {
    let created = session.update(cx, |session, cx| {
        session.edit(cx, |project| {
            let mut changes = Changes::new();
            let marker = changes.create(marker_id(), Marker { value: 1 });
            project.commit("Add marker", changes)?;
            Ok(marker)
        })
    });
    created.unwrap()
}

/// Lets the poll timer fire until `done`, while real time passes for the file watcher.
fn poll_until(
    cx: &mut TestAppContext,
    what: &str,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(cx) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
        cx.executor().advance_clock(POLL_INTERVAL);
        cx.run_until_parked();
    }
}

#[gpui::test]
fn an_outside_change_notifies_observers_and_names_the_instance(cx: &mut TestAppContext) {
    let opened = open(cx);
    let notified = Rc::new(RefCell::new(0));
    let events = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        cx.observe(&opened.session, {
            let notified = notified.clone();
            move |_, _| *notified.borrow_mut() += 1
        })
        .detach();
        cx.subscribe(&opened.session, {
            let events = events.clone();
            move |_, event: &ProjectEvent, _| events.borrow_mut().push(event.clone())
        })
        .detach();
    });
    // The watcher needs a moment before it sees changes.
    std::thread::sleep(Duration::from_millis(200));

    let state = opened.folder.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("marker-a.json"),
        r#"{"tool": "marker", "state": {"value": 7}}"#,
    )
    .unwrap();

    poll_until(cx, "the outside change", |_| !events.borrow().is_empty());
    assert_eq!(*events.borrow(), [ProjectEvent::Created(marker_id())]);
    assert_eq!(*notified.borrow(), 1);
    opened.session.read_with(cx, |session, _| {
        let marker = session.project().resolve::<Marker>(&marker_id()).unwrap();
        assert_eq!(session.project().state(&marker), Some(&Marker { value: 7 }));
    });

    // Undo goes the same way, and nothing more is heard once the folder is quiet.
    opened.session.update(cx, |session, cx| {
        assert_eq!(
            session.edit(cx, Project::undo),
            Some(Some("File change".to_string()))
        );
    });
    cx.run_until_parked();
    assert_eq!(
        events.borrow().last(),
        Some(&ProjectEvent::Deleted(marker_id()))
    );
    let heard = *notified.borrow();
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(10));
        cx.executor().advance_clock(POLL_INTERVAL);
        cx.run_until_parked();
    }
    assert_eq!(*notified.borrow(), heard, "an idle project must not notify");
}

#[gpui::test]
fn a_project_error_reaches_the_notice_and_the_next_good_edit_clears_it(cx: &mut TestAppContext) {
    let opened = open(cx);
    let marker = create_marker(&opened.session, cx);
    let notified = Rc::new(RefCell::new(0));
    cx.update(|cx| {
        cx.observe(&opened.session, {
            let notified = notified.clone();
            move |_, _| *notified.borrow_mut() += 1
        })
        .detach();
    });

    let result = opened.session.update(cx, |session, cx| {
        session.edit(cx, |project| {
            let mut edit = project.begin("Too much");
            project.update(&mut edit, &marker, |state| state.value = 101)?;
            project.finish(edit)
        })
    });
    cx.run_until_parked();
    assert_eq!(result, None);
    assert_eq!(*notified.borrow(), 1);
    opened.session.read_with(cx, |session, _| {
        let notice = session.notice().unwrap();
        assert!(notice.contains("value must be at most 100"), "{notice}");
        assert_eq!(session.project().state(&marker), Some(&Marker { value: 1 }));
    });

    opened.session.update(cx, |session, cx| {
        session.edit(cx, |project| {
            let mut edit = project.begin("Fine");
            project.update(&mut edit, &marker, |state| state.value = 100)?;
            project.finish(edit)
        })
    });
    opened.session.read_with(cx, |session, _| {
        assert_eq!(session.notice(), None);
        assert_eq!(session.project().undo_label(), Some("Fine"));
    });
}

#[gpui::test]
fn a_good_edit_leaves_a_notice_of_the_system_alone(cx: &mut TestAppContext) {
    let opened = open(cx);
    let marker = create_marker(&opened.session, cx);
    opened.session.update(cx, |session, cx| {
        session.report("the output device is gone", cx);
        let moved = session.edit(cx, |project| {
            let mut edit = project.begin("Fine");
            project.update(&mut edit, &marker, |state| state.value = 2)?;
            project.finish(edit)
        });
        assert_eq!(moved, Some(()));
        assert_eq!(
            session.notice().unwrap().as_ref(),
            "the output device is gone"
        );

        // A failed edit takes its place, and that one a good edit clears.
        session.edit(cx, |project| {
            let mut edit = project.begin("Too much");
            project.update(&mut edit, &marker, |state| state.value = 101)
        });
        assert!(session.notice().unwrap().contains("at most 100"));
        session.edit(cx, |project| project.undo());
        assert_eq!(session.notice(), None);
    });
}

#[gpui::test]
fn undo_and_redo_wait_for_the_open_gesture(cx: &mut TestAppContext) {
    let opened = open(cx);
    let marker = create_marker(&opened.session, cx);
    let value = |session: &Session| session.project().state(&marker).unwrap().value;
    opened.session.update(cx, |session, cx| {
        // No gesture: nothing to publish into.
        assert_eq!(session.gesture(cx, |_, _| Ok(())), None);

        session.begin_gesture("Drag", cx);
        for step in [10, 20, 30] {
            session.gesture(cx, |project, edit| {
                project.update(edit, &marker, |state| state.value = step)
            });
        }
        session.undo(cx);
        session.redo(cx);
        assert_eq!(
            value(session),
            30,
            "undo in the middle of a drag is ignored"
        );
        assert_eq!(session.project().undo_label(), Some("Add marker"));

        session.finish_gesture(cx);
        assert!(!session.gesture_open());
        assert_eq!(session.project().undo_label(), Some("Drag"));
        session.undo(cx);
        assert_eq!(value(session), 1, "the whole drag is one step");
        session.redo(cx);
        assert_eq!(value(session), 30);

        // Cancel applies the state from before, and a new gesture ends one left open.
        session.begin_gesture("Cancelled", cx);
        session.gesture(cx, |project, edit| {
            project.update(edit, &marker, |state| state.value = 40)
        });
        session.cancel_gesture(cx);
        assert_eq!(value(session), 30);
        session.begin_gesture("First", cx);
        session.gesture(cx, |project, edit| {
            project.update(edit, &marker, |state| state.value = 50)
        });
        session.begin_gesture("Second", cx);
        assert_eq!(session.project().undo_label(), Some("First"));
        session.cancel_gesture(cx);
    });
}

#[gpui::test]
fn a_control_edit_begins_again_when_its_gesture_was_closed_under_it(cx: &mut TestAppContext) {
    let opened = open(cx);
    let marker = create_marker(&opened.session, cx);
    let session = opened.session.clone();
    let value = |cx: &mut TestAppContext| {
        session.read_with(cx, |session, _| {
            session.project().state(&marker).unwrap().value
        })
    };
    let set = |state: &mut Marker, value: u32| state.value = value;
    let mut edit = ControlEdit::default();

    cx.update(|cx| edit.apply(&session, &marker, "Drag", ValueChange::Drag(10), set, cx));
    assert!(session.read_with(cx, |session, _| session.gesture_open()));
    // The control goes away during its drag and sends no end. The session finishes the
    // gesture, as it does when another one begins or its view is closed.
    session.update(cx, |session, cx| session.finish_gesture(cx));

    // The next drag of this view is a gesture of its own, and its moves are heard.
    cx.update(|cx| edit.apply(&session, &marker, "Again", ValueChange::Drag(20), set, cx));
    assert!(session.read_with(cx, |session, _| session.gesture_open()));
    assert_eq!(value(cx), 20);
    cx.update(|cx| {
        edit.apply(
            &session,
            &marker,
            "Again",
            ValueChange::<u32>::DragEnd,
            set,
            cx,
        )
    });
    assert!(!session.read_with(cx, |session, _| session.gesture_open()));
    assert_eq!(
        session.read_with(cx, |session, _| session
            .project()
            .undo_label()
            .map(String::from)),
        Some("Again".to_string())
    );
}

/// Reads the session when it renders, as every view does.
struct Reader {
    session: Entity<Session>,
    renders: Rc<RefCell<u32>>,
}

impl Render for Reader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        *self.renders.borrow_mut() += 1;
        let instances = self.session.read(cx).project().instances().count();
        div().child(format!("{instances} instances"))
    }
}

#[gpui::test]
fn the_timer_polls_and_render_does_not(cx: &mut TestAppContext) {
    let Opened {
        folder: _folder,
        engine,
        session,
    } = open(cx);
    let renders = Rc::new(RefCell::new(0));
    let (reader, cx) = cx.add_window_view({
        let (session, renders) = (session.clone(), renders.clone());
        move |_, cx| {
            cx.observe(&session, |_, _, cx| cx.notify()).detach();
            Reader { session, renders }
        }
    });

    // A stopped engine is what the next poll reports. Rendering must not be that poll.
    drop(engine);
    for _ in 0..5 {
        reader.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    assert!(*renders.borrow() >= 5);
    session.read_with(cx, |session, _| assert_eq!(session.notice(), None));

    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    session.read_with(cx, |session, _| {
        assert_eq!(
            session.notice().unwrap().as_ref(),
            "the audio engine has stopped"
        );
    });

    // Reported once: dismissing it is not undone by the next poll.
    let before = *renders.borrow();
    session.update(cx, |session, cx| session.dismiss_notice(cx));
    cx.executor().advance_clock(POLL_INTERVAL * 10);
    cx.run_until_parked();
    session.read_with(cx, |session, _| assert_eq!(session.notice(), None));
    assert_eq!(*renders.borrow(), before + 1, "only the dismissal renders");
}

#[gpui::test]
fn the_playhead_follows_the_engine_without_notifying_the_session(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (session_notified, playhead_notified) =
        (Rc::new(RefCell::new(0)), Rc::new(RefCell::new(0)));
    let playhead = opened
        .session
        .read_with(cx, |session, _| session.playhead().clone());
    cx.update(|cx| {
        cx.observe(&opened.session, {
            let count = session_notified.clone();
            move |_, _| *count.borrow_mut() += 1
        })
        .detach();
        cx.observe(&playhead, {
            let count = playhead_notified.clone();
            move |_, _| *count.borrow_mut() += 1
        })
        .detach();
    });

    opened
        .session
        .update(cx, |session, cx| session.toggle_playback(cx));
    let mut buffer = [0.0_f32; 512 * 2];
    opened.engine.process_block(&mut buffer);
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();

    let now = playhead.read_with(cx, |playhead, _| *playhead);
    assert!(now.playing);
    assert!(now.tick.0 > 0);
    assert_eq!(*playhead_notified.borrow(), 1);
    assert_eq!(*session_notified.borrow(), 0);

    // The same position again is no news.
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    assert_eq!(*playhead_notified.borrow(), 1);
}

struct MarkerView {
    marker: Instance<Marker>,
}

impl Render for MarkerView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(self.marker.id().to_string())
    }
}

#[gpui::test]
fn the_registry_makes_the_view_of_a_top_instance(cx: &mut TestAppContext) {
    let opened = open(cx);
    let mut views = Views::new();
    views.register(|_, marker: Instance<Marker>, _, _| MarkerView { marker });
    // Nothing is installed yet: a nested view then gets no view, and no panic.
    cx.update(|cx| assert_eq!(Views::main_instance(&opened.session, cx), None));
    cx.update(|cx| views.install(cx));
    cx.update(|cx| assert_eq!(Views::main_instance(&opened.session, cx), None));

    create_marker(&opened.session, cx);
    let session = opened.session.clone();
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        assert_eq!(Views::main_instance(&session, cx), Some(marker_id()));
        assert!(Views::view_of(&session, &marker_id(), window, cx).is_some());
        let missing = InstanceId::new("missing").unwrap();
        assert!(Views::view_of(&session, &missing, window, cx).is_none());
    });
}
