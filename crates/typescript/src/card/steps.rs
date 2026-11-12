//! Steps: a row of steps in groups of four, each lit as far up as it is strong, so a soft hit
//! is not a full one. A click turns a step, and a drag up or down on it changes how strong it
//! is, from where it is, ten times finer with shift. Tab reaches the row as one stop: left and
//! right move along it, space or enter turns the step they are on, which carries the focus
//! ring, and up and down change how strong it is. The step that plays has a green ring: sound
//! is moving. A secondary row has none, so a stack of rows shows one ring.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Background, DispatchPhase, ElementId, Entity, FocusHandle, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Window,
    canvas, div, linear_color_stop, linear_gradient, prelude::*, px,
};
use serde_json::json;
use sound_ui::typography::{TextVariant, Typography};
use sound_ui::{ActiveTheme, KeyboardFocus, radius};

use crate::bun::EventKind;
use crate::tree::{StepsProps, decimal};

use super::controls::{Control, Keys};
use super::drawing::Drawing;
use super::input::FINE;

/// Steps in a group, with more air before the next group.
const GROUP: usize = 4;

/// The square of a step, and the room it takes with the air around it, which hears the pointer:
/// larger than what is drawn, for a trackpad.
const SQUARE: f32 = 20.;
/// The least of a square a step that is on lights, so the softest hit still shows.
const SOFTEST: f32 = 0.2;
const ACROSS: f32 = 24.;
const DOWN: f32 = 24.;
/// More air between two groups: half a step, so a beat shows without counting.
const AIR: f32 = 12.;
/// The room of a label: a name of two short words, and air before the steps.
const LABEL: f32 = 80.;

/// Points of a drag up a step for its whole strength.
const TRAVEL: f32 = 80.;
/// A press that moves less than this is a click.
const SLOP: f32 = 3.;
/// What a drag rounds a strength to, and with shift. An arrow steps a strength by two of them.
const NOTCH: f32 = 0.05;
const FINE_NOTCH: f32 = 0.01;

/// Tells the code of a row what a step does.
type Turn = Rc<dyn Fn(usize, Turned, &mut App)>;

/// What a click, a drag or a key does to a step.
#[derive(Clone, Copy)]
enum Turned {
    /// Off, or on again.
    Over,
    /// As strong as this, from 0 to 1: a drag, which knows where it started.
    To(f32),
    /// Stronger by this much of the whole, on the grid of that step: a key, which may come
    /// before the row drew what the last one did.
    By(f32),
}

struct StepsState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    /// The step the arrows are on.
    at: usize,
    held: Option<HeldStep>,
}

/// The step the pointer holds.
struct HeldStep {
    step: usize,
    /// Where it was pressed, and how strong the step was then.
    start: Pixels,
    from: f32,
    /// How strong the drag last told it to be; `None` until the pointer moved: a click.
    told: Option<f32>,
}

impl HeldStep {
    /// How strong the step is with the pointer at `y`, if that is new: nothing before the
    /// pointer moved past a click.
    fn dragged_to(&mut self, y: Pixels, fine: bool) -> Option<f32> {
        let up = f32::from(self.start - y);
        if self.told.is_none() && up.abs() < SLOP {
            return None;
        }
        let (finer, notch) = if fine {
            (FINE, FINE_NOTCH)
        } else {
            (1., NOTCH)
        };
        let level = (self.from + up * finer / TRAVEL).clamp(0., 1.);
        let level = (level / notch).round() * notch;
        // A drag that began is one to its end, back at where it started too.
        (self.told.replace(level) != Some(level)).then_some(level)
    }
}

/// Takes no room and follows the pointer while it holds a step of `state`, wherever it goes: a
/// move changes how strong the step is, and a release with no move turns it.
fn hears_the_drag(state: Entity<StepsState>, on_turn: Turn) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            window.on_mouse_event({
                let (state, on_turn) = (state.clone(), on_turn.clone());
                move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let turned = state.update(cx, |state, _| {
                        // The button came up outside the window.
                        if event.pressed_button != Some(MouseButton::Left) {
                            state.held = None;
                        }
                        let held = state.held.as_mut()?;
                        let level = held.dragged_to(event.position.y, event.modifiers.shift)?;
                        Some((held.step, level))
                    });
                    if let Some((step, level)) = turned {
                        on_turn(step, Turned::To(level), cx);
                    }
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                    return;
                }
                let held = state.update(cx, |state, _| state.held.take());
                if let Some(held) = held.filter(|held| held.told.is_none()) {
                    on_turn(held.step, Turned::Over, cx);
                }
            });
        },
    )
    .absolute()
    .size_0()
}

#[derive(IntoElement)]
struct Steps {
    id: ElementId,
    levels: Vec<f32>,
    playing: Option<usize>,
    label: Option<SharedString>,
    secondary: bool,
    on_turn: Option<Turn>,
}

impl RenderOnce for Steps {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| StepsState {
            focus_handle: cx.focus_handle().tab_stop(true),
            keyboard_focus: KeyboardFocus::default(),
            at: 0,
            held: None,
        });
        let focus_handle = state.read(cx).focus_handle.clone();
        let ring_shows = (state.read(cx).keyboard_focus).shows_ring(&focus_handle, window);
        let last = self.levels.len().saturating_sub(1);
        let at = state.read(cx).at.min(last);
        let theme = cx.theme();
        let (lit, dark, plays, focus, label_color) = (
            match self.secondary {
                true => theme.text_muted(),
                false => theme.text(),
            },
            theme.fill_strong(),
            theme.green,
            theme.lavender,
            theme.text_secondary(),
        );
        let on_turn = self.on_turn;
        let playing = self.playing.filter(|_| !self.secondary);
        let steps = self
            .levels
            .iter()
            .copied()
            .enumerate()
            .map(|(step, level)| {
                // The ring of the keys wins over the ring of the step that plays.
                let ring = match (ring_shows && step == at, playing == Some(step)) {
                    (true, _) => focus,
                    (false, true) => plays,
                    (false, false) => Hsla::transparent_black(),
                };
                // Lit from its foot as far up as it is strong: a hard edge, not a fade.
                let fill: Background = match level {
                    level if level <= 0.0 => dark.into(),
                    level if level >= 1.0 => lit.into(),
                    level => {
                        let edge = level.max(SOFTEST);
                        linear_gradient(
                            0.,
                            linear_color_stop(lit, edge),
                            linear_color_stop(dark, edge + 0.001),
                        )
                    }
                };
                let square = div()
                    .size(px(SQUARE))
                    .rounded(radius::SM)
                    .bg(fill)
                    .border_2()
                    .border_color(ring)
                    .when(playing == Some(step), |square| {
                        square.debug_selector(move || format!("step-{step}-playing"))
                    });
                let state = state.clone();
                div()
                    .id(("step", step))
                    .debug_selector(move || format!("step-{step}"))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(ACROSS))
                    .h(px(DOWN))
                    .when(step > 0 && step % GROUP == 0, |room| room.ml(px(AIR)))
                    .when(on_turn.is_some(), |room| {
                        let pressed =
                            move |event: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                                state.update(cx, |state, cx| {
                                    state.at = step;
                                    state.keyboard_focus.pressed(cx);
                                    state.held = Some(HeldStep {
                                        step,
                                        start: event.position.y,
                                        from: level,
                                        told: None,
                                    });
                                });
                            };
                        (room.cursor_pointer()).on_mouse_down(MouseButton::Left, pressed)
                    })
                    .child(square)
            });
        let on_key_down = on_turn.clone().map(|on_turn| {
            let state = state.clone();
            move |event: &KeyDownEvent, _: &mut Window, cx: &mut App| {
                let modifiers = event.keystroke.modifiers;
                if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
                    return;
                }
                // Twice what a drag rounds to: a tenth, and a fiftieth with shift.
                let by = 2. * if modifiers.shift { FINE_NOTCH } else { NOTCH };
                let to = match (event.keystroke.key.as_str(), modifiers.shift) {
                    ("up", _) => return turned(at, Turned::By(by), &on_turn, cx),
                    ("down", _) => return turned(at, Turned::By(-by), &on_turn, cx),
                    ("space" | "enter", false) => return turned(at, Turned::Over, &on_turn, cx),
                    ("left", false) => at.saturating_sub(1),
                    ("right", false) => (at + 1).min(last),
                    _ => return,
                };
                cx.stop_propagation();
                state.update(cx, |state, cx| {
                    state.at = to;
                    cx.notify();
                });
            }
        });
        let row = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .when_some(on_key_down, |row, on_key_down| {
                row.track_focus(&focus_handle).on_key_down(on_key_down)
            })
            .children(steps)
            .children(on_turn.map(|on_turn| hears_the_drag(state.clone(), on_turn)));
        // Every label takes the same width, so rows one over the other line up.
        div()
            .flex()
            .flex_none()
            .items_center()
            .children(self.label.map(|label| {
                div()
                    .flex_none()
                    .w(px(LABEL))
                    .pr(px(8.))
                    .text_variant(TextVariant::Label)
                    .text_color(label_color)
                    .truncate()
                    .child(label)
            }))
            .child(row)
    }
}

/// A key of the row did `turned` to its step `at`.
fn turned(at: usize, turned: Turned, on_turn: &Turn, cx: &mut App) {
    cx.stop_propagation();
    on_turn(at, turned, cx);
}

impl Drawing<'_> {
    /// A row of steps, which tells its code the place of the step a click, a drag or a key
    /// changes, and how.
    pub(super) fn steps(&mut self, id: u64, props: &StepsProps) -> AnyElement {
        let name = self.element_name("steps", props.key.as_deref(), id);
        let selector = format!("steps-{}", props.key.as_deref().unwrap_or_default());
        let playing = (props.playing.as_ref())
            .map(|playing| self.sources().number(playing).floor())
            .filter(|playing| *playing >= 0.0)
            .map(|playing| playing as usize);
        let on_turn = (props.on_change == Some(true)).then(|| {
            let card = self.card.clone();
            Rc::new(move |step: usize, turned: Turned, cx: &mut App| {
                let change = match turned {
                    Turned::Over => json!({ "step": step }),
                    Turned::To(level) => json!({ "step": step, "level": decimal(level) }),
                    Turned::By(by) => json!({ "step": step, "by": decimal(by) }),
                };
                (card.update(cx, |view, cx| view.event(id, EventKind::Change, change, cx))).ok();
            }) as Turn
        });
        let steps = Steps {
            id: ElementId::Name(name.into()),
            levels: props.levels.clone().unwrap_or_default(),
            playing,
            label: props.label.clone().map(SharedString::from),
            secondary: props.secondary == Some(true),
            on_turn,
        };
        let control = Control {
            keys: Keys::Own,
            menu: None,
        };
        // A press on it is its own, not the row's under it.
        let wrap = div().flex_none().occlude().debug_selector(move || selector);
        (self.control(wrap, id, control).child(steps)).into_any_element()
    }
}
