//! The card of the sampler in a rack: the whole file in the waveform display, with the start and
//! end lines, the envelope drawn over it in the time of the file and a green line where the last
//! note is; Root, Velocity, Release and Gain next to it, and Start, End, Attack, Decay and
//! Sustain behind expand. The rack gives the frame of the card, whose title says "Sampler" and
//! is where another instrument is picked.
//!
//! With no file, the display says `Drop an audio file here` over a `Choose file` button, which
//! opens the file panel of macOS and is the way from the keys. A file dropped on the display, or
//! chosen, is copied into `assets/audio/` on a background thread and becomes the sample, as one
//! undo step.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session by [`ControlEdit`], as in the synth: a knob or a handle drag is one
//! gesture and one undo step, a key step or a reset one commit. A handle edits the field of its
//! knob, under the name of its knob in the history.

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, Entity, FocusHandle, PathPromptOptions, Point, Task, Window, div, point,
    prelude::*,
};
use sound_core::{Changes, Instance, ProjectEvent, State};
use sound_media::{Cached, Info};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::waveform_display::{
    FileDrop, NoFile, WaveformDisplay, clamped_end, clamped_start, place,
};
use sound_ui::{
    ControlEdit, DeviceLabel, Devices, Session, Views, Waveforms, every_poll, weak_callback,
};

use crate::{
    ATTACK, DECAY, GAIN, POSITION, Parameter, RELEASE, ROOT, SUSTAIN, SamplerState, VELOCITY,
};

/// The name the rack puts on the card of a sampler.
pub const NAME: &str = "Sampler";
/// The display, so the card is 32 + 312 + 8 + 2 x 56 = 464 pt, and 649 expanded.
pub const DISPLAY_WIDTH: f32 = 312.;
/// What the display says with no file, and while one is dragged over it.
pub const EMPTY: &str = "Drop an audio file here";
pub const DROP_TO_LOAD: &str = "Drop to load the file";
pub const DROP_TO_REPLACE: &str = "Drop to replace the file";
/// The undo step of a file dropped or chosen.
pub const LOAD_LABEL: &str = "Load sample";

/// Registers the card of the `sampler` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(SamplerView::new);
    devices.describe::<SamplerState>(|_| DeviceLabel {
        key: SamplerState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    Seconds,
    /// A part of one, shown as a percentage.
    Part,
    Decibels,
    /// A note number, shown by its name.
    Note,
}

/// A knob of the view on a number with a fixed range.
struct Control {
    parameter: &'static Parameter,
    label: &'static str,
    undo_label: &'static str,
    unit: Unit,
}

impl Control {
    /// A value as the parameter takes it: a handle may ask for one past its ends.
    fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.parameter.min, self.parameter.max)
    }
}

const ROOT_KNOB: Control = Control {
    parameter: &ROOT,
    label: "Root",
    undo_label: "Change root",
    unit: Unit::Note,
};
const VELOCITY_KNOB: Control = Control {
    parameter: &VELOCITY,
    label: "Velocity",
    undo_label: "Change velocity",
    unit: Unit::Part,
};
const RELEASE_KNOB: Control = Control {
    parameter: &RELEASE,
    label: "Release",
    undo_label: "Change release",
    unit: Unit::Seconds,
};
const GAIN_KNOB: Control = Control {
    parameter: &GAIN,
    label: "Gain",
    undo_label: "Change gain",
    unit: Unit::Decibels,
};
const ATTACK_KNOB: Control = Control {
    parameter: &ATTACK,
    label: "Attack",
    undo_label: "Change attack",
    unit: Unit::Seconds,
};
const DECAY_KNOB: Control = Control {
    parameter: &DECAY,
    label: "Decay",
    undo_label: "Change decay",
    unit: Unit::Seconds,
};
const SUSTAIN_KNOB: Control = Control {
    parameter: &SUSTAIN,
    label: "Sustain",
    undo_label: "Change sustain",
    unit: Unit::Part,
};

/// A value with its unit, as the knob shows it: `2 ms`, `1.18 s`, `55%`, `-6 dB`, `C4`.
fn readout(unit: Unit, value: f32) -> String {
    match unit {
        Unit::Seconds if value < 1.0 => format!("{} ms", short(value * 1_000.0)),
        Unit::Seconds => format!("{} s", short(value)),
        Unit::Part => format!("{}%", short(value * 100.0)),
        Unit::Decibels => format!("{} dB", short(value)),
        Unit::Note => sound_notes::Pitch::nearest(value.round() as i64).name(),
    }
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
    /// Whether the card shows Start, End, Attack, Decay and Sustain. Interface state.
    expanded: bool,
    /// Where the green line is, in seconds of the file, while a note sounds.
    playing_at: Option<f32>,
    /// The focus of `Choose file`, a tab stop.
    choose_focus: FocusHandle,
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
            ProjectEvent::Changed(id) if id == view.sampler.id() => cx.notify(),
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
        Self {
            session,
            sampler,
            frame,
            edit: ControlEdit::default(),
            expanded: false,
            playing_at: None,
            choose_focus: cx.focus_handle().tab_stop(true),
            _position: every_poll(cx, Self::read_position),
        }
    }

    /// Shows or hides the hidden knobs, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
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
        let assets = self.session.read(cx).project().assets().clone();
        let importing = cx.background_spawn(async move { sound_media::import(&assets, &path) });
        cx.spawn(async move |view, cx| {
            let imported = importing.await;
            view.update(cx, |view, cx| view.use_imported(imported, cx))
                .ok();
        })
        .detach();
    }

    fn use_imported(
        &mut self,
        imported: Result<sound_media::Imported, sound_media::MediaError>,
        cx: &mut Context<Self>,
    ) {
        let session = self.session.clone();
        let imported = match imported {
            Ok(imported) => imported,
            Err(error) => {
                session.update(cx, |session, cx| session.report(error, cx));
                return;
            }
        };
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
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Load".into()),
        });
        cx.spawn(async move |view, cx| {
            let paths = match chosen.await {
                Ok(Ok(paths)) => paths,
                Ok(Err(error)) => {
                    view.update(cx, |view, cx| {
                        let session = view.session.clone();
                        session.update(cx, |session, cx| session.report(error, cx));
                    })
                    .ok();
                    return;
                }
                // The panel went away without an answer.
                Err(_) => return,
            };
            if let Some(path) = paths.and_then(|paths| paths.into_iter().next()) {
                view.update(cx, |view, cx| view.load_file(path, cx)).ok();
            }
        })
        .detach();
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

    fn knob(
        &self,
        control: &'static Control,
        state: &SamplerState,
        cx: &mut Context<Self>,
    ) -> Knob {
        let value = (control.parameter.get)(state);
        let knob = Knob::new(control.parameter.field)
            .range(KnobRange::of(control.parameter))
            .value(value)
            .default_value(control.parameter.default)
            .label(control.label)
            .readout(readout(control.unit, value))
            .on_change(weak_callback(cx, move |view, change, cx| {
                let (label, set) = (control.undo_label, control.parameter.set);
                view.change(label, change, set, cx);
            }));
        match control.unit {
            // A root between two notes is no root.
            Unit::Note => knob.step(1.),
            _ => knob,
        }
    }

    /// Start and End: seconds of the file, whose length is their range. With no file that
    /// plays they are dimmed.
    fn trim_knobs(&self, state: &SamplerState, cx: &mut Context<Self>) -> [Knob; 2] {
        let file = self.file(cx);
        let length = file.map_or(1.0, |file| file.seconds());
        let end = state.end_seconds.unwrap_or(length);
        let seconds = |value: f64| readout(Unit::Seconds, value as f32);
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

    fn display(&self, state: &SamplerState, cx: &mut Context<Self>) -> AnyElement {
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
            readout(Unit::Seconds, state.attack_seconds),
            readout(Unit::Seconds, state.decay_seconds),
            readout(Unit::Part, state.sustain),
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
        let Some(state) = self
            .session
            .read(cx)
            .project()
            .state(&self.sampler)
            .cloned()
        else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(&ROOT_KNOB, cx))
                .bottom(knob(&RELEASE_KNOB, cx)),
            Column::new()
                .top(knob(&VELOCITY_KNOB, cx))
                .bottom(knob(&GAIN_KNOB, cx)),
        ];
        // Behind expand: the values the handles of the display move, so the keys reach every
        // one of them.
        let [start, end] = self.trim_knobs(&state, cx);
        let hidden = [
            Column::new().top(start).bottom(end),
            Column::new()
                .top(knob(&ATTACK_KNOB, cx))
                .bottom(knob(&DECAY_KNOB, cx)),
            Column::new().top(knob(&SUSTAIN_KNOB, cx)),
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
    fn a_readout_has_its_unit() {
        assert_eq!(readout(Unit::Seconds, 0.002), "2 ms");
        assert_eq!(readout(Unit::Seconds, 0.012), "12 ms");
        assert_eq!(readout(Unit::Seconds, 1.18), "1.18 s");
        assert_eq!(readout(Unit::Part, 0.55), "55%");
        assert_eq!(readout(Unit::Decibels, 0.), "0 dB");
        assert_eq!(readout(Unit::Decibels, -6.), "-6 dB");
        assert_eq!(readout(Unit::Note, 60.), "C4");
        assert_eq!(readout(Unit::Note, 61.), "C#4");
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
