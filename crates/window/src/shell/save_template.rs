//! "Save as template": the template's name, first the project's, and what it keeps, the
//! tools only or the tools and content, as `--save-template` and its `--with-content` do. A
//! name saved before asks to replace that template.

use rune::{El, Tx, col, text};
use rune_kit::{ButtonVariant, TextInput};
use runtime::templates::{Keep, exists_in, save_in};

use super::dialog::{Dialog, Note, button};
use super::{Act, Dialog as Open, Host, Pressable, Projects};
use crate::theme::Theme;

/// Whether each pick keeps the content, by its label and what it says of a new project.
const PICKS: [(bool, &str, &str); 2] = [
    (
        false,
        "Tools only",
        "New projects start with the tools of this one, with nothing in them.",
    ),
    (
        true,
        "Tools and content",
        "New projects start as this one is now, with its sound files.",
    ),
];

#[derive(Default)]
pub(crate) struct SaveTemplate {
    /// It keeps the records and sound files too.
    content: bool,
    /// Why the last save did not happen.
    message: Option<String>,
}

/// What a save of the name does: saves, replaces, or why it cannot.
enum Saving {
    New,
    Replaces,
    Refused(String),
}

impl SaveTemplate {
    fn saving(shell: &Projects) -> Saving {
        let name = shell.name.text.trim();
        let exists = (shell.saved_folder()).and_then(|folder| exists_in(&folder, name));
        match exists {
            Ok(false) => Saving::New,
            Ok(true) => Saving::Replaces,
            Err(error) => Saving::Refused(format!("{error:#}")),
        }
    }

    pub(super) fn controls(&self, shell: &Projects) -> Vec<Pressable> {
        let pressable = |key: &str, label: &str, role, act| Pressable {
            key: key.into(),
            label: label.into(),
            role,
            checked: None,
            value: None,
            act,
        };
        let mut controls = vec![Pressable {
            value: Some(shell.name.text.clone()),
            ..pressable("template-name", "Name", "textbox", Act::Name)
        }];
        controls.extend(PICKS.map(|(content, label, _)| Pressable {
            checked: Some(content == self.content),
            ..pressable(key_of(content), label, "radio", Act::Content(content))
        }));
        controls.push(pressable("save-cancel", "Cancel", "button", Act::Close));
        match Self::saving(shell) {
            Saving::New => controls.push(pressable("save", "Save", "button", Act::Confirm)),
            Saving::Replaces => controls.push(pressable("save", "Replace", "button", Act::Confirm)),
            Saving::Refused(_) => {}
        }
        controls
    }

    pub(super) fn view<S: Host>(&self, shell: &Projects, theme: &Theme) -> Vec<El> {
        let name = TextInput::new(&shell.name)
            .label("Name")
            .placeholder("Name of the template")
            .on_edit(|s: &mut S| &mut s.shell_mut().name)
            .on_submit(|s: &mut S, tx: &mut Tx| super::run(s, Act::Confirm, None, tx))
            .width(None)
            .build(&theme.kit)
            .key("template-name");
        let picks = PICKS.map(|(content, label, description)| {
            let words = col()
                .grow(1.0)
                .gap(2.0)
                .child(
                    text(label)
                        .font(theme.text("heading"))
                        .color(theme.color("text")),
                )
                .child(
                    text(description)
                        .font(theme.text("label"))
                        .color(theme.color("text-secondary"))
                        .wrap(),
                );
            let picked = content == self.content;
            let e = col()
                .key(key_of(content))
                .role("radio")
                .label(label)
                .checked(picked)
                .focusable(true)
                .w_full()
                .padding(theme.space(3))
                .radius(theme.kit.radius.md)
                .border(1.0, theme.color("border-strong"))
                .on_press_tx(move |s: &mut S, tx: &mut Tx| {
                    super::run(s, Act::Content(content), None, tx)
                })
                .child(words);
            match picked {
                true => e.fill(theme.color("selected")),
                false => theme.kit.interactive(e),
            }
        });
        let saving = Self::saving(shell);
        let empty = shell.name.text.trim().is_empty();
        let note = match (&self.message, &saving) {
            (Some(message), _) => Some(Note::Wrong(message.clone())),
            (None, Saving::Refused(_)) if empty => None,
            (None, Saving::Refused(why)) => Some(Note::Wrong(why.clone())),
            (None, Saving::Replaces) => Some(Note::Quiet(format!(
                "Replace the template “{}”?",
                shell.name.text.trim()
            ))),
            (None, Saving::New) => None,
        };
        let (label, enabled) = match saving {
            Saving::New => ("Save", true),
            Saving::Replaces => ("Replace", true),
            Saving::Refused(_) => ("Save", false),
        };
        Dialog::<S> {
            key: "save-template",
            title: "Save as template".into(),
            description: Some("New projects can start from it.".into()),
            body: [name].into_iter().chain(picks).collect(),
            note,
            buttons: vec![
                button(
                    theme,
                    "save-cancel",
                    "Cancel",
                    ButtonVariant::Neutral,
                    true,
                    |s: &mut S, tx| super::run(s, Act::Close, None, tx),
                ),
                button(
                    theme,
                    "save",
                    label,
                    ButtonVariant::Contrast,
                    enabled,
                    |s: &mut S, tx| super::run(s, Act::Confirm, None, tx),
                ),
            ],
            close: |s: &mut S| super::run(s, Act::Close, None, &mut Tx::default()),
        }
        .build(theme)
    }
}

fn key_of(content: bool) -> &'static str {
    match content {
        true => "keep-content",
        false => "keep-tools",
    }
}

pub(super) fn run<S: Host>(s: &mut S, act: Act) {
    let Some(Open::Save(save)) = &mut s.shell_mut().dialog else {
        return;
    };
    match act {
        Act::Content(content) => save.content = content,
        Act::Confirm => {
            if let Err(error) = self::save(s) {
                if let Some(Open::Save(save)) = &mut s.shell_mut().dialog {
                    save.message = Some(format!("{error:#}"));
                }
            }
        }
        _ => {}
    }
}

/// Saves the open project as the template of the name, and closes the dialog.
fn save<S: Host>(s: &mut S) -> anyhow::Result<()> {
    let shell = s.shell();
    let Some(Open::Save(save)) = &shell.dialog else {
        return Ok(());
    };
    let (Some(project), Some(here)) = (s.project(), &shell.here) else {
        return Ok(());
    };
    let keep = match save.content {
        true => Keep::Everything,
        false => Keep::tools_of(project)?,
    };
    save_in(&shell.saved_folder()?, here, shell.name.text.trim(), &keep)?;
    s.shell_mut().dialog = None;
    Ok(())
}
