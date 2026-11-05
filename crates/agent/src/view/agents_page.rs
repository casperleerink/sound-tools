//! The Agents section of the Settings page: every provider found, each with what it needs to
//! be set up, its ways in and its account.

use gpui::{Context, Entity, Focusable, Subscription, Window, div, prelude::*, px};
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
    /// Searches Pi's services.
    search: Entity<TextInput>,
    _observing: Subscription,
}

impl AgentsPage {
    pub fn new(agents: Entity<Agents>, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let search = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("Search services")
                .size(InputSize::Md)
        });
        let weak = cx.weak_entity();
        search.update(cx, |input, _| {
            input.set_on_change(move |_, _, cx| {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |_, cx| cx.notify()).ok();
                });
            });
        });
        let observing = cx.observe_in(&agents, window, |page, _, window, cx| {
            page.agents_changed(window, cx)
        });
        let mut page = Self {
            agents,
            answer,
            asked: None,
            search,
            _observing: observing,
        };
        page.agents_changed(window, cx);
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
                Answer::Text { secret } => Some((program.provider, question.id.clone(), secret)),
                Answer::Pick(_) => None,
            }
        })
    }

    /// A new question empties the field and gives it the keys, so the answer goes in at once.
    fn agents_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asking = self.asking(cx);
        let asked = asking.as_ref().map(|(_, id, _)| id.clone());
        if asked != self.asked {
            let secret = asking.is_some_and(|(_, _, secret)| secret);
            self.answer.update(cx, |input, cx| {
                input.set_text("", cx);
                input.set_secret(secret, cx);
            });
            if asked.is_some() {
                window.focus(&self.answer.focus_handle(cx), cx);
            }
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
        let (muted, border) = (theme.gray_700, theme.alpha_at(0.08));
        let query = self.search.read(cx).text().to_string();
        let agents = self.agents.read(cx);
        let looking = agents.is_finding().then_some("Looking for agents…");
        let sections: Vec<_> = (agents.programs().iter())
            .map(|program| {
                let provider = program.provider;
                let signing_in = (program.signing_in())
                    .and_then(|choice| choice.service())
                    .map(str::to_string);
                let signing_out = program.signing_out().map(str::to_string);
                Onboarding::new(provider, program.setup.clone())
                    .choices(program.choices.clone())
                    .services(program.services.clone(), signing_in, signing_out)
                    .search(self.search.clone(), query.clone())
                    .answer(self.answer.clone())
                    .on_action(cx.listener(move |page, action: &SetupAction, _, cx| {
                        page.act(provider, action.clone(), cx);
                    }))
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(div().text_color(muted).child(
                "The agent in the sidebar runs on Claude Code or Pi. Set up one or both, \
                         then pick a model in the sidebar.",
            ))
            .children(looking.map(|looking| div().text_color(muted).child(looking)))
            .children(sections.into_iter().map(|section| {
                div()
                    .p(px(20.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(border)
                    .child(section)
            }))
    }
}
