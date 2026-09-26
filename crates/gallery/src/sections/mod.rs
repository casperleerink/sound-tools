//! One file per gallery section. Each exposes `fn section(window, cx) -> impl IntoElement`.

pub mod composed;
pub mod foundation;
pub mod overlays;
pub mod rack;
