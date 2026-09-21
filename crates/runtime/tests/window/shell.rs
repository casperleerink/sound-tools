//! The keys of the window, the focus, the transport and the timeline without edits.

use std::cell::RefCell;
use std::rc::Rc;

use arrangement::ArrangementState;
use arrangement::view::layout::{HEADER_WIDTH, LEAD_IN, RULER_HEIGHT, TRACK_HEIGHT, Viewport};
use gpui::{
    AppContext, Context, Entity, Focusable, IntoElement, Modifiers, PlatformInput, Render,
    ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, Window, div, point,
    prelude::*, px,
};
use runtime::window::{Shell, bind_keys};
use runtime::{OFFLINE, main_arrangement, open_or_create};
use sound_core::{Changes, Engine, Instance, InstanceId, Ticks};
use sound_notes::Clip;
use sound_ui::components::text_input::TextInput;
use sound_ui::{POLL_INTERVAL, Session, Views};

use crate::support::{self, BAR, Opened, TOP_ROW};

impl Opened<'_> {
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
    support::open_with(cx, |project| {
        let mut changes = Changes::new();
        changes.create(clip_id(), support::clip(BAR, 2 * BAR, Vec::new()));
        project.commit("Add clip", changes).unwrap();
    })
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
    // The project menu first, then the arrangement, then play, then stop.
    opened.cx.simulate_keystrokes("tab tab tab");
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
    let playhead = opened.playhead();
    assert!(!playhead.playing);
    assert_eq!(playhead.tick, Ticks(0));
    // The stop is a jump, which the arrangement view uses to follow the playhead.
    assert_eq!(playhead.jumps, 1);
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
fn the_timeline_is_notified_for_its_arrangement_and_not_for_the_playhead(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let notified = Rc::new(RefCell::new(0));
    let (timeline, session) = (opened.timeline.clone(), opened.session.clone());
    opened.cx.update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&timeline, move |_, _| *notified.borrow_mut() += 1)
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
    assert_eq!(*notified.borrow(), 1);

    opened.cx.simulate_keystrokes("space");
    for _ in 0..10 {
        opened.settle();
    }
    assert!(opened.playhead().tick > Ticks(0));
    assert_eq!(*notified.borrow(), 1);
}

/// Stands in for a view with a text field, such as a rename or the agent composer later.
struct FieldView {
    field: Entity<TextInput>,
}

impl Render for FieldView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p_4().child(self.field.clone())
    }
}

#[gpui::test]
fn a_focused_text_field_gets_space_and_cmd_z_before_the_window(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let (control, mut engine) = Engine::new(OFFLINE);
    let mut project = open_or_create(folder.path(), control).unwrap();
    let arrangement = main_arrangement(&project).unwrap();
    runtime::add_track(&mut project, &arrangement).unwrap();
    cx.update(sound_ui::init);
    cx.update(bind_keys);
    let session = cx.new(|cx| Session::new(project, cx));

    let field: Rc<RefCell<Option<Entity<TextInput>>>> = Rc::default();
    let mut views = Views::new();
    views.register({
        let field = field.clone();
        move |_, _: Instance<ArrangementState>, _, cx: &mut Context<FieldView>| {
            let input = cx.new(TextInput::new);
            *field.borrow_mut() = Some(input.clone());
            FieldView { field: input }
        }
    });
    let (_shell, cx) = cx.add_window_view({
        let session = session.clone();
        move |window, cx| Shell::new(session, views, "Test device".into(), window, cx)
    });
    cx.run_until_parked();
    let field = field.borrow().clone().unwrap();
    let mut settle = |cx: &mut VisualTestContext| {
        let mut buffer = [0.0_f32; 64 * OFFLINE.channels];
        engine.process_block(&mut buffer);
        cx.executor().advance_clock(POLL_INTERVAL);
        cx.run_until_parked();
    };
    let playing =
        |cx: &mut VisualTestContext| cx.read(|cx| session.read(cx).playhead().read(cx).playing);
    let undo_label = |cx: &mut VisualTestContext| {
        cx.read(|cx| session.read(cx).project().undo_label().map(str::to_string))
    };

    field.update_in(cx, |field, window, cx| {
        window.focus(&field.focus_handle(cx), cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("a space b cmd-z");
    settle(cx);
    assert_eq!(
        field.read_with(cx, |field, _| field.text().to_string()),
        "a b"
    );
    assert!(!playing(cx));
    assert_eq!(undo_label(cx).as_deref(), Some("Add track"));

    // Tab leaves the field, and then the keys are the window's again.
    cx.simulate_keystrokes("tab space");
    settle(cx);
    assert!(playing(cx));
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(undo_label(cx), None);
}

#[gpui::test]
fn two_scrolls_before_a_frame_both_count(cx: &mut TestAppContext) {
    let opened = open(cx);
    let timeline = opened.timeline.clone();
    // Zoomed in, so that three bars and the room after them are wider than any window.
    let zoomed_in = timeline.read_with(opened.cx, |timeline, _| Viewport {
        pixels_per_quarter: 384.0,
        ..timeline.viewport()
    });
    timeline.update(opened.cx, |timeline, cx| {
        timeline.set_viewport(zoomed_in, cx)
    });
    opened.cx.run_until_parked();

    let scroll = ScrollWheelEvent {
        position: point(px(HEADER_WIDTH + 200.), px(TOP_ROW + RULER_HEIGHT + 20.)),
        delta: ScrollDelta::Pixels(point(px(-100.), px(0.))),
        ..Default::default()
    };
    // Both in one update: no frame is drawn between them.
    opened.cx.update(|window, cx| {
        window.dispatch_event(PlatformInput::ScrollWheel(scroll.clone()), cx);
        window.dispatch_event(PlatformInput::ScrollWheel(scroll.clone()), cx);
    });
    opened.cx.run_until_parked();
    let viewport = timeline.read_with(opened.cx, |timeline, _| timeline.viewport());
    assert_eq!(viewport.scroll_x, 200.0);

    // The scroll stops at the start, also for a viewport that is set from code.
    let before_start = viewport.scrolled(10_000.0, 10_000.0);
    timeline.update(opened.cx, |timeline, cx| {
        timeline.set_viewport(before_start, cx)
    });
    let viewport = timeline.read_with(opened.cx, |timeline, _| timeline.viewport());
    assert_eq!((viewport.scroll_x, viewport.scroll_y), (0.0, 0.0));
}

#[gpui::test]
fn tab_reaches_the_dismiss_button_and_the_keys_still_work_after_it_is_gone(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    let session = opened.session.clone();
    opened
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.report("the device is gone", cx)));
    opened.cx.run_until_parked();

    // The menu, the arrangement, play, stop, the seek strip, the tempo, the click, then the
    // notice.
    opened
        .cx
        .simulate_keystrokes("tab tab tab tab tab tab tab tab");
    opened.press_enter();
    opened
        .cx
        .read(|cx| assert_eq!(session.read(cx).notice(), None));

    // The button went away with the focus on it. Space must still reach the window.
    opened.cx.simulate_keystrokes("space");
    opened.settle();
    assert!(opened.playhead().playing);
}

#[gpui::test]
fn cmd_z_waits_for_an_open_gesture(cx: &mut TestAppContext) {
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
    opened
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.begin_gesture("Drag", cx)));
    opened.cx.simulate_keystrokes("cmd-z");
    assert!(has_clip(opened.cx));
    opened
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.cancel_gesture(cx)));
    opened.cx.simulate_keystrokes("cmd-z");
    assert!(!has_clip(opened.cx));
}
