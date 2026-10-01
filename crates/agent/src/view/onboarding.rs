//! What the sidebar shows until the agent is ready: the download, then the sign-in. Plain
//! state in, buttons out, so the gallery shows every state with no process.

use std::rc::Rc;

use gpui::{App, SharedString, Window, div, prelude::*, px};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};

use crate::{Account, Provider, SignInChoice};

/// Where the sidebar is in setting up its agent.
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
    /// The provider's page is open in the browser.
    SigningIn,
    /// The thread shows.
    Ready {
        account: Account,
    },
    /// The agent's program did not run. `message` is the first line it wrote.
    Stopped {
        message: String,
    },
}

/// What a button of the onboarding asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupAction {
    /// Set up, and Try again after a failed download.
    Download,
    /// Stops the download or the sign-in.
    Cancel,
    SignIn(SignInChoice),
    /// Starts the sign-in again, which opens the page again.
    OpenPageAgain,
    /// Try again after the program did not run.
    Check,
}

type ActionHandler = Rc<dyn Fn(&SetupAction, &mut Window, &mut App)>;

/// Every state of [`Setup`] but `Ready`, which is the thread.
#[derive(IntoElement)]
pub struct Onboarding {
    provider: Provider,
    setup: Setup,
    on_action: Option<ActionHandler>,
}

impl Onboarding {
    pub fn new(provider: Provider, setup: Setup) -> Self {
        Self {
            provider,
            setup,
            on_action: None,
        }
    }

    pub fn on_action(
        mut self,
        handler: impl Fn(&SetupAction, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(handler));
        self
    }
}

/// Bytes as the composer reads a download: whole megabytes.
fn megabytes(bytes: u64) -> u64 {
    (bytes + (1 << 19)) >> 20
}

impl RenderOnce for Onboarding {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (text, muted) = (theme.gray_900, theme.gray_700);
        let name = self.provider.name();
        let on_action = self.on_action;
        // The keyboard reaches every button, and its focus stays across renders.
        let mut focus = (0..2usize).map(|index| {
            window
                .use_keyed_state(("onboarding-focus", index), cx, |_, cx| {
                    cx.focus_handle().tab_stop(true)
                })
                .read(cx)
                .clone()
        });
        let mut button = |id: &'static str, label: SharedString, action, variant| {
            let button = Button::new(id, label)
                .debug_selector(move || format!("setup-{id}"))
                .variant(variant)
                .size(ButtonSize::Md)
                .w_full();
            let button = match focus.next() {
                Some(handle) => button.focus_handle(&handle),
                None => button,
            };
            match on_action.clone() {
                Some(on_action) => button
                    .on_click(move |_, window, cx| on_action(&action, window, cx))
                    .into_any_element(),
                None => button.into_any_element(),
            }
        };
        let (sentence, quiet, buttons) = match self.setup {
            Setup::Checking | Setup::Ready { .. } => (None, None, Vec::new()),
            Setup::NotInstalled => {
                let size = self.provider.download().map_or(String::new(), |download| {
                    format!(" {} MB", megabytes(download.size))
                });
                let sentence = format!(
                    "The agent runs {name} by {}. Setup downloads{size}, then opens your browser to sign in.",
                    self.provider.maker()
                );
                let set_up = button(
                    "set-up",
                    "Set up".into(),
                    SetupAction::Download,
                    ButtonVariant::Primary,
                );
                (Some(sentence), None, vec![set_up])
            }
            Setup::Downloading { received, size } => {
                let percent = (received * 100).checked_div(size).unwrap_or(0).min(100);
                let line = format!(
                    "Downloading {name}: {} of {} MB, {percent}%",
                    megabytes(received),
                    megabytes(size)
                );
                let cancel = button(
                    "cancel",
                    "Cancel".into(),
                    SetupAction::Cancel,
                    ButtonVariant::Subtle,
                );
                (Some(line), None, vec![cancel])
            }
            Setup::DownloadFailed { message } => {
                let again = button(
                    "try-again",
                    "Try again".into(),
                    SetupAction::Download,
                    ButtonVariant::Subtle,
                );
                (Some(message), None, vec![again])
            }
            Setup::SignedOut { reason } => {
                let choices = self.provider.sign_in_choices();
                let buttons = choices
                    .into_iter()
                    .zip(["sign-in-first", "sign-in-second"])
                    .enumerate()
                    .map(|(index, (choice, id))| {
                        let variant = if index == 0 {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Subtle
                        };
                        button(
                            id,
                            choice.label.into(),
                            SetupAction::SignIn(choice),
                            variant,
                        )
                    })
                    .collect();
                (None, reason, buttons)
            }
            Setup::SigningIn => {
                let again = button(
                    "open-page-again",
                    "Open the page again".into(),
                    SetupAction::OpenPageAgain,
                    ButtonVariant::Subtle,
                );
                let cancel = button(
                    "cancel",
                    "Cancel".into(),
                    SetupAction::Cancel,
                    ButtonVariant::Ghost,
                );
                let sentence = "Finish signing in in your browser.".to_string();
                (Some(sentence), None, vec![again, cancel])
            }
            Setup::Stopped { message } => {
                let again = button(
                    "try-again",
                    "Try again".into(),
                    SetupAction::Check,
                    ButtonVariant::Subtle,
                );
                (
                    Some(format!("{name} stopped: {message}")),
                    None,
                    vec![again],
                )
            }
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .px(px(40.))
            .text_center()
            .text_size(px(14.))
            .line_height(px(20.))
            .children(sentence.map(|sentence| div().text_color(text).child(sentence)))
            .children(quiet.map(|quiet| div().text_color(muted).child(quiet)))
            .when(!buttons.is_empty(), |body| {
                body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .w_full()
                        .max_w(px(280.))
                        .children(buttons),
                )
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
