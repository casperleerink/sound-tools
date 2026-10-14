//! The menu of the selected clips. A right click on a clip opens it, as do ctrl-click on macOS,
//! shift-F10 and the menu key. It opens at the press on macOS and Linux and at the release on
//! Windows, as the menus of each system do. A clip that is not selected is selected alone
//! first; one that is keeps the others, as in the Finder.
//!
//! It offers what the keys do to the selected clips, and fitting the tempo to the take of a
//! recorded clip. When there is an editor to pick, the editor of the note clips and the choices
//! of the first one's editor come first.

use gpui::{
    App, Context, Entity, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Window, point,
    px,
};
use sound_core::{Changes, Project, ProjectError};
use sound_notes::{Clip, EditorName, Editors, FieldName, ViewField, ViewValue};
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuGroup, MenuItem};

use super::scene::Scene;
use super::{HeldMenu, Timeline};
use crate::AnyClip;
use crate::view::layout::{HEADER_WIDTH, Part, RULER_HEIGHT, TRACK_HEIGHT};

/// Fits the project tempo to the take of a clip. Whoever registers the view passes it in: the
/// fit is an extension of its own, and this crate knows none.
#[derive(Clone, Copy)]
pub struct FitTempo {
    /// Why this project cannot fit, in words, when it cannot.
    pub unavailable: fn(&Project) -> Option<&'static str>,
    /// The fit, as one undo step.
    pub fit: fn(&mut Project, &Clip) -> Result<(), ProjectError>,
}

const FIT_TEMPO: &str = "fit-tempo";
const CUT: &str = "cut";
const COPY: &str = "copy";
const DUPLICATE: &str = "duplicate";
const DELETE: &str = "delete";
const EDITOR: &str = "editor";
/// Before the name of an editor, in the submenu of [`EDITOR`].
const USE_EDITOR: &str = "editor:";
/// Before the name of a clip field that is a choice: the row of its submenu.
const CHOICE: &str = "choice:";
/// Before `<field>:<index of the option>`.
const SET_CHOICE: &str = "set:";
/// Before the name of a clip field that is a toggle.
const TOGGLE: &str = "toggle:";

/// A pick that changes the editor of the selected note clips or a field of their view.
#[derive(Debug, PartialEq)]
enum EditorPick {
    Use(EditorName),
    Choice(FieldName, usize),
    Toggle(FieldName),
}

impl EditorPick {
    fn value(&self) -> String {
        match self {
            Self::Use(name) => format!("{USE_EDITOR}{}", name.as_str()),
            Self::Choice(field, index) => format!("{SET_CHOICE}{}:{index}", field.as_str()),
            Self::Toggle(field) => format!("{TOGGLE}{}", field.as_str()),
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        if let Some(name) = value.strip_prefix(USE_EDITOR) {
            return EditorName::new(name).ok().map(Self::Use);
        }
        if let Some(field) = value.strip_prefix(TOGGLE) {
            return FieldName::new(field).ok().map(Self::Toggle);
        }
        let (field, index) = value.strip_prefix(SET_CHOICE)?.split_once(':')?;
        Some(Self::Choice(
            FieldName::new(field).ok()?,
            index.parse().ok()?,
        ))
    }
}

/// What a menu calls a field: its label, else its name in words, as the SDK's cards do.
fn field_label(field: &FieldName, view_field: &ViewField) -> String {
    let (ViewField::Choice { label, .. } | ViewField::Toggle { label, .. }) = view_field;
    if let Some(label) = label {
        return label.clone();
    }
    let spaced = field.as_str().replace('_', " ");
    let mut characters = spaced.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => spaced,
    }
}

fn option_label(option: &ViewValue) -> String {
    match option {
        ViewValue::Word(word) => word.clone(),
        other => other.to_string(),
    }
}

/// The editor of `clip` and its clip fields, when there is an editor to pick: the project has
/// editors of its own, or the clip names one it lacks. Else none, and the menu is as it was.
fn editor_items(editors: &Editors, clip: &Clip) -> Vec<MenuItem> {
    let shown = editors.of(clip);
    let all = editors.all();
    if all.len() < 2 && shown.missing.is_none() {
        return Vec::new();
    }
    let row = |value: String, label: String| MenuItem::new(value, label).selectable(false);
    let current = clip.editor.clone().unwrap_or_else(EditorName::piano_roll);
    let mut choices: Vec<MenuItem> = all
        .iter()
        .map(|spec| {
            row(
                EditorPick::Use(spec.name.clone()).value(),
                spec.title.clone(),
            )
            .checked(spec.name == current)
        })
        .collect();
    if let Some(missing) = shown.missing {
        let label = missing.as_str().to_owned();
        let item = row(EditorPick::Use(missing).value(), label);
        let item = item.checked(true).disabled(true);
        choices.push(item.description("Not in this project"));
    }
    let mut items = vec![row(EDITOR.to_owned(), "Editor".to_owned()).submenu(choices)];
    for (field, view_field) in shown.spec.clip_fields() {
        let value = view_field.value_in(&clip.view, field);
        let label = field_label(field, view_field);
        items.push(match view_field {
            ViewField::Choice { options, .. } => {
                let options = options.iter().enumerate().map(|(index, option)| {
                    row(
                        EditorPick::Choice(field.clone(), index).value(),
                        option_label(option),
                    )
                    .checked(*option == value)
                });
                row(format!("{CHOICE}{}", field.as_str()), label).submenu(options)
            }
            ViewField::Toggle { .. } => row(EditorPick::Toggle(field.clone()).value(), label)
                .checked(value == ViewValue::Toggle(true)),
        });
    }
    items
}

/// Whether a press is the start of opening the menu.
pub(super) fn opens_menu(event: &MouseDownEvent) -> bool {
    match event.button {
        MouseButton::Right => true,
        MouseButton::Left => cfg!(target_os = "macos") && event.modifiers.control,
        _ => false,
    }
}

/// The key that deletes, as a menu shows it.
pub(super) fn delete_key() -> &'static str {
    match cfg!(target_os = "macos") {
        true => "backspace",
        false => "delete",
    }
}

/// `fit` is `None` when the fit is not offered, else why it cannot run, if it cannot.
fn entries(editor: Vec<MenuItem>, fit: Option<Option<&'static str>>) -> Vec<MenuEntry> {
    let command =
        |value: &'static str, label: &'static str| MenuItem::new(value, label).selectable(false);
    let mut entries = Vec::new();
    if !editor.is_empty() {
        entries.push(MenuEntry::Group(MenuGroup::new().items(editor)));
        entries.push(MenuEntry::Separator);
    }
    if let Some(unavailable) = fit {
        let fit = command(FIT_TEMPO, "Fit tempo to take");
        let fit = match unavailable {
            Some(reason) => fit.disabled(true).description(reason),
            None => fit,
        };
        entries.push(MenuEntry::Group(MenuGroup::new().item(fit)));
        entries.push(MenuEntry::Separator);
    }
    entries.push(MenuEntry::Group(MenuGroup::new().items([
        command(CUT, "Cut").shortcut("mod+x"),
        command(COPY, "Copy").shortcut("mod+c"),
        command(DUPLICATE, "Duplicate").shortcut("mod+d"),
    ])));
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Group(
        MenuGroup::new().item(command(DELETE, "Delete").shortcut(delete_key())),
    ));
    entries
}

impl Timeline {
    /// A press that opens the menu, at `(x, y)` in the tracks and `position` in the window.
    /// Off a clip, nothing. On Windows the menu waits for the release.
    pub(super) fn press_for_clip_menu(
        &mut self,
        (x, y): (f32, f32),
        position: Point<Pixels>,
        scene: &Scene,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let part = scene.viewport.part_at(&scene.layout, y);
        if !matches!(part, Some((_, Part::Track))) {
            return;
        }
        let Some((shape, _)) = scene.zone_at(x, y) else {
            return;
        };
        let id = shape.id.clone();
        if !self.clips.contains(&id) {
            self.select_clip(Some(id), cx);
        }
        match cfg!(windows) {
            true => self.menu_held = Some(HeldMenu::Clip),
            false => self.show_clip_menu(position, window, cx),
        }
    }

    pub(super) fn release_for_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.menu_held.take() {
            Some(HeldMenu::Clip) => self.show_clip_menu(position, window, cx),
            Some(HeldMenu::Ruler) => self.show_ruler_menu(position, window, cx),
            None => {}
        }
    }

    /// The menu at the first selected clip, as shift-F10 and the menu key open it. Whether
    /// there was a clip to open it at.
    pub fn open_clip_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(position) = self.selection_corner(cx) else {
            return false;
        };
        self.show_clip_menu(position, window, cx);
        true
    }

    pub fn clip_menu(&self) -> &Entity<DropdownMenu> {
        &self.clip_menu
    }

    /// The start of the first selected clip, in the middle of its row, in the window.
    fn selection_corner(&mut self, cx: &mut Context<Self>) -> Option<Point<Pixels>> {
        self.refresh_order(cx);
        let id = self.clips.primary()?;
        let clip = AnyClip::read(self.session.read(cx).project(), id)?;
        let row = self.row_of(&id.parent()?)?;
        let viewport = self.painted.get();
        let x = HEADER_WIDTH + viewport.x_of(clip.start()).max(0.0);
        let y = RULER_HEIGHT + viewport.y_of(&self.rows(cx), row) + TRACK_HEIGHT / 2.0;
        Some(self.painted_bounds.get().origin + point(px(x), px(y)))
    }

    fn show_clip_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = self.session.read(cx).project();
        let fit = self
            .take_clip(cx)
            .map(|_| (self.fit_tempo.unavailable)(project));
        let editor = match (project.provided::<Editors>(), self.primary_notes(cx)) {
            (Some(editors), Some(clip)) => editor_items(editors, &clip),
            _ => Vec::new(),
        };
        self.clip_menu.update(cx, |menu, cx| {
            menu.set_entries(entries(editor, fit), cx);
            menu.open_at(position, window, cx);
        });
    }

    /// The selected clip when it is the only one and names a raw take. A fit follows a
    /// performance, so a clip drawn by hand offers none.
    fn take_clip(&self, cx: &App) -> Option<Clip> {
        if self.clips.len() != 1 {
            return None;
        }
        let project = self.session.read(cx).project();
        let clip = project.state(&project.resolve::<Clip>(self.clips.primary()?)?)?;
        clip.take.is_some().then(|| clip.clone())
    }

    /// The first selected clip, when it is a note clip.
    fn primary_notes(&self, cx: &App) -> Option<Clip> {
        let project = self.session.read(cx).project();
        project
            .state(&project.resolve::<Clip>(self.clips.primary()?)?)
            .cloned()
    }

    pub(super) fn on_clip_menu(&mut self, picked: &SharedString, cx: &mut Context<Self>) {
        match picked.as_ref() {
            FIT_TEMPO => self.fit_tempo_to_take(cx),
            CUT => self.cut(cx),
            COPY => {
                self.copy(cx);
            }
            DUPLICATE => self.duplicate(cx),
            DELETE => self.delete_clips("Delete clip", "Delete clips", None, cx),
            other => {
                if let Some(pick) = EditorPick::from_value(other) {
                    self.pick_for_editor(pick, cx);
                }
            }
        }
    }

    /// An editor for every selected note clip, or a field for those in the editor of the
    /// first. One undo step.
    fn pick_for_editor(&mut self, pick: EditorPick, cx: &mut Context<Self>) {
        let editors = self.session.read(cx).project().provided::<Editors>();
        let (Some(editors), Some(primary)) = (editors.cloned(), self.primary_notes(cx)) else {
            return;
        };
        let spec = editors.of(&primary).spec;
        let declared = |field: &FieldName| {
            let mut fields = spec.clip_fields().iter();
            fields
                .find(|(name, _)| name == field)
                .map(|(_, view_field)| view_field)
        };
        let (label, field, value) = match pick {
            EditorPick::Use(name) => {
                let all = editors.all();
                let Some(picked) = all.iter().find(|spec| spec.name == name) else {
                    return;
                };
                let label = format!("Use {} editor", picked.title);
                let editor = (name != EditorName::piano_roll()).then_some(name);
                self.change_note_clips(&label, |clip| clip.editor.clone_from(&editor), cx);
                return;
            }
            EditorPick::Choice(field, index) => {
                let Some(view_field @ ViewField::Choice { options, .. }) = declared(&field) else {
                    return;
                };
                let Some(option) = options.get(index) else {
                    return;
                };
                let label = field_label(&field, view_field).to_lowercase();
                let label = format!("Set {label} to {}", option_label(option));
                (label, field, option.clone())
            }
            EditorPick::Toggle(field) => {
                let Some(view_field @ ViewField::Toggle { .. }) = declared(&field) else {
                    return;
                };
                let on = view_field.value_in(&primary.view, &field) != ViewValue::Toggle(true);
                let label = field_label(&field, view_field).to_lowercase();
                let label = format!("Turn {label} {}", if on { "on" } else { "off" });
                (label, field, ViewValue::Toggle(on))
            }
        };
        let change = |clip: &mut Clip| {
            if clip.editor == primary.editor {
                clip.view.insert(field.clone(), value.clone());
            }
        };
        self.change_note_clips(&label, change, cx);
    }

    /// Changes every selected note clip as one undo step, if the change changes one.
    fn change_note_clips(
        &mut self,
        label: &str,
        change: impl Fn(&mut Clip),
        cx: &mut Context<Self>,
    ) {
        let project = self.session.read(cx).project();
        let changed: Vec<_> = self
            .clips
            .iter()
            .filter_map(|id| {
                let clip = project.resolve::<Clip>(id)?;
                let state = project.state(&clip)?;
                let mut next = state.clone();
                change(&mut next);
                (next != *state).then_some((clip, next))
            })
            .collect();
        if changed.is_empty() {
            return;
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                for (clip, next) in changed {
                    changes.set(&clip, next);
                }
                project.commit(label, changes)
            })
        });
    }

    fn fit_tempo_to_take(&mut self, cx: &mut Context<Self>) {
        let Some(clip) = self.take_clip(cx) else {
            return;
        };
        let FitTempo { unavailable, fit } = self.fit_tempo;
        if unavailable(self.session.read(cx).project()).is_some() {
            return;
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| fit(project, &clip))
        });
    }
}
