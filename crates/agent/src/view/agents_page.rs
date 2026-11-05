//! The Agents section of the Settings page: every provider found, each with what it needs to
//! be set up, its ways in and its account.

use gpui::{Context, Entity, Subscription, Window, div, prelude::*, px};
use sound_ui::ActiveTheme;
use sound_ui::components::text_input::{InputSize, TextInput};

use super::agents::Agents;
use super::onboarding::{Onboarding, Setup, SetupAction};
use crate::{Answer, Provider};

pub struct AgentsPage {
    agents: Entity<Agents>,
    /// Where a question of a sign-in is answered, such as an API key. One sign-in asks at a
    /// time.
    answer: Entity<TextInput>,
    /// The question the field answers now, by its id, so a new one starts empty.
    asked: Option<String>,
    _observing: Subscription,
}

impl AgentsPage {
    pub fn new(agents: Entity<Agents>, cx: &mut Context<Self>) -> Self {
        let answer = cx.new(|cx| TextInput::new(cx).size(InputSize::Md));
        let weak = cx.weak_entity();
        answer.update(cx, |input, _| {
            input.set_on_submit(move |_, _, cx| {
                let weak = weak.clone();
                // The handler runs inside the field's own update, which answering changes.
                cx.defer(move |cx| {
                    weak.update(cx, |page, cx| page.submit(cx)).ok();
                });
            });
        });
        let observing = cx.observe(&agents, |page, _, cx| page.agents_changed(cx));
        let mut page = Self {
            agents,
            answer,
            asked: None,
            _observing: observing,
        };
        page.agents_changed(cx);
        page
    }

    /// The provider whose sign-in asks for text, and whether the text is a secret.
    fn asking(&self, cx: &Context<Self>) -> Option<(Provider, String, bool)> {
        self.agents.read(cx).programs().iter().find_map(|program| {
            let Setup::SigningIn(signing_in) = &program.setup else {
                return None;
            };
            let question = signing_in.question.as_ref()?;
            match question.answer {
                Answer::Text { secret, .. } => {
                    Some((program.provider, question.id.clone(), secret))
                }
                Answer::Pick(_) => None,
            }
        })
    }

    fn agents_changed(&mut self, cx: &mut Context<Self>) {
        let asking = self.asking(cx);
        let asked = asking.as_ref().map(|(_, id, _)| id.clone());
        if asked != self.asked {
            let secret = asking.is_some_and(|(_, _, secret)| secret);
            self.answer.update(cx, |input, cx| {
                input.set_text("", cx);
                input.set_secret(secret, cx);
            });
            self.asked = asked;
        }
        cx.notify();
    }

    /// Answers the question with what the field holds.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some((provider, _, _)) = self.asking(cx) else {
            return;
        };
        let text = self.answer.read(cx).text().to_string();
        (self.agents).update(cx, |agents, cx| agents.answer(provider, text, cx));
    }

    fn act(&mut self, provider: Provider, action: SetupAction, cx: &mut Context<Self>) {
        match action {
            SetupAction::Continue => self.submit(cx),
            action => (self.agents).update(cx, |agents, cx| agents.act(provider, action, cx)),
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
                let provider = program.provider;
                Onboarding::new(provider, program.setup.clone())
                    .choices(program.choices.clone(), program.keys)
                    .answer(self.answer.clone())
                    .on_action(cx.listener(move |page, action: &SetupAction, _, cx| {
                        page.act(provider, action.clone(), cx);
                    }))
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(32.))
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
