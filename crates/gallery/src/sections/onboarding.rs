//! Onboarding section: every state of an agent on the Settings page, from **Set up** to signed
//! in with **Sign out**, each in a frame as wide as the page's column. Claude Code's first,
//! then Pi's ways in and the steps of its sign-in.

use gpui::{
    App, Entity, FontWeight, IntoElement, ParentElement, Styled, Window, div, prelude::*, px,
};
use sound_agent::{
    Account, Answer, Onboarding, Pick, Provider, Setup, SignInChoice, SignInQuestion, SigningIn,
};
use sound_ui::ActiveTheme;
use sound_ui::components::text_input::TextInput;

/// The column of the Settings page, less its padding.
const WIDTH: f32 = 560.;
/// Enough for the tallest state.
const HEIGHT: f32 = 300.;

pub fn section(window: &mut Window, cx: &mut App) -> impl IntoElement {
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
    let answer = window.use_keyed_state("gallery-sign-in-answer", cx, |_, cx| TextInput::new(cx));
    let claude = states.into_iter().map(|(title, setup)| {
        let choices = SignInChoice::examples(Provider::Claude, &[], &[]);
        sample(
            title,
            Onboarding::new(Provider::Claude, setup).choices(choices, false),
            cx,
        )
    });
    let pi = pi_states(&answer)
        .into_iter()
        .map(|(title, onboarding)| sample(title, onboarding, cx));
    div()
        .flex()
        .flex_wrap()
        .gap(px(40.))
        .children(claude)
        .children(pi)
}

/// Pi's ways in: three in the browser and four providers that take a key.
fn pi_choices() -> Vec<SignInChoice> {
    let in_browser = [
        "Sign in with Anthropic (Claude Pro/Max)",
        "Sign in with ChatGPT",
        "Sign in with GitHub Copilot",
    ];
    let with_key = ["Anthropic", "Google", "Mistral", "OpenRouter"];
    SignInChoice::examples(Provider::Pi, &in_browser, &with_key)
}

fn pi_states(answer: &Entity<TextInput>) -> Vec<(&'static str, Onboarding)> {
    let pi = |setup, keys| {
        Onboarding::new(Provider::Pi, setup)
            .choices(pi_choices(), keys)
            .answer(answer.clone())
    };
    let signing_in = |signing_in| pi(Setup::SigningIn(signing_in), false);
    let text = |id: &str, message: &str, secret| SignInQuestion {
        id: id.to_string(),
        message: message.to_string(),
        answer: Answer::Text {
            placeholder: None,
            secret,
        },
    };
    let pick = |id: &str, label: &str| Pick {
        id: id.to_string(),
        label: label.to_string(),
    };
    vec![
        (
            "Pi signed out",
            pi(Setup::SignedOut { reason: None }, false),
        ),
        (
            "Pi providers that take a key",
            pi(Setup::SignedOut { reason: None }, true),
        ),
        (
            "Pi signing in, in the browser",
            signing_in(SigningIn {
                page: Some("https://auth.openai.com/authorize".to_string()),
                question: Some(text(
                    "manual",
                    "If the browser does not come back here, paste the address it ended on.",
                    false,
                )),
                ..SigningIn::default()
            }),
        ),
        (
            "Pi signing in with a code",
            signing_in(SigningIn {
                page: Some("https://github.com/login/device".to_string()),
                code: Some("4F2A-91C3".to_string()),
                note: Some("Waiting for authentication…".to_string()),
                ..SigningIn::default()
            }),
        ),
        (
            "Pi asking how to sign in",
            signing_in(SigningIn {
                question: Some(SignInQuestion {
                    id: "method".to_string(),
                    message: "Select Anthropic login method:".to_string(),
                    answer: Answer::Pick(vec![
                        pick("browser", "Browser login (default)"),
                        pick("copy_code", "Copy code login (headless)"),
                    ]),
                }),
                ..SigningIn::default()
            }),
        ),
        (
            "Pi asking for a key",
            signing_in(SigningIn {
                question: Some(text("key", "Enter Mistral API key", true)),
                ..SigningIn::default()
            }),
        ),
        (
            "Pi signed in, with more ways in",
            pi(
                Setup::Ready {
                    account: Account {
                        email: None,
                        plan: Some("OpenRouter".to_string()),
                    },
                },
                false,
            ),
        ),
    ]
}

/// A heading, then the state on the background of the sidebar.
fn sample(title: &'static str, onboarding: Onboarding, cx: &App) -> impl IntoElement {
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
                .h(px(HEIGHT))
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
