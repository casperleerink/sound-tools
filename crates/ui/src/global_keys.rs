//! Keys that work wherever the focus is in the window: outside a text field, and outside a view
//! that takes them itself. A project's own, such as space to play. What takes them adds itself
//! here; the window asks once the focused view had its turn.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{App, Global, Keystroke};

/// A key going down or up.
pub struct GlobalKey<'a> {
    /// The key with the modifiers held first, as a project names it: `space`, `cmd-c`.
    pub name: String,
    pub keystroke: &'a Keystroke,
    pub down: bool,
}

impl<'a> GlobalKey<'a> {
    pub fn new(keystroke: &'a Keystroke, down: bool) -> Self {
        Self {
            name: chord_name(keystroke),
            keystroke,
            down,
        }
    }
}

/// A key as a project names it: `a`, or with cmd, ctrl and alt held first, `cmd-c`.
pub fn chord_name(keystroke: &Keystroke) -> String {
    let modifiers = keystroke.modifiers;
    let held = [
        (modifiers.platform, "cmd-"),
        (modifiers.control, "ctrl-"),
        (modifiers.alt, "alt-"),
    ];
    let prefix: String = (held.iter())
        .filter(|(held, _)| *held)
        .map(|(_, name)| *name)
        .collect();
    format!("{prefix}{}", keystroke.key)
}

/// Hears a key going down and says whether it took it. A key it took comes up to it too.
type Taker = Rc<dyn Fn(&GlobalKey<'_>, &mut App) -> bool>;

#[derive(Default)]
pub struct GlobalKeys {
    takers: Vec<(u64, Taker)>,
    next: u64,
    /// Which taker holds each key that went down, by the key without modifiers: its up comes
    /// there, whatever is held by then.
    held: HashMap<String, (u64, Keystroke)>,
}

impl Global for GlobalKeys {}

impl GlobalKeys {
    /// Adds a taker, asked after those added before it. Gives what [`Self::remove`] takes.
    pub fn add(taker: impl Fn(&GlobalKey<'_>, &mut App) -> bool + 'static, cx: &mut App) -> u64 {
        let keys = cx.default_global::<Self>();
        keys.next += 1;
        keys.takers.push((keys.next, Rc::new(taker)));
        keys.next
    }

    pub fn remove(id: u64, cx: &mut App) {
        let keys = cx.default_global::<Self>();
        keys.takers.retain(|(taker, _)| *taker != id);
        keys.held.retain(|_, (taker, _)| *taker != id);
    }

    /// A key went down. Gives whether a taker took it.
    pub fn key_down(keystroke: &Keystroke, cx: &mut App) -> bool {
        let takers: Vec<(u64, Taker)> = cx.default_global::<Self>().takers.clone();
        let key = GlobalKey::new(keystroke, true);
        let Some(id) = (takers.into_iter()).find_map(|(id, taker)| taker(&key, cx).then_some(id))
        else {
            return false;
        };
        (cx.default_global::<Self>().held).insert(keystroke.key.clone(), (id, keystroke.clone()));
        true
    }

    /// A key came up. Gives whether it was a taker's, which hears it.
    pub fn key_up(keystroke: &Keystroke, cx: &mut App) -> bool {
        let Some((id, _)) = cx.default_global::<Self>().held.remove(&keystroke.key) else {
            return false;
        };
        Self::up(id, keystroke, cx);
        true
    }

    /// Every key a taker holds comes up: cmd went down, which macOS sends no key up under, or the
    /// window lost the keys.
    pub fn let_go(cx: &mut App) {
        let held: Vec<(u64, Keystroke)> = cx
            .default_global::<Self>()
            .held
            .drain()
            .map(|(_, held)| held)
            .collect();
        for (id, keystroke) in held {
            Self::up(id, &keystroke, cx);
        }
    }

    fn up(id: u64, keystroke: &Keystroke, cx: &mut App) {
        let takers = &cx.default_global::<Self>().takers;
        let Some(taker) = (takers.iter())
            .find(|(taker, _)| *taker == id)
            .map(|(_, taker)| taker.clone())
        else {
            return;
        };
        taker(&GlobalKey::new(keystroke, false), cx);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn the_first_taker_that_takes_a_key_hears_it_come_up_and_a_let_go_ends_what_it_holds(
        cx: &mut TestAppContext,
    ) {
        let heard = Rc::new(RefCell::new(Vec::new()));
        cx.update(|cx| {
            let first = heard.clone();
            GlobalKeys::add(
                move |key, _| {
                    let took = key.name == "space";
                    if took {
                        first.borrow_mut().push(("first", key.down));
                    }
                    took
                },
                cx,
            );
            let second = heard.clone();
            GlobalKeys::add(
                move |key, _| {
                    second.borrow_mut().push(("second", key.down));
                    true
                },
                cx,
            );
            let space = Keystroke::parse("space").unwrap();
            let a = Keystroke::parse("a").unwrap();
            assert!(GlobalKeys::key_down(&space, cx));
            assert!(GlobalKeys::key_down(&a, cx));
            assert!(GlobalKeys::key_up(&space, cx));
            assert!(!GlobalKeys::key_up(&space, cx));
            GlobalKeys::let_go(cx);
        });
        assert_eq!(
            *heard.borrow(),
            [
                ("first", true),
                ("second", true),
                ("first", false),
                ("second", false)
            ]
        );
    }
}
