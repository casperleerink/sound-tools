//! Kbd: one keyboard shortcut drawn as key caps. `Kbd::new("mod+k")` renders `⌘ K` on macOS
//! and `Ctrl K` on Windows and Linux, as the `secondary` of a key binding.

use gpui::{App, Div, SharedString, StyleRefinement, Window, div, prelude::*, px};

use crate::scale::radius;
use crate::theme::ActiveTheme;
use crate::typography::{TextVariant, Typography};

#[derive(IntoElement)]
pub struct Kbd {
    base: Div,
    shortcut: SharedString,
}

impl Kbd {
    /// Keys separated by `+`, e.g. `"mod+shift+k"`.
    pub fn new(shortcut: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            shortcut: shortcut.into(),
        }
    }
}

impl Styled for Kbd {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// The keys of a shortcut, which `+` joins. A last `+` is the key itself: `mod++`.
pub(crate) fn keys_of(shortcut: &str) -> Vec<&str> {
    let (held, plus) = match shortcut.strip_suffix('+') {
        Some(held) => (held, Some("+")),
        None => (shortcut, None),
    };
    let held = held.split('+').filter(|key| !key.is_empty());
    held.chain(plus).collect()
}

pub(crate) fn key_label(key: &str) -> String {
    let label = match key.to_lowercase().as_str() {
        "space" => "Space",
        "escape" => "Esc",
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PgUp",
        "pagedown" => "PgDn",
        modifier if cfg!(target_os = "macos") => match modifier {
            "enter" => "↵",
            "mod" => "⌘",
            "alt" | "option" => "⌥",
            "shift" => "⇧",
            "ctrl" => "⌃",
            "backspace" => "⌫",
            "delete" => "⌦",
            "tab" => "⇥",
            _ => return key.to_uppercase(),
        },
        modifier => match modifier {
            "enter" => "Enter",
            "mod" | "ctrl" => "Ctrl",
            "alt" | "option" => "Alt",
            "shift" => "Shift",
            "super" => "Super",
            "backspace" => "Backspace",
            "delete" => "Del",
            "tab" => "Tab",
            _ => return key.to_uppercase(),
        },
    };
    label.into()
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, border, text) = (theme.fill(), theme.border(), theme.text_secondary());

        let selector = self.shortcut.clone();
        self.base
            .debug_selector(move || format!("kbd-{selector}"))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .children(keys_of(&self.shortcut).into_iter().map(|key| {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(16.))
                    .min_w(px(16.))
                    .px(px(3.))
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .text_variant(TextVariant::Label)
                    .text_color(text)
                    .child(key_label(key))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plus_key_is_a_key_of_a_shortcut() {
        assert_eq!(keys_of("mod+shift+k"), ["mod", "shift", "k"]);
        assert_eq!(keys_of("mod++"), ["mod", "+"]);
        assert_eq!(keys_of("+"), ["+"]);
    }
}
