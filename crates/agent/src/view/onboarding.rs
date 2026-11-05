//! How far a provider is set up, as the Settings page shows it: the download, the ways in and
//! the steps of a sign-in, and the account. Plain state in, buttons out, so the gallery shows
//! every state with no process. The sidebar shows a skeleton of the thread while the app checks.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Entity, FontWeight, SharedString, Window, div,
    prelude::*, pulsating_between, px, relative,
};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::text_input::TextInput;

use crate::{Account, Answer, Provider, SignInChoice, SignInQuestion, SignInStep};

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
        reason: Option<String>,
    },
    SigningIn(SigningIn),
    /// A thread can run on it. `reason` says why a sign-in to another of Pi's providers
    /// ended without it.
    Ready {
        account: Account,
        reason: Option<String>,
    },
    /// The agent's program did not run. `message` is the first line it wrote.
    Stopped {
        message: String,
    },
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
    /// Shows the providers that take a key.
    ShowKeys,
    /// Opens the provider's page again, or starts the sign-in again when it never said which.
    OpenPageAgain,
    /// Answers the question with the id of a pick.
    Pick(String),
    /// Answers the question with what the field holds.
    Continue,
    SignOut,
    /// Try again after the program did not run.
    Check,
}

type ActionHandler = Rc<dyn Fn(&SetupAction, &mut Window, &mut App)>;

/// Every state of [`Setup`] of one provider, under its name.
#[derive(IntoElement)]
pub struct Onboarding {
    provider: Provider,
    setup: Setup,
    /// The ways in, from the last check: in the browser first, then those that take a key.
    choices: Vec<SignInChoice>,
    /// Whether the providers that take a key show.
    keys: bool,
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
            keys: false,
            answer: None,
            on_action: None,
        }
    }

    pub fn choices(mut self, choices: Vec<SignInChoice>, keys: bool) -> Self {
        self.choices = choices;
        self.keys = keys;
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
    /// Shown large, as a code to type.
    code: Option<String>,
    controls: Vec<AnyElement>,
    /// Under the controls, such as the providers that take a key.
    more: Vec<AnyElement>,
}

/// Makes the buttons of one provider. The keyboard reaches every one, and its focus stays
/// across renders.
struct Buttons {
    key: String,
    on_action: Option<ActionHandler>,
}

impl Buttons {
    fn make(
        &self,
        id: String,
        label: impl Into<SharedString>,
        action: SetupAction,
        variant: ButtonVariant,
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
            .size(ButtonSize::Md)
            .w_full()
            .focus_handle(&handle);
        match self.on_action.clone() {
            Some(on_action) => button
                .on_click(move |_, window, cx| on_action(&action, window, cx))
                .into_any_element(),
            None => button.into_any_element(),
        }
    }

    /// The ways in: those in the browser as buttons, and **Use an API key**, which shows the
    /// providers that take one.
    fn choices(
        &self,
        choices: Vec<SignInChoice>,
        keys: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> (Vec<AnyElement>, Vec<AnyElement>) {
        let (with_key, in_browser): (Vec<_>, Vec<_>) =
            choices.into_iter().partition(SignInChoice::takes_key);
        let mut controls: Vec<AnyElement> = (in_browser.into_iter().enumerate())
            .map(|(index, choice)| {
                let label = choice.label().to_string();
                let action = SetupAction::SignIn(choice);
                let id = format!("sign-in-{index}");
                self.make(id, label, action, ButtonVariant::Subtle, window, cx)
            })
            .collect();
        if with_key.is_empty() {
            return (controls, Vec::new());
        }
        if !keys {
            let (action, variant) = (SetupAction::ShowKeys, ButtonVariant::Ghost);
            let keys = self.make("keys".into(), "Use an API key", action, variant, window, cx);
            controls.push(keys);
            return (controls, Vec::new());
        }
        let more = (with_key.into_iter().enumerate())
            .map(|(index, choice)| {
                let label = choice.label().to_string();
                let action = SetupAction::SignIn(choice);
                let id = format!("key-{index}");
                self.make(id, label, action, ButtonVariant::Ghost, window, cx)
            })
            .collect();
        (controls, more)
    }
}

impl RenderOnce for Onboarding {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (text, muted, strong) = (theme.gray_900, theme.gray_700, theme.gray_950);
        let name = self.provider.name();
        let buttons = Buttons {
            key: name.to_lowercase(),
            on_action: self.on_action,
        };
        let (subtle, primary) = (ButtonVariant::Subtle, ButtonVariant::Primary);
        let mut button = |id: &str, label: SharedString, action, variant| {
            buttons.make(id.to_string(), label, action, variant, window, cx)
        };
        let shown = match self.setup {
            Setup::Checking => Shown {
                quiet: Some("Checking…".to_string()),
                ..Shown::default()
            },
            Setup::Ready { account, reason } => {
                let who: Vec<String> = [account.email, account.plan]
                    .into_iter()
                    .flatten()
                    .collect();
                let signed_in = match who.is_empty() {
                    true => "Signed in.".to_string(),
                    false => format!("Signed in: {}", who.join(" · ")),
                };
                // Pi signs in to one provider at a time, and signs out in Pi itself.
                let (controls, more) = match self.provider {
                    Provider::Claude => {
                        let sign_out =
                            button("sign-out", "Sign out".into(), SetupAction::SignOut, subtle);
                        (vec![sign_out], Vec::new())
                    }
                    Provider::Pi => buttons.choices(self.choices, self.keys, window, cx),
                };
                Shown {
                    sentence: reason,
                    quiet: Some(signed_in),
                    controls,
                    more,
                    ..Shown::default()
                }
            }
            Setup::NotInstalled => {
                let size = self.provider.download().map_or(String::new(), |download| {
                    format!(" {} MB", megabytes(download.size))
                });
                let sentence = format!(
                    "The agent runs {name} by {}. Setup downloads{size}, then opens your browser to sign in.",
                    self.provider.maker()
                );
                Shown {
                    sentence: Some(sentence),
                    controls: vec![button(
                        "set-up",
                        "Set up".into(),
                        SetupAction::Download,
                        primary,
                    )],
                    ..Shown::default()
                }
            }
            Setup::Downloading { received, size } => {
                let percent = (received * 100).checked_div(size).unwrap_or(0).min(100);
                let line = format!(
                    "Downloading {name}: {} of {} MB, {percent}%",
                    megabytes(received),
                    megabytes(size)
                );
                Shown {
                    sentence: Some(line),
                    controls: vec![button(
                        "cancel",
                        "Cancel".into(),
                        SetupAction::Cancel,
                        subtle,
                    )],
                    ..Shown::default()
                }
            }
            Setup::DownloadFailed { message } => Shown {
                sentence: Some(message),
                controls: vec![button(
                    "try-again",
                    "Try again".into(),
                    SetupAction::Download,
                    subtle,
                )],
                ..Shown::default()
            },
            Setup::SignedOut { reason } => {
                let (controls, more) = buttons.choices(self.choices, self.keys, window, cx);
                Shown {
                    sentence: Some("Sign in to start.".to_string()),
                    quiet: reason,
                    controls,
                    more,
                    ..Shown::default()
                }
            }
            Setup::SigningIn(signing_in) => {
                let SigningIn {
                    page,
                    code,
                    note,
                    question,
                } = signing_in;
                // The browser's step leads while there is one; a question beside it, such as
                // the address the browser ended on, is the quieter way.
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
                                asked.push(button(&format!("pick-{index}"), label, action, subtle));
                            }
                        }
                        Answer::Text { .. } => {
                            asked.extend(self.answer.clone().map(|field| {
                                div().w_full().text_left().child(field).into_any_element()
                            }));
                            let variant = if in_browser { subtle } else { primary };
                            let action = SetupAction::Continue;
                            asked.push(button("continue", "Continue".into(), action, variant));
                        }
                    }
                }
                let sentence = match &question {
                    _ if code.is_some() => {
                        "Enter this code on the page in your browser:".to_string()
                    }
                    Some(question) if !in_browser => question.message.clone(),
                    _ => "Finish signing in in your browser.".to_string(),
                };
                let (mut controls, more) = match in_browser {
                    true => (Vec::new(), asked),
                    false => (asked, Vec::new()),
                };
                // Claude Code never names its page: only starting again opens it again.
                if in_browser || self.provider == Provider::Claude {
                    let action = SetupAction::OpenPageAgain;
                    controls.push(button(
                        "open-page-again",
                        "Open the page again".into(),
                        action,
                        subtle,
                    ));
                }
                let cancel = SetupAction::Cancel;
                controls.push(button(
                    "cancel",
                    "Cancel".into(),
                    cancel,
                    ButtonVariant::Ghost,
                ));
                Shown {
                    sentence: Some(sentence),
                    quiet: note,
                    code,
                    controls,
                    more,
                }
            }
            Setup::Stopped { message } => Shown {
                sentence: Some(format!("{name} stopped: {message}")),
                controls: vec![button(
                    "try-again",
                    "Try again".into(),
                    SetupAction::Check,
                    subtle,
                )],
                ..Shown::default()
            },
        };
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
            .text_size(px(14.))
            .line_height(px(20.))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(strong)
                    .child(name),
            )
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
            .when(!shown.controls.is_empty(), |body| {
                body.child(column(shown.controls, 8.))
            })
            .when(!shown.more.is_empty(), |body| {
                body.child(column(shown.more, 4.))
            })
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
