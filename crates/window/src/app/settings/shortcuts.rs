//! Shortcuts: every action of the app and of the project's tools, with the keys that do it. A
//! press on an action, or Enter, makes it wait for a key, which is then its one key. Beside it
//! its key is removed, or put back as it was at first.

use rune::{El, Key, KeyEvent, Tx, col, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Keybind, TextInput,
};
use serde_json::Value;
use sound_typescript::shortcuts::{self, Group, Shortcuts, ShortcutsError};

use super::{Act as PageAct, Page, UNDO, control};
use crate::app::State;
use crate::app::keys::{APP, before_tools, key_label};
use crate::controls::Control;

/// What a control of the section does: the action is by its group and its place there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    Record(usize, usize),
    Reset(usize, usize),
    Remove(usize, usize),
    ResetAll,
    Dismiss,
}

/// An action as its row shows it.
struct Line {
    at: (usize, usize),
    id: String,
    title: String,
    /// Its keys as this system draws them.
    keys: Vec<String>,
    changed: bool,
    /// What the composer should know of the other actions on its keys.
    hint: Option<String>,
}

/// An action of the app that comes before a tool's on its key takes the key from it. Other
/// actions of its own group share the key with it, which is worth a line only for a key the
/// composer gave it: a tool shares keys on purpose, such as delete for the chosen notes, else
/// the chosen clips.
fn hint_of(keymap: &Shortcuts, group: &Group, id: &str) -> Option<String> {
    let of_app = group.name == APP;
    let changed = keymap.is_changed(id);
    let (mut taking, mut sharing) = (Vec::new(), Vec::new());
    let others = (keymap.keys(id).iter()).flat_map(|key| keymap.others_with(id, key));
    for (other_group, other) in others {
        let titles = match (other_group.name == APP, of_app) {
            (true, false) if before_tools(&other.name) => &mut taking,
            (true, false) => continue,
            (true, true) => &mut sharing,
            (false, _) if changed && other_group.name == group.name => &mut sharing,
            (false, _) => continue,
        };
        if !titles.contains(&other.title.as_str()) {
            titles.push(other.title.as_str());
        }
    }
    match (taking.is_empty(), sharing.is_empty()) {
        (false, _) => Some(format!(
            "{} has this key, so it does nothing here",
            taking.join(", ")
        )),
        (true, false) => Some(format!("Also on this key: {}", sharing.join(", "))),
        (true, true) => None,
    }
}

/// The groups with the actions the search finds, each with its title.
fn groups(state: &State) -> Vec<(String, Vec<Line>)> {
    let keymap = &state.keys.keymap;
    let words = state.settings.search.text.trim().to_lowercase();
    let found = |line: &Line, group: &str| {
        words.is_empty()
            || line.title.to_lowercase().contains(&words)
            || group.to_lowercase().contains(&words)
            || line
                .keys
                .iter()
                .any(|key| key.to_lowercase().contains(&words))
    };
    let groups = keymap.groups().enumerate().map(|(at, group)| {
        let lines = group.actions.iter().enumerate().map(|(place, action)| {
            let id = shortcuts::id(&group.name, &action.name);
            Line {
                at: (at, place),
                keys: keymap.keys(&id).iter().map(|key| key_label(key)).collect(),
                changed: keymap.is_changed(&id),
                hint: hint_of(keymap, group, &id),
                title: action.title.clone(),
                id,
            }
        });
        let lines: Vec<Line> = lines.filter(|line| found(line, &group.title)).collect();
        (group.title.clone(), lines)
    });
    groups.filter(|(_, lines)| !lines.is_empty()).collect()
}

/// The id of the action at `(group, place)` of the keymap.
fn id_at(state: &State, (group, place): (usize, usize)) -> Option<String> {
    let group = state.keys.keymap.groups().nth(group)?;
    let action = group.actions.get(place)?;
    Some(shortcuts::id(&group.name, &action.name))
}

impl State {
    /// Changes the keymap, and says on the page when the change is not kept. A file that does
    /// not read has its own line there.
    fn change_keys(&mut self, change: impl FnOnce(&mut Shortcuts) -> Result<(), ShortcutsError>) {
        let changed = change(&mut self.keys.keymap);
        if let Err(error) = &changed {
            eprintln!("error: {error}");
        }
        if let Some(page) = &mut self.settings.page {
            page.error = match changed {
                Ok(()) | Err(ShortcutsError::Read { .. }) => None,
                Err(ShortcutsError::Write { error, .. }) => {
                    Some(format!("The change could not be kept: {error}"))
                }
            };
        }
        self.handled();
    }

    fn wait_for_key(&mut self, id: String) {
        if let Some(page) = &mut self.settings.page {
            page.waiting = Some(id);
        }
    }

    /// A key went down on the row of the action `id`. While it waits, the key becomes its one
    /// key; Escape leaves it as it was, Tab moves on, and a modifier alone is no key yet.
    fn take_key_for(&mut self, id: &str, key: &KeyEvent) -> bool {
        let Some(page) = &mut self.settings.page else {
            return false;
        };
        if page.waiting.as_deref() != Some(id) {
            // Return on a row waits for a key, as a press does.
            if !matches!(key.key, Key::Enter) || key.mods != rune::Mods::default() {
                return false;
            }
            page.waiting = Some(id.to_string());
            self.handled();
            return true;
        }
        match &key.key {
            Key::Modifier { .. } => return true,
            Key::Tab => {
                page.waiting = None;
                return false;
            }
            Key::Escape => page.waiting = None,
            _ => {
                let Some(chord) = crate::app::keys::chord_of(key) else {
                    return true;
                };
                page.waiting = None;
                self.change_keys(|keymap| keymap.set_keys(id, vec![chord]));
            }
        }
        self.handled();
        true
    }

    fn reset_every_key(&mut self) {
        let keymap = &self.keys.keymap;
        let groups = keymap.groups();
        let ids = groups.flat_map(|group| {
            (group.actions.iter()).map(|action| shortcuts::id(&group.name, &action.name))
        });
        let changed: Vec<String> = ids.filter(|id| keymap.is_changed(id)).collect();
        self.change_keys(|keymap| changed.iter().try_for_each(|id| keymap.reset(id)));
    }
}

pub(super) fn view(state: &State, page: &Page) -> El {
    let theme = &state.theme;
    let kit = &theme.kit;
    let muted = theme.color("text-muted");
    let keymap = &state.keys.keymap;
    let intro =
        text("Click an action, then press the key that should do it. Escape leaves it as it is.")
            .font(theme.text("body"))
            .color(muted)
            .wrap();
    let any_changed = keymap.groups().any(|group| {
        (group.actions.iter())
            .any(|action| keymap.is_changed(&shortcuts::id(&group.name, &action.name)))
    });
    let reset_all = any_changed.then(|| {
        Button::new("Back to the usual keys")
            .variant(ButtonVariant::Neutral)
            .on_press(|s: &mut State| s.reset_every_key())
            .build(kit)
            .key("shortcuts-reset-all")
    });
    let search = TextInput::new(&state.settings.search)
        .label("Find")
        .placeholder("Find an action or a key")
        .leading_icon(kit.glyph.search)
        .on_edit(|s: &mut State| &mut s.settings.search)
        .width(None)
        .build(kit)
        .key("shortcuts-search");
    let unread = keymap.is_unread().then(|| {
        text("Your own shortcuts could not be read. The keys are as shown here, and a change will not be kept.")
            .key("shortcuts-unread")
            .font(theme.text("label"))
            .color(theme.color("warning"))
            .wrap()
    });
    let error = page.error.clone().map(|error| {
        row()
            .key("shortcuts-error")
            .w_full()
            .gap(theme.space(2))
            .items_center()
            .child(
                text(error)
                    .font(theme.text("label"))
                    .color(theme.color("danger"))
                    .wrap()
                    .grow(1.0)
                    .min_w(0.0),
            )
            .child(
                Button::new("Dismiss")
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(|s: &mut State| {
                        if let Some(page) = &mut s.settings.page {
                            page.error = None;
                        }
                    })
                    .build(kit)
                    .key("shortcuts-dismiss"),
            )
    });
    let groups = groups(state);
    let none = groups.is_empty().then(|| {
        text("No action or key has these words.")
            .font(theme.text("body"))
            .color(muted)
    });
    let cards = groups.into_iter().map(|(title, lines)| {
        let last = lines.len().saturating_sub(1);
        let rows = lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| shortcut_row(state, page, line, index < last));
        col()
            .w_full()
            .px(20.0)
            .pt(20.0)
            .pb(8.0)
            .radius(theme.radius("lg").unwrap_or(8.0))
            .border(1.0, theme.color("border-strong"))
            .child(
                text(title)
                    .font(theme.text("heading"))
                    .color(theme.color("text"))
                    .pb(2.0),
            )
            .children(rows)
    });
    col()
        .w_full()
        .gap(theme.space(4))
        .child(intro)
        .child(
            row()
                .w_full()
                .gap(theme.space(2))
                .items_center()
                .child(rune::el().grow(1.0).min_w(0.0).child(search))
                .children(reset_all),
        )
        .children(unread)
        .children(error)
        .children(none)
        .children(cards)
}

fn shortcut_row(state: &State, page: &Page, line: Line, rule: bool) -> El {
    let theme = &state.theme;
    let kit = &theme.kit;
    let muted = theme.color("text-muted");
    let waiting = page.waiting.as_deref() == Some(line.id.as_str());
    let id = line.id.clone();
    let quiet = |key: String, glyph, label: &str, press: Box<dyn Fn(&mut State)>| {
        IconButton::new(glyph)
            .variant(IconButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .label(label)
            .help(label)
            .on_press(move |s: &mut State| press(s))
            .build(kit)
            .key(key)
    };
    let reset = (line.changed && !waiting).then(|| {
        let id = id.clone();
        quiet(
            format!("shortcut-reset-{id}"),
            &UNDO,
            "Back to its usual key",
            Box::new(move |s| s.change_keys(|keymap| keymap.reset(&id))),
        )
    });
    let remove = (!line.keys.is_empty() && !waiting).then(|| {
        let id = id.clone();
        quiet(
            format!("shortcut-remove-{id}"),
            kit.glyph.close,
            "Remove its key",
            Box::new(move |s| s.change_keys(|keymap| keymap.set_keys(&id, Vec::new()))),
        )
    });
    // They show with the pointer over the row, and while the keys are in it.
    let buttons = row()
        .gap(2.0)
        .group_opacity(0.0, 1.0)
        .children(reset)
        .children(remove);
    let keys = match (waiting, line.keys.is_empty()) {
        (true, _) => text("Press a key")
            .key("shortcut-waiting")
            .font(theme.text("label"))
            .color(theme.color("focus")),
        (false, true) => text("No key").font(theme.text("label")).color(muted),
        (false, false) => row().gap(theme.space(2)).children(
            line.keys
                .iter()
                .map(|key| Keybind::new(&caps(key)).build(kit)),
        ),
    };
    let hint = line.hint.map(|hint| {
        text(hint)
            .key(format!("shortcut-hint-{id}"))
            .font(theme.text("label"))
            .color(muted)
            .truncate()
    });
    let said = match (waiting, line.keys.is_empty()) {
        (true, _) => "Press a key".to_string(),
        (false, true) => "No key".to_string(),
        (false, false) => line.keys.join(", "),
    };
    let record = id.clone();
    let action = row()
        .key(format!("shortcut-{id}"))
        .role("button")
        .label(line.title.clone())
        .a11y_value(said)
        .h(48.0)
        // Room for its ring and fill around what it shows, which stays in line with the title.
        .mx(-8.0)
        .px(8.0)
        .gap(12.0)
        .items_center()
        .radius(theme.radius("md").unwrap_or(6.0))
        .fill(match waiting {
            true => theme.color("selected"),
            false => rune::Color::TRANSPARENT,
        })
        .hover_group()
        .focusable(true)
        .on_press(move |s: &mut State| s.wait_for_key(record.clone()))
        .on_key(move |s: &mut State, key: &KeyEvent, _: &mut Tx| s.take_key_for(&id, key))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(
                    text(line.title)
                        .font(theme.text("body"))
                        .color(theme.color("text"))
                        .truncate()
                        .a11y_hidden(),
                )
                .children(hint),
        )
        .child(buttons)
        .child(keys);
    let mut wrap = col().w_full().child(kit.interactive(action));
    if rule {
        wrap = wrap.border_sides([0.0, 0.0, 1.0, 0.0], theme.color("border-strong"));
    }
    wrap
}

/// A key as caps: `⇧⌘Z` is ⇧, ⌘ and Z; `Ctrl+Z` is Ctrl and Z.
fn caps(key: &str) -> Vec<String> {
    if key.len() > 1 && key.contains('+') && !key.ends_with('+') {
        return key.split('+').map(str::to_string).collect();
    }
    let held = key.chars().take_while(|c| "⌃⌥⇧⌘".contains(*c));
    let mut caps: Vec<String> = held.map(String::from).collect();
    let rest: String = key.chars().skip(caps.len()).collect();
    caps.push(rest);
    caps
}

pub(super) fn controls(state: &State, page: &Page) -> Vec<Control> {
    let shortcut = |act| PageAct::Shortcut(act);
    let mut controls = vec![Control {
        value: Some(Value::String(state.settings.search.text.clone())),
        actions: vec!["commit".into()],
        ..control(
            "shortcuts-search".into(),
            "Find",
            "textbox",
            PageAct::Search,
        )
    }];
    let keymap = &state.keys.keymap;
    if (keymap.groups()).any(|group| {
        (group.actions.iter())
            .any(|action| keymap.is_changed(&shortcuts::id(&group.name, &action.name)))
    }) {
        controls.push(control(
            "shortcuts-reset-all".into(),
            "Back to the usual keys",
            "button",
            shortcut(Act::ResetAll),
        ));
    }
    if page.error.is_some() {
        controls.push(control(
            "shortcuts-dismiss".into(),
            "Dismiss",
            "button",
            shortcut(Act::Dismiss),
        ));
    }
    for (_, lines) in groups(state) {
        for line in lines {
            let (group, place) = line.at;
            let waiting = page.waiting.as_deref() == Some(line.id.as_str());
            controls.push(Control {
                value: Some(match waiting {
                    true => Value::String("Press a key".into()),
                    false => Value::from(line.keys.clone()),
                }),
                actions: vec!["press".into(), "set".into()],
                ..control(
                    format!("shortcut-{}", line.id),
                    &line.title,
                    "button",
                    shortcut(Act::Record(group, place)),
                )
            });
            if line.changed {
                let label = format!("{}: back to its usual key", line.title);
                controls.push(control(
                    format!("shortcut-reset-{}", line.id),
                    &label,
                    "button",
                    shortcut(Act::Reset(group, place)),
                ));
            }
            if !line.keys.is_empty() {
                let label = format!("{}: remove its key", line.title);
                controls.push(control(
                    format!("shortcut-remove-{}", line.id),
                    &label,
                    "button",
                    shortcut(Act::Remove(group, place)),
                ));
            }
        }
    }
    controls
}

/// Runs `act` for `--act`: `set` gives an action the key of its value, as the keymap names one
/// (`cmd-shift-u`), or none for null.
pub(super) fn run(
    state: &mut State,
    act: Act,
    action: &str,
    value: Option<Value>,
) -> anyhow::Result<()> {
    match act {
        Act::Record(group, place) => {
            let Some(id) = id_at(state, (group, place)) else {
                return Ok(());
            };
            match (action, value) {
                ("set", Some(Value::String(chord))) => {
                    state.change_keys(|keymap| keymap.set_keys(&id, vec![chord]))
                }
                ("set", Some(Value::Null)) => {
                    state.change_keys(|keymap| keymap.set_keys(&id, Vec::new()))
                }
                ("set", _) => {
                    anyhow::bail!("set takes a key such as \"cmd-shift-u\", or null for none")
                }
                _ => state.wait_for_key(id),
            }
        }
        Act::Reset(group, place) => {
            if let Some(id) = id_at(state, (group, place)) {
                state.change_keys(|keymap| keymap.reset(&id));
            }
        }
        Act::Remove(group, place) => {
            if let Some(id) = id_at(state, (group, place)) {
                state.change_keys(|keymap| keymap.set_keys(&id, Vec::new()));
            }
        }
        Act::ResetAll => state.reset_every_key(),
        Act::Dismiss => {
            if let Some(page) = &mut state.settings.page {
                page.error = None;
            }
        }
    }
    Ok(())
}
