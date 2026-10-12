//! The menu of the selected clips. A right click on a clip opens it, as do ctrl-click on macOS,
//! shift-F10 and the menu key. It opens at the press on macOS and Linux and at the release on
//! Windows, as the menus of each system do. A clip that is not selected is selected alone
//! first; one that is keeps the others, as in the Finder.
//!
//! It offers what the keys do to the selected clips, and fitting the tempo to the take of a
//! recorded clip.

use gpui::{
    App, Context, Entity, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Window, point,
    px,
};
use sound_core::{Project, ProjectError};
use sound_notes::Clip;
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuGroup, MenuItem};

use super::Timeline;
use super::scene::Scene;
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

/// Whether a press is the start of opening the menu.
pub(super) fn opens_menu(event: &MouseDownEvent) -> bool {
    match event.button {
        MouseButton::Right => true,
        MouseButton::Left => cfg!(target_os = "macos") && event.modifiers.control,
        _ => false,
    }
}

/// `fit` is `None` when the fit is not offered, else why it cannot run, if it cannot.
fn entries(fit: Option<Option<&'static str>>) -> Vec<MenuEntry> {
    let command =
        |value: &'static str, label: &'static str| MenuItem::new(value, label).selectable(false);
    let delete_key = if cfg!(target_os = "macos") {
        "backspace"
    } else {
        "delete"
    };
    let mut entries = Vec::new();
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
        MenuGroup::new().item(command(DELETE, "Delete").shortcut(delete_key)),
    ));
    entries
}

impl Timeline {
    /// A press that opens the menu, at `(x, y)` on the timeline and `position` in the window.
    /// Off a clip, nothing. On Windows the menu waits for the release.
    pub(super) fn press_for_clip_menu(
        &mut self,
        (x, y): (f32, f32),
        position: Point<Pixels>,
        scene: &Scene,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clip_menu_held = false;
        if self.dragging() || x < 0.0 || y < 0.0 {
            return;
        }
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
            true => self.clip_menu_held = true,
            false => self.show_clip_menu(position, window, cx),
        }
    }

    pub(super) fn release_for_clip_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if std::mem::take(&mut self.clip_menu_held) {
            self.show_clip_menu(position, window, cx);
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
        self.clip_menu.update(cx, |menu, cx| {
            menu.set_entries(entries(fit), cx);
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

    pub(super) fn on_clip_menu(&mut self, picked: &SharedString, cx: &mut Context<Self>) {
        match picked.as_ref() {
            FIT_TEMPO => self.fit_tempo_to_take(cx),
            CUT => self.cut(cx),
            COPY => {
                self.copy(cx);
            }
            DUPLICATE => self.duplicate(cx),
            DELETE => self.delete_clips("Delete clip", "Delete clips", None, cx),
            _ => {}
        }
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
