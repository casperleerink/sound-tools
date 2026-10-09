//! The keys a window of its own, such as a plugin's, has no use for. The main window says where
//! they go: it plays them on the computer keys, as it plays its own.

use std::rc::Rc;

use gpui::{App, Global, KeyDownEvent, KeyUpEvent};

/// A key a window had no use for.
pub enum SpareKey<'a> {
    Down(&'a KeyDownEvent),
    Up(&'a KeyUpEvent),
    /// The keys that are down will not come up in the window: cmd went down, which macOS sends
    /// no key up under, or the window lost the keyboard.
    LetGo,
}

/// Where the spare keys go. With none installed they go nowhere.
#[derive(Clone)]
pub struct SpareKeys(Rc<dyn Fn(SpareKey<'_>, &mut App)>);

impl Global for SpareKeys {}

impl SpareKeys {
    pub fn install(pass: impl Fn(SpareKey<'_>, &mut App) + 'static, cx: &mut App) {
        cx.set_global(Self(Rc::new(pass)));
    }

    pub fn pass(key: SpareKey<'_>, cx: &mut App) {
        if let Some(Self(pass)) = cx.try_global::<Self>().cloned() {
            pass(key, cx);
        }
    }
}
