//! The Clip card: the first card of the rack of an audio track, where an instrument track has
//! its instrument. It shows the selected clip of the track: the file name as its title, the
//! whole file in the waveform display with the start and end lines that trim it, the gain line
//! with a fade at each end, and a green line while the playhead is inside the clip. The line
//! under it says which part of the file plays: `2.1 s to 10.1 s of 14.6 s`. Its knobs are
//! Gain, Fade in and Fade out, and behind expand Start and End: the keys reach every value the
//! handles move. With no clip of the track selected it is the narrowest card, with a hint.
//!
//! Every drag is one gesture and one undo step, and a key step one commit, with the names the
//! handles on the timeline use. The card acts on one clip, the effects after it on the whole
//! track, so a hairline stands between them in the rack.

use gpui::{
    AnyElement, Context, Entity, FontWeight, Point, SharedString, Window, div, point, prelude::*,
    px,
};
use sound_core::{Instance, ProjectEvent};
use sound_media::{Cached, Info};
use sound_ui::components::device_card::{Column, DeviceCard, NARROW_CARD_WIDTH, PLAIN_CARD_WIDTH};
use sound_ui::components::display::{Axis, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::waveform_display::{WaveformDisplay, place};
use sound_ui::{ActiveTheme, ControlEdit, Session, Waveforms, weak_callback};

use super::clips::{
    GAIN_DB, fade_in, fade_out, gain_label, played_ms, time_label, with_file_end, with_file_start,
};
use super::timeline::{FADE_IN_LABEL, FADE_OUT_LABEL, GAIN_LABEL};
use crate::{AudioClip, TrackState};

/// The display, so the card is 32 + 312 + 8 + 2 x 56 = 464 pt, and 537 expanded.
pub const DISPLAY_WIDTH: f32 = 312.;
/// The undo step of the start and end lines and of their knobs, as of an edge on the timeline.
pub const TRIM_LABEL: &str = "Trim clip";
/// What the empty card says. Short, because the card is one column of cells wide.
pub const EMPTY: &str = "Select a clip.";
/// The id of the card, which tests find its controls under.
pub const CARD_ID: &str = "clip-card";

/// Where a gain is up the display: the range of the Gain knob, bottom to top.
fn gain_up(db: f32) -> f32 {
    KnobRange::linear(GAIN_DB.0, GAIN_DB.1).position(db)
}

/// The caption and the knobs say seconds to a tenth: `2.1 s`.
fn seconds_label(seconds: f64) -> String {
    format!("{} s", short(seconds as f32))
}

pub struct ClipCard {
    session: Entity<Session>,
    track: Instance<TrackState>,
    /// The first selected clip, when it is an audio clip of this track.
    clip: Option<Instance<AudioClip>>,
    /// The gesture of a drag of a knob or a handle.
    edit: ControlEdit,
    /// Whether Start and End show. Interface state: not saved.
    expanded: bool,
    /// Where the green line is, in seconds of the file, while the playhead is inside the clip.
    playing_at: Option<f32>,
    /// The clip as it was when a drag of its start began: every move of the drag is worked
    /// out from it, so the rounding of one move never adds up over the next.
    start_origin: Option<AudioClip>,
}

impl ClipCard {
    pub fn new(
        session: Entity<Session>,
        track: Instance<TrackState>,
        cx: &mut Context<Self>,
    ) -> Self {
        // The session notifies when the selection changes, and after every edit.
        cx.observe(&session, |card, _, cx| card.follow_selection(cx))
            .detach();
        cx.subscribe(&session, |card, _, event, cx| {
            let shown = card.clip.as_ref().map(|clip| clip.id());
            match event {
                ProjectEvent::Changed(id) if Some(id) == shown => cx.notify(),
                // Deleted under a drag, from outside: the delete was the last write, so the
                // gesture finishes and does not cancel.
                ProjectEvent::Deleted(id) if Some(id) == shown => {
                    card.edit.finish(&card.session, cx);
                    card.follow_selection(cx);
                }
                _ => {}
            }
        })
        .detach();
        let waveforms = Waveforms::entity(cx);
        cx.observe(&waveforms, |_, _, cx| cx.notify()).detach();
        let playhead = session.read(cx).playhead().clone();
        cx.observe(&playhead, |card, _, cx| card.follow_playhead(cx))
            .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|card, cx| card.edit.finish(&card.session, cx))
            .detach();
        let mut card = Self {
            session,
            track,
            clip: None,
            edit: ControlEdit::default(),
            expanded: false,
            playing_at: None,
            start_origin: None,
        };
        card.follow_selection(cx);
        card
    }

    /// The clip the card shows.
    pub fn clip(&self) -> Option<&Instance<AudioClip>> {
        self.clip.as_ref()
    }

    /// Shows or hides Start and End, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    /// The first selected clip, when it is an audio clip of this track.
    fn follow_selection(&mut self, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        let project = session.project();
        let selected = session.selected_clip();
        let clip = selected
            .filter(|clip| clip.parent().as_ref() == Some(self.track.id()))
            .and_then(|clip| project.resolve::<AudioClip>(clip))
            .filter(|clip| project.state(clip).is_some());
        if clip.as_ref().map(|clip| clip.id()) != self.clip.as_ref().map(|clip| clip.id()) {
            // A drag of the clip that was shown ends before another is shown.
            self.edit.finish(&self.session, cx);
            self.clip = clip;
            self.follow_playhead(cx);
            cx.notify();
        }
    }

    /// The green line: where the playhead is in the file, while it is inside the clip. It moves
    /// in steps of a point of the display, so playback draws the card no more than it must.
    fn follow_playhead(&mut self, cx: &mut Context<Self>) {
        let at = self.playhead_in_file(cx);
        if at != self.playing_at {
            self.playing_at = at;
            cx.notify();
        }
    }

    fn playhead_in_file(&self, cx: &mut Context<Self>) -> Option<f32> {
        let clip = self.clip.as_ref()?;
        let session = self.session.read(cx);
        let project = session.project();
        let state = project.state(clip)?;
        let Cached::Plays(file) = sound_media::cached(project.assets(), &state.asset) else {
            return None;
        };
        let tick = session.playhead().read(cx).tick;
        let clock = project.clock();
        if tick < state.start || tick >= state.end(Some(&file), clock) {
            return None;
        }
        let seconds =
            clock.seconds_of(tick) - clock.seconds_of(state.start) + state.file_start_seconds;
        let step = file.seconds() / f64::from(DISPLAY_WIDTH);
        Some(((seconds / step).round() * step) as f32)
    }

    /// A change of the start, from its line or its knob. A drag works from the clip of its
    /// first move; a key step or a reset from the clip as it is.
    fn change_start(
        &mut self,
        change: ValueChange,
        file: Info,
        clock: &sound_core::Clock,
        cx: &mut Context<Self>,
    ) {
        let live = self.clip.as_ref().and_then(|clip| {
            let project = self.session.read(cx).project();
            project.state(clip).cloned()
        });
        let origin = match change {
            ValueChange::Drag(_) => {
                if self.start_origin.is_none() {
                    self.start_origin = live;
                }
                self.start_origin.clone()
            }
            ValueChange::Set(_) | ValueChange::DragEnd | ValueChange::DragCancel => {
                self.start_origin = None;
                None
            }
        };
        let set = move |clip: &mut AudioClip, seconds: f32| {
            let from = origin.as_ref().unwrap_or(clip);
            *clip = with_file_start(from, &file, clock, f64::from(seconds));
        };
        self.change(TRIM_LABEL, change, set, cx);
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut AudioClip, V),
        cx: &mut Context<Self>,
    ) {
        let Some(clip) = self.clip.clone() else {
            return;
        };
        self.edit
            .apply(&self.session, &clip, label, change, set, cx);
    }

    /// The gain line with a fade at each end, over the part of the file that plays, and its
    /// three handles: the fades at its corners, the gain hollow in the middle.
    fn gain_line(
        &self,
        state: &AudioClip,
        file: &Info,
        cx: &mut Context<Self>,
    ) -> (Vec<Point<f32>>, Vec<Handle>) {
        let length = file.seconds();
        let ms = |seconds: f64| (seconds * 1000.) as f32;
        let (start, end) = (
            state.file_start_seconds,
            state.file_end_seconds.unwrap_or(length),
        );
        let across = |seconds: f64| place(seconds as f32, length as f32);
        let up = gain_up(state.gain_db);
        let fade_in_end = start + f64::from(state.fade_in_ms) / 1000.;
        let fade_out_start = end - f64::from(state.fade_out_ms) / 1000.;
        let curve = vec![
            point(across(start), 0.),
            point(across(fade_in_end), up),
            point(across(fade_out_start), up),
            point(across(end), 0.),
        ];
        let gain_range = KnobRange::linear(GAIN_DB.0, GAIN_DB.1);
        let gain_axis = Axis::new(gain_range, state.gain_db.max(GAIN_DB.0), 0.);
        // A fade handle moves its fade across the whole file: its range is offset by where the
        // clip starts or ends in it, so the handle is where the fade ends.
        let fade_in_range = KnobRange::linear(-ms(start), ms(length - start));
        let fade_out_range = KnobRange::linear(-ms(end), ms(length - end));
        let (file_in, file_out) = (*file, *file);
        let fade_in_handle = Handle::new(
            "fade-in",
            Axis::new(fade_in_range, state.fade_in_ms, 0.),
            Axis::fixed(up),
        )
        .on_change(weak_callback(
            cx,
            move |card, change: ValueChange<Point<f32>>, cx| {
                let set =
                    |clip: &mut AudioClip, ms: f32| clip.fade_in_ms = fade_in(clip, &file_in, ms);
                card.change(FADE_IN_LABEL, change.map(|at| at.x), set, cx);
            },
        ));
        let fade_out_handle = Handle::new(
            "fade-out",
            Axis::new(fade_out_range, -state.fade_out_ms, 0.),
            Axis::fixed(up),
        )
        .on_change(weak_callback(
            cx,
            move |card, change: ValueChange<Point<f32>>, cx| {
                let set = |clip: &mut AudioClip, ms: f32| {
                    clip.fade_out_ms = fade_out(clip, &file_out, -ms)
                };
                card.change(FADE_OUT_LABEL, change.map(|at| at.x), set, cx);
            },
        ));
        let middle = (across(fade_in_end) + across(fade_out_start)) / 2.;
        let gain_handle = Handle::new("gain", Axis::fixed(middle), gain_axis)
            .hollow(true)
            .on_change(weak_callback(
                cx,
                |card, change: ValueChange<Point<f32>>, cx| {
                    let set = |clip: &mut AudioClip, db: f32| clip.gain_db = db;
                    card.change(GAIN_LABEL, change.map(|at| at.y), set, cx);
                },
            ));
        (curve, vec![fade_in_handle, gain_handle, fade_out_handle])
    }

    fn display(&self, state: &AudioClip, file: &Info, cx: &mut Context<Self>) -> WaveformDisplay {
        let project = self.session.read(cx).project();
        let (assets, clock) = (project.assets().clone(), project.clock().clone());
        let overview = Waveforms::overview(&assets, &state.asset, cx);
        let length = file.seconds();
        let end = state.file_end_seconds.unwrap_or(length);
        let (curve, handles) = self.gain_line(state, file, cx);
        let caption = format!(
            "{} to {} of {}",
            seconds_label(state.file_start_seconds),
            seconds_label(end),
            seconds_label(length)
        );
        let file = *file;
        let display = WaveformDisplay::new("clip-display", DISPLAY_WIDTH, overview, length as f32)
            .trim(state.file_start_seconds as f32, end as f32)
            .on_start(weak_callback(cx, move |card, change: ValueChange, cx| {
                card.change_start(change, file, &clock, cx);
            }))
            .on_end(weak_callback(cx, move |card, change: ValueChange, cx| {
                let set = |clip: &mut AudioClip, seconds: f32| {
                    *clip = with_file_end(clip, &file, f64::from(seconds));
                };
                card.change(TRIM_LABEL, change, set, cx);
            }))
            .playhead(self.playing_at)
            .curve(curve)
            .caption(caption);
        handles.into_iter().fold(display, WaveformDisplay::handle)
    }

    fn knobs(&self, state: &AudioClip, file: &Info, cx: &mut Context<Self>) -> [Knob; 5] {
        let played = played_ms(state, file).max(1.);
        let length = file.seconds() as f32;
        let end = state.file_end_seconds.map_or(length, |end| end as f32);
        let (file_in, file_out, file_trim) = (*file, *file, *file);
        let clock = self.session.read(cx).project().clock().clone();
        let gain = Knob::new("gain")
            .range(KnobRange::linear(GAIN_DB.0, GAIN_DB.1))
            .value(state.gain_db.max(GAIN_DB.0))
            .default_value(0.)
            .label("Gain")
            .readout(gain_label(state.gain_db))
            .on_change(weak_callback(cx, |card, change, cx| {
                let set = |clip: &mut AudioClip, db: f32| clip.gain_db = db;
                card.change(GAIN_LABEL, change, set, cx);
            }));
        let fade_in_knob = Knob::new("fade-in")
            .range(KnobRange::linear(0., played))
            .value(state.fade_in_ms)
            .default_value(0.)
            .label("Fade in")
            .readout(time_label(state.fade_in_ms))
            .on_change(weak_callback(cx, move |card, change, cx| {
                let set = |clip: &mut AudioClip, ms| clip.fade_in_ms = fade_in(clip, &file_in, ms);
                card.change(FADE_IN_LABEL, change, set, cx);
            }));
        let fade_out_knob = Knob::new("fade-out")
            .range(KnobRange::linear(0., played))
            .value(state.fade_out_ms)
            .default_value(0.)
            .label("Fade out")
            .readout(time_label(state.fade_out_ms))
            .on_change(weak_callback(cx, move |card, change, cx| {
                let set =
                    |clip: &mut AudioClip, ms| clip.fade_out_ms = fade_out(clip, &file_out, ms);
                card.change(FADE_OUT_LABEL, change, set, cx);
            }));
        let start = Knob::new("start")
            .range(KnobRange::linear(0., length))
            .value(state.file_start_seconds as f32)
            .default_value(0.)
            .label("Start")
            .readout(seconds_label(state.file_start_seconds))
            .on_change(weak_callback(cx, move |card, change, cx| {
                card.change_start(change, file_trim, &clock, cx);
            }));
        let end_knob = Knob::new("end")
            .range(KnobRange::linear(0., length))
            .value(end)
            .default_value(length)
            .label("End")
            .readout(seconds_label(f64::from(end)))
            .on_change(weak_callback(cx, move |card, change, cx| {
                let set = |clip: &mut AudioClip, seconds: f32| {
                    *clip = with_file_end(clip, &file_trim, f64::from(seconds));
                };
                card.change(TRIM_LABEL, change, set, cx);
            }));
        [gain, fade_in_knob, fade_out_knob, start, end_knob]
    }
}

/// The title of the card: the file name, plain.
fn title(text: impl Into<SharedString>) -> AnyElement {
    div()
        .min_w_0()
        .truncate()
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
        .into_any_element()
}

impl Render for ClipCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = self.session.read(cx).project();
        let shown = self.clip.as_ref().and_then(|clip| {
            let state = project.state(clip)?.clone();
            let file = sound_media::cached(project.assets(), &state.asset);
            Some((state, file))
        });
        let muted = cx.theme().gray_800;
        let quiet = |text: String| {
            div()
                .min_w_0()
                .flex_1()
                .text_size(px(12.))
                .line_height(px(14.))
                .text_color(muted)
                .child(text)
        };
        let Some((state, file)) = shown else {
            return DeviceCard::new(CARD_ID, title("Clip"))
                .w(px(NARROW_CARD_WIDTH))
                .child(quiet(EMPTY.to_string()))
                .into_any_element();
        };
        let name = state.asset.to_string();
        // A clip whose file is missing or does not play keeps its place, and says so here as
        // on the timeline. Its file is known from memory only; until then the card waits.
        let says = match file {
            Cached::Plays(file) => Ok(file),
            Cached::Missing => Err(format!("{name} is missing")),
            Cached::DoesNotPlay(_) => Err(format!("{name} does not play")),
            // Asking for the waveform asks a background thread what the file is, and the
            // cache of waveforms tells this card when it knows.
            Cached::Unknown => {
                let assets = self.session.read(cx).project().assets().clone();
                Waveforms::overview(&assets, &state.asset, cx);
                Err(String::new())
            }
        };
        let file = match says {
            Ok(file) => file,
            Err(says) => {
                return DeviceCard::new(CARD_ID, title(name))
                    .w(px(PLAIN_CARD_WIDTH))
                    .child(quiet(says))
                    .into_any_element();
            }
        };
        let display = self.display(&state, &file, cx);
        let [gain, fade_in_knob, fade_out_knob, start, end] = self.knobs(&state, &file, cx);
        let expand = cx.listener(|card, _, _, cx| card.set_expanded(!card.expanded, cx));
        DeviceCard::new(CARD_ID, title(name))
            .expand(self.expanded, expand)
            .display(display)
            .column(Column::new().top(gain).bottom(fade_in_knob))
            .column(Column::new().bottom(fade_out_knob))
            .hidden_column(Column::new().top(start).bottom(end))
            .into_any_element()
    }
}
