//! Composed examples: agent sidebar, transport pill, project menu, mixer strip.
//!
//! `GALLERY_STATE=idle|working|done|failed` preselects the sidebar state,
//! `GALLERY_OPEN=project|model` opens a menu at startup.

use gpui::{
    AnyElement, App, Entity, FontWeight, IntoElement, ParentElement, Styled, Subscription, Window,
    div, prelude::*, px,
};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::components::switch::Switch;
use sound_ui::ActiveTheme;

use crate::composed::mixer_strip::MixerStrip;
use crate::composed::project_menu::ProjectMenu;
use crate::composed::sidebar::{AgentSidebar, AgentState};
use crate::composed::transport::Transport;

struct ComposedState {
    sidebar: Entity<AgentSidebar>,
    transport: Entity<Transport>,
    project: Entity<ProjectMenu>,
    mixer: Entity<MixerStrip>,
    _subs: [Subscription; 2],
}

impl ComposedState {
    fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let state = AgentState::from_value(
            &std::env::var("GALLERY_STATE").unwrap_or_else(|_| "idle".into()),
        );
        let sidebar = cx.new(|cx| AgentSidebar::new(state, cx));
        let transport = cx.new(|_| Transport::default());
        let project = cx.new(ProjectMenu::new);
        let mixer = cx.new(MixerStrip::new);

        match std::env::var("GALLERY_OPEN").unwrap_or_default().as_str() {
            "project" => project.update(cx, |this, cx| this.open(window, cx)),
            "model" => sidebar.update(cx, |this, cx| this.open_model(window, cx)),
            _ => {}
        }

        // The gallery view observes this state, so forwarding the children's notifications keeps
        // the controls above each block in step with the block itself.
        let subs = [
            cx.observe(&sidebar, |_, _, cx| cx.notify()),
            cx.observe(&transport, |_, _, cx| cx.notify()),
        ];

        Self {
            sidebar,
            transport,
            project,
            mixer,
            _subs: subs,
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
    let (sidebar, transport, project, mixer) = (
        state.sidebar.clone(),
        state.transport.clone(),
        state.project.clone(),
        state.mixer.clone(),
    );
    let state_control = SegmentedControl::new("sidebar-state", sidebar.read(cx).state().value())
        .options([
            ("idle", "Idle"),
            ("working", "Working"),
            ("done", "Done"),
            ("failed", "Failed"),
        ])
        .on_change({
            let sidebar = sidebar.clone();
            move |value, _, cx| {
                sidebar.update(cx, |this, cx| {
                    this.set_state(AgentState::from_value(&value), cx)
                })
            }
        })
        .into_any_element();

    let reload_control = Switch::new("reload-pending", transport.read(cx).reload_pending())
        .on_change({
            let transport = transport.clone();
            move |on, _, cx| transport.update(cx, |this, cx| this.set_reload_pending(on, cx))
        })
        .into_any_element();

    div()
        .flex()
        .flex_col()
        .gap(px(24.))
        .child(block("Agent sidebar", cx, Some(state_control), sidebar))
        .child(block("Transport", cx, Some(reload_control), transport))
        .child(block("Project menu", cx, None, project))
        .child(block("Mixer strip", cx, None, mixer))
}
