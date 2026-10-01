//! Onboarding section: every state of the agent sidebar before the thread shows, from
//! **Set up** to signed in, each in a frame as wide as the sidebar.

use gpui::{App, FontWeight, IntoElement, ParentElement, Styled, Window, div, prelude::*, px};
use sound_agent::{Onboarding, Provider, Setup};
use sound_ui::ActiveTheme;

/// The width of the sidebar.
const WIDTH: f32 = 360.;
/// Enough for the tallest state, as a sidebar below its header.
const HEIGHT: f32 = 320.;

pub fn section(_window: &mut Window, cx: &mut App) -> impl IntoElement {
    let size = Provider::Claude
        .download()
        .map_or(0, |download| download.size);
    let states = [
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
        ("Signing in", Setup::SigningIn),
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
    div().flex().flex_wrap().gap(px(40.)).children(
        states
            .into_iter()
            .map(|(title, setup)| sample(title, setup, cx)),
    )
}

/// A heading, then the state on the background of the sidebar.
fn sample(title: &'static str, setup: Setup, cx: &App) -> impl IntoElement {
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
                .rounded(px(12.))
                .border_1()
                .border_color(border)
                .bg(background)
                .child(Onboarding::new(Provider::Claude, setup)),
        )
}
