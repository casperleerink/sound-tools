//! The card of the Drum pad: the 4 by 4 grid of pads as its display, then Volume, Pitch, Decay
//! and Pan of the selected pad, and behind expand its Sound and whether it is in the choke
//! group. The rack gives the view a [`CardFrame`], whose title says "Drum pad" and is where
//! another instrument is picked.
//!
//! A press on a pad selects it and plays it. The grid is one tab stop: the arrows move the
//! selection and enter plays the selected pad. A file from the Finder let go of on a pad makes
//! it a sample pad, and so does `Choose file…` in the Sound list, which opens the file panel of
//! macOS. The file is copied into `assets/audio/` away from the thread that draws.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a knob drag is one gesture and one undo step, a
//! key step, a reset, a sound, a sample or the choke switch is one commit. The ranges, the
//! defaults and the travel of each knob come from the [`PARAMETERS`](crate::PARAMETERS) of the
//! crate. What is only about the interface is here: the labels, the units, the names of the undo
//! steps, which pad is selected, whether the card is expanded and how loud each pad sounds.
//! A number that an automation lane of the track moves shows the value that plays on its knob,
//! and does not drag ([`Lanes`]).

use std::path::PathBuf;

use gpui::{
    Context, Entity, FocusHandle, KeyDownEvent, MouseButton, PathPromptOptions, Task, Window, div,
    prelude::*, px,
};
use sound_core::{Instance, ProjectEvent, State};
use sound_media::{Cached, Imported, MediaError};
use sound_notes::Velocity;
use sound_ui::components::cell::{CELL_WIDTH, Cell};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, pan_readout, short};
use sound_ui::components::pad::{PAD_GAP, PAD_HEIGHT, PAD_WIDTH, Pad as PadElement, PadGlyph};
use sound_ui::components::toggle::Toggle;
use sound_ui::{
    ActiveTheme, ControlEdit, DeviceLabel, Devices, KeyboardFocus, Lanes, Session, Views,
    every_poll, weak_callback,
};

use crate::{
    DECAY, DrumPad, DrumPadState, DrumUpdate, PAD_LANES, PADS, PAN, PARAMETERS, PITCH, PROCESSOR,
    Pad, PadParameter, Sound, Source, VOLUME, note_of, peaks_name,
};

/// The name the rack puts on the card of a Drum pad.
pub const NAME: &str = "Drum pad";

/// Pads across and up the grid.
pub const COLUMNS: usize = 4;

/// The grid: 4 pads of 72 across with 4 pt between them, 300 x 140, as a display of the card.
pub const GRID_WIDTH: f32 = COLUMNS as f32 * PAD_WIDTH + (COLUMNS - 1) as f32 * PAD_GAP;
pub const GRID_HEIGHT: f32 = COLUMNS as f32 * PAD_HEIGHT + (COLUMNS - 1) as f32 * PAD_GAP;

/// How hard a press on a pad plays it.
pub const PRESS_VELOCITY: u8 = 100;

/// The width of the trigger of the Sound select, in its two cells.
const SOUND_SELECT_WIDTH: f32 = 2. * CELL_WIDTH - 8.;

/// How finely the card follows how loud a pad sounds: a card whose pads hold still asks for no
/// frame.
const SOUNDING_STEPS: f32 = 24.;

/// The value of the item of the pad's own file in the Sound list.
const SAMPLE_VALUE: &str = "sample";
/// The value of `Choose file…` in the Sound list.
const CHOOSE_VALUE: &str = "choose";

/// Registers the card of the `drum-pad` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(DrumPadView::new);
    devices.describe::<DrumPadState>(|_| DeviceLabel {
        key: DrumPadState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    Decibels,
    Semitones,
    Milliseconds,
    Pan,
}

/// A knob of the card, on one number of the selected pad.
struct Control {
    number: usize,
    label: &'static str,
    undo_label: &'static str,
    unit: Unit,
}

const VOLUME_KNOB: Control = Control {
    number: VOLUME,
    label: "Volume",
    undo_label: "Change volume",
    unit: Unit::Decibels,
};
const PITCH_KNOB: Control = Control {
    number: PITCH,
    label: "Pitch",
    undo_label: "Change pitch",
    unit: Unit::Semitones,
};
const DECAY_KNOB: Control = Control {
    number: DECAY,
    label: "Decay",
    undo_label: "Change decay",
    unit: Unit::Milliseconds,
};
const PAN_KNOB: Control = Control {
    number: PAN,
    label: "Pan",
    undo_label: "Change pan",
    unit: Unit::Pan,
};

impl Control {
    fn parameter(&self, pad: usize) -> &'static PadParameter {
        &PARAMETERS[pad % PADS][self.number]
    }
}

/// A value with its unit, as a knob shows it: `0 dB`, `-7 st`, `180 ms`, `1.8 s`, `C`, `25L`.
fn readout(unit: Unit, value: f32) -> String {
    match unit {
        Unit::Decibels => format!("{} dB", short(value)),
        Unit::Semitones => format!("{} st", short(value)),
        Unit::Milliseconds if value < 1_000.0 => format!("{} ms", short(value)),
        Unit::Milliseconds => format!("{} s", short(value / 1_000.0)),
        Unit::Pan => pan_readout(value),
    }
}

/// The pad an arrow key moves the selection to, from `pad`: sideways within a row, up and down
/// between rows, and nowhere past an edge. The bottom row is pads 0 to 3.
pub fn pad_from_key(pad: usize, key: &str) -> Option<usize> {
    let (row, column) = (pad / COLUMNS, pad % COLUMNS);
    let (row, column) = match key {
        "left" => (row, column.checked_sub(1)?),
        "right" => (row, column + 1),
        "up" => (row + 1, column),
        "down" => (row.checked_sub(1)?, column),
        _ => return None,
    };
    (row < COLUMNS && column < COLUMNS).then_some(row * COLUMNS + column)
}

pub struct DrumPadView {
    session: Entity<Session>,
    drums: Instance<DrumPadState>,
    frame: CardFrame,
    /// The gesture of a knob drag.
    edit: ControlEdit,
    lanes: Entity<Lanes>,
    /// Whether the card shows Sound and Choke. Interface state: not saved.
    expanded: bool,
    /// The pad the knobs show, from 0. Interface state: not saved.
    selected: usize,
    /// The select of the sound of the selected pad. It shows what the record says, see
    /// [`Self::show_sound`].
    sounds: Entity<DropdownMenu>,
    /// The grid is one tab stop.
    grid_focus: FocusHandle,
    keyboard_focus: KeyboardFocus,
    /// How loud each pad sounds, from 0 to 1, in steps of [`SOUNDING_STEPS`].
    sounding: [f32; PADS],
    _metering: Task<()>,
}

impl DrumPadView {
    pub fn new(
        session: Entity<Session>,
        drums: Instance<DrumPadState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.drums.id() => {
                view.show_sound(cx);
                cx.notify();
            }
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.drums.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            // A file that arrives or goes changes what a sample pad shows.
            ProjectEvent::ProblemsChanged => cx.notify(),
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let sounds = cx.new(|cx| {
            DropdownMenu::new("Sound", Vec::new(), cx)
                .trigger(Trigger::Select)
                .trigger_width(SOUND_SELECT_WIDTH)
                .width(200.)
                .debug_name("sound")
        });
        cx.subscribe(&sounds, |view, _, MenuPicked(value), cx| {
            view.pick_sound(value.as_ref(), cx)
        })
        .detach();
        // What the pads did before this card was made is not what they do now.
        let project = session.read(cx).project();
        for pad in 0..PADS {
            if let Some(peaks) = project.peaks(drums.id(), &peaks_name(pad)) {
                peaks.take();
            }
        }
        let lanes = Lanes::follow(&session, drums.id(), cx);
        let mut view = Self {
            session,
            drums,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
            selected: 0,
            sounds,
            grid_focus: cx.focus_handle().tab_stop(true),
            keyboard_focus: KeyboardFocus::default(),
            sounding: [0.; PADS],
            _metering: every_poll(cx, Self::read_levels),
        };
        view.show_sound(cx);
        view
    }

    fn state<'a>(&self, cx: &'a Context<Self>) -> Option<&'a DrumPadState> {
        self.session.read(cx).project().state(&self.drums)
    }

    /// Takes how loud each pad sounded since the last look, and draws again when that changes
    /// what the pads show. Called once per poll of the session.
    pub fn read_levels(&mut self, cx: &mut Context<Self>) {
        let (project, id) = (self.session.read(cx).project(), self.drums.id());
        let mut sounding = [0.; PADS];
        for (pad, level) in sounding.iter_mut().enumerate() {
            let taken = project
                .peaks(id, &peaks_name(pad))
                .map_or(0., |peaks| peaks.take()[0]);
            *level = (taken.clamp(0., 1.) * SOUNDING_STEPS).round() / SOUNDING_STEPS;
        }
        if sounding != self.sounding {
            self.sounding = sounding;
            cx.notify();
        }
    }

    /// Shows or hides Sound and Choke, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    /// Makes pad `pad`, from 0, the one the knobs show.
    pub fn select(&mut self, pad: usize, cx: &mut Context<Self>) {
        if pad < PADS && pad != self.selected {
            self.selected = pad;
            self.show_sound(cx);
            cx.notify();
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The select of the sound of the selected pad, for a test that opens it.
    pub fn sound_list(&self) -> &Entity<DropdownMenu> {
        &self.sounds
    }

    /// How loud the card shows each pad, from 0 to 1. For tests.
    pub fn sounding(&self) -> [f32; PADS] {
        self.sounding
    }

    /// Plays pad `pad` now, as a note of it would. Not an edit: nothing is saved.
    pub fn play(&mut self, pad: usize, cx: &mut Context<Self>) {
        let id = self.drums.id().clone();
        let velocity = Velocity::nearest(i64::from(PRESS_VELOCITY));
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                project.send::<DrumPad>(&id, PROCESSOR, DrumUpdate::hit(pad, velocity))
            })
        });
    }

    /// A press on a pad: it takes the focus of the grid, and is selected and played.
    fn press(&mut self, pad: usize, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.grid_focus, cx);
        self.keyboard_focus.pressed(cx);
        self.select(pad, cx);
        self.play(pad, cx);
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return false;
        }
        let key = keystroke.key.as_str();
        if key == "enter" {
            self.play(self.selected, cx);
            return true;
        }
        match pad_from_key(self.selected, key) {
            Some(pad) => {
                self.select(pad, cx);
                true
            }
            // An arrow at an edge is used up too, so it does not scroll what holds the card.
            None => matches!(key, "left" | "right" | "up" | "down"),
        }
    }

    /// Puts the sound of the selected pad in its select, after any change: a pick, another
    /// pad, an undo or an outside edit. The list has the sounds of the kit, the pad's own file
    /// when it plays one, and `Choose file…`.
    fn show_sound(&mut self, cx: &mut Context<Self>) {
        let Some(pad) = self
            .state(cx)
            .map(|state| state.pads[self.selected].clone())
        else {
            return;
        };
        let sounds = Sound::ALL.map(|sound| MenuItem::new(sound.key(), sound.name()));
        let mut entries = vec![MenuEntry::Group(MenuGroup::new().items(sounds))];
        let selected = match &pad.source {
            Source::Sound(sound) => sound.key(),
            Source::Sample(asset) => {
                let file = MenuItem::new(SAMPLE_VALUE, asset.to_string());
                entries.push(MenuEntry::Group(MenuGroup::new().item(file)));
                SAMPLE_VALUE
            }
        };
        entries.push(MenuEntry::Separator);
        let choose = MenuItem::new(CHOOSE_VALUE, "Choose file…").selectable(false);
        entries.push(MenuEntry::Group(MenuGroup::new().item(choose)));
        self.sounds.update(cx, |select, cx| {
            select.set_entries(entries, cx);
            select.set_selected(selected, cx);
        });
    }

    fn pick_sound(&mut self, value: &str, cx: &mut Context<Self>) {
        let pad = self.selected;
        if value == CHOOSE_VALUE {
            self.choose_file(pad, cx);
            return;
        }
        let Some(sound) = Sound::ALL.into_iter().find(|sound| sound.key() == value) else {
            return;
        };
        let set =
            move |state: &mut DrumPadState, sound| state.pads[pad].source = Source::Sound(sound);
        self.change("Change sound", ValueChange::Set(sound), set, cx);
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut DrumPadState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, drums) = (&self.session, &self.drums);
        self.edit.apply(session, drums, label, change, set, cx);
    }

    /// Opens the file panel of macOS for a sample for pad `pad`: the keyboard path to a sample
    /// pad.
    pub fn choose_file(&mut self, pad: usize, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |view, cx| {
            let paths = match chosen.await {
                Ok(Ok(Some(paths))) => paths,
                // Cancelled, or the panel went away: nothing to load.
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    view.update(cx, |view, cx| {
                        let session = view.session.clone();
                        session.update(cx, |session, cx| session.report(error, cx));
                    })
                    .ok();
                    return;
                }
            };
            view.update(cx, |view, cx| view.load_files(pad, paths, cx))
                .ok();
        })
        .detach();
    }

    /// Copies the first of `paths` into `assets/audio/` away from the thread that draws, then
    /// makes pad `pad` play it, as one undo step. A file that does not play is the notice of
    /// the window, and the pad stays as it was. The handler of a drop of files on a pad.
    pub fn load_files(&mut self, pad: usize, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(path) = paths.into_iter().next() else {
            return;
        };
        let assets = self.session.read(cx).project().assets().clone();
        let importing = cx.background_spawn(async move { sound_media::import(&assets, &path) });
        cx.spawn(async move |view, cx| {
            let imported = importing.await;
            view.update(cx, |view, cx| view.take_sample(pad, imported, cx))
                .ok();
        })
        .detach();
    }

    fn take_sample(
        &mut self,
        pad: usize,
        imported: Result<Imported, MediaError>,
        cx: &mut Context<Self>,
    ) {
        let imported = match imported {
            Ok(imported) => imported,
            Err(error) => {
                let session = self.session.clone();
                session.update(cx, |session, cx| session.report(error, cx));
                return;
            }
        };
        let (asset, seconds) = (imported.asset.clone(), imported.audio.seconds());
        let set = move |state: &mut DrumPadState, (): ()| {
            state.pads[pad].load_sample(asset, seconds);
        };
        // The file is held until the edit is made, so the behaviour reads nothing.
        self.change("Load sample", ValueChange::Set(()), set, cx);
        drop(imported);
        self.select(pad, cx);
    }

    fn knob(&self, control: &'static Control, pad: &Pad, cx: &mut Context<Self>) -> Knob {
        let selected = self.selected;
        let parameter = control.parameter(selected);
        let value = (parameter.get)(pad);
        // The lane of this number of the selected pad is named by its path: `pads.36.pan`.
        let mut paths = PAD_LANES[selected].iter().map(|lane| lane.field);
        let path = paths.find(|path| path.rsplit('.').next() == Some(parameter.field));
        let automated = path.is_some_and(|path| self.lanes.read(cx).is_automated(path));
        Knob::new(parameter.field)
            .range(KnobRange::of(parameter))
            .value(value)
            .automated(automated)
            .default_value(parameter.default)
            // Pitch and pan go both ways from the middle, so their arcs start at the top.
            .bipolar(matches!(control.unit, Unit::Semitones | Unit::Pan))
            .label(control.label)
            .readout(readout(control.unit, value))
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = move |state: &mut DrumPadState, value| {
                    (parameter.set)(&mut state.pads[selected], value);
                };
                view.change(control.undo_label, change, set, cx);
            }))
    }

    /// What the glyph of a pad says: nothing for a synthesized sound, the waveform for a
    /// sample, and a warning for a sample whose file is not there. It asks what is known of the
    /// file in memory only, so the thread that draws never looks at the disk.
    fn glyph(&self, pad: &Pad, cx: &Context<Self>) -> Option<PadGlyph> {
        let Source::Sample(asset) = &pad.source else {
            return None;
        };
        let assets = self.session.read(cx).project().assets();
        match sound_media::cached(assets, asset) {
            Cached::Missing | Cached::DoesNotPlay(_) => Some(PadGlyph::Missing),
            Cached::Plays(_) | Cached::Unknown => Some(PadGlyph::Sample),
        }
    }

    fn grid(
        &self,
        state: &DrumPadState,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ring = cx.theme().lavender;
        let ring_shows = self.keyboard_focus.shows_ring(&self.grid_focus, window);
        let rows = (0..COLUMNS).rev().map(|row| {
            let pads = (0..COLUMNS).map(|column| {
                let index = row * COLUMNS + column;
                let pad = &state.pads[index];
                let view = cx.weak_entity();
                let press = move |window: &mut Window, cx: &mut gpui::App| {
                    // Released: there is nothing left to press.
                    view.update(cx, |view, cx| view.press(index, window, cx))
                        .ok();
                };
                let drop_files = weak_callback(cx, move |view: &mut Self, paths, cx| {
                    view.load_files(index, paths, cx)
                });
                // For tests: `pad-36` to `pad-51`.
                PadElement::new(usize::from(note_of(index)), pad.name(index))
                    .selected(index == self.selected)
                    .sounding(self.sounding[index])
                    .glyph(self.glyph(pad, cx))
                    .on_press(press)
                    .on_drop_files(drop_files)
            });
            div()
                .flex()
                .gap(px(PAD_GAP))
                .children(pads.collect::<Vec<_>>())
        });
        div()
            .id("pads")
            .relative()
            .w(px(GRID_WIDTH))
            .h(px(GRID_HEIGHT))
            .flex()
            .flex_col()
            .gap(px(PAD_GAP))
            .track_focus(&self.grid_focus)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                if view.on_key(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .children(rows.collect::<Vec<_>>())
            .when(ring_shows, |grid| {
                grid.child(
                    div()
                        .absolute()
                        .top(px(-3.))
                        .left(px(-3.))
                        .w(px(GRID_WIDTH + 6.))
                        .h(px(GRID_HEIGHT + 6.))
                        .rounded(px(9.))
                        .border_1()
                        .border_color(ring),
                )
            })
    }
}

impl gpui::Render for DrumPadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(mut state) = self.state(cx).cloned() else {
            return div().into_any_element();
        };
        // What plays: the knobs show the lanes.
        let parameters = DrumPad::AUTOMATION.parameters();
        self.lanes.read(cx).apply(parameters, &mut state);
        let pad = &state.pads[self.selected];
        let columns = [
            Column::new()
                .top(self.knob(&VOLUME_KNOB, pad, cx))
                .bottom(self.knob(&DECAY_KNOB, pad, cx)),
            Column::new()
                .top(self.knob(&PITCH_KNOB, pad, cx))
                .bottom(self.knob(&PAN_KNOB, pad, cx)),
        ];
        let kind = match pad.source {
            Source::Sound(_) => "Synthesized",
            Source::Sample(_) => "Sample",
        };
        let sound = Cell::new(self.sounds.clone())
            .span(2)
            .label("Sound")
            .value(kind);
        let selected = self.selected;
        let on = pad.choke;
        let choke = Toggle::new("choke", if on { "On" } else { "Off" }, on).on_change(
            weak_callback(cx, move |view: &mut Self, on: bool, cx| {
                let label = if on {
                    "Add to choke group"
                } else {
                    "Take out of choke group"
                };
                let set = move |state: &mut DrumPadState, on| state.pads[selected].choke = on;
                view.change(label, ValueChange::Set(on), set, cx);
            }),
        );
        let hidden = [
            Column::new()
                .top(sound)
                .bottom(Cell::new(choke).label("Choke")),
            Column::new(),
        ];
        let expand = cx.listener(|view, _, _, cx| view.set_expanded(!view.expanded, cx));
        let card = self
            .frame
            .card()
            .expand(self.expanded, expand)
            .display(self.grid(&state, window, cx));
        let card = columns
            .into_iter()
            .fold(card, |card, column| card.column(column));
        hidden
            .into_iter()
            .fold(card, |card, column| card.hidden_column(column))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_is_the_size_of_a_display_and_its_line() {
        assert_eq!((GRID_WIDTH, GRID_HEIGHT), (300., 140.));
    }

    #[test]
    fn the_arrows_move_the_selection_and_stop_at_the_edges() {
        // The bottom left is pad 0, the top right pad 15.
        assert_eq!(pad_from_key(0, "right"), Some(1));
        assert_eq!(pad_from_key(0, "up"), Some(4));
        assert_eq!(pad_from_key(0, "left"), None);
        assert_eq!(pad_from_key(0, "down"), None);
        assert_eq!(pad_from_key(3, "right"), None);
        assert_eq!(pad_from_key(15, "up"), None);
        assert_eq!(pad_from_key(15, "down"), Some(11));
        assert_eq!(pad_from_key(6, "left"), Some(5));
        assert_eq!(pad_from_key(6, "enter"), None);
    }

    #[test]
    fn a_readout_has_its_unit_and_three_digits_at_most() {
        assert_eq!(readout(Unit::Decibels, 0.0), "0 dB");
        assert_eq!(readout(Unit::Semitones, -7.0), "-7 st");
        assert_eq!(readout(Unit::Milliseconds, 180.0), "180 ms");
        assert_eq!(readout(Unit::Milliseconds, 1_800.0), "1.8 s");
        assert_eq!(readout(Unit::Pan, 0.0), "C");
        assert_eq!(readout(Unit::Pan, -0.25), "25L");
    }

    /// The defaults and both ends of every range, through the travel of its knob.
    #[test]
    fn every_knob_gives_the_ends_of_its_range() {
        for pad in 0..PADS {
            for control in [&VOLUME_KNOB, &PITCH_KNOB, &DECAY_KNOB, &PAN_KNOB] {
                let parameter = control.parameter(pad);
                let range = KnobRange::of(parameter);
                assert_eq!(range.value(0.0), parameter.min);
                assert_eq!(range.value(1.0), parameter.max);
                let default = range.value(range.position(parameter.default));
                assert!((default - parameter.default).abs() < 1e-3, "{default}");
            }
        }
    }
}
