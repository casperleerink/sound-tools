//! The card of a `region`: the file name as its title, the whole file in the waveform display
//! with the start and end lines that trim it, the gain line with a fade at each end, and a green
//! line while the playhead is inside the region. The line under it says which part of the file
//! plays: `2.1 s to 10.1 s of 14.6 s`. Its knobs are Gain, Fade in and Fade out, and behind
//! expand Start and End.

use gpui::{
    AnyElement, App, Context, Entity, FontWeight, Point, SharedString, Window, div, point, prelude::*,
    px,
};
use serde_json::Value;
use sound_core::{Clock, Frames, InstanceId, ProjectEvent, Ticks};
use sound_media::{Cached, Info};
use sound_ui::components::device_card::{CardFrame, Column, PLAIN_CARD_WIDTH};
use sound_ui::components::display::{Axis, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short, signed};
use sound_ui::components::waveform_display::{
    WaveformDisplay, clamped_end, clamped_start, latest_start, place,
};
use sound_ui::{ActiveTheme, ControlEdit, Session, Views, Waveforms, weak_callback};

use crate::region::{MAX_GAIN_DB, NAME, Record};

/// The display, so the card is 32 + 312 + 8 + 2 x 56 = 464 pt, and 537 expanded.
const DISPLAY_WIDTH: f32 = 312.;
/// The gains a knob and a handle give: -48 to +24 dB. A record may say less.
const GAIN_DB: (f32, f32) = (-48.0, MAX_GAIN_DB);
const TRIM_LABEL: &str = "Trim a region";
const FADE_IN_LABEL: &str = "Change fade in";
const FADE_OUT_LABEL: &str = "Change fade out";
const GAIN_LABEL: &str = "Change gain";

pub(crate) fn register(views: &mut Views) {
    views.register_json_card(NAME, RegionCard::new);
}

fn seconds_label(seconds: f64) -> String {
    format!("{} s", short(seconds as f32))
}

fn gain_label(db: f32) -> String {
    match db == f32::NEG_INFINITY {
        true => "−inf".to_string(),
        false => format!("{} dB", signed(db)),
    }
}

fn time_label(ms: f32) -> String {
    match ms < 1000.0 {
        true => format!("{} ms", short(ms)),
        false => format!("{} s", short(ms / 1000.0)),
    }
}

fn played_ms(region: &Record, file: &Info) -> f32 {
    let end = region.file_end_seconds.unwrap_or_else(|| file.seconds());
    ((end - region.file_start_seconds).max(0.0) * 1000.0) as f32
}

fn fade_in(region: &Record, file: &Info, ms: f32) -> f32 {
    let room = (played_ms(region, file) - region.fade_out_ms).max(0.0);
    ms.round().clamp(0.0, room)
}

fn fade_out(region: &Record, file: &Info, ms: f32) -> f32 {
    let room = (played_ms(region, file) - region.fade_in_ms).max(0.0);
    ms.round().clamp(0.0, room)
}

/// The region with its fades inside what it plays after a trim.
fn fitted(mut region: Record, file: &Info) -> Record {
    let played = played_ms(&region, file);
    region.fade_in_ms = region.fade_in_ms.min(played);
    region.fade_out_ms = region.fade_out_ms.min(played - region.fade_in_ms);
    region
}

/// The region playing its file from `seconds` on. The sound stays where it is in time: the region
/// starts that much later or earlier, on a tick, at tick 0 or after.
fn with_file_start(origin: &Record, file: &Info, clock: &Clock, seconds: f64) -> Record {
    let end = origin.file_end_seconds.unwrap_or_else(|| file.seconds());
    let place = clock.seconds_of(origin.start) - origin.file_start_seconds;
    let earliest = (-place).max(0.0);
    let latest = latest_start(earliest, end);
    let seconds = clamped_start(seconds, earliest, end);
    let at = |tick: Ticks| clock.seconds_of(tick) - place;
    let mut start = clock.tick_at_seconds(place + seconds);
    while start > Ticks(0) && at(start) > latest {
        start = Ticks(start.0 - 1);
    }
    let trimmed = Record {
        start,
        file_start_seconds: at(start).max(0.0),
        ..origin.clone()
    };
    fitted(trimmed, file)
}

fn with_file_end(region: &Record, file: &Info, seconds: f64) -> Record {
    let trimmed = Record {
        file_end_seconds: clamped_end(seconds, region.file_start_seconds, file),
        ..region.clone()
    };
    fitted(trimmed, file)
}

pub(crate) struct RegionCard {
    session: Entity<Session>,
    region: InstanceId,
    frame: CardFrame,
    edit: ControlEdit,
    /// Whether Start and End show. Interface state: not saved.
    expanded: bool,
    /// Where the green line is, in seconds of the file, while the playhead is inside.
    playing_at: Option<f32>,
    /// The region as it was when a drag of its start began, so no rounding adds up.
    start_origin: Option<Record>,
}

impl RegionCard {
    pub(crate) fn new(
        session: Entity<Session>,
        region: InstanceId,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |card, _, event, cx| match event {
            ProjectEvent::Changed(id) if *id == card.region => cx.notify(),
            ProjectEvent::Deleted(id) if *id == card.region => {
                card.edit.finish(&card.session, cx);
            }
            _ => {}
        })
        .detach();
        let waveforms = Waveforms::entity(cx);
        cx.observe(&waveforms, |_, _, cx| cx.notify()).detach();
        let playhead = session.read(cx).playhead().clone();
        cx.observe(&playhead, |card, _, cx| card.follow_playhead(cx))
            .detach();
        cx.on_release(|card, cx| card.edit.finish(&card.session, cx))
            .detach();
        let mut card = Self {
            session,
            region,
            frame,
            edit: ControlEdit::default(),
            expanded: false,
            playing_at: None,
            start_origin: None,
        };
        card.follow_playhead(cx);
        card
    }

    /// The record as the project keeps it.
    fn record(&self, cx: &App) -> Option<Record> {
        let text = self.session.read(cx).project().state_json(&self.region)?;
        serde_json::from_str(&text).ok()
    }

    fn follow_playhead(&mut self, cx: &mut Context<Self>) {
        let at = self.playhead_in_file(cx);
        if at != self.playing_at {
            self.playing_at = at;
            cx.notify();
        }
    }

    fn playhead_in_file(&self, cx: &mut Context<Self>) -> Option<f32> {
        let state = &self.record(cx)?;
        let session = self.session.read(cx);
        let project = session.project();
        let Cached::Plays(file) = sound_media::cached(project.assets(), &state.asset) else {
            return None;
        };
        let tick = session.playhead().read(cx).tick;
        let clock = project.clock();
        let region = crate::region::region(state);
        let end = clock.tick_at(Frames(
            clock.frame_of(state.start).0 + region.length(&file, clock.sample_rate()),
        ));
        if tick < state.start || tick >= end {
            return None;
        }
        let seconds =
            clock.seconds_of(tick) - clock.seconds_of(state.start) + state.file_start_seconds;
        let step = file.seconds() / f64::from(DISPLAY_WIDTH);
        Some(((seconds / step).round() * step) as f32)
    }

    fn change_start(&mut self, change: ValueChange, file: Info, clock: &Clock, cx: &mut Context<Self>) {
        let live = self.record(cx);
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
        let set = move |region: &mut Record, seconds: f32| {
            let from = origin.as_ref().unwrap_or(region);
            *region = with_file_start(from, &file, clock, f64::from(seconds));
        };
        self.change(TRIM_LABEL, change, set, cx);
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut Record, V),
        cx: &mut Context<Self>,
    ) {
        // The record is JSON in the project: read as a region, changed, and written back.
        let set = |state: &mut Value, value: V| {
            let Ok(mut region) = serde_json::from_value::<Record>(state.clone()) else {
                return;
            };
            set(&mut region, value);
            if let Ok(changed) = serde_json::to_value(&region) {
                *state = changed;
            }
        };
        self.edit
            .apply_json(&self.session, &self.region, label, change, set, cx);
    }

    /// The gain line with a fade at each end, and its three handles: the fades at its corners,
    /// the gain hollow in the middle.
    fn gain_line(&self, state: &Record, file: &Info, cx: &mut Context<Self>) -> (Vec<Point<f32>>, Vec<Handle>) {
        let length = file.seconds();
        let ms = |seconds: f64| (seconds * 1000.) as f32;
        let (start, end) = (state.file_start_seconds, state.file_end_seconds.unwrap_or(length));
        let across = |seconds: f64| place(seconds as f32, length as f32);
        let up = KnobRange::linear(GAIN_DB.0, GAIN_DB.1).position(state.gain_db);
        let fade_in_end = start + f64::from(state.fade_in_ms) / 1000.;
        let fade_out_start = end - f64::from(state.fade_out_ms) / 1000.;
        let curve = vec![
            point(across(start), 0.),
            point(across(fade_in_end), up),
            point(across(fade_out_start), up),
            point(across(end), 0.),
        ];
        let gain_axis = Axis::new(KnobRange::linear(GAIN_DB.0, GAIN_DB.1), state.gain_db.max(GAIN_DB.0), 0.);
        let fade_in_range = KnobRange::linear(-ms(start), ms(length - start));
        let fade_out_range = KnobRange::linear(-ms(end), ms(length - end));
        let (file_in, file_out) = (*file, *file);
        let fade_in_handle = Handle::new("fade-in", Axis::new(fade_in_range, state.fade_in_ms, 0.), Axis::fixed(up))
            .on_change(weak_callback(cx, move |card, change: ValueChange<Point<f32>>, cx| {
                let set = |region: &mut Record, ms: f32| region.fade_in_ms = fade_in(region, &file_in, ms);
                card.change(FADE_IN_LABEL, change.map(|at| at.x), set, cx);
            }));
        let fade_out_handle = Handle::new("fade-out", Axis::new(fade_out_range, -state.fade_out_ms, 0.), Axis::fixed(up))
            .on_change(weak_callback(cx, move |card, change: ValueChange<Point<f32>>, cx| {
                let set = |region: &mut Record, ms: f32| region.fade_out_ms = fade_out(region, &file_out, -ms);
                card.change(FADE_OUT_LABEL, change.map(|at| at.x), set, cx);
            }));
        let middle = (across(fade_in_end) + across(fade_out_start)) / 2.;
        let gain_handle = Handle::new("gain", Axis::fixed(middle), gain_axis)
            .hollow(true)
            .on_change(weak_callback(cx, |card, change: ValueChange<Point<f32>>, cx| {
                let set = |region: &mut Record, db: f32| region.gain_db = db;
                card.change(GAIN_LABEL, change.map(|at| at.y), set, cx);
            }));
        (curve, vec![fade_in_handle, gain_handle, fade_out_handle])
    }

    fn display(&self, state: &Record, file: &Info, cx: &mut Context<Self>) -> WaveformDisplay {
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
        let display = WaveformDisplay::new("region-display", DISPLAY_WIDTH, overview, length as f32)
            .trim(state.file_start_seconds as f32, end as f32)
            .on_start(weak_callback(cx, move |card, change: ValueChange, cx| {
                card.change_start(change, file, &clock, cx);
            }))
            .on_end(weak_callback(cx, move |card, change: ValueChange, cx| {
                let set = |region: &mut Record, seconds: f32| {
                    *region = with_file_end(region, &file, f64::from(seconds));
                };
                card.change(TRIM_LABEL, change, set, cx);
            }))
            .playhead(self.playing_at)
            .curve(curve)
            .caption(caption);
        handles.into_iter().fold(display, WaveformDisplay::handle)
    }

    fn knobs(&self, state: &Record, file: &Info, cx: &mut Context<Self>) -> [Knob; 5] {
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
                let set = |region: &mut Record, db: f32| region.gain_db = db;
                card.change(GAIN_LABEL, change, set, cx);
            }));
        let fade_in_knob = Knob::new("fade-in")
            .range(KnobRange::linear(0., played))
            .value(state.fade_in_ms)
            .default_value(0.)
            .label("Fade in")
            .readout(time_label(state.fade_in_ms))
            .on_change(weak_callback(cx, move |card, change, cx| {
                let set = |region: &mut Record, ms| region.fade_in_ms = fade_in(region, &file_in, ms);
                card.change(FADE_IN_LABEL, change, set, cx);
            }));
        let fade_out_knob = Knob::new("fade-out")
            .range(KnobRange::linear(0., played))
            .value(state.fade_out_ms)
            .default_value(0.)
            .label("Fade out")
            .readout(time_label(state.fade_out_ms))
            .on_change(weak_callback(cx, move |card, change, cx| {
                let set = |region: &mut Record, ms| region.fade_out_ms = fade_out(region, &file_out, ms);
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
                let set = |region: &mut Record, seconds: f32| {
                    *region = with_file_end(region, &file_trim, f64::from(seconds));
                };
                card.change(TRIM_LABEL, change, set, cx);
            }));
        [gain, fade_in_knob, fade_out_knob, start, end_knob]
    }
}

/// A title that is no device's picker: the file name, plain.
fn title(text: impl Into<SharedString>) -> AnyElement {
    div()
        .min_w_0()
        .truncate()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
        .into_any_element()
}

impl Render for RegionCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.record(cx) else {
            return div().into_any_element();
        };
        let project = self.session.read(cx).project();
        let name = state.asset.to_string();
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
        let card = self.frame.card().title(title(name.clone()));
        let says = match sound_media::cached(project.assets(), &state.asset) {
            Cached::Plays(file) => Ok(file),
            Cached::Missing => Err(format!("{name} is missing")),
            Cached::DoesNotPlay(_) => Err(format!("{name} does not play")),
            // Asking for the waveform asks a thread what the file is; the waveforms tell the card.
            Cached::Unknown => {
                let assets = project.assets().clone();
                Waveforms::overview(&assets, &state.asset, cx);
                Err(String::new())
            }
        };
        let file = match says {
            Ok(file) => file,
            Err(says) => return card.w(px(PLAIN_CARD_WIDTH)).child(quiet(says)).into_any_element(),
        };
        let display = self.display(&state, &file, cx);
        let [gain, fade_in_knob, fade_out_knob, start, end] = self.knobs(&state, &file, cx);
        let expand = cx.listener(|card, _, _, cx| {
            card.expanded = !card.expanded;
            cx.notify();
        });
        card.expand(self.expanded, expand)
            .display(display)
            .column(Column::new().top(gain).bottom(fade_in_knob))
            .column(Column::new().bottom(fade_out_knob))
            .hidden_column(Column::new().top(start).bottom(end))
            .into_any_element()
    }
}
