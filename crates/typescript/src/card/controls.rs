//! The controls of the app that nodes draw, and how a control meets the keys and the pointer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ClickEvent, Div, ElementId, ExternalPaths, KeyDownEvent, KeyUpEvent,
    Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, SharedString, Stateful,
    Window, anchored, canvas, deferred, div, hsla, point, prelude::*, px,
};
use serde_json::{Value, json};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::gesture::ValueKey;
use sound_ui::components::notice::{Notice, NoticeTone};
use sound_ui::components::popover;
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::components::slider::Slider;
use sound_ui::components::toggle::Toggle;
use sound_ui::components::tooltip::Tooltip;

use crate::bun::EventKind;
use crate::tree::{
    self, ButtonLook, ButtonProps, ButtonSized, NoticeProps, NoticeToned, OverlayProps, Props,
    SegmentedProps, SliderProps, ToggleProps,
};

use super::TypeScriptCard;
use super::drawing::{Drawing, element_name, typing};
use super::input::{Carried, Hears, placed, press_of};

impl Drawing<'_> {
    /// A slider of the app, which turns its field or input as a knob's drag does.
    pub(super) fn slider(&mut self, props: &SliderProps, index: &[usize]) -> AnyElement {
        let name = element_name("slider", props.key.as_deref(), index);
        let (min, max) = (props.min.unwrap_or(0.0), props.max.unwrap_or(1.0));
        let value = (props.value.as_ref()).map_or(min, |value| self.sources().number(value) as f32);
        let slider = Slider::new(ElementId::Name(name.into()), props.width.unwrap_or(120.0))
            .range(crate::tools::knob_range(min, max, props.unit))
            .value(value)
            .bipolar(props.bipolar == Some(true))
            .when_some(props.default, Slider::default_value);
        let turns = props.field.clone().map(|field| tree::Turns {
            value: field,
            min: Some(min),
            max: Some(max),
            unit: props.unit,
            travel: None,
            absolute: false,
            default: props.default,
            label: props.label.clone(),
        });
        let card = self.card.clone();
        let slider = slider.when_some(turns, |slider, turns| {
            slider.on_change(move |change, _, cx| {
                (card.update(cx, |view, cx| view.turn(&turns, change, cx))).ok();
            })
        });
        // A drag of it is its own, not the row's under it.
        div().flex_none().occlude().child(slider).into_any_element()
    }

    /// One of a few options side by side, which tells its code the place of a pick.
    pub(super) fn segmented(
        &mut self,
        id: u64,
        props: &SegmentedProps,
        index: &[usize],
    ) -> AnyElement {
        let name = element_name("segmented", props.key.as_deref(), index);
        let options = (props.options.iter().flatten().enumerate())
            .map(|(place, label)| (SharedString::from(place.to_string()), label.clone()));
        let chosen = props.chosen.unwrap_or(-1).to_string();
        let selector = format!("segmented-{}", props.key.as_deref().unwrap_or_default());
        let control = SegmentedControl::new(ElementId::Name(name.into()), chosen)
            .options(options)
            .disabled(props.disabled == Some(true))
            .when(props.on_change == Some(true), |control| {
                let card = self.card.clone();
                control.on_change(move |place, _, cx| {
                    let Ok(place) = place.parse::<usize>() else {
                        return;
                    };
                    (card.update(cx, |view, cx| {
                        view.event(id, EventKind::Change, json!(place), cx)
                    }))
                    .ok();
                })
            });
        (div().flex_none().occlude().debug_selector(move || selector))
            .child(control)
            .into_any_element()
    }

    /// A quiet line of the app, whose button tells a click and whose close button a close.
    pub(super) fn notice(&mut self, id: u64, props: &NoticeProps, index: &[usize]) -> AnyElement {
        let name = element_name("notice", props.key.as_deref(), index);
        let tone = match props.tone {
            Some(NoticeToned::Error) | None => NoticeTone::Error,
            Some(NoticeToned::Warning) => NoticeTone::Warning,
            Some(NoticeToned::Info) => NoticeTone::Info,
        };
        let card = self.card.clone();
        let tell = move |kind: EventKind, payload: Value| {
            let card = card.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                let payload = payload.clone();
                (card.update(cx, |view, cx| view.event(id, kind, payload, cx))).ok();
            }
        };
        let click = press_of((0.0, 0.0), MouseButton::Left, 1, Modifiers::default());
        Notice::new(
            ElementId::Name(name.into()),
            props.message.clone().unwrap_or_default(),
        )
        .tone(tone)
        .when_some(props.progress, Notice::progress)
        .when_some(
            props
                .action
                .clone()
                .filter(|_| props.on_click == Some(true)),
            |notice, action| {
                let button =
                    Button::new("notice-action", action).on_click(tell(EventKind::Click, click));
                notice.action(button)
            },
        )
        .when(props.on_close == Some(true), |notice| {
            notice.on_dismiss(tell(EventKind::Close, json!({})))
        })
        .into_any_element()
    }

    /// A popover under the node it is in, or a dialog over the window. A press outside it asks
    /// its code to close it, as escape does in [`TypeScriptCard::key`]; a press on the node a
    /// popover hangs from is that node's own, such as a button that opens and closes it.
    pub(super) fn overlay(
        &mut self,
        id: u64,
        props: &OverlayProps,
        children: &[usize],
        index: &[usize],
    ) -> AnyElement {
        let table = self.table;
        let content: Vec<AnyElement> = (children.iter())
            .filter(|child| {
                !matches!(
                    table.get(**child).map(|child| &child.props),
                    Some(Props::Marker)
                )
            })
            .enumerate()
            .map(|(position, child)| self.draw(*child, &[index, &[position]].concat()))
            .collect();
        let card = self.card.clone();
        let close = move |cx: &mut App| {
            (card.update(cx, |view, cx| {
                view.event(id, EventKind::Close, json!({}), cx)
            }))
            .ok();
        };
        let modal = props.modal == Some(true);
        let look = if modal { "dialog" } else { "popover" };
        let selector = format!("{look}-{}", props.key.as_deref().unwrap_or_default());
        let panel = popover::surface(self.cx)
            .flex()
            .flex_col()
            .p(px(if modal { 16. } else { 8. }))
            .text_size(px(12.))
            .debug_selector(move || selector)
            .children(content);
        if modal {
            let focus = self.focus.clone();
            let backdrop = div()
                .occlude()
                .w(self.viewport.width)
                .h(self.viewport.height)
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0., 0., 0., 0.4))
                // A text field in it lets go of the keys on this press: the card takes them, to
                // give them back as it closes.
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&focus, cx);
                    close(cx);
                })
                .child(panel);
            let placed = anchored().position(point(px(0.), px(0.))).child(backdrop);
            // As menus, so what opens in it paints over it.
            return deferred(placed).with_priority(1).into_any_element();
        }
        let hangs_from = Rc::new(Cell::new(Bounds::default()));
        let measure = canvas(
            {
                let hangs_from = hangs_from.clone();
                move |bounds, _, _| hangs_from.set(bounds)
            },
            |_, _, _, _| {},
        );
        let panel = panel.on_mouse_down_out(move |event: &MouseDownEvent, _, cx| {
            if !hangs_from.get().contains(&event.position) {
                close(cx);
            }
        });
        // As large as the node it is in, which lays it out as if it were not there.
        (div().absolute().top_0().left_0().size_full())
            .child(measure.absolute().size_full())
            .child(popover::anchor(
                popover::Side::Bottom,
                popover::Align::Start,
                panel,
            ))
            .into_any_element()
    }

    /// A switch of the app, which tells the pointer as a div does: its code turns it.
    pub(super) fn toggle(&mut self, id: u64, props: &ToggleProps, index: &[usize]) -> AnyElement {
        let name: SharedString = element_name("toggle", props.key.as_deref(), index).into();
        let on = props.on == Some(true);
        let toggle = match (&props.icon, props.dot == Some(true)) {
            (Some(icon), _) => Toggle::icon(name, icon.clone(), on)
                .count(props.count.unwrap_or_default().max(0.0) as usize),
            (None, true) => Toggle::dot(name, on),
            (None, false) => Toggle::new(name, props.label.clone().unwrap_or_default(), on),
        };
        let selector = format!("toggle-{}", props.key.as_deref().unwrap_or_default());
        let hears =
            Hears::of(props.on_press, props.on_release, props.on_click).unless(props.disabled);
        let toggle = toggle
            .when(hears.click, |toggle| toggle.on_click(self.clicked(id)))
            .when_some(props.color, |toggle, color| toggle.color(color.0))
            .when_some(tooltip_of(&props.tooltip, &props.shortcut), Toggle::tooltip)
            .disabled(props.disabled == Some(true));
        // A click on it is its own, not the row's under it.
        let wrap = self
            .control(
                div().flex_none().occlude().debug_selector(move || selector),
                id,
                Control::press(),
            )
            .when(hears.any(), Div::cursor_pointer);
        self.hears_pointer(wrap, id, hears)
            .child(toggle)
            .into_any_element()
    }

    /// A button of the app, which tells the pointer as a div does.
    pub(super) fn button(&mut self, id: u64, props: &ButtonProps, index: &[usize]) -> AnyElement {
        let name = element_name("button", props.key.as_deref(), index);
        let accent = props.color.map(|color| color.0);
        let variant = match (props.variant, accent) {
            (Some(ButtonLook::Subtle), Some(accent)) => ButtonVariant::SubtleColor(accent),
            (Some(ButtonLook::Ghost), Some(accent)) => ButtonVariant::GhostColor(accent),
            (Some(ButtonLook::Solid), accent) => {
                ButtonVariant::Solid(accent.unwrap_or(self.cx.theme().lavender))
            }
            (Some(ButtonLook::Subtle), None) => ButtonVariant::Subtle,
            (Some(ButtonLook::Ghost), None) => ButtonVariant::Ghost,
            (Some(ButtonLook::Primary) | None, _) => ButtonVariant::Primary,
        };
        let size = match props.size {
            Some(ButtonSized::Xs) => ButtonSize::Xs,
            Some(ButtonSized::Sm) => ButtonSize::Sm,
            Some(ButtonSized::Md) | None => ButtonSize::Md,
        };
        let button = match (&props.label, &props.icon) {
            (Some(label), icon) => Button::new(ElementId::Name(name.into()), label.clone())
                .when_some(icon.clone(), |button, icon| button.icon(icon)),
            (None, icon) => Button::icon_only(
                ElementId::Name(name.into()),
                icon.clone().unwrap_or_default(),
            ),
        };
        let selector = format!("button-{}", props.key.as_deref().unwrap_or_default());
        let hears =
            Hears::of(props.on_press, props.on_release, props.on_click).unless(props.disabled);
        let button = button
            .tab_stop(hears.click)
            .when(hears.click, |button| button.on_click(self.clicked(id)))
            .variant(variant)
            .size(size)
            .rounded(props.rounded == Some(true))
            .disabled(props.disabled == Some(true))
            .when_some(props.opacity, |button, opacity| button.opacity(opacity))
            .when_some(
                tooltip_of(&props.tooltip, &props.shortcut),
                |mut button, tooltip| {
                    (button.interactivity()).tooltip(move |_, cx| tooltip.clone().view(cx));
                    button
                },
            )
            .debug_selector(move || selector);
        let wrap = self.control(div().flex_none().relative(), id, Control::press());
        self.hears_pointer(wrap, id, hears)
            .child(button)
            .when(hears.any(), |wrap| wrap.child(placed(id, &self.placed)))
            .into_any_element()
    }

    /// The one way a control of a tool meets the keys and the pointer, see [`Control`]. A press
    /// with the pointer leaves the keys where a press on its page would, as a control of the
    /// system takes no focus from the pointer: space after a click on play still pauses.
    pub(super) fn control<E: InteractiveElement>(
        &mut self,
        wrap: E,
        id: u64,
        control: Control,
    ) -> E {
        let page = match self.out_of_page {
            false => self.takes_keys.then(|| self.focus.clone()),
            true => self.title_focus.clone(),
        };
        let before = Rc::new(RefCell::new(None));
        let wrap = wrap
            .capture_any_mouse_down({
                let before = before.clone();
                move |_, window, cx| *before.borrow_mut() = window.focused(cx)
            })
            // After GPUI gives the press's focus away: a div that is a tab stop takes it after
            // this listener, on the same element. A text field in it keeps the focus.
            .on_any_mouse_down(move |_, window, cx| {
                let focus = page.clone().or_else(|| before.borrow_mut().take());
                window.defer(cx, move |window, cx| match focus {
                    _ if typing(window) => {}
                    Some(focus) => window.focus(&focus, cx),
                    None => window.blur(cx),
                });
            });
        let wrap = match control.menu {
            Some(hears) => {
                wrap.on_key_down(self.listener(move |view, event: &KeyDownEvent, _, cx| {
                    let keys = &event.keystroke;
                    let opens = keys.key == "menu" || (keys.key == "f10" && keys.modifiers.shift);
                    if opens && !event.is_held {
                        // A right press of it, as the pointer gives.
                        let press =
                            press_of((0.0, 0.0), MouseButton::Right, 1, Modifiers::default());
                        view.hang_menus_under(id);
                        view.event(id, EventKind::Press, press.clone(), cx);
                        if hears.release {
                            view.event(id, EventKind::Release, press, cx);
                        }
                        cx.stop_propagation();
                    }
                }))
            }
            None => wrap,
        };
        match control.keys {
            Keys::Press => press_keys(wrap),
            Keys::Own => wrap,
            Keys::Turn(turns) => {
                wrap.on_key_down(self.listener(move |view, event: &KeyDownEvent, _, cx| {
                    if let Some(key) = ValueKey::of(&event.keystroke)
                        && view.turn_by_key(&turns, key, cx)
                    {
                        cx.stop_propagation();
                    }
                }))
            }
        }
    }

    /// The keys `keys` of a div, while the focus is in it: its code hears them, and its page does
    /// not. A text field in it has the keys first.
    pub(super) fn own_keys(
        &mut self,
        id: u64,
        keys: Vec<String>,
    ) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
        self.listener(move |view, event: &KeyDownEvent, window, cx| {
            let stroke = &event.keystroke;
            if typing(window) || !keys.contains(&sound_ui::global_keys::chord_name(stroke)) {
                return;
            }
            let modifiers = stroke.modifiers;
            let key = json!({
                "key": stroke.key, "down": true, "shift": modifiers.shift,
                "alt": modifiers.alt, "cmd": modifiers.platform, "ctrl": modifiers.control,
            });
            view.event(id, EventKind::Key, key, cx);
            cx.stop_propagation();
        })
    }

    /// The click of a button or a toggle, which is a tab stop: enter clicks it too. A mouse
    /// click is the one its press began, as on a div.
    pub(super) fn clicked(
        &mut self,
        id: u64,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        self.listener(
            move |view, event: &ClickEvent, _, cx| match event.is_keyboard() {
                true => {
                    let press = press_of((0.0, 0.0), MouseButton::Left, 1, event.modifiers());
                    view.hang_menus_under(id);
                    view.event(id, EventKind::Click, press, cx);
                }
                false => view.released_over(id, cx),
            },
        )
    }

    /// `element`, the node `id`, tells its code what of the pointer `hears` asks for.
    fn hears_pointer<E: InteractiveElement + FluentBuilder>(
        &mut self,
        element: E,
        id: u64,
        hears: Hears,
    ) -> E {
        let down = |button| {
            self.listener(
                move |view: &mut TypeScriptCard, event: &MouseDownEvent, _, cx| {
                    let press = press_of((0.0, 0.0), button, event.click_count, event.modifiers);
                    match button {
                        MouseButton::Left => {
                            view.hang_menus_under(id);
                            if hears.click && view.clicking.is_none() {
                                view.clicking = Some((id, press.clone()));
                            }
                        }
                        _ => view.anchor.set(None),
                    }
                    view.pointer_down(id, hears, button, press, cx);
                    // A right press opens one menu, its own and not one of a node under it.
                    if button == MouseButton::Right {
                        cx.stop_propagation();
                    }
                },
            )
        };
        let up = self.listener(move |view, _: &MouseUpEvent, _, cx| view.released_over(id, cx));
        element
            .when(hears.any(), |element| {
                element.on_mouse_down(MouseButton::Left, down(MouseButton::Left))
            })
            .when(hears.press, |element| {
                element.on_mouse_down(MouseButton::Right, down(MouseButton::Right))
            })
            .when(hears.click, |element| {
                element.on_mouse_up(MouseButton::Left, up)
            })
    }

    /// A div that hears the pointer, drops or carries something when dragged.
    pub(super) fn heard_div(
        &mut self,
        element: Stateful<Div>,
        id: u64,
        hears: Hears,
        carry: &Option<String>,
        on_drop: &Option<bool>,
    ) -> Stateful<Div> {
        let lavender = self.cx.theme().lavender;
        let element = (self.hears_pointer(element, id, hears)).when(hears.any(), |element| {
            element.child(placed(id, &self.placed))
        });
        let element = match carry {
            Some(carry) => element
                .on_drag(Carried(carry.clone()), |carried: &Carried, _, _, cx| {
                    cx.new(|_| carried.clone())
                }),
            None => element,
        };
        if *on_drop != Some(true) {
            return element;
        }
        element
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(lavender.opacity(0.2)))
            .drag_over::<Carried>(move |style, _, _, _| style.bg(lavender.opacity(0.2)))
            .on_drop(self.listener(move |view, paths: &ExternalPaths, _, cx| {
                view.drop_files(id, paths.paths().to_vec(), (0.0, 0.0), cx);
            }))
            .on_drop(self.listener(move |view, carried: &Carried, _, cx| {
                let dropped = json!({ "files": [], "carried": carried.0, "x": 0, "y": 0 });
                view.event(id, EventKind::Drop, dropped, cx);
            }))
    }
}

/// A control of a tool, as [`Drawing::control`] gives it the keys and the pointer: what it does
/// with the keys while it has the focus, as a control of the window does, and whether the menu
/// key opens its menu. Every other key goes on to its page.
pub(super) struct Control {
    pub(super) keys: Keys,
    /// The menu key, or shift-F10 where there is none, presses it as the right button does: what
    /// of the pointer it hears.
    pub(super) menu: Option<Hears>,
}

impl Control {
    fn press() -> Self {
        Self {
            keys: Keys::Press,
            menu: None,
        }
    }
}

pub(super) enum Keys {
    /// A button, a toggle, a select or a div that is a tab stop: enter clicks it, see
    /// [`press_keys`].
    Press,
    /// A knob or a fader on a field: the arrows step it and backspace resets it, see
    /// [`TypeScriptCard::turn_by_key`].
    Turn(tree::Turns),
    /// A div that is a tab stop for its own `keys` alone, such as a number of what code keeps.
    Own,
}

/// The keys of a pressed control with the focus. Enter clicks it and is no key of its view,
/// such as the song's enter that opens the selected clip. Space stays its view's alone, such as
/// the song's play and pause, which GPUI would follow with a click.
fn press_keys<E: InteractiveElement>(wrap: E) -> E {
    wrap.capture_key_down(|event: &KeyDownEvent, window, _| {
        if event.keystroke.key == "space" {
            window.prevent_default();
        }
    })
    .capture_key_up(|event: &KeyUpEvent, window, _| {
        if event.keystroke.key == "space" {
            window.prevent_default();
        }
    })
    .on_key_down(|event: &KeyDownEvent, _, cx| {
        if clicks(&event.keystroke) {
            cx.stop_propagation();
        }
    })
    .on_key_up(|event: &KeyUpEvent, _, cx| {
        if clicks(&event.keystroke) {
            cx.stop_propagation();
        }
    })
}

/// Enter with no modifier, which GPUI turns into a click.
fn clicks(key: &Keystroke) -> bool {
    key.key == "enter" && !key.modifiers.modified()
}

/// The tooltip of a button or a toggle, with its shortcut's keys after its words.
fn tooltip_of(text: &Option<String>, shortcut: &Option<String>) -> Option<Tooltip> {
    let tooltip = Tooltip::new(text.clone()?);
    Some(match shortcut {
        Some(keys) => tooltip.shortcut(&shortcut_of(keys)),
        None => tooltip,
    })
}

/// A key as the SDK names it, `cmd-alt-c`, as a menu draws it: `mod+alt+c`.
pub(super) fn shortcut_of(mut keys: &str) -> String {
    let mut parts = Vec::new();
    for (prefix, drawn) in [("cmd-", "mod"), ("ctrl-", "ctrl"), ("alt-", "alt")] {
        if let Some(rest) = keys.strip_prefix(prefix) {
            parts.push(drawn);
            keys = rest;
        }
    }
    parts.push(keys);
    parts.join("+")
}
