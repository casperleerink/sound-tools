//! The Agents section of Settings: each provider in a card, from setting it up to signed in.
//! The section is [`agents_settings`], on the Settings page; what it shows is
//! `sound_agent::Agents::sections`, drawn as it is.

use rune::{El, TextEdit, Tx, col, icon, row, text, text_input};
use rune_kit::{Button, ButtonVariant, ControlSize};
use sound_agent::{
    ADD_A_SERVICE, AGENTS_INTRO, Agents, Body, COPIED, COPY_DETAILS, Emphasis, LOOKING_FOR_AGENTS,
    PanelBody, Provider, Reason, SEARCH_SERVICES, SIGNED_IN_GROUP, ServiceRow, SetupAction, Steps,
};

use super::thread::press;
use super::{Act, Sidebar, control, icons};
use crate::app::State;
use crate::controls::Control;
use crate::theme::Theme;

/// What the section keeps that the agents do not: its fields, and which details were copied.
#[derive(Default)]
pub(crate) struct Page {
    /// The search of Pi's services.
    services: TextEdit,
    /// The answer to a sign-in's question.
    answer: TextEdit,
    /// The question `answer` is for: a new one empties it and gives it the keys.
    question: Option<String>,
    copied: Option<Provider>,
}

impl Page {
    pub(super) fn typing(&self) -> bool {
        self.services.focused || self.answer.focused
    }

    /// Follows the agents: a new question of a sign-in empties the field and gives it the keys.
    pub(super) fn follow(&mut self, agents: &Agents) -> bool {
        let asked = agents.text_question().map(|(_, id, _)| id.to_string());
        if asked == self.question {
            return false;
        }
        self.answer = TextEdit::new("");
        self.answer.focused = asked.is_some();
        self.question = asked;
        true
    }

    /// The button at `place` among `provider`'s, in the order they show.
    pub(super) fn press(&mut self, agents: &mut Agents, provider: Provider, place: usize) {
        let sections = agents.sections(&self.services.text);
        let section = sections.iter().find(|section| section.provider == provider);
        let Some(button) =
            section.and_then(|section| buttons(&section.body).into_iter().nth(place))
        else {
            return;
        };
        match button.action.clone() {
            SetupAction::Continue => agents.answer(provider, self.answer.text.clone()),
            action => agents.act(provider, action),
        }
        self.copied = None;
    }

    /// The details of why `provider`'s sign-in ended, for whoever helps.
    pub(super) fn copy(&mut self, agents: &Agents, provider: Provider) -> Option<String> {
        let sections = agents.sections(&self.services.text);
        let section = sections
            .iter()
            .find(|section| section.provider == provider)?;
        let details = reason(&section.body)?.details.clone()?;
        self.copied = Some(provider);
        Some(details)
    }
}

/// The buttons of a body, in the order they show.
fn buttons(body: &Body) -> Vec<&sound_agent::Button> {
    fn of_steps(steps: &Steps) -> Vec<&sound_agent::Button> {
        (steps.controls.iter().chain(&steps.more))
            .filter_map(|control| match control {
                sound_agent::Control::Button(button) => Some(button),
                _ => None,
            })
            .collect()
    }
    fn rows(rows: &[ServiceRow]) -> Vec<&sound_agent::Button> {
        rows.iter().flat_map(|row| &row.buttons).collect()
    }
    match body {
        Body::Steps(steps) => of_steps(steps),
        Body::Ready { sign_out, .. } => vec![sign_out],
        Body::Services(services) => {
            let mut all = rows(&services.signed_in);
            if let Some(PanelBody::Steps(steps)) = services.panel.as_ref().map(|panel| &panel.body)
            {
                all.extend(of_steps(steps));
            }
            all.extend(rows(&services.listed));
            all
        }
    }
}

/// Why a sign-in of the body ended, when it says.
fn reason(body: &Body) -> Option<&Reason> {
    match body {
        Body::Steps(steps) => steps.reason.as_ref(),
        Body::Ready { reason, .. } => reason.as_ref(),
        Body::Services(services) => match &services.panel.as_ref()?.body {
            PanelBody::Ended(reason) => Some(reason),
            PanelBody::Steps(steps) => steps.reason.as_ref(),
        },
    }
}

fn button_key(id: &str) -> String {
    format!("agent-setup-{id}")
}

fn copy_key(provider: Provider) -> String {
    format!("agent-copy-details-{}", provider.name().to_lowercase())
}

/// The Agents section: what the agent runs on, and a card per provider.
pub(crate) fn agents_settings(state: &State, sidebar: &Sidebar) -> El {
    let theme = &state.theme;
    let agents = sidebar.chat.agents();
    let page = &sidebar.settings;
    let muted = theme.color("text-secondary");
    let looking = agents.is_finding().then(|| {
        text(LOOKING_FOR_AGENTS)
            .font(theme.text("body"))
            .color(muted)
    });
    let cards = agents
        .sections(&page.services.text)
        .into_iter()
        .map(|section| {
            let provider = section.provider;
            let mut places = 0..;
            let body = body(theme, page, provider, &section.body, &mut places);
            col()
                .key(format!("agent-setup-{}", provider.name().to_lowercase()))
                .w_full()
                .gap(theme.space(3))
                .items_start()
                .padding(20.0)
                .radius(theme.kit.radius.lg)
                .border(1.0, theme.color("border-strong"))
                .child(
                    row()
                        .gap(theme.space(2))
                        .items_center()
                        .child(icon(icons::logo(provider), 16.0))
                        .child(
                            text(provider.name())
                                .font(theme.kit.ty.ui_medium)
                                .color(theme.color("text")),
                        ),
                )
                .child(body)
        });
    col()
        .key("agent-agents-section")
        .w_full()
        .gap(theme.space(4))
        .child(
            text(AGENTS_INTRO)
                .font(theme.text("body"))
                .color(muted)
                .wrap(),
        )
        .children(looking)
        .children(cards)
}

fn body(
    theme: &Theme,
    page: &Page,
    provider: Provider,
    body: &Body,
    places: &mut std::ops::RangeFrom<usize>,
) -> El {
    match body {
        Body::Steps(steps) => steps_view(theme, page, provider, steps, places),
        Body::Ready {
            reason,
            signed_in,
            sign_out,
        } => col()
            .w_full()
            .gap(theme.space(2))
            .children(
                reason
                    .as_ref()
                    .map(|reason| reason_line(theme, page, provider, reason)),
            )
            .child(
                row()
                    .w_full()
                    .gap(theme.space(3))
                    .items_center()
                    .child(
                        text(signed_in.clone())
                            .font(theme.text("body"))
                            .color(theme.color("text-secondary"))
                            .wrap()
                            .grow(1.0)
                            .shrink(),
                    )
                    .child(setup_button(theme, provider, sign_out, places, false)),
            ),
        Body::Services(services) => {
            let group = |label: &'static str| {
                text(label)
                    .font(theme.kit.ty.small_medium)
                    .color(theme.color("text-muted"))
            };
            let prompt = services.prompt.map(|prompt| {
                text(prompt)
                    .font(theme.text("body"))
                    .color(theme.color("text"))
                    .wrap()
            });
            let signed_in = (!services.signed_in.is_empty()).then(|| {
                let rows: Vec<El> = (services.signed_in.iter())
                    .map(|row| service_row(theme, provider, row, places))
                    .collect();
                col()
                    .w_full()
                    .child(group(SIGNED_IN_GROUP))
                    .children(theme.lined(rows))
            });
            let panel = services.panel.as_ref().map(|panel| {
                let inner = match &panel.body {
                    PanelBody::Steps(steps) => steps_view(theme, page, provider, steps, places),
                    PanelBody::Ended(reason) => reason_line(theme, page, provider, reason),
                };
                col()
                    .w_full()
                    .gap(theme.space(2))
                    .padding(theme.space(3))
                    .radius(theme.kit.radius.md)
                    .fill(theme.color("fill"))
                    .child(
                        text(panel.title.clone())
                            .font(theme.kit.ty.ui_medium)
                            .color(theme.color("text")),
                    )
                    .child(inner)
            });
            let search = text_input(&page.services, theme.text("body"))
                .single_line()
                .key("agent-services-search")
                .label(SEARCH_SERVICES)
                .w_full()
                .color(theme.color("text"))
                .placeholder(SEARCH_SERVICES, theme.color("text-muted"))
                .caret(theme.color("text"), theme.kit.selection())
                .on_edit(services_edit);
            let search = row()
                .w_full()
                .h(32.0)
                .px(10.0)
                .gap(theme.space(2))
                .items_center()
                .radius(theme.kit.radius.md)
                .border(1.0, theme.color("border-strong"))
                .child(icon(&icons::SEARCH, 14.0).color(theme.color("text-muted")))
                .child(search);
            let listed: Vec<El> = (services.listed.iter())
                .map(|row| service_row(theme, provider, row, places))
                .collect();
            let empty = services.empty.map(|empty| {
                text(empty)
                    .font(theme.text("body"))
                    .color(theme.color("text-muted"))
            });
            col()
                .w_full()
                .gap(theme.space(3))
                .children(prompt)
                .children(signed_in)
                .child(group(ADD_A_SERVICE))
                .children(panel)
                .child(search)
                .child(col().w_full().children(theme.lined(listed)).children(empty))
        }
    }
}

/// A sentence, a code shown large, a quieter line, why it ended, then the controls.
fn steps_view(
    theme: &Theme,
    page: &Page,
    provider: Provider,
    steps: &Steps,
    places: &mut std::ops::RangeFrom<usize>,
) -> El {
    let sentence = steps.sentence.as_ref().map(|sentence| {
        text(sentence.clone())
            .font(theme.text("body"))
            .color(theme.color("text"))
            .wrap()
    });
    let code = steps.code.as_ref().map(|code| {
        text(code.clone())
            .font(theme.text("display"))
            .color(theme.color("text"))
            .selectable(true)
    });
    let quiet = steps.quiet.as_ref().map(|quiet| {
        text(quiet.clone())
            .font(theme.text("label"))
            .color(theme.color("text-secondary"))
            .wrap()
    });
    let reason = (steps.reason.as_ref()).map(|reason| reason_line(theme, page, provider, reason));
    let mut controls = Vec::new();
    for control in &steps.controls {
        controls.push(control_view(theme, page, provider, control, places, true));
    }
    let mut more = Vec::new();
    for control in &steps.more {
        more.push(control_view(theme, page, provider, control, places, false));
    }
    col()
        .w_full()
        .gap(theme.space(3))
        .items_start()
        .children(sentence)
        .children(code)
        .children(quiet)
        .children(reason)
        .child(col().w(280.0).gap(theme.space(2)).children(controls))
        .children((!more.is_empty()).then(|| col().w(280.0).gap(theme.space(2)).children(more)))
}

fn control_view(
    theme: &Theme,
    page: &Page,
    provider: Provider,
    control: &sound_agent::Control,
    places: &mut std::ops::RangeFrom<usize>,
    wide: bool,
) -> El {
    match control {
        sound_agent::Control::Button(button) => setup_button(theme, provider, button, places, wide),
        sound_agent::Control::Words(words) => text(words.clone())
            .font(theme.text("label"))
            .color(theme.color("text-secondary"))
            .wrap(),
        sound_agent::Control::Field { secret } => {
            let field = text_input(&page.answer, theme.text("body"))
                .single_line()
                .key("agent-setup-answer")
                .label("Answer")
                .w_full()
                .color(theme.color("text"))
                .caret(theme.color("text"), theme.kit.selection())
                .on_edit(answer_edit)
                .on_submit(move |s: &mut State, _: &mut Tx| {
                    if let Some(sidebar) = s.panel.agent_mut::<Sidebar>() {
                        let answer = sidebar.settings.answer.text.clone();
                        sidebar.chat.agents_mut().answer(provider, answer);
                    }
                    s.handled();
                });
            let field = if *secret { field.secure() } else { field };
            row()
                .w_full()
                .h(32.0)
                .px(10.0)
                .items_center()
                .radius(theme.kit.radius.md)
                .border(1.0, theme.color("border-strong"))
                .fill(theme.color("well"))
                .child(field)
        }
    }
}

fn setup_button(
    theme: &Theme,
    provider: Provider,
    button: &sound_agent::Button,
    places: &mut std::ops::RangeFrom<usize>,
    wide: bool,
) -> El {
    let place = places.next().unwrap_or_default();
    let variant = match button.emphasis {
        Emphasis::Primary => ButtonVariant::Submit,
        Emphasis::Subtle => ButtonVariant::Neutral,
        Emphasis::Ghost => ButtonVariant::Ghost,
    };
    let act = Act::Setup(provider, place);
    let built = Button::new(button.label.clone())
        .variant(variant)
        .control_size(if wide {
            ControlSize::ExtraLarge
        } else {
            ControlSize::Large
        })
        .on_press_tx(move |s: &mut State, tx: &mut Tx| press(s, act, tx))
        .build(&theme.kit)
        .key(button_key(&button.id));
    if wide { built.w_full() } else { built }
}

/// Our sentence of why a sign-in ended, and a button that copies the program's own words.
fn reason_line(theme: &Theme, page: &Page, provider: Provider, reason: &Reason) -> El {
    let copy = reason.details.as_ref().map(|_| {
        let label = if page.copied == Some(provider) {
            COPIED
        } else {
            COPY_DETAILS
        };
        Button::new(label)
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Large)
            .on_press_tx(move |s: &mut State, tx: &mut Tx| press(s, Act::CopyDetails(provider), tx))
            .build(&theme.kit)
            .key(copy_key(provider))
    });
    row()
        .w_full()
        .gap(theme.space(3))
        .items_center()
        .child(
            text(reason.sentence.clone())
                .font(theme.text("body"))
                .color(theme.color("text-secondary"))
                .wrap()
                .grow(1.0)
                .shrink(),
        )
        .children(copy)
}

/// A service of Pi: its name and how it is signed in, and its buttons.
fn service_row(
    theme: &Theme,
    provider: Provider,
    service: &ServiceRow,
    places: &mut std::ops::RangeFrom<usize>,
) -> El {
    let about = service.about.as_ref().map(|about| {
        text(about.clone())
            .font(theme.text("label"))
            .color(theme.color("text-muted"))
            .truncate()
    });
    let buttons: Vec<El> = (service.buttons.iter())
        .map(|button| setup_button(theme, provider, button, places, false))
        .collect();
    row()
        .key(format!("agent-service-{}", service.id))
        .w_full()
        .min_h(44.0)
        .gap(theme.space(3))
        .items_center()
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .child(
                    text(service.name.clone())
                        .font(theme.text("body"))
                        .color(theme.color("text")),
                )
                .children(about),
        )
        .child(row().gap(theme.space(2)).children(buttons))
}

fn services_edit(s: &mut State) -> &mut TextEdit {
    match s.panel.agent_mut::<Sidebar>() {
        Some(sidebar) => &mut sidebar.settings.services,
        // Only the sidebar draws the section.
        None => Box::leak(Box::default()),
    }
}

fn answer_edit(s: &mut State) -> &mut TextEdit {
    match s.panel.agent_mut::<Sidebar>() {
        Some(sidebar) => &mut sidebar.settings.answer,
        // Only the sidebar draws the section.
        None => Box::leak(Box::default()),
    }
}

/// The section's buttons, for `--tree` and `--act`.
pub(super) fn controls(agents: &Agents, page: &Page) -> Vec<Control> {
    let mut controls = Vec::new();
    for section in agents.sections(&page.services.text) {
        let provider = section.provider;
        for (place, button) in buttons(&section.body).into_iter().enumerate() {
            controls.push(control(
                button_key(&button.id),
                button.label.clone(),
                "button",
                Act::Setup(provider, place),
            ));
        }
        if reason(&section.body).is_some_and(|reason| reason.details.is_some()) {
            let label = if page.copied == Some(provider) {
                COPIED
            } else {
                COPY_DETAILS
            };
            controls.push(control(
                copy_key(provider),
                label,
                "button",
                Act::CopyDetails(provider),
            ));
        }
    }
    controls
}
