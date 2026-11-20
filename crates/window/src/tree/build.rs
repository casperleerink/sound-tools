//! A view's nodes as Rune elements.

use std::collections::HashSet;

use rune::{Align, El, Phase, ScrollSpec, col, el, memo, row, text};
use serde::Deserialize;
use serde_json::{Value, json};
use sound_core::InstanceId;
use sound_typescript::live::Num;
use sound_typescript::next::Drawn;

use crate::app::State;
use crate::lives::{Lives, Shown, Shows};

use super::field::{kept, typed};
use super::gesture::Gesture;
use super::semantics::{delegates_enter, semantics};
use super::shapes::drawing;
use super::{BoxNode, Memos, Node, Scope, Scroll, Size, TextNode, live_place};

/// The name of every binding the views shown now make, of their memos too, whether Rune builds
/// them again or not: what [`crate::lives::Lives::keep`] keeps.
pub(crate) fn bound(state: &State) -> HashSet<String> {
    let mut named = HashSet::new();
    for id in &state.shown {
        if let Some(Drawn::View { root, memos }) = state.drawn(id)
            && let Ok(node) = Node::deserialize(&**root)
        {
            walk(&node, memos, &mut |shows| {
                named.insert(Lives::name(id, &shows));
            });
        }
    }
    named
}

/// The keys of the elements of `node` and its memos that a paint enters, see [`BoxNode::enter`].
pub(crate) fn enterable(node: &Node, memos: &Memos, keys: &mut Vec<String>) {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            keys.extend(b.key.clone().filter(|_| b.enter));
            for child in &b.children {
                enterable(child, memos, keys);
            }
        }
        Node::Memo { key } => {
            let inner = memos
                .get(key)
                .and_then(|(_, inner)| Node::deserialize(&**inner).ok());
            if let Some(inner) = inner {
                enterable(&inner, memos, keys);
            }
        }
        Node::Text(_) | Node::Shapes(_) | Node::Field(_) | Node::Icon(_) | Node::Child { .. } => {}
    }
}

/// Gives `bind` everything `node` binds, and its memos.
fn walk(node: &Node, memos: &Memos, bind: &mut impl FnMut(Shows)) {
    let live = match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            for child in &b.children {
                walk(child, memos, bind);
            }
            live_place(b.at.as_ref(), b.size(), b.opacity.as_ref())
        }
        Node::Text(t) => {
            if let Some(value) = &t.live {
                bind(t.shows(value));
            }
            live_place(t.at.as_ref(), (None, None), t.opacity.as_ref())
        }
        Node::Shapes(d) => {
            if let Some(trim) = d
                .trim
                .as_ref()
                .filter(|trim| trim.iter().any(|t| t.fixed().is_none()))
            {
                bind(Shows::Trim(trim.clone()));
            }
            live_place(d.at.as_ref(), (Some(&d.w), Some(&d.h)), d.opacity.as_ref())
        }
        Node::Memo { key } => {
            let inner = memos
                .get(key)
                .and_then(|(_, inner)| Node::deserialize(&**inner).ok());
            if let Some(inner) = inner {
                walk(&inner, memos, bind);
            }
            return;
        }
        Node::Child { .. } | Node::Field(_) | Node::Icon(_) => return,
    };
    live.into_iter().for_each(bind);
}

/// What a device whose tool's code did not load shows, where it sits.
pub(crate) fn code_failed(state: &State, id: &InstanceId) -> El {
    text("This device's code has a mistake, so it shows nothing until it is mended.")
        .key(format!("{}code-failed", state.prefix_of(id)))
        .font(state.theme.text("label"))
        .color(state.theme.color("text-secondary"))
        .wrap()
}

/// The view `drawn` of `instance`, with its keys after `prefix`.
pub(crate) fn view(state: &State, instance: &InstanceId, prefix: &str, drawn: &Drawn) -> El {
    match drawn {
        Drawn::View { root, memos } => {
            let scope = Scope {
                state,
                instance,
                prefix,
                memos,
                within: None,
                in_control: false,
            };
            match Node::deserialize(&**root) {
                Ok(node) => build(&scope, &node),
                Err(error) => failed(state, &format!("its view sent what is no view: {error}")),
            }
        }
        Drawn::Failed(error) => failed(state, error),
        Drawn::None => el(),
        Drawn::NoCard(why) => text(why.clone())
            .font(state.theme.text("label"))
            .color(state.theme.color("text-secondary"))
            .wrap(),
    }
}

pub(crate) fn failed(state: &State, message: &str) -> El {
    text(format!("This view does not draw: {message}"))
        .font(state.theme.text("label"))
        .color(state.theme.color("peach"))
        .wrap()
}

pub(super) fn build(scope: &Scope, node: &Node) -> El {
    match node {
        Node::Row(b) => boxed(scope, row(), b),
        Node::Col(b) => boxed(scope, col(), b),
        Node::Plain(b) => boxed(scope, el(), b),
        Node::Text(t) => label(scope, t),
        Node::Shapes(shapes) => drawing(scope, shapes),
        Node::Field(field) => typed(scope, field),
        Node::Icon(icon) => super::icon::drawn(scope, icon),
        Node::Child { path } => match scope.instance.child(path) {
            Ok(child) => match scope.state.drawn(&child) {
                _ if scope.state.code_failed(&child) => code_failed(scope.state, &child),
                Some(drawn) => view(scope.state, &child, &scope.state.prefix_of(&child), drawn),
                None => el(),
            },
            Err(error) => failed(scope.state, &error.to_string()),
        },
        Node::Memo { key } => match scope.memos.get(key) {
            Some((version, node)) => {
                // Its controls are listed by the build that made it; Rune keeps that build.
                let node = node.clone();
                let built = || match Node::deserialize(&*node) {
                    Ok(node) => build(scope, &node),
                    Err(error) => failed(scope.state, &error.to_string()),
                };
                // Rune keys a memo's root with the memo's key: the root keeps its own.
                let root = node.get("key").and_then(Value::as_str).unwrap_or(key);
                memo(scope.key(root), *version, built)
            }
            None => el(),
        },
    }
}

fn boxed(scope: &Scope, mut e: El, b: &BoxNode) -> El {
    let theme = &scope.state.theme;
    let key = b.key.as_deref().map(|key| scope.key(key));
    if let Some(key) = &key {
        e = e.key(key.clone());
    }
    e = match &b.w {
        Some(Size::Points(Num::Fixed(w))) => e.w(*w),
        Some(Size::Full(_)) => e.w_full(),
        _ => e,
    };
    e = match &b.h {
        Some(Size::Points(Num::Fixed(h))) => e.h(*h),
        Some(Size::Full(_)) => e.h_full(),
        _ => e,
    };
    if let Some(grow) = b.grow {
        e = e.grow(grow);
    }
    if let Some(step) = b.pad {
        e = e.padding(theme.space(step));
    }
    if let Some(step) = b.px {
        e = e.px(theme.space(step));
    }
    if let Some(step) = b.py {
        e = e.py(theme.space(step));
    }
    if let Some(step) = b.gap {
        e = e.gap(theme.space(step));
    }
    match b.align.as_deref() {
        Some("start") => e = e.items_start(),
        Some("center") => e = e.items_center(),
        Some("end") => e = e.items_end(),
        _ => {}
    }
    match b.justify.as_deref() {
        Some("center") => e = e.justify_center(),
        Some("between") => e = e.justify_between(),
        Some("end") => e = e.justify_end(),
        _ => {}
    }
    // Before `inset`, which pins it to more edges than `at` does.
    e = scope.placed(e, b.at.as_ref(), b.size(), b.opacity.as_ref());
    if let Some([left, top, right, bottom]) = b.inset {
        e = e.pin(left, top, right, bottom);
    }
    if b.clip {
        e = e.clip_soft(0.0);
    }
    if let Some(h) = b.max_h {
        e = e.max_h(h);
    }
    if let Some(scroll) = b.scroll {
        e = scrolled(e, scroll);
    }
    if b.inert {
        e = e.inert();
    }
    if let Some(colour) = &b.fill {
        e = e.fill(theme.color(colour));
    }
    if let Some(colour) = &b.hover {
        e = e.hover(theme.color(colour));
    }
    if b.hover_group {
        e = e.hover_group();
    }
    if b.reveal {
        e = e.group_opacity(0.0, 1.0);
    }
    if b.group {
        e = e.focus_group();
    }
    if let Some(colour) = &b.border {
        e = match b.border_sides {
            Some(sides) => e.border_sides(sides, theme.color(colour)),
            None => e.border(1.0, theme.color(colour)),
        };
    }
    if let Some(radius) = b.radius.as_deref().and_then(|name| theme.radius(name)) {
        e = e.radius(radius);
    }
    if let Some(shadow) = b.shadow.as_deref().and_then(|name| theme.shadow(name)) {
        e = e.shadow(shadow.y, shadow.blur, theme.color(&shadow.color));
    }
    e = semantics(scope, e, b, key.as_deref());
    if let Some(key) = &key {
        let lives = &scope.state.lives;
        if let (Some(spec), Some(element)) = (b.drag, &b.key) {
            let moved = lives.dragged(key);
            let (instance, element, key) = (scope.instance.clone(), element.clone(), key.clone());
            e = e.on_drag(
                move |s: &mut State, d: &mut rune::Drag, tx: &mut rune::Tx| {
                    if d.phase == Phase::Down {
                        s.gesture = Some(Gesture::of(&key));
                        return;
                    }
                    let Some(gesture) = s.gesture.as_mut().filter(|gesture| gesture.key == key)
                    else {
                        return;
                    };
                    gesture.follow(d, tx.mods().shift);
                    let (far, offset) =
                        (gesture.far, spec.offset(gesture.moved[0], gesture.moved[1]));
                    match d.phase {
                        Phase::Down => {}
                        Phase::Move => {
                            moved.set(if far { offset } else { [0.0; 2] });
                            s.lives.held(&key, far);
                            s.lives.refresh(s);
                        }
                        Phase::Up => {
                            s.lives.held(&key, false);
                            s.gesture = None;
                            // A gesture that once went past the slop is a drag, also when it comes
                            // back: it never presses.
                            if !far {
                                return;
                            }
                            s.dragged = Some(key.clone());
                            if d.cancelled() {
                                moved.set([0.0; 2]);
                                return;
                            }
                            moved.set(offset);
                            s.pointed(
                                &instance,
                                &element,
                                "drag",
                                Some(json!({ "dx": offset[0], "dy": offset[1] })),
                            );
                        }
                    }
                },
            );
        }
    }
    let inside = Scope {
        in_control: scope.in_control || delegates_enter(b),
        within: scope.within.clone(),
        ..*scope
    };
    let shown = (b.children.iter()).filter(|child| !is_overlay(child) && kept(scope, child));
    e.children(shown.map(|child| build(&inside, child)))
}

pub(super) fn is_overlay(node: &Node) -> bool {
    matches!(node, Node::Row(b) | Node::Col(b) | Node::Plain(b) if b.overlay.is_some())
}

/// `e` scrolling along `axes`, its edges fading where more waits.
pub(crate) fn scrolled(e: El, axes: Scroll) -> El {
    let e = match axes {
        Scroll::X => e.scroll_x(),
        Scroll::Y => e.scroll(ScrollSpec {
            stick_bottom: false,
        }),
        Scroll::Both => e.scroll_xy(),
    };
    e.scroll_edge_fade(FADE)
}

/// How far the edge of a scroll fades, in points.
const FADE: f32 = 24.0;

fn label(scope: &Scope, t: &TextNode) -> El {
    let theme = &scope.state.theme;
    let lives = &scope.state.lives;
    let live =
        (t.live.as_ref()).map(|value| lives.bind(scope.state, scope.instance, t.shows(value)));
    let words = match &live {
        Some(Shown::Text(_, widest)) => widest.borrow().clone(),
        _ => t.text.clone(),
    };
    let mut e = text(words)
        .font(theme.text(t.style.as_deref().unwrap_or("body")))
        .color(theme.color(t.color.as_deref().unwrap_or("text")));
    if let Some(key) = &t.key {
        e = e.key(scope.key(key));
    }
    if let Some(w) = t.w {
        e = e.w(w);
    }
    match t.align.as_deref() {
        Some("center") => e = e.align(Align::Center),
        Some("end") => e = e.align(Align::End),
        _ => {}
    }
    if t.truncate {
        e = e.truncate();
    }
    if let Some(live) = live {
        e = live.apply(e);
    }
    scope.placed(e, t.at.as_ref(), (None, None), t.opacity.as_ref())
}
