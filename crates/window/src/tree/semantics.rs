//! What an element is for an agent and a screen reader, and what a press, a key or a step does.

use rune::{El, Key, KeyEvent};
use serde_json::json;
use sound_core::InstanceId;

use crate::app::State;

use super::build::is_overlay;
use super::gesture::pointer;
use super::reach::{TO_THE_END, steps, tracked};
use super::{BoxNode, Node, Opens, Scope, Within};

/// What the element is for an agent and a screen reader, and what a press does.
pub(super) fn semantics(scope: &Scope, mut e: El, b: &BoxNode, key: Option<&str>) -> El {
    if let Some(role) = b.role.as_deref().and_then(role) {
        e = e.role(role);
    }
    if let Some(label) = &b.label {
        e = e.label(label.clone());
    }
    if let Some(checked) = b.checked() {
        e = e.checked(checked);
    }
    if let (Some(value), Some(key)) = (b.value(), key) {
        let lives = &scope.state.lives;
        let meter = b.role.as_deref() == Some("meter");
        e = e.a11y_value(lives.said_live(scope.state, scope.instance, key, value, meter));
    }
    let (Some(element), Some(key)) = (&b.key, key) else {
        return e;
    };
    e = tracked(scope, e, key);
    // Rune leaves the ring off the controls of a list, a grid or a menu; ours keep it.
    e = e.focus_ring(2.0, 2.0, scope.state.theme.color("focus"));
    let (instance, element, key) = (scope.instance.clone(), element.clone(), key.to_string());
    // What a tip says is for the keys too.
    if b.overlay_of(Opens::Hover).is_some() {
        e = e.hover_group_shared(key.clone()).focusable(true);
    }
    let opens = b.overlay_of(Opens::Press).is_some();
    let press = Press {
        instance: instance.clone(),
        element: element.clone(),
        pressed: b.press,
        opens,
        within: scope.within.clone(),
    };
    e = pointer(scope, e, b, &key, &press);
    e = super::drop::target(scope, e, b, &press);
    if b.press || opens {
        let press = press.clone();
        e = e.focusable(true).on_press(move |s: &mut State| {
            // The release of a pointer was its press, or the end of its drag.
            if s.dragged.as_deref() == Some(key.as_str()) {
                return;
            }
            s.press(&press, "press", None);
        });
        // A part a control's Enter opens leaves the keys with the control, also when pressed.
        if opens && scope.in_control {
            e = e.focusable(false);
        }
    }
    let has = |name: &str| b.actions.iter().any(|action| action == name);
    if steps(b) {
        let (stepped, by) = (instance.clone(), element.clone());
        e = e.on_step(move |s: &mut State, steps: i32, _: &mut rune::Tx| {
            let action = if steps > 0 { "increment" } else { "decrement" };
            s.pointed(&stepped, &by, action, Some(json!(steps.abs())));
        });
        // Home and End: to its least and most, which its steps stop at.
        let (instance, element) = (instance.clone(), element.clone());
        e = e.on_step_limit(move |s: &mut State, end: bool, _: &mut rune::Tx| {
            let action = if end { "increment" } else { "decrement" };
            s.pointed(&instance, &element, action, Some(json!(TO_THE_END)));
        });
    }
    let reset = has("reset");
    if reset {
        let (instance, element) = (instance.clone(), element.clone());
        e = e
            .focusable(true)
            .on_double_press(move |s: &mut State| s.pointed(&instance, &element, "reset", None));
    }
    // Enter opens what it opens, or what a part of it opens, such as a knob's typed value.
    let enter = match opens {
        true => Some(press),
        false => delegates_enter(b)
            .then(|| opener_inside(b))
            .flatten()
            .map(|inner| Press {
                element: inner.to_string(),
                pressed: false,
                opens: true,
                ..press
            }),
    };
    if reset || enter.is_some() {
        e = e.on_key(move |s: &mut State, pressed: &KeyEvent, _: &mut rune::Tx| {
            match (&pressed.key, &enter) {
                (Key::Backspace | Key::Delete, _) if reset => {
                    s.pointed(&instance, &element, "reset", None);
                    true
                }
                (Key::Enter, Some(enter)) => {
                    s.press(enter, "open", None);
                    true
                }
                _ => false,
            }
        });
    }
    e
}

/// What a press on an element of a view does, by a pointer, a key or an agent: see
/// [`State::press`].
#[derive(Clone)]
pub(crate) struct Press {
    pub(crate) instance: InstanceId,
    /// Its key in the view.
    pub(crate) element: String,
    /// It has `onPress`.
    pub(crate) pressed: bool,
    /// It opens an overlay.
    pub(crate) opens: bool,
    pub(crate) within: Option<Within>,
}

/// Whether Enter on `b` opens what a part of it opens, such as a knob's typed value: a control
/// with no press of its own. That part is then no stop of Tab.
pub(super) fn delegates_enter(b: &BoxNode) -> bool {
    !b.press && (b.drag.is_some() || !b.actions.is_empty())
}

/// The key of the first element inside `b` that opens an overlay, not in an overlay itself.
fn opener_inside(b: &BoxNode) -> Option<&str> {
    let mut shown = (b.children.iter()).filter(|child| !is_overlay(child));
    shown.find_map(|child| match child {
        Node::Row(inner) | Node::Col(inner) | Node::Plain(inner) => {
            match (&inner.key, inner.overlay_of(Opens::Press)) {
                (Some(key), Some(_)) => Some(key.as_str()),
                _ => opener_inside(inner),
            }
        }
        _ => None,
    })
}

/// A role of `sdk/view.ts` as Rune names it: ARIA's names.
fn role(name: &str) -> Option<&'static str> {
    [
        "button",
        "switch",
        "slider",
        "tab",
        "meter",
        "group",
        "heading",
        "textbox",
        "radiogroup",
        "radio",
        "menu",
        "menuitem",
        "listbox",
        "option",
        "grid",
        "gridcell",
        "toolbar",
        "dialog",
        "tooltip",
    ]
    .into_iter()
    .find(|known| *known == name)
}
