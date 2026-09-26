//! The master panel: the sum of every track, in the panel below the timeline where a track
//! panel shows. The header column holds the volume of the master on its meter, and the rack
//! holds the card of the limiter, which is part of the master and cannot be taken off, so it has
//! expand and power and no close.
//!
//! The limiter's display shows the last four seconds of what the master sent out, in green
//! under the ceiling line, and how much the limiter took, hanging from the top. The handle at
//! the right end of the ceiling line drags it up and down; the Ceiling knob is the way to it from
//! the keys. Every control edits the record of the arrangement through the session, as one undo
//! step, and a file edit of the same record shows at once.

use std::collections::VecDeque;

use gpui::{
    App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Pixels, Point,
    SharedString, Task, Window, canvas, div, fill, point, prelude::*, px, size,
};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::device_card::{Column, DeviceCard};
use sound_ui::components::display::{Axis, Display, Handle, INSET_HEIGHT};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::meter::GainReduction;
use sound_ui::components::volume::Volume;
use sound_ui::metering::decibels;
use sound_ui::{
    ActiveTheme, ControlEdit, Metering, Session, every_poll, weak_action, weak_callback,
};

use super::layout::HEADER_WIDTH;
use super::track_panel::{RACK_LEFT, RACK_TOP, ROW_TOP, TITLE_MIDDLE, VOLUME_LEFT};
use crate::{ArrangementState, LimiterState, MasterState};

/// The display of the limiter: its card is 352 pt with two columns of cells.
const DISPLAY_WIDTH: f32 = 200.;
/// Columns of the history, and the polls each one gathers: 50 columns of 80 ms, four seconds.
const COLUMNS: usize = 50;
const POLLS_PER_COLUMN: u32 = 5;
/// Where the ceiling handle sits across the display.
const HANDLE_ACROSS: f32 = 0.96;

/// What the panel asks of the view that holds it.
pub enum MasterPanelEvent {
    /// The close control.
    Close,
}

/// One column of the history: the loudest the master sent out and the most the limiter took,
/// in dB, over its 80 ms.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Moment {
    peak_db: f32,
    reduction_db: f32,
}

impl Moment {
    const QUIET: Self = Self {
        peak_db: f32::NEG_INFINITY,
        reduction_db: 0.0,
    };

    fn is_quiet(&self) -> bool {
        self.peak_db < LimiterState::CEILING_DB.0 && self.reduction_db <= 0.0
    }
}

/// The last four seconds, and the column being gathered.
#[derive(Default)]
struct History {
    moments: VecDeque<Moment>,
    gathering: Option<Moment>,
    polls: u32,
}

impl History {
    /// One poll. Whether a column was finished that changes what the display shows.
    fn read(&mut self, peak: f32, reduction: f32) -> bool {
        let now = Moment {
            peak_db: decibels(peak),
            // The reduction is kept as the factor the sound was above the output.
            reduction_db: decibels(reduction).max(0.0),
        };
        let gathering = self.gathering.get_or_insert(Moment::QUIET);
        gathering.peak_db = gathering.peak_db.max(now.peak_db);
        gathering.reduction_db = gathering.reduction_db.max(now.reduction_db);
        self.polls += 1;
        if self.polls < POLLS_PER_COLUMN {
            return false;
        }
        self.polls = 0;
        let moment = self.gathering.take().unwrap_or(Moment::QUIET);
        // At rest, a quiet column in a quiet history changes nothing and costs no frame.
        if moment.is_quiet() && self.moments.iter().all(Moment::is_quiet) {
            return false;
        }
        if self.moments.len() == COLUMNS {
            self.moments.pop_front();
        }
        self.moments.push_back(moment);
        true
    }

    /// The largest reduction of the last column, for the line under the display.
    fn reduction_now(&self) -> f32 {
        self.moments
            .back()
            .map_or(0.0, |moment| moment.reduction_db)
    }
}

pub struct MasterPanel {
    session: Entity<Session>,
    arrangement: Instance<ArrangementState>,
    /// The gesture of a drag of the volume, a knob or the ceiling handle.
    edit: ControlEdit,
    /// Whether the card shows the lookahead. Interface state: nothing saves it.
    expanded: bool,
    /// The meter of the volume: what the master sends out.
    metering: Metering,
    history: History,
    /// Not a tab stop. It tells whether the focus is inside the panel.
    focus_handle: FocusHandle,
    close_focus: FocusHandle,
    _metering: Task<()>,
}

impl EventEmitter<MasterPanelEvent> for MasterPanel {}

impl MasterPanel {
    pub(super) fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |panel, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == panel.arrangement.id() => cx.notify(),
            ProjectEvent::Deleted(id) if id == panel.arrangement.id() => {
                panel.edit.finish(&panel.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|panel, cx| panel.edit.finish(&panel.session, cx))
            .detach();
        Self {
            session,
            arrangement,
            edit: ControlEdit::default(),
            expanded: false,
            metering: Metering::default(),
            history: History::default(),
            focus_handle: cx.focus_handle(),
            close_focus: cx.focus_handle().tab_stop(true),
            _metering: every_poll(cx, |panel: &mut Self, cx| panel.read_meters(cx)),
        }
    }

    /// One poll of the meters: the peaks of the master feed both its meter and the history of
    /// the display, so they are taken once here. Its timer calls it; a snapshot calls it to
    /// skip the wait.
    pub fn read_meters(&mut self, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let id = self.arrangement.id();
        let peaks = crate::master_peaks(project, id).map_or([0.0; 2], |peaks| peaks.take());
        let reduction = crate::reduction_peaks(project, id).map_or(0.0, |peaks| peaks.take()[0]);
        let meter = self.metering.read_amplitudes(peaks);
        let history = self.history.read(peaks[0].max(peaks[1]), reduction);
        if meter || history {
            cx.notify();
        }
    }

    pub fn arrangement(&self) -> &Instance<ArrangementState> {
        &self.arrangement
    }

    /// One change of a control on the master, named `label` in the undo history.
    fn apply<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut MasterState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, arrangement) = (&self.session, &self.arrangement);
        let set = |state: &mut ArrangementState, value| set(&mut state.master, value);
        self.edit
            .apply(session, arrangement, label, change, set, cx);
    }

    fn knob(
        &self,
        control: &'static LimiterKnob,
        limiter: &LimiterState,
        cx: &mut Context<Self>,
    ) -> Knob {
        let value = (control.get)(limiter);
        let (min, max) = control.range;
        let range = match control.logarithmic {
            true => KnobRange::logarithmic(min, max),
            false => KnobRange::linear(min, max),
        };
        Knob::new(control.field)
            .range(range)
            .value(value)
            .default_value((control.get)(&LimiterState::default()))
            .label(control.label)
            .readout(format!("{} {}", short(value), control.unit))
            .on_change(weak_callback(cx, move |panel, change, cx| {
                let set = |master: &mut MasterState, value: f32| {
                    (control.set)(&mut master.limiter, value.clamp(min, max))
                };
                panel.apply(control.undo_label, change, set, cx);
            }))
    }

    /// The history under the ceiling line, whose handle drags the ceiling.
    fn display(&self, limiter: &LimiterState, cx: &mut Context<Self>) -> Display {
        let (min, max) = LimiterState::CEILING_DB;
        let range = KnobRange::linear(min, max);
        let ceiling = range.position(limiter.ceiling_db);
        let handle = Handle::new(
            "ceiling",
            Axis::fixed(HANDLE_ACROSS),
            Axis::new(
                range,
                limiter.ceiling_db,
                LimiterState::default().ceiling_db,
            ),
        )
        .on_change(weak_callback(
            cx,
            move |panel, change: ValueChange<Point<f32>>, cx| {
                let set = |master: &mut MasterState, place: Point<f32>| {
                    master.limiter.ceiling_db = place.y.clamp(min, max);
                };
                panel.apply(CEILING.undo_label, change, set, cx);
            },
        ));
        let theme = cx.theme();
        let colors = (theme.green, theme.gray_950);
        let moments: Vec<Moment> = self.history.moments.iter().copied().collect();
        let history = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_history(bounds, &moments, range, colors, window),
        )
        .absolute()
        .top_0()
        .left_0()
        .w(px(DISPLAY_WIDTH))
        .h(px(INSET_HEIGHT));
        let caption = format!(
            "Ceiling {} dB · GR {}",
            short(limiter.ceiling_db),
            reduction_readout(self.history.reduction_now())
        );
        Display::new("limiter", DISPLAY_WIDTH)
            .curve([point(0., ceiling), point(1., ceiling)])
            .handle(handle)
            .caption(caption)
            .child(history)
    }

    fn card(&self, master: &MasterState, cx: &mut Context<Self>) -> DeviceCard {
        let limiter = &master.limiter;
        let title = div()
            .font_weight(FontWeight::MEDIUM)
            .line_height(px(20.))
            .child("Limiter");
        let expand = cx.listener(|panel, _, _, cx| {
            panel.expanded = !panel.expanded;
            cx.notify();
        });
        let power = cx.listener(|panel, _, _, cx| {
            let project = panel.session.read(cx).project();
            let Some(bypass) = project
                .state(&panel.arrangement)
                .map(|state| state.master.limiter.bypass)
            else {
                return;
            };
            let label = match bypass {
                true => "Turn on Limiter",
                false => "Turn off Limiter",
            };
            let change = ValueChange::Set(!bypass);
            panel.apply(
                label,
                change,
                |master, bypass| master.limiter.bypass = bypass,
                cx,
            );
        });
        DeviceCard::new("card-limiter", title)
            .expand(self.expanded, expand)
            .power(!limiter.bypass, power)
            .display(self.display(limiter, cx))
            .column(
                Column::new()
                    .top(self.knob(&GAIN, limiter, cx))
                    .bottom(self.knob(&RELEASE, limiter, cx)),
            )
            .column(Column::new().top(self.knob(&CEILING, limiter, cx)))
            .hidden_column(Column::new().top(self.knob(&LOOKAHEAD, limiter, cx)))
    }
}

/// The bars of the history: the peaks in green on the scale of the ceiling line, and the
/// reduction hanging from the top, 24 dB for the whole height.
fn paint_history(
    bounds: Bounds<Pixels>,
    moments: &[Moment],
    range: KnobRange,
    (green, reduction): (gpui::Hsla, gpui::Hsla),
    window: &mut Window,
) {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let inset = 6.;
    let step = (width - 2. * inset) / COLUMNS as f32;
    let bar = (step - 1.).max(1.);
    // The newest column at the right, as time goes.
    let first = COLUMNS - moments.len();
    for (index, moment) in moments.iter().enumerate() {
        let x = inset + (first + index) as f32 * step;
        let up = match moment.peak_db.is_finite() {
            true => range.position(moment.peak_db),
            false => 0.,
        };
        if up > 0. {
            let top = height * (1. - up);
            let body = Bounds::new(
                bounds.origin + point(px(x), px(top)),
                size(px(bar), px(height - top)),
            );
            window.paint_quad(fill(body, green));
        }
        // Narrower than the level and over it, so the two read apart where they meet under
        // the ceiling.
        let down = (moment.reduction_db / GainReduction::RANGE_DB).clamp(0., 1.) * height;
        if down > 0. {
            let thin = (bar / 3.).max(1.);
            let left = x + (bar - thin) / 2.;
            let body = Bounds::new(
                bounds.origin + point(px(left), px(0.)),
                size(px(thin), px(down)),
            );
            window.paint_quad(fill(body, reduction));
        }
    }
}

/// The reduction under the display, to a tenth of a dB: `0 dB`, `-4.1 dB`.
fn reduction_readout(db: f32) -> String {
    match db < 0.05 {
        true => "0 dB".into(),
        false => format!("-{db:.1} dB"),
    }
}

/// A knob of the limiter: a field of its record, its range and how it reads.
struct LimiterKnob {
    field: &'static str,
    label: &'static str,
    undo_label: &'static str,
    unit: &'static str,
    range: (f32, f32),
    logarithmic: bool,
    get: fn(&LimiterState) -> f32,
    set: fn(&mut LimiterState, f32),
}

const GAIN: LimiterKnob = LimiterKnob {
    field: "limiter-gain_db",
    label: "Gain",
    undo_label: "Change limiter gain",
    unit: "dB",
    range: LimiterState::GAIN_DB,
    logarithmic: false,
    get: |limiter| limiter.gain_db,
    set: |limiter, value| limiter.gain_db = value,
};
const CEILING: LimiterKnob = LimiterKnob {
    field: "limiter-ceiling_db",
    label: "Ceiling",
    undo_label: "Change ceiling",
    unit: "dB",
    range: LimiterState::CEILING_DB,
    logarithmic: false,
    get: |limiter| limiter.ceiling_db,
    set: |limiter, value| limiter.ceiling_db = value,
};
const RELEASE: LimiterKnob = LimiterKnob {
    field: "limiter-release_ms",
    label: "Release",
    undo_label: "Change release",
    unit: "ms",
    range: LimiterState::RELEASE_MS,
    // Times are heard in ratios.
    logarithmic: true,
    get: |limiter| limiter.release_ms,
    set: |limiter, value| limiter.release_ms = value,
};
const LOOKAHEAD: LimiterKnob = LimiterKnob {
    field: "limiter-lookahead_ms",
    label: "Lookahead",
    undo_label: "Change lookahead",
    unit: "ms",
    range: LimiterState::LOOKAHEAD_MS,
    // From 0, so not in ratios.
    logarithmic: false,
    get: |limiter| limiter.lookahead_ms,
    set: |limiter, value| limiter.lookahead_ms = value,
};

impl Focusable for MasterPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MasterPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let master = self
            .session
            .read(cx)
            .project()
            .state(&self.arrangement)
            .map(|state| state.master.clone());
        let theme = cx.theme();
        let (background, hairline, text, ring) = (
            theme.gray_100,
            theme.alpha_at(0.05),
            theme.gray_950,
            theme.gray_800,
        );
        let close = Button::icon_only("close-master-panel", "x")
            .opacity(0.6)
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Xs)
            .focus_handle(&self.close_focus)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(MasterPanelEvent::Close)));
        let volume = master.as_ref().map(|master| {
            let volume = Volume::new("master-gain_db", master.gain_db)
                .level(self.metering.level())
                .on_clear_clip(weak_action(cx, |panel: &mut Self, cx| {
                    panel.metering.clear_clip();
                    cx.notify();
                }))
                .on_change(weak_callback(cx, |panel, change: ValueChange, cx| {
                    let set = |master: &mut MasterState, db: f32| {
                        master.gain_db = match db.is_nan() {
                            true => f32::NEG_INFINITY,
                            false => db.min(MasterState::MAX_GAIN_DB),
                        };
                    };
                    panel.apply("Change master volume", change, set, cx);
                }));
            div()
                .absolute()
                .left(px(VOLUME_LEFT))
                .top(px(ROW_TOP))
                .child(volume)
        });
        let card = master.as_ref().map(|master| self.card(master, cx));
        // The header column: a ring where a track has its dot, the name, the close icon, and
        // the volume of the master on the rows of the cards.
        let header = div()
            .relative()
            .flex_none()
            .w(px(HEADER_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(hairline)
            .child(
                div()
                    .absolute()
                    .left(px(24.))
                    .top(px(TITLE_MIDDLE - 4.))
                    .size(px(8.))
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(ring),
            )
            .child(
                div()
                    .absolute()
                    .left(px(44.))
                    .top(px(TITLE_MIDDLE - 10.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(text)
                    .child(SharedString::from(MASTER_NAME)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(TITLE_MIDDLE - 12.))
                    .left(px(HEADER_WIDTH - 8. - 24.))
                    .child(close),
            )
            .children(volume);
        div()
            .size_full()
            .relative()
            .flex()
            .bg(background)
            .track_focus(&self.focus_handle)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(1.))
                    .bg(hairline),
            )
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_start()
                    .pt(px(RACK_TOP))
                    .px(px(RACK_LEFT))
                    .children(card),
            )
    }
}

/// What the master row and its panel are called.
pub const MASTER_NAME: &str = "Master";

#[cfg(test)]
mod tests {
    use super::{COLUMNS, History, POLLS_PER_COLUMN};

    #[test]
    fn a_column_gathers_the_loudest_of_its_polls_and_rest_costs_nothing() {
        let mut history = History::default();
        // Silence from the start never draws.
        for _ in 0..3 * POLLS_PER_COLUMN {
            assert!(!history.read(0.0, 0.0));
        }
        let mut finished = Vec::new();
        for poll in 0..POLLS_PER_COLUMN {
            finished.push(history.read(0.1 * poll as f32, 2.0));
        }
        assert_eq!(finished.iter().filter(|done| **done).count(), 1);
        let moment = history.moments[0];
        assert!((moment.peak_db - 20.0 * 0.4_f32.log10()).abs() < 1e-4);
        assert!((moment.reduction_db - 6.0206).abs() < 1e-3);
        // Four seconds at most.
        for _ in 0..(COLUMNS as u32 + 10) * POLLS_PER_COLUMN {
            history.read(0.5, 1.0);
        }
        assert_eq!(history.moments.len(), COLUMNS);
    }
}
