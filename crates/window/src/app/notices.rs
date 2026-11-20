//! The notices in the bottom right: a render while it runs, with how far it is, and what went
//! wrong last, which a dismiss takes away. Nothing blocks.

use rune::{El, Icon, col, el, icon, row, text};

use super::State;
use crate::controls::{Control, Target};

/// Clear of the edges of the window, where the left panel and the tracks' heads are.
const INSET: f32 = 24.0;
/// A notice is as wide as its words up to this; longer words wrap to three lines at most.
const WIDTH: f32 = 400.0;
const DISMISS: &str = "Dismiss";
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
    let error = state.session.notice();
    if rendering.is_none() && error.is_none() {
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
    let error = error.map(|words| notice(state, "error", words, Tone::Error).dismiss().build());
    Some(notices.children(rendering).children(error))
}

/// The dismiss of the notice, for `--tree` and `--act`.
pub(super) fn controls(state: &State) -> Option<Control> {
    state.session.notice()?;
    Some(Control {
        key: "notice-dismiss".into(),
        label: DISMISS.into(),
        role: Some("button".into()),
        value: None,
        checked: None,
        expanded: None,
        actions: vec!["press".into()],
        place: None,
        target: Target::Chrome(super::Act::Dismiss),
        menu: None,
        drag: None,
    })
}

struct Notice<'a> {
    state: &'a State,
    key: &'static str,
    words: &'a str,
    tone: Tone,
    progress: Option<f32>,
    dismiss: bool,
}

fn notice<'a>(state: &'a State, key: &'static str, words: &'a str, tone: Tone) -> Notice<'a> {
    Notice {
        state,
        key,
        words,
        tone,
        progress: None,
        dismiss: false,
    }
}

impl Notice<'_> {
    fn progress(self, done: f32) -> Self {
        Self {
            progress: Some(done.clamp(0.0, 1.0)),
            ..self
        }
    }

    fn dismiss(self) -> Self {
        Self {
            dismiss: true,
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
        let dismiss = self.dismiss.then(|| {
            row()
                .key("notice-dismiss")
                .role("button")
                .label(DISMISS)
                .help(DISMISS)
                .focusable(true)
                .size(20.0, 20.0)
                .center()
                .radius(theme.radius("sm").unwrap_or(4.0))
                .hover(theme.color("fill"))
                .on_press(|s: &mut State| {
                    s.session.dismiss_notice();
                    s.handled();
                })
                .child(
                    icon(&CLOSE, 14.0)
                        .color(theme.color("text-muted"))
                        .a11y_hidden(),
                )
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
            .pr(if self.dismiss { 4.0 } else { 12.0 })
            .radius(theme.radius("lg").unwrap_or(8.0))
            .fill(theme.color("raised"))
            .border(1.0, theme.color("border-strong"))
            .child(dot)
            .child(col().shrink().min_w(0.0).child(words).children(bar))
            .children(dismiss)
    }
}

static CLOSE: Icon = Icon {
    name: "notice-close",
    view: 24.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></g>"#,
};
