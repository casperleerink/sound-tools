use gpui::{App, Window, div, prelude::*};

pub fn section(_window: &mut Window, _cx: &mut App) -> impl IntoElement {
    div().child("TODO")
}
