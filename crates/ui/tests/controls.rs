//! The volume, a display handle and two device cards, driven with a simulated mouse and keys.

// Clippy allows unwrap inside `#[test]` functions only, and it does not know `#[gpui::test]`.
#![allow(clippy::unwrap_used)]

use gpui::{
    Context, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    Render, TestAppContext, VisualTestContext, Window, div, point, prelude::*, px,
};
use sound_ui::components::device_card::{Column, DeviceCard};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange};
use sound_ui::components::meter;
use sound_ui::components::volume::Volume;

const UNIT: KnobRange = KnobRange::linear(0., 1.);
/// The height of the volume, and of its scale under the clip light.
const HEIGHT: f32 = 118.;
const SCALE: f32 = HEIGHT - meter::SCALE_TOP;
const START_DB: f32 = -6.;

/// A value that follows the changes of its control, as an owner of saved state does.
struct Followed<V> {
    value: V,
    origin: Option<V>,
    changes: Vec<ValueChange<V>>,
}

impl<V: Copy> Followed<V> {
    fn new(value: V) -> Self {
        Self {
            value,
            origin: None,
            changes: Vec::new(),
        }
    }

    fn follow(&mut self, change: ValueChange<V>) {
        self.changes.push(change);
        match change {
            ValueChange::Drag(next) => {
                self.origin.get_or_insert(self.value);
                self.value = next;
            }
            ValueChange::DragEnd => self.origin = None,
            ValueChange::DragCancel => {
                if let Some(origin) = self.origin.take() {
                    self.value = origin;
                }
            }
            ValueChange::Set(next) => self.value = next,
        }
    }
}

struct Controls {
    volume: Followed<f32>,
    handle: Followed<Point<f32>>,
    /// A handle whose second axis does not move.
    sideways: Followed<Point<f32>>,
    /// The presses that `handle` and a handle that does not drag heard.
    presses: [usize; 2],
}

/// A callback of a control into the view.
fn follow<V: Copy + 'static>(
    cx: &Context<Controls>,
    field: fn(&mut Controls) -> &mut Followed<V>,
) -> impl Fn(ValueChange<V>, &mut Window, &mut gpui::App) + 'static {
    let view = cx.weak_entity();
    move |change, _, cx| {
        view.update(cx, |view, cx| {
            field(view).follow(change);
            cx.notify();
        })
        .unwrap();
    }
}

/// A callback of a press on a handle into the view: counts it.
fn pressed(cx: &Context<Controls>, index: usize) -> impl Fn(&mut Window, &mut gpui::App) + 'static {
    let view = cx.weak_entity();
    move |_, cx| {
        view.update(cx, |view, _| view.presses[index] += 1).unwrap();
    }
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = Handle::new(
            "handle",
            Axis::new(UNIT, self.handle.value.x, 0.2),
            Axis::new(UNIT, self.handle.value.y, 0.8),
        )
        .on_press(pressed(cx, 0))
        .on_change(follow(cx, |view| &mut view.handle));
        let still =
            Handle::new("still", Axis::fixed(0.9), Axis::fixed(0.9)).on_press(pressed(cx, 1));
        // Drags sideways only. Its other axis has a default, which a reset must leave alone.
        let fixed = Axis {
            drags: false,
            ..Axis::new(UNIT, self.sideways.value.y, 0.9)
        };
        let sideways = Handle::new(
            "sideways",
            Axis::new(UNIT, self.sideways.value.x, 0.1),
            fixed,
        )
        .on_change(follow(cx, |view| &mut view.sideways));
        div()
            .flex()
            .gap(px(40.))
            .p(px(40.))
            .child(
                Volume::new("volume", self.volume.value)
                    .height(HEIGHT)
                    .on_change(follow(cx, |view| &mut view.volume)),
            )
            .child(
                Display::new("display", 200.)
                    .handle(handle)
                    .handle(sideways)
                    .handle(still),
            )
    }
}

fn open(cx: &mut TestAppContext) -> (gpui::Entity<Controls>, &mut VisualTestContext) {
    cx.update(sound_ui::init);
    cx.add_window_view(|_, _| Controls {
        volume: Followed::new(START_DB),
        handle: Followed::new(point(0.5, 0.5)),
        sideways: Followed::new(point(0.5, 0.3)),
        presses: [0; 2],
    })
}

fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> gpui::Bounds<Pixels> {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing is called {selector}"))
}

fn press(cx: &mut VisualTestContext, at: Point<Pixels>, click_count: usize) {
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
    cx.run_until_parked();
}

fn drag_to(cx: &mut VisualTestContext, at: Point<Pixels>, shift: bool) {
    let modifiers = if shift {
        Modifiers::shift()
    } else {
        Modifiers::default()
    };
    cx.simulate_mouse_move(at, MouseButton::Left, modifiers);
    cx.run_until_parked();
}

fn release(cx: &mut VisualTestContext, at: Point<Pixels>, click_count: usize) {
    cx.simulate_event(MouseUpEvent {
        position: at,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count,
    });
    cx.run_until_parked();
}

/// The level at `start` plus a part of the scale, in tenths as a drag gives it.
fn moved_db(start: f32, part: f32) -> f32 {
    let db = meter::db_at(meter::position_of(start) + part);
    (db * 10.).round() / 10.
}

#[gpui::test]
fn the_volume_moves_from_where_it_is_and_shift_escape_and_backspace_work(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let fader = bounds(cx, "volume-volume");
    // Far under the thumb, where the level would be about -60 dB if a press jumped.
    let low = point(fader.center().x, fader.bottom() - px(10.));
    press(cx, low, 1);
    assert!(view.read_with(cx, |view, _| view.volume.changes.is_empty()));

    // A tenth of the scale up: a tenth of the scale from -6 dB, not the level under the pointer.
    let tenth = px(SCALE / 10.);
    drag_to(cx, low - point(px(0.), tenth), false);
    let coarse = moved_db(START_DB, 0.1);
    assert_eq!(view.read_with(cx, |view, _| view.volume.value), coarse);

    // Shift goes on from there, ten times finer.
    drag_to(cx, low - point(px(0.), tenth), true);
    assert_eq!(view.read_with(cx, |view, _| view.volume.value), coarse);
    drag_to(cx, low - point(px(0.), tenth * 2.), true);
    let fine = moved_db(START_DB, 0.11);
    assert_eq!(view.read_with(cx, |view, _| view.volume.value), fine);

    // Escape puts it back.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.volume.changes.last().copied()),
        Some(ValueChange::DragCancel)
    );
    assert_eq!(view.read_with(cx, |view, _| view.volume.value), START_DB);
    release(cx, low, 1);

    // Backspace on the focused volume is 0 dB.
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.volume.changes.last().copied()),
        Some(ValueChange::Set(0.))
    );
    assert_eq!(view.read_with(cx, |view, _| view.volume.value), 0.);
}

#[gpui::test]
fn a_handle_drags_from_where_it_is_and_escape_puts_it_back(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let handle = bounds(cx, "handle-handle").center();
    // Beside the middle of the dot: the value stays what it was.
    let beside = handle + point(px(3.), px(-2.));
    press(cx, beside, 1);
    assert!(view.read_with(cx, |view, _| view.handle.changes.is_empty()));

    // A tenth of the width right and a tenth of the height up.
    let across = point(px(20.), px(-11.8));
    drag_to(cx, beside + across, false);
    let value = view.read_with(cx, |view, _| view.handle.value);
    assert!(
        (value.x - 0.6).abs() < 1e-3 && (value.y - 0.6).abs() < 1e-3,
        "{value:?}"
    );
    // Shift: the next tenth moves a hundredth.
    drag_to(cx, beside + across, true);
    drag_to(cx, beside + across + across, true);
    let value = view.read_with(cx, |view, _| view.handle.value);
    assert!(
        (value.x - 0.61).abs() < 1e-3 && (value.y - 0.61).abs() < 1e-3,
        "{value:?}"
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.handle.value),
        point(0.5, 0.5)
    );
    release(cx, beside, 1);
}

#[gpui::test]
fn a_double_click_resets_only_what_a_handle_moves(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let sideways = bounds(cx, "handle-sideways").center();
    press(cx, sideways, 1);
    release(cx, sideways, 1);
    press(cx, sideways, 2);
    release(cx, sideways, 2);
    assert_eq!(
        view.read_with(cx, |view, _| view.sideways.changes.clone()),
        [ValueChange::Set(point(0.1, 0.3))]
    );
}

#[gpui::test]
fn the_next_press_ends_a_drag_whose_mouse_up_was_lost(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let fader = bounds(cx, "volume-volume").center();
    press(cx, fader, 1);
    drag_to(cx, fader - point(px(0.), px(20.)), false);
    assert!(view.read_with(cx, |view, _| view.volume.origin.is_some()));

    // No mouse up. The button goes down on the handle.
    let handle = bounds(cx, "handle-handle").center();
    cx.simulate_event(MouseDownEvent {
        position: handle,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.volume.changes.last().copied()),
        Some(ValueChange::DragEnd)
    );
    // Only the handle moves now.
    let before = view.read_with(cx, |view, _| view.volume.value);
    drag_to(cx, handle + point(px(20.), px(0.)), false);
    assert_eq!(view.read_with(cx, |view, _| view.volume.value), before);
    assert!(view.read_with(cx, |view, _| view.handle.value.x > 0.5));
}

/// Two cards whose controls have the same ids, as two filters in one rack.
struct Rack {
    knobs: [Vec<ValueChange>; 2],
    handles: [Vec<ValueChange<Point<f32>>>; 2],
}

impl Render for Rack {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let card = |index: usize, id: &'static str, cx: &Context<Self>| {
            let view = cx.weak_entity();
            let knob = Knob::new("cutoff")
                .value(0.5)
                .on_change(move |change, _, cx| {
                    view.update(cx, |view, _| view.knobs[index].push(change))
                        .unwrap();
                });
            let view = cx.weak_entity();
            let handle = Handle::new(
                "cutoff",
                Axis::new(UNIT, 0.5, 0.5),
                Axis::new(UNIT, 0.5, 0.5),
            )
            .on_change(move |change, _, cx| {
                view.update(cx, |view, _| view.handles[index].push(change))
                    .unwrap();
            });
            DeviceCard::new(id, "Filter")
                .display(Display::new("display", 120.).handle(handle))
                .column(Column::new().top(knob))
        };
        div()
            .flex()
            .gap(px(12.))
            .child(card(0, "first", cx))
            .child(card(1, "second", cx))
    }
}

#[gpui::test]
fn two_cards_with_the_same_inner_ids_keep_a_focus_and_a_drag_each(cx: &mut TestAppContext) {
    cx.update(sound_ui::init);
    let (view, cx) = cx.add_window_view(|_, _| Rack {
        knobs: [Vec::new(), Vec::new()],
        handles: [Vec::new(), Vec::new()],
    });
    cx.run_until_parked();

    // Tab reaches two knobs, and a key goes to the one that has the focus.
    for _ in 0..2 {
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
    }
    let knobs = view.read_with(cx, |view, _| {
        view.knobs.clone().map(|changes| changes.len())
    });
    assert_eq!(knobs, [1, 1]);

    // A drag of the handle of the first card moves that one only.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let bounds = cx.debug_bounds("handle-cutoff").unwrap();
    let at = bounds.center();
    press(cx, at, 1);
    drag_to(cx, at + point(px(12.), px(0.)), false);
    release(cx, at + point(px(12.), px(0.)), 1);
    let handles = view.read_with(cx, |view, _| {
        view.handles.clone().map(|changes| changes.len())
    });
    assert!(handles[0] > 0 || handles[1] > 0, "{handles:?}");
    assert!(
        handles[0] == 0 || handles[1] == 0,
        "both handles moved: {handles:?}"
    );
}

/// `on_press` hears every press, a click with no drag as well, on a handle that drags and on
/// one that does not. The click changes no value.
#[gpui::test]
fn a_press_on_a_handle_is_heard_whether_or_not_it_drags(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    for (selector, index) in [("handle-handle", 0), ("handle-still", 1)] {
        let at = bounds(cx, selector).center();
        press(cx, at, 1);
        release(cx, at, 1);
        assert_eq!(
            view.read_with(cx, |view, _| view.presses[index]),
            1,
            "{selector}"
        );
    }
    assert!(view.read_with(cx, |view, _| view.handle.changes.is_empty()));
}
