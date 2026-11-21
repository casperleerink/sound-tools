//! A dialog inside the window: a title, a line on what it is for, its body, a line that says
//! what is wrong or what the main button will do, and its buttons at the foot, the main one
//! last. Rune keeps the keys inside while it is open and gives them back to where they were
//! when it closes; Escape, a press outside and Cancel close it.

use rune::{El, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant};

use crate::theme::Theme;

pub(crate) struct Dialog<S: 'static> {
    /// Keys `<key>-dialog` and its parts, as `--tree` lists them.
    pub(crate) key: &'static str,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) body: Vec<El>,
    pub(crate) note: Option<Note>,
    /// Built with [`button`], the main one last.
    pub(crate) buttons: Vec<El>,
    pub(crate) close: fn(&mut S),
}

pub(crate) enum Note {
    Quiet(String),
    Wrong(String),
}

const WIDTH: f32 = 480.0;

impl<S: 'static> Dialog<S> {
    /// The layers of the open dialog, over the window.
    pub(crate) fn build(self, theme: &Theme) -> Vec<El> {
        let note = self.note.map(|note| {
            let (words, color) = match note {
                Note::Quiet(words) => (words, theme.color("text-secondary")),
                Note::Wrong(words) => (words, theme.color("danger")),
            };
            text(words)
                .key(format!("{}-note", self.key))
                .font(theme.text("label"))
                .color(color)
                .wrap()
        });
        let foot = row()
            .w_full()
            .gap(theme.space(2))
            .justify_end()
            .pt(theme.space(1))
            .children(self.buttons);
        let body = col()
            .w_full()
            .gap(theme.space(3))
            .px(theme.space(4))
            .pb(theme.space(4))
            .children(self.body)
            .children(note)
            .child(foot);
        let mut dialog = rune_kit::Dialog::new(self.key, self.title)
            .open(true)
            .fit()
            .width(WIDTH)
            .child(body)
            .on_close(self.close);
        if let Some(description) = self.description {
            dialog = dialog.description(description);
        }
        let mut layer = Vec::new();
        dialog.build(&theme.kit, &mut layer);
        layer
    }
}

/// A button of a dialog's foot: `Contrast` for the main one, `Danger` for one that removes.
/// Return presses the main one from anywhere in the dialog, as a native default button.
pub(crate) fn button<S: 'static>(
    theme: &Theme,
    key: impl Into<String>,
    label: impl Into<String>,
    variant: ButtonVariant,
    enabled: bool,
    press: impl Fn(&mut S, &mut Tx) + 'static,
) -> El {
    let button = Button::new(label)
        .variant(variant)
        .disabled(!enabled)
        .on_press_tx(press)
        .build(&theme.kit)
        .key(key);
    match variant {
        ButtonVariant::Neutral => button,
        _ if enabled => button.default_button(),
        _ => button,
    }
}
