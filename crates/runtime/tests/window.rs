//! The window driven by simulated keys and clicks, on the default project with an offline
//! engine. No display and no device.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them, and
// it does not know `#[gpui::test]`.
#![allow(clippy::unwrap_used)]

use std::cell::RefCell;
use std::rc::Rc;

use arrangement::view::layout::{HEADER_WIDTH, LEAD_IN, RULER_HEIGHT, TRACK_HEIGHT};
use arrangement::view::{ArrangementView, Timeline};
use gpui::{
    AppContext, Entity, KeyUpEvent, Keystroke, Modifiers, TestAppContext, VisualTestContext, point,
    px,
};
use runtime::window::{Shell, bind_actions};
use runtime::{OFFLINE, main_arrangement, open_or_create, views};
use sound_core::{Changes, Engine, InstanceId, Ticks};
use sound_notes::{Clip, Length};
use sound_ui::{POLL_INTERVAL, Playhead, Session};
use tempfile::TempDir;

const BAR: u64 = 3840;
/// The height of the row with the project menu, above the main area.
const TOP_ROW: f32 = 48.;

struct Opened<'a> {
    _folder: TempDir,
    engine: Engine,
    session: Entity<Session>,
    timeline: Entity<Timeline>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    /// Lets the engine take what was sent to it and the poll timer see the result.
    fn settle(&mut self) {
        let mut buffer = [0.0_f32; 64 * OFFLINE.channels];
        self.engine.process_block(&mut buffer);
        self.cx.executor().advance_clock(POLL_INTERVAL);
        self.cx.run_until_parked();
    }

    fn playhead(&mut self) -> Playhead {
        let session = self.session.clone();
        self.cx.read(|cx| *session.read(cx).playhead().read(cx))
    }

    /// A whole key press. GPUI clicks the focused control when enter comes up again, and
    /// `simulate_keystrokes` only sends the key down.
    fn press_enter(&mut self) {
        self.cx.simulate_keystrokes("enter");
        let keystroke = Keystroke::parse("enter").unwrap();
        self.cx.simulate_event(KeyUpEvent { keystroke });
        self.cx.run_until_parked();
    }

    fn click_timeline(&mut self, bar: f32, track: f32) {
        let x = HEADER_WIDTH + LEAD_IN + bar * 96.;
        let y = TOP_ROW + RULER_HEIGHT + track * TRACK_HEIGHT;
        self.cx
            .simulate_click(point(px(x), px(y)), Modifiers::default());
        self.cx.run_until_parked();
    }
}

/// The default project with one clip of two bars at bar 2 on its track.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let folder = tempfile::tempdir().unwrap();
    let (control, engine) = Engine::new(OFFLINE);
    let mut project = open_or_create(folder.path(), control).unwrap();
    let mut changes = Changes::new();
    let clip = Clip {
        start: Ticks(BAR),
        length: Length::new(Ticks(2 * BAR)).unwrap(),
        notes: Vec::new(),
    };
    changes.create(clip_id(), clip);
    project.commit("Add clip", changes).unwrap();

    cx.update(sound_ui::init);
    let session = cx.new(|cx| Session::new(project, cx));
    cx.update(|cx| bind_actions(session.downgrade(), cx));
    let (shell, cx) = cx.add_window_view({
        let session = session.clone();
        move |window, cx| Shell::new(session, views(), "Test device".into(), window, cx)
    });
    cx.run_until_parked();
    let main = shell
        .read_with(cx, |shell, _| shell.main_view().cloned())
        .unwrap();
    let arrangement = main.downcast::<ArrangementView>().ok().unwrap();
    let timeline = arrangement.read_with(cx, |view, _| view.timeline().clone());
    Opened {
        _folder: folder,
        engine,
        session,
        timeline,
        cx,
    }
}

fn clip_id() -> InstanceId {
    InstanceId::new("arrangement/track-1/part").unwrap()
}

#[gpui::test]
fn space_plays_and_pauses(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    assert!(!opened.playhead().playing);
    opened.cx.simulate_keystrokes("space");
    opened.settle();
    assert!(opened.playhead().playing);
    opened.cx.simulate_keystrokes("space");
    opened.settle();
    assert!(!opened.playhead().playing);
}

#[gpui::test]
fn tab_reaches_the_menu_and_the_transport_and_enter_activates(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // The project menu first, then play, then stop.
    opened.cx.simulate_keystrokes("tab tab");
    opened.press_enter();
    opened.settle();
    assert!(opened.playhead().playing);
    for _ in 0..10 {
        opened.settle();
    }
    assert!(opened.playhead().tick > Ticks(0));
    opened.cx.simulate_keystrokes("tab");
    opened.press_enter();
    opened.settle();
    assert_eq!(opened.playhead(), Playhead::default());
}

#[gpui::test]
fn the_standard_keys_undo_and_redo(cx: &mut TestAppContext) {
    let opened = open(cx);
    let session = opened.session.clone();
    let has_clip = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            session
                .read(cx)
                .project()
                .resolve::<Clip>(&clip_id())
                .is_some()
        })
    };
    assert!(has_clip(opened.cx));
    opened.cx.simulate_keystrokes("cmd-z");
    assert!(!has_clip(opened.cx));
    opened.cx.simulate_keystrokes("shift-cmd-z");
    assert!(has_clip(opened.cx));

    // Nothing left to redo is not an error.
    opened.cx.simulate_keystrokes("shift-cmd-z");
    opened
        .cx
        .read(|cx| assert_eq!(session.read(cx).notice(), None));
}

#[gpui::test]
fn a_click_on_the_ruler_seeks_to_the_nearest_sixteenth(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // A little past bar 3 and a sixteenth, in the ruler above the tracks.
    opened.click_timeline(2.0 + 0.0625 + 0.01, -0.25);
    opened.settle();
    assert_eq!(opened.playhead().tick, Ticks(2 * BAR + 240));
    assert!(!opened.playhead().playing);
}

#[gpui::test]
fn a_click_selects_the_clip_under_it_and_a_click_beside_it_clears(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let timeline = opened.timeline.clone();
    let selected =
        |cx: &mut VisualTestContext| cx.read(|cx| timeline.read(cx).selected_clip().cloned());

    opened.click_timeline(1.5, 0.5);
    assert_eq!(selected(opened.cx), Some(clip_id()));
    opened.click_timeline(3.5, 0.5);
    assert_eq!(selected(opened.cx), None);

    // A selected clip that goes away is no longer selected.
    opened.click_timeline(1.5, 0.5);
    opened.cx.simulate_keystrokes("cmd-z");
    assert_eq!(selected(opened.cx), None);
}

#[gpui::test]
fn the_timeline_repaints_for_its_arrangement_and_not_for_the_playhead(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let repaints = Rc::new(RefCell::new(0));
    let (timeline, session) = (opened.timeline.clone(), opened.session.clone());
    opened.cx.update(|_, cx| {
        let repaints = repaints.clone();
        cx.observe(&timeline, move |_, _| *repaints.borrow_mut() += 1)
            .detach();
    });

    // A track written from outside, as an agent does.
    let arrangement = opened
        .cx
        .read(|cx| main_arrangement(session.read(cx).project()))
        .unwrap();
    opened.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.edit(cx, |project| runtime::add_track(project, &arrangement));
        })
    });
    opened.cx.run_until_parked();
    assert_eq!(*repaints.borrow(), 1);

    opened.cx.simulate_keystrokes("space");
    for _ in 0..10 {
        opened.settle();
    }
    assert!(opened.playhead().tick > Ticks(0));
    assert_eq!(*repaints.borrow(), 1);
}
