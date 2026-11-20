//! "New project": the templates to start from, the name of the project and the folder it goes
//! in. Create writes the template there and opens it. A template the composer saved can be
//! removed here.

use std::path::{Path, PathBuf};

use rune::{El, Icon, ScrollSpec, Tx, col, row, text};
use rune_kit::{ButtonVariant, IconButton, IconButtonVariant, TextInput};
use runtime::templates::{Choice, Saved};

use super::dialog::{Dialog, Note, button};
use super::{Act, Dialog as Open, Host, Pressable, Projects, saved_of};
use crate::theme::Theme;

/// The list scrolls past this, so many saved templates never push the name out of the dialog.
const LIST_HEIGHT: f32 = 264.0;

static TRASH: Icon = Icon {
    name: "trash-2",
    view: 24.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><path d="M10 11v6"/><path d="M14 11v6"/></g>"#,
};

pub(crate) struct NewProject {
    choices: Vec<Choice>,
    /// The row of the template the project starts from.
    picked: usize,
    /// The folder the project goes in.
    place: PathBuf,
    /// Why the saved templates did not read, or the project was not made.
    message: Option<String>,
    /// The saved template asked about, by its row.
    removing: Option<usize>,
}

impl NewProject {
    pub(super) fn new(choices: Vec<Choice>, place: PathBuf, message: Option<String>) -> Self {
        Self {
            choices,
            picked: 0,
            place,
            message,
            removing: None,
        }
    }

    pub(super) fn controls(&self, shell: &Projects) -> Vec<Pressable> {
        let pressable = |key: String, label: String, role, act| Pressable {
            key,
            label,
            role,
            checked: None,
            value: None,
            act,
        };
        if let Some(saved) = self
            .removing
            .and_then(|index| saved_of(&self.choices, index))
        {
            return vec![
                pressable(
                    "remove-cancel".into(),
                    "Cancel".into(),
                    "button",
                    Act::Close,
                ),
                pressable(
                    "remove-confirm".into(),
                    format!("Remove {}", saved.name),
                    "button",
                    Act::Confirm,
                ),
            ];
        }
        let mut controls = Vec::new();
        for (index, choice) in self.choices.iter().enumerate() {
            controls.push(Pressable {
                checked: Some(index == self.picked),
                ..pressable(
                    format!("template-{index}"),
                    choice.name().into(),
                    "radio",
                    Act::Pick(index),
                )
            });
            if let Choice::Saved(saved) = choice {
                controls.push(pressable(
                    format!("remove-template-{index}"),
                    remove_words(&saved.name),
                    "button",
                    Act::Remove(index),
                ));
            }
        }
        let name = &shell.name.text;
        controls.push(Pressable {
            value: Some(name.clone()),
            ..pressable("project-name".into(), "Name".into(), "textbox", Act::Name)
        });
        controls.push(pressable(
            "project-place".into(),
            "Change folder".into(),
            "button",
            Act::Place,
        ));
        controls.push(pressable(
            "new-cancel".into(),
            "Cancel".into(),
            "button",
            Act::Close,
        ));
        if refused(name, &self.place).is_none() {
            controls.push(pressable(
                "create".into(),
                "Create".into(),
                "button",
                Act::Confirm,
            ));
        }
        controls
    }

    pub(super) fn view<S: Host>(&self, shell: &Projects, theme: &Theme) -> Vec<El> {
        if let Some(saved) = self
            .removing
            .and_then(|index| saved_of(&self.choices, index))
        {
            return Dialog::<S> {
                key: "remove-template",
                title: format!("Remove the template “{}”?", saved.name),
                description: Some("Projects made from it keep their tools.".into()),
                body: Vec::new(),
                note: None,
                buttons: vec![
                    button(
                        theme,
                        "remove-cancel",
                        "Cancel",
                        ButtonVariant::Neutral,
                        true,
                        |s: &mut S, tx| super::run(s, Act::Close, None, tx),
                    ),
                    button(
                        theme,
                        "remove-confirm",
                        "Remove",
                        ButtonVariant::Danger,
                        true,
                        |s: &mut S, tx| super::run(s, Act::Confirm, None, tx),
                    ),
                ],
                close: close::<S>,
            }
            .build(theme);
        }
        let rows = self.choices.iter().enumerate().map(|(index, choice)| {
            template_row(
                theme,
                format!("template-{index}"),
                choice,
                Some(index == self.picked),
                remove_button::<S>(theme, index, choice),
                move |s: &mut S, tx: &mut Tx| super::run(s, Act::Pick(index), None, tx),
            )
        });
        let list = col()
            .key("templates")
            .role("radiogroup")
            .label("Template")
            .w_full()
            .max_h(LIST_HEIGHT)
            .gap(theme.space(1))
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .children(rows);
        let name = TextInput::new(&shell.name)
            .label("Name")
            .placeholder("Name of the project")
            .on_edit(|s: &mut S| &mut s.shell_mut().name)
            .on_submit(|s: &mut S, tx: &mut Tx| super::run(s, Act::Confirm, None, tx))
            .width(None)
            .build(&theme.kit)
            .key("project-name");
        let place = row()
            .w_full()
            .gap(theme.space(2))
            .items_center()
            .child(
                text(format!("In {}", super::short_path(&self.place)))
                    .key("project-place-words")
                    .font(theme.text("label"))
                    .color(theme.color("text-secondary"))
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .wrap(),
            )
            .child(button(
                theme,
                "project-place",
                "Change…",
                ButtonVariant::Neutral,
                true,
                |s: &mut S, tx| super::run(s, Act::Place, None, tx),
            ));
        let refused = refused(&shell.name.text, &self.place);
        let note = match (&self.message, &refused) {
            (Some(message), _) => Some(Note::Wrong(message.clone())),
            (None, Some(Refused::Empty)) => None,
            (None, Some(refused)) => Some(Note::Wrong(refused.words(&shell.name.text))),
            (None, None) => None,
        };
        Dialog::<S> {
            key: "new-project",
            title: "New project".into(),
            description: Some("Start from a template, or from nothing with Bare.".into()),
            body: vec![list, name, place],
            note,
            buttons: vec![
                button(
                    theme,
                    "new-cancel",
                    "Cancel",
                    ButtonVariant::Neutral,
                    true,
                    |s: &mut S, tx| super::run(s, Act::Close, None, tx),
                ),
                button(
                    theme,
                    "create",
                    "Create",
                    ButtonVariant::Contrast,
                    refused.is_none(),
                    |s: &mut S, tx| super::run(s, Act::Confirm, None, tx),
                ),
            ],
            close: close::<S>,
        }
        .build(theme)
    }
}

fn remove_words(name: &str) -> String {
    format!("Remove the template “{name}”")
}

/// A template by its name and what it is: a pick of a list when `picked` says whether it is
/// the one, else a button.
pub(crate) fn template_row<S: 'static>(
    theme: &Theme,
    key: String,
    choice: &Choice,
    picked: Option<bool>,
    trailing: Option<El>,
    press: impl Fn(&mut S, &mut Tx) + 'static,
) -> El {
    let words = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(
            text(choice.name().to_string())
                .font(theme.text("heading"))
                .color(theme.color("text")),
        )
        .child(
            text(choice.description().to_string())
                .font(theme.text("label"))
                .color(theme.color("text-secondary"))
                .wrap(),
        );
    let e = row()
        .key(key)
        .role(if picked.is_some() { "radio" } else { "button" })
        .label(choice.name().to_string())
        .focusable(true)
        .w_full()
        .padding(theme.space(3))
        .gap(theme.space(2))
        .items_start()
        .radius(theme.kit.radius.md)
        .border(1.0, theme.color("border"))
        .on_press_tx(press)
        .child(words)
        .children(trailing);
    match picked {
        Some(true) => e.checked(true).fill(theme.color("selected")),
        Some(false) => theme.kit.interactive(e.checked(false)),
        None => theme.kit.interactive(e),
    }
}

/// The button that asks to remove the saved template of row `index`.
fn remove_button<S: Host>(theme: &Theme, index: usize, choice: &Choice) -> Option<El> {
    let Choice::Saved(saved) = choice else {
        return None;
    };
    let button = IconButton::new(&TRASH)
        .variant(IconButtonVariant::Ghost)
        .label(remove_words(&saved.name))
        .on_press_tx(move |s: &mut S, tx: &mut Tx| super::run(s, Act::Remove(index), None, tx))
        .build(&theme.kit)
        .key(format!("remove-template-{index}"));
    Some(button)
}

/// Why a name makes no new project in its folder.
#[derive(Debug, PartialEq, Eq)]
enum Refused {
    Empty,
    /// It cannot be the name of a folder on every system.
    Name,
    /// A folder of that name has files in it.
    Taken,
}

impl Refused {
    fn words(&self, name: &str) -> String {
        let name = name.trim();
        match self {
            Refused::Empty => "Give the project a name.".into(),
            Refused::Name => {
                format!("“{name}” cannot be the name of a project. Pick another name.")
            }
            Refused::Taken => format!("“{name}” has files in it. Pick another name."),
        }
    }
}

fn refused(name: &str, place: &Path) -> Option<Refused> {
    let name = name.trim();
    if name.is_empty() {
        return Some(Refused::Empty);
    }
    let bad = |character: char| {
        matches!(
            character,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        ) || character.is_control()
    };
    // A leading dot hides the folder; Windows drops dots and spaces at the end.
    if name.starts_with('.') || name.ends_with('.') || name.chars().any(bad) {
        return Some(Refused::Name);
    }
    let folder = place.join(name);
    let taken = folder.exists() && !runtime::templates::is_empty(&folder).unwrap_or(false);
    taken.then_some(Refused::Taken)
}

fn close<S: Host>(s: &mut S) {
    super::run(s, Act::Close, None, &mut Tx::default());
}

pub(super) fn run<S: Host>(s: &mut S, act: Act, tx: &mut Tx) {
    let shell = s.shell_mut();
    let Some(Open::New(new)) = &mut shell.dialog else {
        return;
    };
    match act {
        Act::Close if new.removing.is_some() => new.removing = None,
        Act::Close => shell.dialog = None,
        Act::Pick(index) if index < new.choices.len() => new.picked = index,
        Act::Remove(index) if saved_of(&new.choices, index).is_some() => new.removing = Some(index),
        Act::Place => {
            let options = rune::FileDialogOptions {
                title: "Where the project goes".into(),
                directory: Some(new.place.clone()),
                ..Default::default()
            };
            tx.open_folder(options, |s: &mut S, picked, _| {
                let shell = s.shell_mut();
                // The name is what is left to give.
                shell.name.focused = true;
                let Some(Open::New(new)) = &mut shell.dialog else {
                    return;
                };
                match picked {
                    Ok(Some(folder)) => new.place = folder,
                    Ok(None) => {}
                    Err(error) => {
                        new.message = Some(format!("The folder panel did not open: {error}"))
                    }
                }
            });
        }
        Act::Confirm => match new.removing.take() {
            Some(index) => remove(s, index),
            None => create(s, tx),
        },
        _ => {}
    }
}

/// Removes the saved template of row `index`, and reads them again.
fn remove<S: Host>(s: &mut S, index: usize) {
    let shell = s.shell_mut();
    let Some(Open::New(new)) = &shell.dialog else {
        return;
    };
    let removed = saved_of(&new.choices, index).map(Saved::remove);
    let choices = shell.choices();
    let Some(Open::New(new)) = &mut shell.dialog else {
        return;
    };
    new.message = match (removed, choices) {
        (Some(Err(error)), _) | (_, Err(error)) => Some(format!("{error:#}")),
        (_, Ok(choices)) => {
            new.choices = choices;
            None
        }
    };
    new.picked = new.picked.min(new.choices.len().saturating_sub(1));
}

/// Writes the template into the folder of the name and opens it.
fn create<S: Host>(s: &mut S, tx: &mut Tx) {
    let shell = s.shell_mut();
    let Some(Open::New(new)) = &mut shell.dialog else {
        return;
    };
    let name = shell.name.text.trim().to_string();
    if refused(&name, &new.place).is_some() {
        return;
    }
    let folder = new.place.join(&name);
    let Some(choice) = new.choices.get(new.picked) else {
        return;
    };
    if let Err(error) = choice.write(&folder) {
        new.message = Some(format!("{error:#}"));
        return;
    }
    shell.dialog = None;
    if let Err(error) = shell.open(&folder, tx) {
        s.tell(format!("{error:#}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_refused_when_empty_unfit_for_a_folder_or_taken() {
        let place = tempfile::tempdir().unwrap();
        std::fs::create_dir(place.path().join("empty")).unwrap();
        std::fs::create_dir(place.path().join("song")).unwrap();
        std::fs::write(place.path().join("song/notes.txt"), "").unwrap();
        let refused = |name| refused(name, place.path());
        assert_eq!(refused("  "), Some(Refused::Empty));
        assert_eq!(refused("a/b"), Some(Refused::Name));
        assert_eq!(refused(".hidden"), Some(Refused::Name));
        assert_eq!(refused("song"), Some(Refused::Taken));
        assert_eq!(refused("empty"), None);
        assert_eq!(refused(" New song "), None);
    }
}
