//! What changes at a point of the piece, edited in the ruler: tempo changes, time signature
//! changes and sections. A press on a mark selects it and moves the playhead onto it, so the
//! transport edits its tempo or time signature; backspace removes it. A right click opens a
//! menu that adds one, and shows where it lands while a row is pointed at. A double click on
//! the ruler adds a section, and one on the name of a section renames it.

use std::num::NonZeroU64;
use std::ops::Range;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, FontWeight, Pixels, Point, SharedString,
    Window, fill, point, px, quad, size,
};
use sound_core::{Bar, Changes, ProjectFile, SectionName, Ticks, TimeSignatures};
use sound_ui::ActiveTheme;
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};

use super::scene::Scene;
use super::state::RenameTarget;
use super::{HeldMenu, Timeline};
use crate::view::layout::{HEADER_WIDTH, RULER_HEIGHT, RulerBar};
use crate::view::paint::{Fit, paint_text, text_width};
use crate::view::snap::{Grid, Snap};

/// A mark in the ruler, by where it starts. Selecting one selects no clip.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RulerMark {
    Tempo(Ticks),
    /// By the number of its bar.
    TimeSignature(u64),
    Section(NonZeroU64),
}

impl RulerMark {
    fn tick(self, time_signatures: &TimeSignatures) -> Option<Ticks> {
        match self {
            RulerMark::Tempo(tick) => Some(tick),
            RulerMark::TimeSignature(bar) => Some(time_signatures.bar(bar)?.start),
            RulerMark::Section(bar) => Some(time_signatures.bar(bar.get())?.start),
        }
    }

    /// Whether the project has it. The tempo change at tick 0 and the time signature of bar 1
    /// are not marks: they show in the transport, and every piece keeps them.
    fn is_in(self, file: &ProjectFile) -> bool {
        let tempo_map = &file.tempo_map;
        match self {
            RulerMark::Tempo(tick) => tick > Ticks(0) && tempo_map.change_at(tick).tick == tick,
            RulerMark::TimeSignature(bar) => {
                let mut changes = tempo_map.time_signatures().changes().skip(1);
                changes.any(|change| change.number == bar)
            }
            RulerMark::Section(bar) => file
                .sections
                .as_slice()
                .iter()
                .any(|section| section.bar == bar),
        }
    }
}

/// Where each thing the menu adds lands, for a press at a tick: a tempo change on the grid, as
/// clips are placed, and a time signature change or a section on the nearest bar line, the only
/// place they can start.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) struct Landing {
    pub(super) tempo: Ticks,
    pub(super) bar: Bar,
}

impl Landing {
    pub(super) fn at(tick: Ticks, grid: &Grid, time_signatures: &TimeSignatures) -> Self {
        let bar_line = Snap::Bar.grid(time_signatures).snap(tick);
        Self {
            tempo: grid.snap(tick),
            bar: time_signatures.bar_at(bar_line),
        }
    }

    fn section_bar(self) -> Option<NonZeroU64> {
        NonZeroU64::new(self.bar.number)
    }
}

/// What the menu of the ruler opened for: where its rows land, and the mark it opened on.
#[derive(Copy, Clone, Debug)]
pub(super) struct RulerMenuAt {
    pub(super) landing: Landing,
    pub(super) mark: Option<RulerMark>,
}

const ADD_TEMPO: &str = "add-tempo";
const ADD_TIME_SIGNATURE: &str = "add-time-signature";
const ADD_SECTION: &str = "add-section";
const REMOVE: &str = "remove";

const ADD_TEMPO_LABEL: &str = "Add tempo change";
const ADD_TIME_SIGNATURE_LABEL: &str = "Add time signature change";
const ADD_SECTION_LABEL: &str = "Add section";
pub(super) const RENAME_SECTION_LABEL: &str = "Rename section";

/// Icons and a word each. A row whose place has that mark already is disabled.
fn entries(file: &ProjectFile, at: RulerMenuAt) -> Vec<MenuEntry> {
    let Landing { tempo, bar } = at.landing;
    let row = |value: &'static str, icon: &'static str, label: &'static str, taken: bool| {
        let item = MenuItem::new(value, label).icon(icon).selectable(false);
        item.disabled(taken)
    };
    let section_taken = at
        .landing
        .section_bar()
        .is_none_or(|bar| RulerMark::Section(bar).is_in(file));
    let adds = [
        row(
            ADD_TEMPO,
            "tempo-change",
            "Tempo",
            file.tempo_map.change_at(tempo).tick == tempo,
        ),
        row(
            ADD_TIME_SIGNATURE,
            "time-signature",
            "Time signature",
            file.tempo_map.time_signatures().change_at(bar.start).number == bar.number,
        ),
        row(ADD_SECTION, "section", "Section", section_taken),
    ];
    let mut entries = vec![MenuEntry::Group(MenuGroup::new().items(adds))];
    if at.mark.is_some() {
        let delete_key = if cfg!(target_os = "macos") {
            "backspace"
        } else {
            "delete"
        };
        let remove = row(REMOVE, "trash-2", "Remove", false).shortcut(delete_key);
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Group(MenuGroup::new().item(remove)));
    }
    entries
}

impl Timeline {
    /// A press in the ruler: on a mark it selects it and moves the playhead onto it, and a
    /// double click on the name of a section renames it. Anywhere else it moves the playhead
    /// to the grid, and a double click adds a section on the nearest bar line.
    pub(super) fn on_ruler(
        &mut self,
        x: f32,
        double: bool,
        scene: &Scene,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(mark) = scene.mark_at(x) {
            self.select_mark_and_seek(mark, cx);
            if let (true, RulerMark::Section(bar)) = (double, mark) {
                self.start_section_rename(bar, window, cx);
            }
            return;
        }
        let tick = scene.viewport.tick_at(x);
        if double {
            let landing = Landing::at(tick, &self.grid(cx), self.time_signatures(cx));
            self.add_section(landing, window, cx);
            return;
        }
        self.select_mark(None, cx);
        self.seek(self.grid(cx).snap(tick), cx);
    }

    pub(super) fn seek(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, _| session.engine().seek(tick));
    }

    /// The selected mark of the ruler.
    pub fn selected_mark(&self) -> Option<RulerMark> {
        self.selected_mark
    }

    /// Selects a mark of the ruler and no clip, so delete removes it.
    pub(super) fn select_mark(&mut self, mark: Option<RulerMark>, cx: &mut Context<Self>) {
        if mark.is_some() {
            self.select_clip(None, cx);
            self.selected_point = None;
        }
        if self.selected_mark != mark {
            self.selected_mark = mark;
            cx.notify();
        }
    }

    /// Selects a mark and moves the playhead onto it, where the transport shows its values.
    fn select_mark_and_seek(&mut self, mark: RulerMark, cx: &mut Context<Self>) {
        self.select_mark(Some(mark), cx);
        if let Some(tick) = mark.tick(self.time_signatures(cx)) {
            self.seek(tick, cx);
        }
    }

    /// A mark removed from outside is not selected any more.
    pub(super) fn forget_lost_mark(&mut self, cx: &App) {
        let file = self.session.read(cx).project().project_file();
        if self.selected_mark.is_some_and(|mark| !mark.is_in(file)) {
            self.selected_mark = None;
        }
    }

    /// One undo step on `project.json`, from what it holds now. Whether there was one.
    fn edit_project_file(
        &mut self,
        label: &str,
        change: impl FnOnce(&ProjectFile) -> Option<Changes>,
        cx: &mut Context<Self>,
    ) -> bool {
        let done = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let Some(changes) = change(project.project_file()) else {
                    return Ok(false);
                };
                project.commit(label, changes)?;
                Ok(true)
            })
        });
        done.unwrap_or(false)
    }

    /// Adds a tempo change at `tick` with the tempo that plays there, and selects it. A change
    /// that is there already is selected.
    pub(super) fn add_tempo_change(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.edit_project_file(
            ADD_TEMPO_LABEL,
            |file| {
                let mut changes = Changes::new();
                changes.set_tempo_map(file.tempo_map.with_change_at(tick)?);
                Some(changes)
            },
            cx,
        );
        let mark = RulerMark::Tempo(tick);
        if mark.is_in(self.session.read(cx).project().project_file()) {
            self.select_mark_and_seek(mark, cx);
        }
    }

    /// Adds a change to the time signature that plays at `bar`, so nothing moves until the
    /// transport changes it, and selects it.
    fn add_time_signature_change(&mut self, bar: Bar, cx: &mut Context<Self>) {
        let added = self.edit_project_file(
            ADD_TIME_SIGNATURE_LABEL,
            |file| {
                let time_signatures = file.tempo_map.time_signatures();
                let time_signatures = time_signatures.with_change_at(bar.number)?;
                let mut changes = Changes::new();
                changes.set_tempo_map(file.tempo_map.with_time_signatures(time_signatures));
                Some(changes)
            },
            cx,
        );
        if added {
            self.select_mark_and_seek(RulerMark::TimeSignature(bar.number), cx);
        }
    }

    /// Adds a section at the bar of `landing`, named after how many there are, selects it and
    /// opens its name.
    fn add_section(&mut self, landing: Landing, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = landing.section_bar() else {
            return;
        };
        let added = self.edit_project_file(
            ADD_SECTION_LABEL,
            |file| {
                let count = file.sections.as_slice().len() + 1;
                let name = SectionName::new(format!("Section {count}")).ok()?;
                let mut changes = Changes::new();
                changes.set_sections(file.sections.with_section_at(bar, name)?);
                Some(changes)
            },
            cx,
        );
        if added {
            self.select_mark_and_seek(RulerMark::Section(bar), cx);
            self.start_section_rename(bar, window, cx);
        }
    }

    /// Removes a mark of the ruler as one undo step.
    pub(super) fn remove_mark(&mut self, mark: RulerMark, cx: &mut Context<Self>) {
        let label = match mark {
            RulerMark::Tempo(_) => "Remove tempo change",
            RulerMark::TimeSignature(_) => "Remove time signature change",
            RulerMark::Section(_) => "Remove section",
        };
        self.edit_project_file(
            label,
            |file| {
                let mut changes = Changes::new();
                let tempo_map = &file.tempo_map;
                match mark {
                    RulerMark::Tempo(tick) => {
                        changes.set_tempo_map(tempo_map.without_change_at(tick)?);
                    }
                    RulerMark::TimeSignature(bar) => {
                        let time_signatures = tempo_map.time_signatures().without_change_at(bar)?;
                        changes.set_tempo_map(tempo_map.with_time_signatures(time_signatures));
                    }
                    RulerMark::Section(bar) => {
                        changes.set_sections(file.sections.without_section_at(bar)?);
                    }
                }
                Some(changes)
            },
            cx,
        );
        self.select_mark(None, cx);
    }

    /// Renames a section, one undo step. A name of only spaces is no name.
    pub(super) fn rename_section(&mut self, bar: NonZeroU64, name: &str, cx: &mut Context<Self>) {
        let Ok(name) = SectionName::new(name.trim()) else {
            return;
        };
        self.edit_project_file(
            RENAME_SECTION_LABEL,
            |file| {
                let same = file
                    .sections
                    .as_slice()
                    .iter()
                    .any(|section| section.bar == bar && section.name == name);
                let mut changes = Changes::new();
                changes.set_sections(file.sections.with_name_at(bar, name).filter(|_| !same)?);
                Some(changes)
            },
            cx,
        );
    }

    /// Opens the name field of the section at `bar`, with its name selected.
    pub(super) fn start_section_rename(
        &mut self,
        bar: NonZeroU64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let file = self.session.read(cx).project().project_file();
        let section = file
            .sections
            .as_slice()
            .iter()
            .find(|section| section.bar == bar);
        let Some(name) = section.map(|section| section.name.as_str().to_string()) else {
            return;
        };
        self.open_rename(RenameTarget::Section(bar), name, window, cx);
    }

    /// A right press in the ruler at `x`, at `position` in the window. On a mark it selects it
    /// first. On Windows the menu waits for the release.
    pub(super) fn press_for_ruler_menu(
        &mut self,
        x: f32,
        position: Point<Pixels>,
        scene: &Scene,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mark = scene.mark_at(x);
        if let Some(mark) = mark {
            self.select_mark_and_seek(mark, cx);
        }
        let tick = scene.viewport.tick_at(x);
        let landing = Landing::at(tick, &self.grid(cx), self.time_signatures(cx));
        self.ruler_menu_at = Some(RulerMenuAt { landing, mark });
        match cfg!(windows) {
            true => self.menu_held = Some(HeldMenu::Ruler),
            false => self.show_ruler_menu(position, window, cx),
        }
    }

    /// The menu at the playhead, for the menu key while no clip is selected. On the selected
    /// mark when there is one.
    pub(super) fn open_ruler_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let playhead = self.playhead.read(cx).tick;
        let landing = Landing::at(playhead, &self.grid(cx), self.time_signatures(cx));
        self.ruler_menu_at = Some(RulerMenuAt {
            landing,
            mark: self.selected_mark,
        });
        let (width, _) = self.painted_size.get();
        let x = self.painted.get().x_of(playhead).clamp(0., width.max(0.));
        let origin = self.painted_bounds.get().origin;
        let position = origin + point(px(HEADER_WIDTH + x), px(RULER_HEIGHT));
        self.show_ruler_menu(position, window, cx);
    }

    pub(super) fn show_ruler_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(at) = self.ruler_menu_at else {
            return;
        };
        let entries = entries(self.session.read(cx).project().project_file(), at);
        self.ruler_menu.update(cx, |menu, cx| {
            menu.set_entries(entries, cx);
            menu.open_at(position, window, cx);
        });
    }

    pub(super) fn on_ruler_menu(
        &mut self,
        picked: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(RulerMenuAt { landing, mark }) = self.ruler_menu_at else {
            return;
        };
        match picked.as_ref() {
            ADD_TEMPO => self.add_tempo_change(landing.tempo, cx),
            ADD_TIME_SIGNATURE => self.add_time_signature_change(landing.bar, cx),
            ADD_SECTION => self.add_section(landing, window, cx),
            REMOVE => {
                if let Some(mark) = mark {
                    self.remove_mark(mark, cx);
                }
            }
            _ => {}
        }
    }

    /// Where the row of the open menu under the pointer or the keys would add its mark.
    pub(super) fn ghost_tick(&self, cx: &App) -> Option<Ticks> {
        let at = self.ruler_menu_at?;
        let pointed = self.ruler_menu.read(cx).pointed()?;
        match pointed.value.as_ref() {
            ADD_TEMPO => Some(at.landing.tempo),
            ADD_TIME_SIGNATURE | ADD_SECTION => Some(at.landing.bar.start),
            _ => None,
        }
    }
}

/// A section as one paint shows it: from its bar to the next one, or past the right edge.
pub(super) struct SectionBand {
    pub(super) bar: NonZeroU64,
    pub(super) x: Range<f32>,
    pub(super) name: SharedString,
    pub(super) selected: bool,
    /// The name is being edited: the field shows it, not the paint.
    pub(super) renaming: bool,
}

/// What the ruler paints besides the bars, and where a press on each mark lands.
pub(super) type MarkZones = Vec<(RulerMark, Range<f32>)>;

/// Where a label of a mark at `x` starts: after the number of a bar there, so it stays
/// readable, and after a label of `taken` there.
pub(super) fn label_left(x: f32, bars: &[RulerBar], taken: &MarkZones, window: &Window) -> f32 {
    let bar = bars.iter().find(|bar| (bar.x.round() - x).abs() < 1.);
    let after_bar = match bar {
        Some(bar) => x + 8. + text_width(&bar.label(), 12., FontWeight::NORMAL, window) + 4.,
        None => x + 4.,
    };
    let after_other = taken.iter().filter(|(_, zone)| zone.contains(&after_bar));
    after_other
        .map(|(_, zone)| zone.end + 4.)
        .fold(after_bar, f32::max)
}

/// A faint band behind the bar numbers for each section.
pub(super) fn paint_section_bands(
    bands: &[SectionBand],
    ruler: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let band_fill = cx.theme().alpha_at(0.05);
    window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
        for band in bands {
            let (start, end) = (band.x.start.round(), band.x.end.round());
            let band_bounds = Bounds::new(
                ruler.origin + point(px(start + 1.), px(2.)),
                size(px((end - start - 2.).max(1.)), px(RULER_HEIGHT - 4.)),
            );
            window.paint_quad(fill(band_bounds, band_fill).corner_radii(px(4.)));
        }
    });
}

/// The name of each section after the number of its bar, in a box of the colour of its band
/// that covers the numbers under it, cut at the end of the band. The selected one has a light
/// border, as a selected tempo change. Gives where each name is, for a press.
pub(super) fn paint_section_names(
    bands: &[SectionBand],
    bars: &[RulerBar],
    ruler: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> MarkZones {
    let theme = cx.theme();
    let (label_fill, selected_fill, selected_border, text) = (
        theme.gray_100.blend(theme.alpha_at(0.05)),
        theme.alpha_at(0.10),
        theme.gray_950,
        theme.gray_950,
    );
    let solid = BorderStyle::Solid;
    let mut zones = MarkZones::new();
    for band in bands.iter().filter(|band| !band.renaming) {
        let (start, end) = (band.x.start.round(), band.x.end.round());
        let left = label_left(start, bars, &MarkZones::new(), window);
        let room = end - left - 4.;
        if room < 24. {
            continue;
        }
        let width = text_width(&band.name, 12., FontWeight::MEDIUM, window).min(room - 12.);
        let label = Bounds::new(
            ruler.origin + point(px(left), px(6.)),
            size(px(width + 12.), px(20.)),
        );
        window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
            let (fill, border) = match band.selected {
                true => (label_fill.blend(selected_fill), selected_border),
                false => (label_fill, gpui::transparent_black()),
            };
            window.paint_quad(quad(label, px(6.), fill, px(1.), border, solid));
            let origin = ruler.origin + point(px(left + 6.), px(8.));
            let weight = FontWeight::MEDIUM;
            // The measured width may round below what the shaper needs for the whole name.
            let fit = Fit::Truncate(room - 12.);
            paint_text(
                band.name.clone(),
                origin,
                12.,
                weight,
                text,
                fit,
                window,
                cx,
            );
        });
        zones.push((
            RulerMark::Section(band.bar),
            start - 4.0..left + width + 12.,
        ));
    }
    zones
}

/// A light border around the time signature of a bar label whose change is selected, painted
/// before the labels. Gives where each time signature that starts there is, for a press.
pub(super) fn paint_time_signature_marks(
    marks: &[(u64, bool)],
    bars: &[RulerBar],
    ruler: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> MarkZones {
    let theme = cx.theme();
    let (selected_fill, selected_border) = (theme.alpha_at(0.10), theme.gray_950);
    let mut zones = MarkZones::new();
    for &(number, selected) in marks {
        let Some(bar) = bars
            .iter()
            .find(|bar| bar.number == number && bar.signature.is_some())
        else {
            continue;
        };
        let x = bar.x.round() + 8.;
        let before = text_width(&format!("{number}  "), 12., FontWeight::NORMAL, window);
        let whole = text_width(&bar.label(), 12., FontWeight::NORMAL, window);
        let (left, right) = (x + before - 6., x + whole + 6.);
        if selected {
            let label = Bounds::new(
                ruler.origin + point(px(left), px(6.)),
                size(px(right - left), px(20.)),
            );
            window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
                let solid = BorderStyle::Solid;
                let border = selected_border;
                window.paint_quad(quad(label, px(6.), selected_fill, px(1.), border, solid));
            });
        }
        zones.push((RulerMark::TimeSignature(number), left..right));
    }
    zones
}

/// Where the row of the ruler menu under the pointer or the keys would add its mark: a line in
/// the colour of where things land.
pub(super) fn paint_ghost(x: f32, ruler: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let color = cx.theme().lavender.opacity(0.7);
    let line = Bounds::new(
        ruler.origin + point(px(x.round() - 1.), px(0.)),
        size(px(2.), px(RULER_HEIGHT)),
    );
    window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
        window.paint_quad(fill(line, color));
    });
}

#[cfg(test)]
mod tests {
    use sound_core::Ticks;

    use super::Landing;
    use crate::view::snap::{Snap, time_signatures};

    /// A tempo change lands on the grid, as a clip does; a time signature change and a section
    /// on the nearest bar line, the only place they can start, in the bars of each signature.
    #[test]
    fn the_menu_adds_a_tempo_change_on_the_grid_and_the_rest_on_the_nearest_bar() {
        let changing = time_signatures(&[("4/4", 2), ("3/4", 1)]);
        let grid = Snap::Sixteenth.grid(&changing);
        let at = |tick: u64| {
            let landing = Landing::at(Ticks(tick), &grid, &changing);
            (landing.tempo.0, landing.bar.number)
        };
        assert_eq!(at(0), (0, 1));
        // Just past a sixteenth, in the first half of bar 1.
        assert_eq!(at(250), (240, 1));
        // Past the middle of bar 2: bar 3, the bar of 3/4.
        assert_eq!(at(3840 + 1921), (3840 + 1920, 3));
        // In the 3/4 bar, past its middle: bar 4 starts 2880 ticks after bar 3.
        assert_eq!(at(7680 + 1500), (7680 + 1440, 4));
        assert_eq!(at(7680 + 1400), (7680 + 1440, 3));
    }
}
