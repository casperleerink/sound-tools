//! Keys that move between controls: in a `group` the arrows go to the nearest control in their
//! direction by where the controls are drawn, Home and End to the ends of its row, and Escape
//! gives the keys back to the window. Rune alone moves in tree order, along one line.

use std::cell::RefCell;
use std::collections::HashMap;

use rune::{El, Key, KeyEvent, Live, Tx};

use crate::app::State;

use super::{BoxNode, Node, Opens, Scope};

/// Where each control of a group is drawn, in window points, by key.
#[derive(Default)]
pub(crate) struct Reach(RefCell<HashMap<String, Live<[f32; 4]>>>);

struct Item {
    key: String,
    /// The arrows, Home and End are its own: it steps.
    steps: bool,
}

/// A group: one stop of Tab, whose controls the arrows move between.
pub(super) fn group(scope: &Scope, e: El, b: &BoxNode) -> El {
    let mut items = Vec::new();
    items_in(scope, &b.children, &mut items);
    let mut rects = scope.state.reach.0.borrow_mut();
    for item in &items {
        (rects.entry(item.key.clone())).or_insert_with(|| Live::new([0.0; 4]));
    }
    // In an overlay Escape closes it, which gives the keys back to what opened it.
    let leaves = scope.within.is_none();
    e.focus_group()
        .on_key(move |s: &mut State, pressed: &KeyEvent, tx: &mut Tx| {
            s.reach(&items, leaves, pressed, tx)
        })
}

/// `e`, the element `key`, telling where it is drawn when it is a control of a group.
pub(super) fn tracked(scope: &Scope, e: El, key: &str) -> El {
    match scope.state.reach.0.borrow().get(key) {
        Some(rect) => e.drawn_rect_into(rect),
        None => e,
    }
}

/// How many steps Home and End take a control that steps: as many as it takes to its end.
pub(super) const TO_THE_END: i32 = 10_000;

/// Whether an element takes the keys, as [`super::semantics::semantics`] makes it.
fn takes_keys(b: &BoxNode) -> bool {
    let has = |name: &str| b.actions.iter().any(|action| action == name);
    b.key.is_some()
        && (b.press
            || b.overlay_of(Opens::Press).is_some()
            || b.overlay_of(Opens::Hover).is_some()
            || has("reset")
            || steps(b))
}

pub(super) fn steps(b: &BoxNode) -> bool {
    let has = |name: &str| b.actions.iter().any(|action| action == name);
    has("increment") && has("decrement")
}

/// The controls inside `children`, not inside one another, another group or an overlay.
fn items_in(scope: &Scope, children: &[Node], items: &mut Vec<Item>) {
    for child in children {
        match child {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) if b.overlay.is_none() && !b.group => {
                match (&b.key, takes_keys(b)) {
                    (Some(key), true) => items.push(Item {
                        key: scope.key(key),
                        steps: steps(b),
                    }),
                    _ => items_in(scope, &b.children, items),
                }
            }
            Node::Field(f) => items.push(Item {
                key: scope.key(&f.key),
                steps: false,
            }),
            Node::Memo { key } => {
                if let Some(inner) = scope.memo(key) {
                    items_in(scope, std::slice::from_ref(&*inner), items);
                }
            }
            _ => {}
        }
    }
}

impl State {
    /// A key while a control of a group has the keys.
    fn reach(&mut self, items: &[Item], leaves: bool, pressed: &KeyEvent, tx: &mut Tx) -> bool {
        let mods = pressed.mods;
        if mods.cmd || mods.ctrl || mods.alt || mods.shift {
            return false;
        }
        let Some(at) = (tx.focused()).and_then(|key| items.iter().position(|item| item.key == key))
        else {
            return false;
        };
        let moves = matches!(
            pressed.key,
            Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End
        );
        if moves && items[at].steps {
            return false;
        }
        let rects = self.reach.0.borrow();
        let drawn = |item: &Item| {
            let rect = rects.get(&item.key)?.get();
            (rect[2] > 0.0 || rect[3] > 0.0)
                .then(|| (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0, rect[3]))
        };
        let Some((x, y, tall)) = drawn(&items[at]) else {
            return false;
        };
        let others = (items.iter()).filter_map(|item| Some((item, drawn(item)?)));
        let to = match pressed.key {
            Key::Left | Key::Right | Key::Up | Key::Down => {
                // How far ahead in the direction, and how far off to the side.
                let towards = |(ox, oy): (f32, f32)| match pressed.key {
                    Key::Left => (x - ox, (oy - y).abs()),
                    Key::Right => (ox - x, (oy - y).abs()),
                    Key::Up => (y - oy, (ox - x).abs()),
                    _ => (oy - y, (ox - x).abs()),
                };
                let ahead = others.filter_map(|(item, (ox, oy, _))| {
                    let (ahead, aside) = towards((ox, oy));
                    (ahead > 0.5).then_some((item, ahead + 2.0 * aside))
                });
                ahead
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(item, _)| item)
            }
            Key::Home | Key::End => {
                let row = others.filter(|(_, (_, oy, _))| (oy - y).abs() < tall / 2.0);
                let by_x = |a: &(&Item, (f32, f32, f32)), b: &(&Item, (f32, f32, f32))| {
                    a.1.0.total_cmp(&b.1.0)
                };
                match pressed.key == Key::Home {
                    true => row.min_by(by_x),
                    false => row.max_by(by_x),
                }
                .map(|(item, _)| item)
            }
            Key::Escape if leaves => {
                tx.blur();
                return true;
            }
            _ => return false,
        };
        if let Some(to) = to.filter(|to| to.key != items[at].key) {
            tx.focus(&to.key, true);
        }
        true
    }
}
