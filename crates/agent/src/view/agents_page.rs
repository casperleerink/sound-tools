//! The Agents section of the Settings page: every provider found, each with what it needs to
//! be set up, its account and **Sign out**.

use gpui::{Context, Entity, Subscription, Window, div, prelude::*, px};
use sound_ui::ActiveTheme;

use super::agents::Agents;
use super::onboarding::{Onboarding, SetupAction};

pub struct AgentsPage {
    agents: Entity<Agents>,
    _observing: Subscription,
}

impl AgentsPage {
    pub fn new(agents: Entity<Agents>, cx: &mut Context<Self>) -> Self {
        let observing = cx.observe(&agents, |_, _, cx| cx.notify());
        Self {
            agents,
            _observing: observing,
        }
    }
}

impl Render for AgentsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.gray_700;
        let agents = self.agents.read(cx);
        let looking = agents.programs().is_empty();
        let sections: Vec<_> = (agents.programs().iter())
            .map(|program| {
                let (provider, agents) = (program.provider, self.agents.clone());
                Onboarding::new(provider, program.setup.clone()).on_action(
                    move |action: &SetupAction, _, cx| {
                        agents.update(cx, |agents, cx| agents.act(provider, *action, cx));
                    },
                )
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .text_color(muted)
                    .child("The agent in the sidebar runs one of these programs."),
            )
            .when(looking, |page| {
                page.child(div().text_color(muted).child("Looking for agents…"))
            })
            .children(sections)
    }
}
