//! The notices in the bottom right: a render while it runs, with how far it is, a newer
//! release with Restart or Download, a newer template that changes files the project changed
//! too, with a merge by the agent, and what went wrong last, which a dismiss takes away.
//! Nothing blocks.

use rune::{El, Icon, Tx, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant};
use runtime::update::Action;

use super::State;
use crate::controls::{Control, Target};

/// Clear of the edges of the window, where the left panel and the tracks' heads are.
const INSET: f32 = 24.0;
/// A notice is as wide as its words up to this; longer words wrap to three lines at most.
const WIDTH: f32 = 400.0;
const DISMISS: &str = "Dismiss";
const UPDATE_BUTTON: &str = "notice-update-button";
const MERGE: &str = "Merge with the agent";
const MERGE_BUTTON: &str = "notice-template-merge";
const TEMPLATE_DISMISS: &str = "notice-template-dismiss";
const DOT: f32 = 6.0;
const GAP: f32 = 8.0;

/// How a notice reads: what went wrong, or work that runs.
#[derive(Clone, Copy)]
enum Tone {
    Error,
    Info,
}

/// The column of notices, if there is one.
pub(super) fn view(state: &State) -> Option<El> {
    let rendering = state.session.rendering();
    let update = state.incoming.ready.as_ref();
    let template = template_words(state);
    let error = state.session.notice();
    if rendering.is_none() && update.is_none() && template.is_none() && error.is_none() {
        return None;
    }
    let notices = col()
        .key("notices")
        .w(WIDTH)
        .gap(8.0)
        .items_end()
        .absolute(0.0, 0.0)
        .pin(None, None, Some(INSET), Some(INSET));
    let rendering = rendering.map(|(words, done)| {
        notice(state, "render", words, Tone::Info)
            .progress(done)
            .build()
    });
    let message = update.map(runtime::update::Ready::message);
    let update = update.zip(message.as_deref()).map(|(ready, words)| {
        let tone = match ready.action {
            Action::Failed { .. } => Tone::Error,
            Action::Restart | Action::Download { .. } => Tone::Info,
        };
        let mut update = notice(state, "update", words, tone);
        update.button = Some(Pressed {
            key: UPDATE_BUTTON,
            label: ready.button(),
            press: State::take_update,
        });
        update.build()
    });
    let template = template.as_deref().map(|words| {
        let mut template = notice(state, "template", words, Tone::Info);
        template.dismiss = Some((TEMPLATE_DISMISS, |s: &mut State| s.template_update = None));
        template.button = state.panel.agent.is_some().then_some(Pressed {
            key: MERGE_BUTTON,
            label: MERGE,
            press: State::merge_template,
        });
        template.build()
    });
    let error = error.map(|words| {
        let mut error = notice(state, "error", words, Tone::Error);
        error.dismiss = Some(("notice-dismiss", |s: &mut State| s.session.dismiss_notice()));
        error.build()
    });
    Some(
        (notices.children(rendering).children(update))
            .children(template)
            .children(error),
    )
}

/// What the notice of a newer template says, while files wait for a merge.
fn template_words(state: &State) -> Option<String> {
    let update = state.template_update.as_ref()?;
    let files = match update.files.len() {
        1 => "a file".to_string(),
        count => format!("{count} files"),
    };
    Some(format!(
        "A newer {} template changes {files} this project changed too.",
        update.template.name
    ))
}

impl State {
    /// Puts the old and the new template beside the project's files and writes what to ask into
    /// the agent's composer, for the composer to send. The panel opens on it.
    pub(crate) fn merge_template(&mut self, tx: &mut Tx) {
        let Some(update) = self.template_update.take() else {
            return;
        };
        let words = match runtime::templates::begin_merge(self.project().root(), &update) {
            Ok(words) => words,
            Err(error) => return self.session.report(format!("{error:#}")),
        };
        let composed = (self.panel.agent.as_mut()).is_some_and(|agent| agent.compose(&words));
        if !composed {
            self.session.report(
                "The agent cannot take a message now: ask it later to merge the newer template.",
            );
        }
        self.agent_keys(false, tx);
    }
}

/// The buttons of the notices, for `--tree` and `--act`.
pub(super) fn controls(state: &State) -> Vec<Control> {
    let button = |key: &str, label: &str, act| Control {
        key: key.into(),
        label: label.into(),
        role: Some("button".into()),
        value: None,
        checked: None,
        expanded: None,
        actions: vec!["press".into()],
        place: None,
        target: Target::Chrome(act),
        menu: None,
        drag: None,
    };
    let update = (state.incoming.ready.as_ref())
        .map(|ready| button(UPDATE_BUTTON, ready.button(), super::Act::Update));
    let template = state.template_update.as_ref().map(|_| {
        let merge =
            (state.panel.agent.as_ref()).map(|_| button(MERGE_BUTTON, MERGE, super::Act::Merge));
        let dismiss = button(TEMPLATE_DISMISS, DISMISS, super::Act::DismissTemplate);
        merge.into_iter().chain([dismiss])
    });
    let dismiss =
        (state.session.notice()).map(|_| button("notice-dismiss", DISMISS, super::Act::Dismiss));
    (update.into_iter())
        .chain(template.into_iter().flatten())
        .chain(dismiss)
        .collect()
}

/// A button of a notice and what a press does.
struct Pressed {
    key: &'static str,
    label: &'static str,
    press: fn(&mut State, &mut Tx),
}

struct Notice<'a> {
    state: &'a State,
    key: &'static str,
    words: &'a str,
    tone: Tone,
    progress: Option<f32>,
    /// The key of its dismiss, and what it takes away.
    dismiss: Option<(&'static str, fn(&mut State))>,
    button: Option<Pressed>,
}

fn notice<'a>(state: &'a State, key: &'static str, words: &'a str, tone: Tone) -> Notice<'a> {
    Notice {
        state,
        key,
        words,
        tone,
        progress: None,
        dismiss: None,
        button: None,
    }
}

impl Notice<'_> {
    fn progress(self, done: f32) -> Self {
        Self {
            progress: Some(done.clamp(0.0, 1.0)),
            ..self
        }
    }

    fn build(self) -> El {
        let theme = &self.state.theme;
        let body = theme.text("body");
        let dot = match self.tone {
            Tone::Error => "red",
            Tone::Info => "text-muted",
        };
        // The dot sits on the first line, however many lines there are.
        let dot = el()
            .h(20.0)
            .items_center()
            .child(el().size(DOT, DOT).radius(DOT / 2.0).fill(theme.color(dot)))
            .a11y_hidden();
        let words = text(self.words.to_string())
            .font(body)
            .color(theme.color("text"))
            .wrap()
            .max_lines(3);
        // A value is a bright line on dark, as on a meter.
        let bar = self.progress.map(|done| {
            el().key(format!("notice-{}-progress", self.key))
                .role("progressbar")
                .a11y_value(format!("{:.0}%", done * 100.0))
                .mt(4.0)
                .mb(5.0)
                .h(2.0)
                .w_full()
                .radius(1.0)
                .fill(theme.color("fill-strong"))
                .child(
                    el().h_full()
                        .w_frac(done)
                        .radius(1.0)
                        .fill(theme.color("text")),
                )
        });
        let dismiss = self.dismiss.map(|(key, dismiss)| {
            row()
                .key(key)
                .role("button")
                .label(DISMISS)
                .help(DISMISS)
                .focusable(true)
                .size(20.0, 20.0)
                .center()
                .radius(theme.radius("sm").unwrap_or(4.0))
                .hover(theme.color("fill"))
                .on_press(move |s: &mut State| {
                    dismiss(s);
                    s.handled();
                })
                .child(
                    icon(&CLOSE, 14.0)
                        .color(theme.color("text-muted"))
                        .a11y_hidden(),
                )
        });
        let button = self.button.map(|Pressed { key, label, press }| {
            Button::new(label)
                .variant(ButtonVariant::Neutral)
                .on_press_tx(move |s: &mut State, tx| {
                    press(s, tx);
                    s.handled();
                })
                .build(&theme.kit)
                .key(key)
        });
        row()
            .key(format!("notice-{}", self.key))
            .role(match self.tone {
                Tone::Error => "alert",
                Tone::Info => "status",
            })
            .label(self.words.to_string())
            .max_w(WIDTH)
            .items_start()
            .gap(GAP)
            .py(5.0)
            .pl(12.0)
            .pr(if self.dismiss.is_some() { 4.0 } else { 12.0 })
            .radius(theme.radius("lg").unwrap_or(8.0))
            .fill(theme.color("raised"))
            .border(1.0, theme.color("border-strong"))
            .child(dot)
            .child(col().shrink().min_w(0.0).child(words).children(bar))
            .children(button)
            .children(dismiss)
    }
}

static CLOSE: Icon = Icon {
    name: "notice-close",
    view: 24.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></g>"#,
};
