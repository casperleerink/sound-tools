//! Steps: a row of steps, each on or off, in groups of four. Tab reaches the row as one stop:
//! left and right move along it, and space or enter turns the step they are on, which carries
//! the focus ring. The step that plays has a green ring: sound is moving.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, FocusHandle, Hsla, KeyDownEvent, MouseButton, SharedString, Window,
    div, prelude::*, px,
};
use serde_json::json;
use sound_ui::components::cell::CELL_WIDTH;
use sound_ui::typography::{TextVariant, Typography};
use sound_ui::{ActiveTheme, KeyboardFocus, radius};

use crate::bun::EventKind;
use crate::tree::StepsProps;

use super::controls::{Control, Keys};
use super::drawing::Drawing;

/// Steps in a group, with more air before the next group.
const GROUP: usize = 4;

/// The square of a step, and the room it takes with the air around it, which hears the pointer:
/// larger than what is drawn, for a trackpad.
const SQUARE: f32 = 20.;
const ACROSS: f32 = 24.;
const DOWN: f32 = 24.;
/// More air between two groups.
const AIR: f32 = 4.;

type Turn = Rc<dyn Fn(usize, &mut Window, &mut App)>;

struct StepsState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    /// The step the arrows are on.
    at: usize,
}

#[derive(IntoElement)]
struct Steps {
    id: ElementId,
    on: Vec<bool>,
    playing: Option<usize>,
    label: Option<SharedString>,
    on_turn: Option<Turn>,
}

impl RenderOnce for Steps {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| StepsState {
            focus_handle: cx.focus_handle().tab_stop(true),
            keyboard_focus: KeyboardFocus::default(),
            at: 0,
        });
        let focus_handle = state.read(cx).focus_handle.clone();
        let ring_shows = (state.read(cx).keyboard_focus).shows_ring(&focus_handle, window);
        let last = self.on.len().saturating_sub(1);
        let at = state.read(cx).at.min(last);
        let theme = cx.theme();
        let (lit, dark, plays, focus, label_color) = (
            theme.text(),
            theme.fill_strong(),
            theme.green,
            theme.lavender,
            theme.text_secondary(),
        );
        let on_turn = self.on_turn;
        let steps = self.on.iter().enumerate().map(|(step, on)| {
            // The ring of the keys wins over the ring of the step that plays.
            let ring = match (ring_shows && step == at, self.playing == Some(step)) {
                (true, _) => focus,
                (false, true) => plays,
                (false, false) => Hsla::transparent_black(),
            };
            let square = div()
                .size(px(SQUARE))
                .rounded(radius::SM)
                .bg(if *on { lit } else { dark })
                .border_2()
                .border_color(ring);
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
                .when_some(on_turn.clone(), |room, on_turn| {
                    room.cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            state.update(cx, |state, cx| {
                                state.at = step;
                                state.keyboard_focus.pressed(cx);
                            });
                            on_turn(step, window, cx);
                        })
                })
                .child(square)
        });
        let on_key_down = on_turn.clone().map(|on_turn| {
            let state = state.clone();
            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let to = match event.keystroke.key.as_str() {
                    "left" => at.saturating_sub(1),
                    "right" => (at + 1).min(last),
                    "space" | "enter" => {
                        cx.stop_propagation();
                        return on_turn(at, window, cx);
                    }
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
            .children(steps);
        // A label takes the width of a cell, so rows one over the other line up.
        div()
            .flex()
            .flex_none()
            .items_center()
            .children(self.label.map(|label| {
                div()
                    .flex_none()
                    .w(px(CELL_WIDTH))
                    .text_variant(TextVariant::Label)
                    .text_color(label_color)
                    .truncate()
                    .child(label)
            }))
            .child(row)
    }
}

impl Drawing<'_> {
    /// A row of steps, which tells its code the place of the step a press or a key turns.
    pub(super) fn steps(&mut self, id: u64, props: &StepsProps) -> AnyElement {
        let name = self.element_name("steps", props.key.as_deref(), id);
        let selector = format!("steps-{}", props.key.as_deref().unwrap_or_default());
        let playing = (props.playing.as_ref())
            .map(|playing| self.sources().number(playing).floor())
            .filter(|playing| *playing >= 0.0)
            .map(|playing| playing as usize);
        let on_turn = (props.on_change == Some(true)).then(|| {
            let card = self.card.clone();
            Rc::new(move |step: usize, _: &mut Window, cx: &mut App| {
                (card.update(cx, |view, cx| {
                    view.event(id, EventKind::Change, json!(step), cx)
                }))
                .ok();
            }) as Turn
        });
        let steps = Steps {
            id: ElementId::Name(name.into()),
            on: props.on.clone().unwrap_or_default(),
            playing,
            label: props.label.clone().map(SharedString::from),
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
