//! Onboarding sections: every state of an agent on the Settings page, from **Set up** to signed
//! in with **Sign out**, each in a frame as wide as the page's column. Claude Code's in one,
//! Pi's services and the steps of a sign-in to one in the other.

use gpui::{
    App, Entity, FontWeight, IntoElement, ParentElement, Styled, Window, div, prelude::*, px,
};
use sound_agent::{
    Account, Answer, Onboarding, Pick, Provider, Service, Setup, SignInChoice, SignInQuestion,
    SignedIn, SigningIn,
};
use sound_ui::ActiveTheme;
use sound_ui::components::text_input::TextInput;

/// The column of the Settings page, less its padding.
const WIDTH: f32 = 560.;
/// Enough for the tallest state of Claude Code, and of Pi with its list of services.
const HEIGHT: f32 = 300.;
const PI_HEIGHT: f32 = 700.;

pub fn section(_: &mut Window, cx: &mut App) -> impl IntoElement {
    let size = Provider::Claude
        .download()
        .map_or(0, |download| download.size);
    let states = [
        ("Checking", Setup::Checking),
        (
            "Signed in",
            Setup::Ready {
                account: Account {
                    email: Some("composer@example.com".to_string()),
                    plan: Some("Claude Max".to_string()),
                },
                reason: None,
            },
        ),
        ("Not installed", Setup::NotInstalled),
        (
            "Downloading",
            Setup::Downloading {
                received: size * 2 / 5,
                size,
            },
        ),
        (
            "Download failed",
            Setup::DownloadFailed {
                message: "Could not download Claude Code. Check the internet connection."
                    .to_string(),
            },
        ),
        (
            "Download damaged",
            Setup::DownloadFailed {
                message: "The download was damaged.".to_string(),
            },
        ),
        (
            "Download refused by the server",
            Setup::DownloadFailed {
                message:
                    "Could not download Claude Code: Claude Code is not available in your region."
                        .to_string(),
            },
        ),
        ("Signed out", Setup::SignedOut { reason: None }),
        ("Signing in", Setup::SigningIn(SigningIn::default())),
        (
            "Sign-in cancelled",
            Setup::SignedOut {
                reason: Some("The sign-in was cancelled.".to_string()),
            },
        ),
        (
            "Sign-in failed",
            Setup::SignedOut {
                reason: Some("The sign-in failed: OAuth error: access denied".to_string()),
            },
        ),
        (
            "CLI fails to start",
            Setup::Stopped {
                message: "Permission denied (os error 13)".to_string(),
            },
        ),
    ];
    let claude = states.into_iter().map(|(title, setup)| {
        let onboarding = Onboarding::new(Provider::Claude, setup).choices(SignInChoice::claude());
        sample(title, onboarding, HEIGHT, cx)
    });
    div().flex().flex_wrap().gap(px(40.)).children(claude)
}

pub fn pi_section(window: &mut Window, cx: &mut App) -> impl IntoElement {
    let answer = window.use_keyed_state("gallery-sign-in-answer", cx, |_, cx| TextInput::new(cx));
    let search = window.use_keyed_state("gallery-search", cx, |_, cx| {
        TextInput::new(cx).placeholder("Search services")
    });
    let pi = pi_states(&answer, &search)
        .into_iter()
        .map(|(title, onboarding)| sample(title, onboarding, PI_HEIGHT, cx));
    div().flex().flex_wrap().gap(px(40.)).children(pi)
}

/// Some of Pi's services: four with a sign-in in the browser, and some that take a key only.
/// `signed_in` are signed in here, and Google with a key set up outside.
fn services(signed_in: &[&str]) -> Vec<Service> {
    let in_browser = [
        ("Anthropic", "Anthropic (Claude Pro/Max)"),
        ("GitHub Copilot", "GitHub Copilot"),
        ("OpenAI", "Sign in with ChatGPT"),
        ("OpenRouter", "Sign in with OpenRouter"),
    ];
    let with_key = ["DeepSeek", "Google", "Groq", "Mistral", "xAI"];
    let state = |name: &str| match name {
        "Google" if !signed_in.is_empty() => Some(SignedIn::Elsewhere),
        name if signed_in.contains(&name) => Some(SignedIn::Here),
        _ => None,
    };
    (in_browser.iter())
        .map(|(name, label)| Service::example(name, Some(label), state(name)))
        .chain(
            with_key
                .iter()
                .map(|name| Service::example(name, None, state(name))),
        )
        .collect()
}

fn pi_states(
    answer: &Entity<TextInput>,
    search: &Entity<TextInput>,
) -> Vec<(&'static str, Onboarding)> {
    let pi = |setup, signed_in: &[&str], signing_in: Option<&str>, signing_out: Option<&str>| {
        Onboarding::new(Provider::Pi, setup)
            .services(
                services(signed_in),
                signing_in.map(str::to_string),
                signing_out.map(str::to_string),
            )
            .search(search.clone(), String::new())
            .answer(answer.clone())
    };
    let signing_in = |service: &str, steps| pi(Setup::SigningIn(steps), &[], Some(service), None);
    let text = |id: &str, message: &str, secret| SignInQuestion {
        id: id.to_string(),
        message: message.to_string(),
        answer: Answer::Text { secret },
    };
    let pick = |id: &str, label: &str| Pick {
        id: id.to_string(),
        label: label.to_string(),
    };
    let ready = Setup::Ready {
        account: Account::default(),
        reason: None,
    };
    vec![
        ("Pi not installed", pi(Setup::NotInstalled, &[], None, None)),
        (
            "Pi signed in to no service",
            pi(Setup::SignedOut { reason: None }, &[], None, None),
        ),
        (
            "Pi signed in to two services",
            pi(ready.clone(), &["OpenRouter"], None, None),
        ),
        (
            "Pi signing out of a service",
            pi(ready, &["OpenRouter"], None, Some("openrouter")),
        ),
        (
            "Pi signing in, in the browser",
            signing_in(
                "openai",
                SigningIn {
                    page: Some("https://auth.openai.com/authorize".to_string()),
                    question: Some(text(
                        "manual",
                        "If the browser does not come back here, paste the address it ended on.",
                        false,
                    )),
                    ..SigningIn::default()
                },
            ),
        ),
        (
            "Pi signing in with a code",
            signing_in(
                "github-copilot",
                SigningIn {
                    page: Some("https://github.com/login/device".to_string()),
                    code: Some("4F2A-91C3".to_string()),
                    note: Some("Waiting for authentication…".to_string()),
                    ..SigningIn::default()
                },
            ),
        ),
        (
            "Pi asking how to sign in",
            signing_in(
                "anthropic",
                SigningIn {
                    question: Some(SignInQuestion {
                        id: "method".to_string(),
                        message: "Select Anthropic login method:".to_string(),
                        answer: Answer::Pick(vec![
                            pick("browser", "Browser login (default)"),
                            pick("copy_code", "Copy code login (headless)"),
                        ]),
                    }),
                    ..SigningIn::default()
                },
            ),
        ),
        (
            "Pi asking for a key",
            signing_in(
                "mistral",
                SigningIn {
                    question: Some(text("key", "Enter Mistral API key", true)),
                    ..SigningIn::default()
                },
            ),
        ),
        (
            "Pi after a sign-in that failed",
            pi(
                Setup::SignedOut {
                    reason: Some("The sign-in failed: access denied".to_string()),
                },
                &[],
                Some("openai"),
                None,
            ),
        ),
    ]
}

/// A heading, then the state on the background of the sidebar.
fn sample(title: &'static str, onboarding: Onboarding, height: f32, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, background, border) = (theme.gray_700, theme.gray_50, theme.alpha_at(0.10));
    // Its own id, so the buttons of two states keep apart.
    div()
        .id(title)
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(title),
        )
        .child(
            div()
                .w(px(WIDTH))
                .min_h(px(height))
                .flex()
                .flex_col()
                .p(px(24.))
                .rounded(px(12.))
                .border_1()
                .border_color(border)
                .bg(background)
                .child(onboarding),
        )
}
