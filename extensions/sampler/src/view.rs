//! The card of the sampler in a rack: the whole file in the waveform display, with the start and
//! end lines, the envelope drawn over it in the time of the file and a green line where the last
//! note is; the Instrument select, Root, Velocity, Release and Gain next to it, and Start, End,
//! Attack, Decay and Sustain behind expand. The rack gives the frame of the card, whose title
//! says "Sampler" and is where another instrument is picked.
//!
//! The Instrument select lists the library by category, with the size of each download, and
//! `Audio file…`. Picking a library instrument is one undo step and starts its download, as
//! `Download` on the display does for one an agent named. The display shows how far it is, or
//! that it failed, with `Try again`.
//!
//! With an SFZ or library instrument the display names it and the card has only Gain: the
//! instrument has its own pitch, envelope and velocity. A file dropped or chosen replaces it.
//!
//! With no file, the display says `Drop an audio file here` over a `Choose file` button, which
//! opens the file panel of macOS and is the way from the keys. A file dropped on the display, or
//! chosen, is copied into `assets/audio/` on a background thread and becomes the sample, as one
//! undo step.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session by [`ControlEdit`], as in the synth: a knob or a handle drag is one
//! gesture and one undo step, a key step or a reset one commit. A handle edits the field of its
//! knob, under the name of its knob in the history. A number that an automation lane of the
//! track moves shows the value that plays, on its knob and on the display, and does not drag
//! ([`Lanes`]).

use std::path::PathBuf;

use gpui::{AnyElement, Context, Entity, FocusHandle, Point, Task, Window, div, point, prelude::*};
use sound_core::{Changes, Instance, ProjectEvent};
use sound_media::{Cached, Imported, Info};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::cell::{CELL_WIDTH, Cell};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Handle};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    Knob, KnobRange, ParameterKnob, decibels_readout, percent_readout, seconds_readout,
};
use sound_ui::components::waveform_display::{
    FileDrop, NoFile, WaveformDisplay, clamped_end, clamped_start, place,
};
use sound_ui::import;
use sound_ui::{
    ControlEdit, Devices, Lanes, OfferGroup, Session, Views, Waveforms, every_poll, weak_callback,
};

use crate::instrument;
use crate::library::{self, CATALOG, Category, Entry, LibraryId, Status, size_text};
use crate::{
    ATTACK, DECAY, GAIN, POSITION, RELEASE, ROOT, SUSTAIN, Sampler, SamplerState, SfzPath, VELOCITY,
};

/// The name the rack puts on the card of a sampler.
pub const NAME: &str = "Sampler";
/// The display, so the card is 32 + 312 + 8 + 3 x 56 = 520 pt with a sample, and 705 expanded.
pub const DISPLAY_WIDTH: f32 = 312.;
/// What the display says with no file, and while one is dragged over it.
pub const EMPTY: &str = "Drop an audio file here";
pub const DROP_TO_LOAD: &str = "Drop to load the file";
pub const DROP_TO_REPLACE: &str = "Drop to replace the file";
/// The undo step of a file dropped or chosen.
pub const LOAD_LABEL: &str = "Load sample";
/// The undo step of a library instrument picked in the Instrument select.
pub const INSTRUMENT_LABEL: &str = "Load instrument";

/// The values of the Instrument select: a file, the sample, the SFZ file of the project, and
/// each library instrument by its id after the prefix.
const FILE_VALUE: &str = "file";
const SAMPLE_VALUE: &str = "sample";
const SFZ_VALUE: &str = "sfz";
const LIBRARY_PREFIX: &str = "library:";
const INSTRUMENT_SELECT_WIDTH: f32 = 2. * CELL_WIDTH - 8.;

/// Registers the card of the `sampler` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(SamplerView::new);
    devices.built_in::<SamplerState>(
        NAME,
        OfferGroup::BuiltIn,
        "device-sampler",
        crate::EXTENSION,
        "This project does not load the sampler.",
    );
}

/// A knob of the view on a number with a fixed range.
type Control = ParameterKnob<SamplerState>;

const ROOT_KNOB: Control = Control::new(&ROOT, "Root", "Change root", note_readout);
const VELOCITY_KNOB: Control =
    Control::new(&VELOCITY, "Velocity", "Change velocity", percent_readout);
const RELEASE_KNOB: Control = Control::new(&RELEASE, "Release", "Change release", seconds_readout);
const GAIN_KNOB: Control = Control::new(&GAIN, "Gain", "Change gain", decibels_readout);
const ATTACK_KNOB: Control = Control::new(&ATTACK, "Attack", "Change attack", seconds_readout);
const DECAY_KNOB: Control = Control::new(&DECAY, "Decay", "Change decay", seconds_readout);
const SUSTAIN_KNOB: Control = Control::new(&SUSTAIN, "Sustain", "Change sustain", percent_readout);

/// A note number by its name: `C4`, `C#4`.
fn note_readout(note: f32) -> String {
    sound_notes::Pitch::nearest(note.round() as i64).name()
}

/// The second line of a library instrument in the select: its library, the disk it takes,
/// and whether it is downloaded.
fn described(entry: &'static Entry) -> String {
    let size = size_text(entry.disk_bytes);
    match library::status(entry) {
        Status::Here => format!("{} · {size} · downloaded", entry.library.name),
        _ => format!("{} · {size} on disk", entry.library.name),
    }
}

/// The name of an SFZ instrument for the display: its file name without `.sfz`.
fn sfz_name(sfz: &SfzPath) -> String {
    let path = sfz.to_string();
    let file = path.rsplit('/').next().unwrap_or(&path);
    let stem = file.len().saturating_sub(".sfz".len());
    file.get(..stem).unwrap_or(file).to_string()
}

/// Where the envelope sits in the display, `y` up: full level near the top, so its handles can
/// be taken, and silence at the bottom, where the start line has its handle.
const TOP: f32 = 0.88;

/// A start, with the shortest part of a clip left before the end.
fn with_start(state: &mut SamplerState, seconds: f64, file_seconds: f64) {
    let end = state.end_seconds.unwrap_or(file_seconds);
    state.start_seconds = clamped_start(seconds, 0.0, end);
}

/// An end after the start, with no end at the end of the file, as for a clip.
fn with_end(state: &mut SamplerState, seconds: f64, file: &Info) {
    state.end_seconds = clamped_end(seconds.min(file.seconds()), state.start_seconds, file);
}

pub struct SamplerView {
    session: Entity<Session>,
    sampler: Instance<SamplerState>,
    /// The title and the icons the rack gives the card.
    frame: CardFrame,
    /// The gesture of a knob or handle drag.
    edit: ControlEdit,
    lanes: Entity<Lanes<SamplerState>>,
    /// Whether the card shows Start, End, Attack, Decay and Sustain. Interface state.
    expanded: bool,
    /// Where the green line is, in seconds of the file, while a note sounds.
    playing_at: Option<f32>,
    /// The focus of `Choose file`, a tab stop.
    choose_focus: FocusHandle,
    /// The Instrument select.
    instruments: Entity<DropdownMenu>,
    /// Where the library instrument of the record is on this machine, and whether the
    /// instrument loads, as last shown.
    download: Option<Status>,
    loading: bool,
    _position: Task<()>,
}

impl SamplerView {
    pub fn new(
        session: Entity<Session>,
        sampler: Instance<SamplerState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.sampler.id() => {
                view.show_instruments(cx);
                cx.notify();
            }
            // Deleted under a drag, from outside: the delete was the last write, so the gesture
            // finishes and does not cancel.
            ProjectEvent::Deleted(id) if id == view.sampler.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The waveform, and what is known of the file, arrive from a background thread.
        let waveforms = Waveforms::entity(cx);
        cx.observe(&waveforms, |_, _, cx| cx.notify()).detach();
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        // Where the notes were before this card was made is not where they are now.
        if let Some(peaks) = session.read(cx).project().peaks(sampler.id(), POSITION) {
            peaks.take();
        }
        let lanes = Lanes::follow(&session, sampler.id(), Sampler::AUTOMATION, cx);
        let instruments = cx.new(|cx| {
            DropdownMenu::new("Instrument", Vec::new(), cx)
                .trigger(Trigger::Select)
                .trigger_width(INSTRUMENT_SELECT_WIDTH)
                .width(280.)
                .max_height(420.)
                .debug_name("instrument")
        });
        cx.subscribe(&instruments, |view, _, MenuPicked(value), cx| {
            view.pick_instrument(value.as_ref(), cx)
        })
        .detach();
        let mut view = Self {
            session,
            sampler,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
            playing_at: None,
            choose_focus: cx.focus_handle().tab_stop(true),
            instruments,
            download: None,
            loading: false,
            _position: every_poll(cx, |view, cx| {
                view.read_position(cx);
                view.follow_download(cx);
            }),
        };
        view.show_instruments(cx);
        view
    }

    /// The Instrument select, after any change of the record or of what is downloaded: the
    /// file of the record, its SFZ file, and the library by category.
    fn show_instruments(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self
            .session
            .read(cx)
            .project()
            .state(&self.sampler)
            .cloned()
        else {
            return;
        };
        let mut first = MenuGroup::new();
        if let Some(asset) = &state.sample {
            first = first.item(MenuItem::new(SAMPLE_VALUE, asset.to_string()));
        }
        if let Some(sfz) = &state.sfz {
            first = first.item(MenuItem::new(SFZ_VALUE, sfz_name(sfz)));
        }
        let file = MenuItem::new(FILE_VALUE, "Audio file…").selectable(false);
        let mut entries = vec![MenuEntry::Group(first.item(file))];
        for category in Category::ALL {
            let items: Vec<MenuItem> = CATALOG
                .iter()
                .filter(|entry| entry.category == category)
                .map(|entry| {
                    let value = format!("{LIBRARY_PREFIX}{}", entry.id);
                    MenuItem::new(value, entry.name).description(described(entry))
                })
                .collect();
            if !items.is_empty() {
                let group = MenuGroup::new().label(category.name()).items(items);
                entries.push(MenuEntry::Group(group));
            }
        }
        let selected = match (&state.library, &state.sfz, &state.sample) {
            (Some(id), ..) => format!("{LIBRARY_PREFIX}{id}"),
            (None, Some(_), _) => SFZ_VALUE.to_string(),
            (None, None, Some(_)) => SAMPLE_VALUE.to_string(),
            (None, None, None) => FILE_VALUE.to_string(),
        };
        self.instruments.update(cx, |select, cx| {
            select.set_entries(entries, cx);
            select.set_selected(selected, cx);
        });
    }

    fn pick_instrument(&mut self, value: &str, cx: &mut Context<Self>) {
        if value == FILE_VALUE {
            self.choose_file(cx);
            return;
        }
        let Some(id) = value.strip_prefix(LIBRARY_PREFIX) else {
            return;
        };
        let Ok(id) = LibraryId::try_from(id.to_string()) else {
            return;
        };
        let set = |state: &mut SamplerState, id| {
            state.library = Some(id);
            state.sample = None;
            state.sfz = None;
        };
        let entry = id.entry();
        self.change(INSTRUMENT_LABEL, ValueChange::Set(id), set, cx);
        // Picking it from a list that shows its size is asking for it.
        library::download(entry);
        self.follow_download(cx);
    }

    /// Follows the download of the library instrument of the record, and its loading, once
    /// per poll of the session, and draws again when either moved on.
    fn follow_download(&mut self, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let Some(state) = project.state(&self.sampler) else {
            return;
        };
        let loading = instrument::is_loading(state, project.assets());
        let status = state
            .library
            .as_ref()
            .map(LibraryId::entry)
            .map(library::status);
        if loading != self.loading {
            self.loading = loading;
            cx.notify();
        }
        if status != self.download {
            // Done, so the select marks it as downloaded.
            let done = status == Some(Status::Here);
            self.download = status;
            if done {
                self.show_instruments(cx);
            }
            cx.notify();
        }
    }

    /// Starts the download of `entry`, and the Sampler waits for it.
    fn download(&mut self, entry: &'static Entry, cx: &mut Context<Self>) {
        library::download(entry);
        let sampler = self.sampler.id().clone();
        self.session
            .update(cx, |session, cx| session.rebind(&[sampler], cx));
        self.follow_download(cx);
    }

    /// Shows or hides the hidden knobs, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    /// The Instrument select, for a test that opens it.
    pub fn instrument_list(&self) -> &Entity<DropdownMenu> {
        &self.instruments
    }

    /// Where the green line is now, in seconds of the file.
    pub fn playing_at(&self) -> Option<f32> {
        self.playing_at
    }

    /// Takes where the last note is from the audio thread, once per poll of the session. It
    /// moves in steps of a point of the display, so a note draws the card no more than it must.
    pub fn read_position(&mut self, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let Some(peaks) = project.peaks(self.sampler.id(), POSITION) else {
            return;
        };
        let [seconds, _] = peaks.take();
        let length = self.file(cx).map(|file| file.seconds() as f32);
        let at = length.filter(|_| seconds > 0.0).map(|length| {
            let step = length / DISPLAY_WIDTH;
            (seconds / step).round() * step
        });
        if at != self.playing_at {
            self.playing_at = at;
            cx.notify();
        }
    }

    /// The file of the sampler, when it is known to play. From memory only.
    fn file(&self, cx: &Context<Self>) -> Option<Info> {
        let project = self.session.read(cx).project();
        let asset = project.state(&self.sampler)?.sample.clone()?;
        match sound_media::cached(project.assets(), &asset) {
            Cached::Plays(file) => Some(file),
            _ => None,
        }
    }

    /// Copies a file into `assets/audio/` on a background thread and makes it the sample, as
    /// one undo step. A file that does not play is not copied, and the notice says why.
    pub fn load_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        import::import_file(&self.session, path, cx, Self::use_imported);
    }

    fn use_imported(&mut self, imported: Imported, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let Some(mut state) = session.read(cx).project().state(&self.sampler).cloned() else {
            return;
        };
        let sampler = self.sampler.clone();
        // `imported` holds the file in memory until the edit is made, so the behaviour that
        // hands it to the sampler reads nothing.
        session.update(cx, |session, cx| {
            // The file the record names, missing and now there under that name: the record
            // stays as it is, trims and all, and the sampler loads the file. No edit and no
            // undo step, as when the file arrives by the watcher.
            if state.sample.as_ref() == Some(&imported.asset) {
                session.rebind(std::slice::from_ref(sampler.id()), cx);
                return;
            }
            // Where the old file started and ended means nothing in the new one.
            state.sample = Some(imported.asset.clone());
            state.sfz = None;
            state.library = None;
            state.start_seconds = 0.0;
            state.end_seconds = None;
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.set(&sampler, state);
                project.commit(LOAD_LABEL, changes)
            });
        });
        drop(imported);
    }

    /// The file panel of macOS, for one audio file.
    fn choose_file(&mut self, cx: &mut Context<Self>) {
        import::choose_file(&self.session, "Load", cx, Self::load_file);
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut SamplerState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, sampler) = (&self.session, &self.sampler);
        self.edit.apply(session, sampler, label, change, set, cx);
    }

    fn knob(&self, control: Control, state: &SamplerState, cx: &mut Context<Self>) -> Knob {
        let automated = self.lanes.read(cx).is_automated(control.parameter.field);
        control
            .knob(state)
            .automated(automated)
            .on_change(weak_callback(cx, move |view, change, cx| {
                view.change(control.undo_label, change, control.parameter.set, cx);
            }))
    }

    /// Start and End: seconds of the file, whose length is their range. With no file that
    /// plays they are dimmed.
    fn trim_knobs(&self, state: &SamplerState, cx: &mut Context<Self>) -> [Knob; 2] {
        let file = self.file(cx);
        let length = file.map_or(1.0, |file| file.seconds());
        let end = state.end_seconds.unwrap_or(length);
        let seconds = |value: f64| seconds_readout(value as f32);
        let start = Knob::new("start")
            .range(KnobRange::linear(0., length as f32))
            .value(state.start_seconds as f32)
            .default_value(0.)
            .label("Start")
            .readout(seconds(state.start_seconds))
            .disabled(file.is_none())
            .on_change(weak_callback(cx, move |view, change: ValueChange, cx| {
                let set = |state: &mut SamplerState, value: f32| {
                    with_start(state, f64::from(value), length)
                };
                view.change("Change start", change, set, cx);
            }));
        let end_knob = Knob::new("end")
            .range(KnobRange::linear(0., length as f32))
            .value(end as f32)
            .default_value(length as f32)
            .label("End")
            .readout(seconds(end))
            .disabled(file.is_none())
            .on_change(weak_callback(cx, move |view, change: ValueChange, cx| {
                if let Some(file) = file {
                    let set = |state: &mut SamplerState, value: f32| {
                        with_end(state, f64::from(value), &file)
                    };
                    view.change("Change end", change, set, cx);
                }
            }));
        [start, end_knob]
    }

    /// Where a drop of a file goes: into [`Self::load_file`].
    fn file_drop(&self, message: &'static str, cx: &mut Context<Self>) -> FileDrop {
        let view = cx.weak_entity();
        FileDrop::new(message, move |paths: &[PathBuf], _, cx| {
            if let Some(path) = paths.first().cloned() {
                view.update(cx, |view, cx| view.load_file(path, cx)).ok();
            }
        })
    }

    /// The display with no file that plays: what it says, `Choose file` and the drop.
    fn no_file(&self, says: String, has_sample: bool, cx: &mut Context<Self>) -> NoFile {
        let button = Button::new("choose-file", "Choose file")
            .debug_selector(|| "choose-file".to_string())
            .variant(ButtonVariant::Subtle)
            .size(ButtonSize::Sm)
            .focus_handle(&self.choose_focus)
            .on_click(cx.listener(|view, _, _, cx| view.choose_file(cx)));
        let drop = match has_sample {
            true => DROP_TO_REPLACE,
            false => DROP_TO_LOAD,
        };
        NoFile::new("sampler-display", DISPLAY_WIDTH, says)
            .button(button)
            .drop_file(self.file_drop(drop, cx))
    }

    /// The envelope over the whole file, in the time of the file from the start line, with the
    /// attack peak and the decay corner as handles.
    fn envelope(
        &self,
        state: &SamplerState,
        length: f64,
        cx: &mut Context<Self>,
    ) -> (Vec<Point<f32>>, [Handle; 2]) {
        let length = length as f32;
        let start = state.start_seconds as f32;
        let end = state.end_seconds.map_or(length, |end| end as f32);
        let (attack, decay) = (state.attack_seconds, state.decay_seconds);
        let sustain = state.sustain.clamp(0., 1.) * TOP;
        let across = |seconds: f32| place(seconds, length);
        let curve = vec![
            point(across(start), 0.),
            point(across(start + attack), TOP),
            point(across(start + attack + decay), sustain),
            point(across(end.max(start + attack + decay)), sustain),
        ];
        let lanes = self.lanes.read(cx);
        let attack_held = lanes.is_automated(ATTACK.field);
        let corner_held = lanes.is_automated(DECAY.field) || lanes.is_automated(SUSTAIN.field);
        // The attack peak moves the attack: its range across the display is the file, offset
        // by where the attack starts.
        let attack_handle = Handle::new(
            "attack",
            Axis::new(
                KnobRange::linear(-start, length - start),
                attack,
                ATTACK.default,
            ),
            Axis::fixed(TOP),
        )
        .automated(attack_held)
        .on_change(weak_callback(
            cx,
            |view, change: ValueChange<Point<f32>>, cx| {
                let set = |state: &mut SamplerState, at: f32| {
                    state.attack_seconds = ATTACK_KNOB.clamp(at)
                };
                view.change(ATTACK_KNOB.undo_label, change.map(|at| at.x), set, cx);
            },
        ));
        // The corner moves two values: the decay sideways, the sustain level up and down. One
        // drag of it is one undo step.
        let decay_starts = start + attack;
        let corner = Handle::new(
            "decay",
            Axis::new(
                KnobRange::linear(-decay_starts, length - decay_starts),
                decay,
                DECAY.default,
            ),
            Axis::new(
                KnobRange::linear(0., 1. / TOP),
                state.sustain,
                SUSTAIN.default,
            ),
        )
        .automated(corner_held)
        .on_change(weak_callback(
            cx,
            |view, change: ValueChange<Point<f32>>, cx| {
                let set = |state: &mut SamplerState, at: Point<f32>| {
                    state.decay_seconds = DECAY_KNOB.clamp(at.x);
                    state.sustain = SUSTAIN_KNOB.clamp(at.y);
                };
                view.change("Change decay and sustain", change, set, cx);
            },
        ));
        (curve, [attack_handle, corner])
    }

    /// The display of a library instrument: what it is and whose, or how far its download is.
    fn library_display(&self, entry: &'static Entry, cx: &mut Context<Self>) -> AnyElement {
        let library = entry.library;
        let license = match library.attribution {
            Some(attribution) => format!("{} · {attribution}", library.license),
            None => library.license.to_string(),
        };
        let download = size_text(entry.download_bytes);
        let (says, button) = match library::status(entry) {
            Status::Here if self.loading => (format!("Loading {}…", entry.name), None),
            Status::Here => (
                format!("{} · {} · {license}", entry.name, library.name),
                None,
            ),
            Status::Downloading { bytes } => (
                format!(
                    "Downloading {} · {} of {download}",
                    entry.name,
                    size_text(bytes)
                ),
                None,
            ),
            Status::Failed(_) => (
                format!("The download of {} failed", entry.name),
                Some("Try again"),
            ),
            Status::Missing => (
                format!("{} · {} · {license}", entry.name, library.name),
                Some("Download"),
            ),
            Status::NoLibrary => (format!("{} is not downloaded", entry.name), None),
        };
        let display = NoFile::new("sampler-display", DISPLAY_WIDTH, says)
            .drop_file(self.file_drop(DROP_TO_REPLACE, cx));
        let display = match button {
            Some(label) => display.button(
                Button::new(
                    "download",
                    format!("{label} · {} on disk", size_text(entry.disk_bytes)),
                )
                .debug_selector(|| "download".to_string())
                .variant(ButtonVariant::Subtle)
                .size(ButtonSize::Sm)
                .on_click(cx.listener(move |view, _, _, cx| view.download(entry, cx))),
            ),
            None => display,
        };
        display.into_any_element()
    }

    fn display(&self, state: &SamplerState, cx: &mut Context<Self>) -> AnyElement {
        if let Some(id) = &state.library {
            return self.library_display(id.entry(), cx);
        }
        if let Some(sfz) = &state.sfz {
            let says = match self.loading {
                true => format!("Loading {}…", sfz_name(sfz)),
                false => format!("{} · SFZ instrument", sfz_name(sfz)),
            };
            return self.no_file(says, true, cx).into_any_element();
        }
        let Some(asset) = state.sample.clone() else {
            return self.no_file(EMPTY.into(), false, cx).into_any_element();
        };
        let assets = self.session.read(cx).project().assets().clone();
        let name = asset.to_string();
        let file = match sound_media::cached(&assets, &asset) {
            Cached::Plays(file) => file,
            Cached::Missing => {
                return self
                    .no_file(format!("{name} is missing"), true, cx)
                    .into_any_element();
            }
            Cached::DoesNotPlay(_) => {
                return self
                    .no_file(format!("{name} does not play"), true, cx)
                    .into_any_element();
            }
            // Asking for the waveform asks a background thread what the file is, and the cache
            // of waveforms tells this card when it knows.
            Cached::Unknown => {
                Waveforms::overview(&assets, &asset, cx);
                return self.no_file(String::new(), true, cx).into_any_element();
            }
        };
        let overview = Waveforms::overview(&assets, &asset, cx);
        let length = file.seconds();
        let end = state.end_seconds.unwrap_or(length);
        let (curve, handles) = self.envelope(state, length, cx);
        let caption = format!(
            "{name} · A {} · D {} · S {}",
            seconds_readout(state.attack_seconds),
            seconds_readout(state.decay_seconds),
            percent_readout(state.sustain),
        );
        let display =
            WaveformDisplay::new("sampler-display", DISPLAY_WIDTH, overview, length as f32)
                .trim(state.start_seconds as f32, end as f32)
                .on_start(weak_callback(cx, move |view, change: ValueChange, cx| {
                    let set = |state: &mut SamplerState, seconds: f32| {
                        with_start(state, f64::from(seconds), length)
                    };
                    view.change("Change start", change, set, cx);
                }))
                .on_end(weak_callback(cx, move |view, change: ValueChange, cx| {
                    let set = |state: &mut SamplerState, seconds: f32| {
                        with_end(state, f64::from(seconds), &file)
                    };
                    view.change("Change end", change, set, cx);
                }))
                .playhead(self.playing_at)
                .curve(curve)
                .caption(caption)
                .drop_file(self.file_drop(DROP_TO_REPLACE, cx));
        handles
            .into_iter()
            .fold(display, WaveformDisplay::handle)
            .into_any_element()
    }
}

impl Render for SamplerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.lanes.read(cx).state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let instrument = Cell::new(self.instruments.clone())
            .span(2)
            .label("Instrument");
        if state.sfz.is_some() || state.library.is_some() {
            let columns = [
                Column::new().top(instrument).bottom(knob(GAIN_KNOB, cx)),
                Column::new(),
            ];
            let card = self.frame.card().display(self.display(&state, cx));
            let card = columns
                .into_iter()
                .fold(card, |card, column| card.column(column));
            return card.into_any_element();
        }
        let columns = [
            // A root between two notes is no root.
            Column::new()
                .top(instrument)
                .bottom(knob(ROOT_KNOB, cx).step(1.)),
            Column::new().bottom(knob(VELOCITY_KNOB, cx)),
            Column::new()
                .top(knob(RELEASE_KNOB, cx))
                .bottom(knob(GAIN_KNOB, cx)),
        ];
        // Behind expand: the values the handles of the display move, so the keys reach every
        // one of them.
        let [start, end] = self.trim_knobs(&state, cx);
        let hidden = [
            Column::new().top(start).bottom(end),
            Column::new()
                .top(knob(ATTACK_KNOB, cx))
                .bottom(knob(DECAY_KNOB, cx)),
            Column::new().top(knob(SUSTAIN_KNOB, cx)),
        ];
        let expand = cx.listener(|view, _, _, cx| view.set_expanded(!view.expanded, cx));
        let card = self
            .frame
            .card()
            .expand(self.expanded, expand)
            .display(self.display(&state, cx));
        let card = columns
            .into_iter()
            .fold(card, |card, column| card.column(column));
        let card = hidden
            .into_iter()
            .fold(card, |card, column| card.hidden_column(column));
        card.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_reads_as_a_note() {
        assert_eq!(note_readout(60.), "C4");
        assert_eq!(note_readout(61.), "C#4");
    }

    #[test]
    fn start_and_end_keep_the_shortest_part_between_them_and_the_end_of_the_file_is_no_end() {
        let file = Info {
            frames: 48_000,
            channels: 1,
            sample_rate: 48_000,
            container: sound_media::Container::Wav,
        };
        let mut state = SamplerState {
            end_seconds: Some(0.5),
            ..SamplerState::default()
        };
        with_start(&mut state, 0.7, 1.0);
        assert_eq!(state.start_seconds, 0.49);
        with_start(&mut state, -1.0, 1.0);
        assert_eq!(state.start_seconds, 0.0);
        with_end(&mut state, 0.001, &file);
        assert_eq!(
            state.end_seconds,
            Some(sound_ui::components::waveform_display::SHORTEST_SECONDS)
        );
        with_end(&mut state, 0.99999, &file);
        assert_eq!(state.end_seconds, None);
        with_end(&mut state, 0.8, &file);
        assert_eq!(state.end_seconds, Some(0.8));
    }
}
