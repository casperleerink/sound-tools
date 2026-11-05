//! How far a provider is set up, as the Settings page shows it: the download, the ways in and
//! the steps of a sign-in, and the account; for Pi, each of its services. Plain state in,
//! buttons out, so the gallery shows every state with no process. The sidebar shows a skeleton
//! of the thread while the app checks.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Entity, FontWeight, SharedString, Window, div,
    prelude::*, pulsating_between, px, relative,
};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::text_input::TextInput;

use super::model_picker::logo;
use crate::{
    Account, Answer, Provider, Service, SignInChoice, SignInQuestion, SignInStep, SignedIn,
};

/// Where the app is in setting up the agent of one provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setup {
    /// The login shell is read and the agent asked whether it is signed in.
    Checking,
    NotInstalled,
    /// `received` of `size` bytes are on disk.
    Downloading {
        received: u64,
        size: u64,
    },
    DownloadFailed {
        message: String,
    },
    /// `reason` says why an attempt to sign in ended without an account.
    SignedOut {
        reason: Option<Reason>,
    },
    SigningIn(SigningIn),
    /// A thread can run on it. `reason` says why a sign-in to another of Pi's providers
    /// ended without it.
    Ready {
        account: Account,
        reason: Option<Reason>,
    },
    /// The agent's program did not run. `message` is the first line it wrote.
    Stopped {
        message: String,
    },
}

/// Why a sign-in ended without an account: our sentence, then what the program said, which
/// may be in its own words, quieter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason {
    pub sentence: String,
    pub detail: Option<String>,
}

impl Reason {
    pub fn new(sentence: impl Into<String>) -> Self {
        Reason {
            sentence: sentence.into(),
            detail: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    fn element(self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_col()
            .child(div().text_color(theme.gray_900).child(self.sentence))
            .children(self.detail.map(|detail| {
                div()
                    .text_size(px(12.))
                    .text_color(theme.gray_700)
                    .child(detail)
            }))
            .into_any_element()
    }
}

/// What a running sign-in has shown so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SigningIn {
    /// The provider's page, once it said which.
    pub page: Option<String>,
    /// A code to enter on the page.
    pub code: Option<String>,
    /// The provider's last words on it.
    pub note: Option<String>,
    pub question: Option<SignInQuestion>,
}

impl SigningIn {
    pub fn show(&mut self, step: SignInStep) {
        match step {
            SignInStep::Page { url } => self.page = Some(url),
            SignInStep::Code { url, code } => {
                self.page = Some(url);
                self.code = Some(code);
            }
            SignInStep::Note(note) => self.note = Some(note),
            SignInStep::Question(question) => self.question = Some(question),
        }
    }
}

/// What a button of the onboarding asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupAction {
    /// Set up, and Try again after a failed download.
    Download,
    /// Stops the download or the sign-in.
    Cancel,
    SignIn(SignInChoice),
    /// Opens the provider's page again, or starts the sign-in again when it never said which.
    OpenPageAgain,
    /// Answers the question with the id of a pick.
    Pick(String),
    /// Answers the question with what the field holds.
    Continue,
    /// Signs out of the program, or of the service of Pi with this id.
    SignOut(Option<String>),
    /// Try again after the program did not run.
    Check,
    /// Opens the page that tells how to install the program.
    GetProgram,
    /// Looks for the program again, once the composer installed it.
    LookAgain,
}

type ActionHandler = Rc<dyn Fn(&SetupAction, &mut Window, &mut App)>;

/// Every state of [`Setup`] of one provider, under its name.
#[derive(IntoElement)]
pub struct Onboarding {
    provider: Provider,
    setup: Setup,
    /// Claude Code's ways in.
    choices: Vec<SignInChoice>,
    /// Pi's services.
    services: Vec<Service>,
    /// The service of the running or the last sign-in, and the one being signed out of, by
    /// their ids.
    signing_in: Option<String>,
    signing_out: Option<String>,
    /// The field that searches Pi's services, and what it holds.
    search: Option<(Entity<TextInput>, String)>,
    /// The field a question of the sign-in is answered in.
    answer: Option<Entity<TextInput>>,
    on_action: Option<ActionHandler>,
}

impl Onboarding {
    pub fn new(provider: Provider, setup: Setup) -> Self {
        Self {
            provider,
            setup,
            choices: Vec::new(),
            services: Vec::new(),
            signing_in: None,
            signing_out: None,
            search: None,
            answer: None,
            on_action: None,
        }
    }

    pub fn choices(mut self, choices: Vec<SignInChoice>) -> Self {
        self.choices = choices;
        self
    }

    /// Pi's services, with the one of the running or the last sign-in, and the one being
    /// signed out of.
    pub fn services(
        mut self,
        services: Vec<Service>,
        signing_in: Option<String>,
        signing_out: Option<String>,
    ) -> Self {
        self.services = services;
        self.signing_in = signing_in;
        self.signing_out = signing_out;
        self
    }

    pub fn search(mut self, field: Entity<TextInput>, query: String) -> Self {
        self.search = Some((field, query));
        self
    }

    pub fn answer(mut self, answer: Entity<TextInput>) -> Self {
        self.answer = Some(answer);
        self
    }

    pub fn on_action(
        mut self,
        handler: impl Fn(&SetupAction, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(handler));
        self
    }
}

/// The height of the sidebar's composer box with one line of text.
const COMPOSER_HEIGHT: f32 = 110.;

/// Bytes as the composer reads a download: whole megabytes.
fn megabytes(bytes: u64) -> u64 {
    (bytes + (1 << 19)) >> 20
}

/// The thread as it will show, in grey and pulsing: a message, an answer and the composer.
/// Most starts end signed in, so the thread takes the same place without a jump.
pub(super) fn skeleton(cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (bar, border, fill) = (theme.alpha_at(0.05), theme.alpha_at(0.10), theme.gray_200);
    let line = |width: f32| div().h(px(12.)).w(relative(width)).rounded(px(6.)).bg(bar);
    let thread = div()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(24.))
        .px(px(24.))
        .pt(px(8.))
        // A message: one line of text in the padding of its box.
        .child(div().h(px(50.)).rounded(px(12.)).bg(bar))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(line(1.))
                .child(line(0.85))
                .child(line(0.6)),
        )
        .with_animation(
            "agent-skeleton",
            Animation::new(Duration::from_millis(1500))
                .repeat()
                .with_easing(pulsating_between(0.4, 1.0)),
            |thread, delta| thread.opacity(delta),
        );
    // The composer's box, as tall as it is with one line.
    let composer = div().px(px(12.)).pb(px(12.)).pt(px(8.)).child(
        div()
            .h(px(COMPOSER_HEIGHT))
            .rounded(px(20.))
            .border_1()
            .border_color(border)
            .bg(fill),
    );
    div()
        .flex_1()
        .flex()
        .flex_col()
        .child(thread)
        .child(composer)
        .into_any_element()
}

/// What one state shows: its sentence, a quieter line, then its controls.
#[derive(Default)]
struct Shown {
    sentence: Option<String>,
    quiet: Option<String>,
    reason: Option<Reason>,
    /// Shown large, as a code to type.
    code: Option<String>,
    controls: Vec<AnyElement>,
    /// Under the controls, such as the question beside the browser's step.
    more: Vec<AnyElement>,
}

/// Makes the buttons of one provider. The keyboard reaches every one, and its focus stays
/// across renders.
struct Buttons {
    key: String,
    on_action: Option<ActionHandler>,
}

impl Buttons {
    /// A button as wide as the column of controls, or a small one in a row of a list.
    #[allow(clippy::too_many_arguments)]
    fn make(
        &self,
        id: &str,
        label: impl Into<SharedString>,
        action: SetupAction,
        variant: ButtonVariant,
        wide: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let state = SharedString::from(format!("onboarding-{}-{id}", self.key));
        let handle = window
            .use_keyed_state(state, cx, |_, cx| cx.focus_handle().tab_stop(true))
            .read(cx)
            .clone();
        let selector = format!("setup-{id}");
        let button = Button::new(SharedString::from(format!("{}-{id}", self.key)), label)
            .debug_selector(move || selector)
            .variant(variant)
            .focus_handle(&handle);
        let button = match wide {
            true => button.size(ButtonSize::Md).w_full(),
            false => button.size(ButtonSize::Sm),
        };
        match self.on_action.clone() {
            Some(on_action) => button
                .on_click(move |_, window, cx| on_action(&action, window, cx))
                .into_any_element(),
            None => button.into_any_element(),
        }
    }

    /// The steps of a running sign-in: the browser's, a code, a question, and the way out.
    fn signing_in(
        &self,
        provider: Provider,
        signing_in: SigningIn,
        answer: Option<Entity<TextInput>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Shown {
        let muted = cx.theme().gray_700;
        let (subtle, primary) = (ButtonVariant::Subtle, ButtonVariant::Primary);
        let SigningIn {
            page,
            code,
            note,
            question,
        } = signing_in;
        // The browser's step leads while there is one; a question beside it, such as the
        // address the browser ended on, is the quieter way.
        let in_browser = page.is_some() || code.is_some();
        let mut asked = Vec::new();
        if let Some(question) = &question {
            if in_browser {
                let label = div().text_color(muted).child(question.message.clone());
                asked.push(label.into_any_element());
            }
            match &question.answer {
                Answer::Pick(picks) => {
                    for (index, pick) in picks.iter().enumerate() {
                        let action = SetupAction::Pick(pick.id.clone());
                        let label = SharedString::from(pick.label.clone());
                        let id = format!("pick-{index}");
                        asked.push(self.make(&id, label, action, subtle, true, window, cx));
                    }
                }
                Answer::Text { .. } => {
                    asked.extend(
                        answer.map(|field| {
                            div().w_full().text_left().child(field).into_any_element()
                        }),
                    );
                    let variant = if in_browser { subtle } else { primary };
                    let action = SetupAction::Continue;
                    asked
                        .push(self.make("continue", "Continue", action, variant, true, window, cx));
                }
            }
        }
        let sentence = match &question {
            _ if code.is_some() => "Enter this code on the page in your browser:".to_string(),
            Some(question) if !in_browser => question.message.clone(),
            _ => "Finish in your browser.".to_string(),
        };
        let (mut controls, more) = match in_browser {
            true => (Vec::new(), asked),
            false => (asked, Vec::new()),
        };
        // Claude Code never names its page: only starting again opens it again.
        if in_browser || provider == Provider::Claude {
            let action = SetupAction::OpenPageAgain;
            let label = "Open the page again";
            controls.push(self.make("open-page-again", label, action, subtle, true, window, cx));
        }
        let (cancel, ghost) = (SetupAction::Cancel, ButtonVariant::Ghost);
        controls.push(self.make("cancel", "Cancel", cancel, ghost, true, window, cx));
        Shown {
            sentence: Some(sentence),
            // Beside a code, the program's note only says it waits, in its own words.
            quiet: note.filter(|_| code.is_none()),
            code,
            controls,
            more,
            ..Shown::default()
        }
    }

    /// Claude Code's ways in, one button each.
    fn choices(
        &self,
        choices: Vec<SignInChoice>,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        (choices.into_iter().enumerate())
            .map(|(index, choice)| {
                let label = choice.label().to_string();
                let action = SetupAction::SignIn(choice);
                let id = format!("sign-in-{index}");
                self.make(&id, label, action, ButtonVariant::Subtle, true, window, cx)
            })
            .collect()
    }
}

/// A sentence, a code, a quieter line, then the controls and what goes under them.
fn shown_element(shown: Shown, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (text, muted, strong) = (theme.gray_900, theme.gray_700, theme.gray_950);
    let column = |children: Vec<AnyElement>, gap: f32| {
        div()
            .flex()
            .flex_col()
            .gap(px(gap))
            .w_full()
            .max_w(px(280.))
            .children(children)
    };
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(12.))
        .w_full()
        .children(
            shown
                .sentence
                .map(|sentence| div().text_color(text).child(sentence)),
        )
        .children(shown.code.map(|code| {
            div()
                .text_size(px(24.))
                .line_height(px(32.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(strong)
                .child(code)
        }))
        .children(
            shown
                .quiet
                .map(|quiet| div().text_color(muted).child(quiet)),
        )
        .children(shown.reason.map(|reason| reason.element(cx)))
        .when(!shown.controls.is_empty(), |body| {
            body.child(column(shown.controls, 8.))
        })
        .when(!shown.more.is_empty(), |body| {
            body.child(column(shown.more, 4.))
        })
        .into_any_element()
}

/// Whether the service's name or its plan, such as ChatGPT, holds what the composer searched
/// for, in any case.
fn matches(service: &Service, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    [Some(service.name.as_str()), service.plan()]
        .into_iter()
        .flatten()
        .any(|said| said.to_lowercase().contains(&query))
}

/// Pi's services: those signed in first, then a search over the others, those with a sign-in
/// in the browser first. The one being signed in to shows the steps under its row.
struct Services {
    services: Vec<Service>,
    /// The running sign-in, or how the last one ended, shown under **Add a service**.
    panel: Option<Panel>,
    signing_out: Option<String>,
    search: Option<(Entity<TextInput>, String)>,
    answer: Option<Entity<TextInput>>,
}

/// What shows of a sign-in to one service, by its id: its steps while it runs, or why it ended
/// without it.
enum Panel {
    Steps(String, SigningIn),
    Ended(String, Reason),
}

impl Services {
    /// The sign-in's steps, or its end, under the name of its service, above the search:
    /// in a row of the list they could scroll out of view.
    fn panel(&self, buttons: &Buttons, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let theme = cx.theme();
        let (strong, fill) = (theme.gray_950, theme.alpha_at(0.04));
        let named = |id: &str| {
            (self.services.iter())
                .find(|service| service.id == id)
                .map_or(id.to_string(), |service| service.name.clone())
        };
        let (title, body) = match self.panel.as_ref()? {
            Panel::Steps(id, steps) => {
                let name = named(id);
                let mut steps = steps.clone();
                // Pi's words for it name an API.
                let asks_key = (steps.question.as_mut())
                    .filter(|question| question.answer == Answer::Text { secret: true })
                    .map(|question| question.message = format!("Paste your {name} key."))
                    .is_some();
                let mut shown =
                    buttons.signing_in(Provider::Pi, steps, self.answer.clone(), window, cx);
                if asks_key {
                    shown.quiet = Some(
                        "A key is a long code from the service's website, usually under API \
                         keys or Developers."
                            .to_string(),
                    );
                }
                (format!("Signing in to {name}"), shown_element(shown, cx))
            }
            Panel::Ended(id, reason) => (named(id), reason.clone().element(cx)),
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(16.))
                .rounded(px(8.))
                .bg(fill)
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(strong)
                        .child(title),
                )
                .child(body)
                .into_any_element(),
        )
    }

    fn element(self, buttons: &Buttons, window: &mut Window, cx: &mut App) -> AnyElement {
        let theme = cx.theme();
        let (muted, hairline) = (theme.gray_700, theme.alpha_at(0.08));
        let query = self.search.as_ref().map_or("", |(_, query)| query.as_str());
        let (signed_in, mut others): (Vec<_>, Vec<_>) =
            (self.services.iter()).partition(|service| service.signed_in.is_some());
        // Stable, so Pi's order holds within each group.
        others.sort_by_key(|service| service.in_browser.is_none());
        let others: Vec<_> = others
            .into_iter()
            .filter(|service| matches(service, query))
            .collect();
        let row = |service: &Service, window: &mut Window, cx: &mut App| {
            self.row(service, buttons, hairline, window, cx)
        };
        let signed_in: Vec<AnyElement> = signed_in
            .into_iter()
            .map(|service| row(service, window, cx))
            .collect();
        let listed: Vec<AnyElement> = others
            .iter()
            .map(|service| row(service, window, cx))
            .collect();
        let heading = |label: &'static str| {
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(label)
        };
        let panel = self.panel(buttons, window, cx);
        let empty = listed.is_empty().then(|| {
            div()
                .py(px(10.))
                .text_color(muted)
                .child("No service matches.")
        });
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .w_full()
            .when(!signed_in.is_empty(), |list| {
                list.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(heading("Signed in"))
                        .children(signed_in),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(heading("Add a service"))
                    .children(panel)
                    .children(self.search.map(|(field, _)| field))
                    .child(
                        div()
                            .id("pi-services")
                            .flex()
                            .flex_col()
                            .max_h(px(360.))
                            .overflow_y_scroll()
                            .children(listed)
                            .children(empty),
                    ),
            )
            .into_any_element()
    }

    /// The service's name and how it is signed in, its buttons, and the steps of its sign-in
    /// under it while one runs.
    fn row(
        &self,
        service: &Service,
        buttons: &Buttons,
        hairline: gpui::Hsla,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, strong) = (theme.gray_700, theme.gray_950);
        let id = &service.id;
        let steps = matches!(&self.panel, Some(Panel::Steps(signing_in, _)) if signing_in == id);
        let signing_out = self.signing_out.as_deref() == Some(id.as_str());
        let about = match (service.signed_in, signing_out) {
            _ if steps => Some("Signing in…"),
            (_, true) => Some("Signing out…"),
            (Some(SignedIn::Here), false) => None,
            (Some(SignedIn::Elsewhere), false) => Some("Signed in outside Sound Tools"),
            (None, false) => service.plan(),
        };
        let (subtle, ghost) = (ButtonVariant::Subtle, ButtonVariant::Ghost);
        let mut actions = Vec::new();
        match (service.signed_in, steps, signing_out) {
            (_, true, _) | (_, _, true) | (Some(SignedIn::Elsewhere), _, _) => {}
            (Some(SignedIn::Here), false, false) => {
                let action = SetupAction::SignOut(Some(id.clone()));
                let button_id = format!("{id}-sign-out");
                actions
                    .push(buttons.make(&button_id, "Sign out", action, subtle, false, window, cx));
            }
            (None, false, false) => {
                if let Some(choice) = &service.in_browser {
                    let action = SetupAction::SignIn(choice.clone());
                    let button_id = format!("{id}-sign-in");
                    actions.push(
                        buttons.make(&button_id, "Sign in", action, subtle, false, window, cx),
                    );
                }
                if let Some(choice) = &service.with_key {
                    let action = SetupAction::SignIn(choice.clone());
                    let button_id = format!("{id}-add-key");
                    actions.push(
                        buttons.make(&button_id, "Add key", action, ghost, false, window, cx),
                    );
                }
            }
        }
        div()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(hairline)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_h(px(48.))
                    .py(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_color(strong).child(service.name.clone()))
                            .children(about.map(|about| {
                                div()
                                    .text_size(px(12.))
                                    .text_color(muted)
                                    .child(about.to_string())
                            })),
                    )
                    .child(div().flex().flex_none().gap(px(8.)).children(actions)),
            )
            .into_any_element()
    }
}

impl RenderOnce for Onboarding {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let strong = theme.gray_950;
        let name = self.provider.name();
        let buttons = Buttons {
            key: name.to_lowercase(),
            on_action: self.on_action,
        };
        let (subtle, primary) = (ButtonVariant::Subtle, ButtonVariant::Primary);
        let mut button = |id: &str, label: &'static str, action, variant| {
            buttons.make(id, label, action, variant, true, window, cx)
        };
        let pi = self.provider == Provider::Pi;
        let body = match self.setup {
            Setup::Checking => shown_element(
                Shown {
                    quiet: Some("Checking…".to_string()),
                    ..Shown::default()
                },
                cx,
            ),
            Setup::NotInstalled if pi => shown_element(
                Shown {
                    sentence: Some(
                        "Pi runs the agent on the AI service you choose, such as ChatGPT, \
                         Gemini or OpenRouter. It is not on this computer yet: get it from \
                         its website, then come back here."
                            .to_string(),
                    ),
                    controls: vec![
                        button("get", "Get Pi", SetupAction::GetProgram, primary),
                        button("look-again", "Check again", SetupAction::LookAgain, subtle),
                    ],
                    ..Shown::default()
                },
                cx,
            ),
            Setup::NotInstalled => {
                let size = self.provider.download().map_or(String::new(), |download| {
                    format!(" {} MB", megabytes(download.size))
                });
                let sentence = format!(
                    "The agent runs {name} by {}. Setup downloads{size}, then opens your browser to sign in.",
                    self.provider.maker()
                );
                let set_up = button("set-up", "Set up", SetupAction::Download, primary);
                shown_element(
                    Shown {
                        sentence: Some(sentence),
                        controls: vec![set_up],
                        ..Shown::default()
                    },
                    cx,
                )
            }
            Setup::Downloading { received, size } => {
                let percent = (received * 100).checked_div(size).unwrap_or(0).min(100);
                let line = format!(
                    "Downloading {name}: {} of {} MB, {percent}%",
                    megabytes(received),
                    megabytes(size)
                );
                let cancel = button("cancel", "Cancel", SetupAction::Cancel, subtle);
                shown_element(
                    Shown {
                        sentence: Some(line),
                        controls: vec![cancel],
                        ..Shown::default()
                    },
                    cx,
                )
            }
            Setup::DownloadFailed { message } => {
                let again = button("try-again", "Try again", SetupAction::Download, subtle);
                shown_element(
                    Shown {
                        sentence: Some(message),
                        controls: vec![again],
                        ..Shown::default()
                    },
                    cx,
                )
            }
            Setup::Stopped { message } => {
                let again = button("try-again", "Try again", SetupAction::Check, subtle);
                shown_element(
                    Shown {
                        sentence: Some(format!("{name} did not start.")),
                        quiet: Some(message),
                        controls: vec![again],
                        ..Shown::default()
                    },
                    cx,
                )
            }
            setup if pi => {
                let panel = match (setup, self.signing_in) {
                    (Setup::SigningIn(steps), Some(id)) => Some(Panel::Steps(id, steps)),
                    (
                        Setup::Ready {
                            reason: Some(reason),
                            ..
                        }
                        | Setup::SignedOut {
                            reason: Some(reason),
                        },
                        Some(id),
                    ) => Some(Panel::Ended(id, reason)),
                    _ => None,
                };
                let ready = (self.services.iter()).any(|service| service.signed_in.is_some());
                let services = Services {
                    services: self.services,
                    panel,
                    signing_out: self.signing_out,
                    search: self.search,
                    answer: self.answer,
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .w_full()
                    .when(!ready, |body| {
                        body.child(div().child("Sign in to one of Pi's services to start."))
                    })
                    .child(services.element(&buttons, window, cx))
                    .into_any_element()
            }
            Setup::Ready { account, reason } => {
                let who: Vec<String> = [account.email, account.plan]
                    .into_iter()
                    .flatten()
                    .collect();
                let signed_in = match who.is_empty() {
                    true => "Signed in.".to_string(),
                    false => format!("Signed in: {}", who.join(" · ")),
                };
                let sign_out = buttons.make(
                    "sign-out",
                    "Sign out",
                    SetupAction::SignOut(None),
                    subtle,
                    false,
                    window,
                    cx,
                );
                let muted = cx.theme().gray_700;
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .w_full()
                    .children(reason.map(|reason| reason.element(cx)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(div().flex_1().min_w_0().text_color(muted).child(signed_in))
                            .child(sign_out),
                    )
                    .into_any_element()
            }
            Setup::SignedOut { reason } => {
                let controls = buttons.choices(self.choices, window, cx);
                shown_element(
                    Shown {
                        sentence: Some("Sign in to start.".to_string()),
                        reason,
                        controls,
                        ..Shown::default()
                    },
                    cx,
                )
            }
            Setup::SigningIn(signing_in) => {
                let shown = buttons.signing_in(self.provider, signing_in, self.answer, window, cx);
                shown_element(shown, cx)
            }
        };
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(12.))
            .w_full()
            .text_size(px(14.))
            .line_height(px(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(logo(self.provider, 16.))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(strong)
                            .child(name),
                    ),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_in_whole_megabytes() {
        assert_eq!(megabytes(225_167_728), 215);
        assert_eq!(megabytes(0), 0);
        assert_eq!(megabytes(1 << 19), 1);
    }
}
