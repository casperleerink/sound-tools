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
    taker: Option<Taker>,
    /// Each key the taker took as it went down, by the key without modifiers: its up comes
    /// there, whatever is held by then.
    held: HashMap<String, Keystroke>,
}

impl Global for GlobalKeys {}

impl GlobalKeys {
    /// Sets what takes the keys, in place of what took them before.
    pub fn set(taker: impl Fn(&GlobalKey<'_>, &mut App) -> bool + 'static, cx: &mut App) {
        let keys = cx.default_global::<Self>();
        keys.taker = Some(Rc::new(taker));
        keys.held.clear();
    }

    /// A key went down. Gives whether the taker took it.
    pub fn key_down(keystroke: &Keystroke, cx: &mut App) -> bool {
        let Some(taker) = cx.default_global::<Self>().taker.clone() else {
            return false;
        };
        if !taker(&GlobalKey::new(keystroke, true), cx) {
            return false;
        }
        (cx.default_global::<Self>().held).insert(keystroke.key.clone(), keystroke.clone());
        true
    }

    /// A key came up. Gives whether the taker took it, which hears it.
    pub fn key_up(keystroke: &Keystroke, cx: &mut App) -> bool {
        if cx
            .default_global::<Self>()
            .held
            .remove(&keystroke.key)
            .is_none()
        {
            return false;
        }
        Self::up(keystroke, cx);
        true
    }

    /// Every key the taker holds comes up: cmd went down, which macOS sends no key up under, or
    /// the window lost the keys.
    pub fn let_go(cx: &mut App) {
        let held: Vec<Keystroke> = cx
            .default_global::<Self>()
            .held
            .drain()
            .map(|(_, held)| held)
            .collect();
        for keystroke in held {
            Self::up(&keystroke, cx);
        }
    }

    fn up(keystroke: &Keystroke, cx: &mut App) {
        if let Some(taker) = cx.default_global::<Self>().taker.clone() {
            taker(&GlobalKey::new(keystroke, false), cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn a_key_the_taker_took_comes_up_to_it_and_a_let_go_ends_what_it_holds(
        cx: &mut TestAppContext,
    ) {
        let heard = Rc::new(RefCell::new(Vec::new()));
        cx.update(|cx| {
            let taker = heard.clone();
            GlobalKeys::set(
                move |key, _| {
                    let took = key.name == "space" || key.name == "a";
                    if took {
                        taker.borrow_mut().push((key.name.clone(), key.down));
                    }
                    took
                },
                cx,
            );
            let [space, a, b] = ["space", "a", "b"].map(|key| Keystroke::parse(key).unwrap());
            assert!(GlobalKeys::key_down(&space, cx));
            assert!(GlobalKeys::key_down(&a, cx));
            assert!(!GlobalKeys::key_down(&b, cx));
            assert!(GlobalKeys::key_up(&space, cx));
            assert!(!GlobalKeys::key_up(&space, cx));
            assert!(!GlobalKeys::key_up(&b, cx));
            GlobalKeys::let_go(cx);
        });
        let expected = [("space", true), ("a", true), ("space", false), ("a", false)];
        let expected = expected.map(|(name, down)| (name.to_string(), down));
        assert_eq!(*heard.borrow(), expected);
    }
}
