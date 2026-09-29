//! The card of the limiter: the display the master's limiter has too, the last four seconds of
//! what the limiter sent out under the ceiling line and how much it took, then Gain and Release,
//! Ceiling and Lookahead. Four controls fit, so the card has nothing behind expand. The rack
//! gives the view a [`CardFrame`]: the picker of the slot as the title, and the power and close
//! icons.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of the ceiling handle is one
//! gesture and one undo step, a key step, a reset or a pick is one commit. The ranges and the
//! defaults come from the [`Parameter`]s of the crate.

use gpui::{Context, Entity, Point, Task, Window, div, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::limiter_display::LimiterHistory;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Session, Views, every_poll, weak_callback};

use crate::{CEILING, GAIN, LimiterState, Lookahead, Meters, Parameter, RELEASE};

/// The name the rack puts on the card of a limiter.
pub const NAME: &str = "Limiter";

/// Registers the view of the `limiter` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(LimiterView::new);
    devices.describe::<LimiterState>(|_| DeviceLabel {
        key: LimiterState::TOOL.into(),
        name: NAME.into(),
    });
}

/// A knob of the card.
struct Control {
    parameter: &'static Parameter,
    label: &'static str,
    undo_label: &'static str,
    scale: KnobRange,
    unit: &'static str,
}

const GAIN_KNOB: Control = Control {
    parameter: &GAIN,
    label: "Gain",
    undo_label: "Change gain",
    scale: KnobRange::linear(GAIN.min, GAIN.max),
    unit: "dB",
};
const CEILING_KNOB: Control = Control {
    parameter: &CEILING,
    label: "Ceiling",
    undo_label: "Change ceiling",
    scale: KnobRange::linear(CEILING.min, CEILING.max),
    unit: "dB",
};
const RELEASE_KNOB: Control = Control {
    parameter: &RELEASE,
    label: "Release",
    undo_label: "Change release",
    // Times are heard in ratios.
    scale: KnobRange::logarithmic(RELEASE.min, RELEASE.max),
    unit: "ms",
};

/// Every knob, in the order of the card.
#[cfg(test)]
const KNOBS: [&Control; 3] = [&GAIN_KNOB, &RELEASE_KNOB, &CEILING_KNOB];

/// The value of a row of the lookahead select for each lookahead. The select says the number
/// and its cell says `ms`, as on the compressor.
const LOOKAHEADS: [(Lookahead, &str); 3] = [
    (Lookahead::Off, "0"),
    (Lookahead::One, "1"),
    (Lookahead::Five, "5"),
];

fn lookahead_value(lookahead: Lookahead) -> &'static str {
    LOOKAHEADS
        .iter()
        .find(|(value, _)| *value == lookahead)
        .map_or("", |(_, label)| label)
}

/// The width of the open list of the lookahead select.
const LOOKAHEAD_MENU_WIDTH: f32 = 96.;

pub struct LimiterView {
    session: Entity<Session>,
    limiter: Instance<LimiterState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of the ceiling handle.
    edit: ControlEdit,
    /// The select of the lookahead. It is a view of its own because it opens a list; it shows
    /// what the record says, see [`Self::show_lookahead`].
    lookahead: Entity<DropdownMenu>,
    history: LimiterHistory,
    _metering: Task<()>,
}

impl LimiterView {
    pub fn new(
        session: Entity<Session>,
        limiter: Instance<LimiterState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.limiter.id() => {
                view.show_lookahead(cx);
                cx.notify();
            }
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.limiter.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let lookahead = cx.new(|cx| {
            let rows = LOOKAHEADS.map(|(_, label)| MenuItem::new(label, label));
            let rows = vec![MenuEntry::Group(
                MenuGroup::new().label("Lookahead, ms").items(rows),
            )];
            let shown = session.read(cx).project().state(&limiter);
            let shown = shown.map_or(Lookahead::One, |state| state.lookahead);
            DropdownMenu::new("Lookahead", rows, cx)
                .selected(lookahead_value(shown))
                .trigger(Trigger::Select)
                .width(LOOKAHEAD_MENU_WIDTH)
                .debug_name("lookahead")
        });
        cx.subscribe(&lookahead, |view, _, MenuPicked(value), cx| {
            let picked = LOOKAHEADS
                .iter()
                .find(|(_, label)| *label == value.as_ref());
            if let Some((lookahead, _)) = picked {
                let set = |state: &mut LimiterState, lookahead| state.lookahead = lookahead;
                view.change("Change lookahead", ValueChange::Set(*lookahead), set, cx);
            }
        })
        .detach();
        // What the limiter did before this card was made, such as while its panel was closed,
        // is not what it does now.
        let project = session.read(cx).project();
        for name in [Meters::OUTPUT, Meters::REDUCTION] {
            if let Some(peaks) = project.peaks(limiter.id(), name) {
                peaks.take();
            }
        }
        Self {
            session,
            limiter,
            frame,
            edit: ControlEdit::default(),
            lookahead,
            history: LimiterHistory::new(CEILING_KNOB.scale),
            _metering: every_poll(cx, Self::read_meters),
        }
    }

    /// Puts the lookahead of the record in its select, after any change of the record: a pick,
    /// an undo or an outside edit.
    fn show_lookahead(&mut self, cx: &mut Context<Self>) {
        let state = self.session.read(cx).project().state(&self.limiter);
        let Some(shown) = state.map(|state| lookahead_value(state.lookahead)) else {
            return;
        };
        self.lookahead.update(cx, |select, cx| {
            if select.value().map(AsRef::as_ref) != Some(shown) {
                select.set_selected(shown, cx);
            }
        });
    }

    /// Takes what the limiter sent out and took since the last look, and draws again when that
    /// changes what the card shows. Called once per poll of the session.
    pub fn read_meters(&mut self, cx: &mut Context<Self>) {
        let (project, id) = (self.session.read(cx).project(), self.limiter.id());
        let take = |name| {
            project
                .peaks(id, name)
                .map_or([0.; 2], |peaks| peaks.take())
        };
        let [left, right] = take(Meters::OUTPUT);
        if self
            .history
            .read(left.max(right), take(Meters::REDUCTION)[0])
        {
            cx.notify();
        }
    }

    /// The most the limiter took in the last column of its display, in dB: what the line under
    /// the display says.
    pub fn reduction_db(&self) -> f32 {
        self.history.reduction_now()
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut LimiterState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, limiter) = (&self.session, &self.limiter);
        self.edit.apply(session, limiter, label, change, set, cx);
    }

    fn knob(
        &self,
        control: &'static Control,
        state: &LimiterState,
        cx: &mut Context<Self>,
    ) -> Knob {
        let value = (control.parameter.get)(state);
        Knob::new(control.parameter.field)
            .range(control.scale)
            .value(value)
            .default_value(control.parameter.default)
            .label(control.label)
            .readout(format!("{} {}", short(value), control.unit))
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = control.parameter.set;
                view.change(control.undo_label, change, set, cx);
            }))
    }
}

impl Render for LimiterView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self
            .session
            .read(cx)
            .project()
            .state(&self.limiter)
            .copied()
        else {
            return div().into_any_element();
        };
        let handle = self
            .history
            .handle(state.ceiling_db, CEILING.default)
            .on_change(weak_callback(
                cx,
                |view, change: ValueChange<Point<f32>>, cx| {
                    let set = |state: &mut LimiterState, place: Point<f32>| {
                        state.ceiling_db = place.y.clamp(CEILING.min, CEILING.max);
                    };
                    view.change(CEILING_KNOB.undo_label, change, set, cx);
                },
            ));
        let display = self
            .history
            .display("display", state.ceiling_db, handle, cx);
        let lookahead = Cell::new(self.lookahead.clone())
            .label("Lookahead")
            .value("ms");
        self.frame
            .card()
            .display(display)
            .column(
                Column::new()
                    .top(self.knob(&GAIN_KNOB, &state, cx))
                    .bottom(self.knob(&RELEASE_KNOB, &state, cx)),
            )
            .column(
                Column::new()
                    .top(self.knob(&CEILING_KNOB, &state, cx))
                    .bottom(lookahead),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults and both ends of every range, through the travel of its knob and back.
    #[test]
    fn every_knob_gives_the_ends_of_its_range_and_keeps_a_value_it_gave() {
        for control in KNOBS {
            let (range, parameter) = (control.scale, control.parameter);
            assert_eq!(range.value(0.0), parameter.min, "{}", parameter.field);
            assert_eq!(range.value(1.0), parameter.max, "{}", parameter.field);
            for value in [parameter.min, parameter.default, parameter.max] {
                let back = range.value(range.position(value));
                assert_eq!(back, value, "{}", parameter.field);
            }
        }
    }

    #[test]
    fn every_lookahead_has_a_row_of_the_select() {
        for lookahead in Lookahead::ALL {
            let value = lookahead_value(lookahead);
            assert_eq!(value, lookahead.milliseconds().to_string());
        }
    }
}
