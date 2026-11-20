//! A line of text the composer types.

use rune::{Align, El};
use serde_json::json;

use crate::app::State;

use super::semantics::Press;
use super::{FieldNode, Scope};

/// A line of text the composer types: what it says is the window's while it has the keys.
pub(super) fn typed(scope: &Scope, f: &FieldNode) -> El {
    let state = scope.state;
    let theme = &state.theme;
    let key = scope.key(&f.key);
    let edit = state.typed(&key, &f.value);
    let colour = theme.color(f.color.as_deref().unwrap_or("text"));
    let mut e = rune::text_input(&edit, theme.text(f.style.as_deref().unwrap_or("body")))
        .single_line()
        .key(key.clone())
        .label(f.label.clone())
        .color(colour)
        .caret(colour, theme.color("focus/40"));
    if let Some(placeholder) = &f.placeholder {
        e = e.placeholder(placeholder.clone(), theme.color("text-muted"));
    }
    if let Some(w) = f.w {
        e = e.w(w);
    }
    match f.align.as_deref() {
        Some("center") => e = e.align(Align::Center),
        Some("end") => e = e.align(Align::End),
        _ => {}
    }
    let press = Press {
        instance: scope.instance.clone(),
        element: f.key.clone(),
        pressed: false,
        opens: false,
        within: scope.within.clone(),
    };
    let (typing, value) = (key.clone(), f.value.clone());
    e.on_edit(rune::edit_access(move |s: &mut State| {
        s.typing(&typing, &value)
    }))
    .on_submit(move |s: &mut State, _: &mut rune::Tx| {
        let text = s.typing(&key, "").text.clone();
        s.press(&press, "commit", Some(json!(text)));
    })
}
