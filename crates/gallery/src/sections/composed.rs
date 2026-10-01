//! Composed examples: transport pill, project menu. The agent sidebar is the real one, in
//! the window snapshots of `crates/runtime`.
//!
//! `GALLERY_OPEN=project` opens the project menu at startup.

use gpui::{
    AnyElement, App, Entity, FontWeight, IntoElement, ParentElement, Styled, Subscription, Window,
    div, prelude::*, px,
};
use sound_ui::ActiveTheme;
use sound_ui::components::toggle::Toggle;

use crate::composed::project_menu::ProjectMenu;
use crate::composed::transport::Transport;

struct ComposedState {
    transport: Entity<Transport>,
    project: Entity<ProjectMenu>,
    _subscription: Subscription,
}

impl ComposedState {
    fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let transport = cx.new(|_| Transport::default());
        let project = cx.new(ProjectMenu::new);

        if std::env::var("GALLERY_OPEN").as_deref() == Ok("project") {
            project.update(cx, |this, cx| this.open(window, cx));
        }

        // The gallery view observes this state, so forwarding the transport's notifications
        // keeps the control above it in step with the block itself.
        let subscription = cx.observe(&transport, |_, _, cx| cx.notify());

        Self {
            transport,
            project,
            _subscription: subscription,
        }
    }
}

/// One example: a muted heading with its gallery-only controls, then the framed screen.
fn block(
    title: &'static str,
    cx: &App,
    control: Option<AnyElement>,
    content: impl IntoElement,
) -> impl IntoElement {
    let muted = cx.theme().gray_700;
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(title),
                )
                .children(control),
        )
        .child(content)
}

pub fn section(window: &mut Window, cx: &mut App) -> impl IntoElement {
    let state = window.use_keyed_state("composed-state", cx, ComposedState::new);
    let state = state.read(cx);
    let (transport, project) = (state.transport.clone(), state.project.clone());
    let reload_control = Toggle::new(
        "reload-pending",
        "Reload",
        transport.read(cx).reload_pending(),
    )
    .on_change({
        let transport = transport.clone();
        move |on, _, cx| transport.update(cx, |this, cx| this.set_reload_pending(on, cx))
    })
    .into_any_element();

    div()
        .flex()
        .flex_col()
        .gap(px(24.))
        .child(block("Transport", cx, Some(reload_control), transport))
        .child(block("Project menu", cx, None, project))
}
