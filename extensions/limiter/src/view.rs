//! The card of the limiter: the display the master's limiter has too, the last four seconds of
//! what the limiter sent out under the ceiling line and how much it took, then Gain and Release,
//! Ceiling and Lookahead. Four controls fit, so the card has nothing behind expand. The rack
//! gives the view a [`CardFrame`]: the picker of the slot as the title, and the power and close
//! icons.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change goes
//! through the session, by [`DeviceEdit`]: a drag of a knob or of the ceiling handle is one
//! gesture and one undo step, a key step, a reset or a pick is one commit. The ranges, the defaults
//! and the travel of each knob come from the [`Parameter`](crate::Parameter)s of the crate. A
//! number that an automation lane of the track moves shows the value that plays on its knob, and
//! does not drag ([`Lanes`]).

use gpui::{Context, Entity, Point, Task, Window, div, prelude::*};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{ParameterKnob, decibels_readout, milliseconds_readout};
use sound_ui::components::limiter_display::LimiterHistory;
use sound_ui::{
    DeviceEdit, DeviceView, Devices, OfferGroup, Session, Views, every_poll, weak_callback,
};

use crate::{CEILING, GAIN, Limiter, LimiterState, Lookahead, Meters, RELEASE};

/// The name the rack puts on the card of a limiter.
pub const NAME: &str = "Limiter";

/// Registers the view of the `limiter` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(LimiterView::new);
    devices.built_in::<LimiterState>(
        NAME,
        OfferGroup::Dynamics,
        "device-limiter",
        crate::EXTENSION,
        "This project does not load the limiter.",
    );
}

/// A knob of the card.
type Control = ParameterKnob<LimiterState>;

const GAIN_KNOB: Control = Control::new(&GAIN, "Gain", "Change gain", decibels_readout);
const CEILING_KNOB: Control = Control::new(&CEILING, "Ceiling", "Change ceiling", decibels_readout);
const RELEASE_KNOB: Control =
    Control::new(&RELEASE, "Release", "Change release", milliseconds_readout);

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
    device: DeviceEdit<LimiterState>,
    frame: CardFrame,
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
        cx.subscribe(&session, |view, _, event, cx| {
            if let ProjectEvent::Changed(id) = event
                && id == view.device.instance.id()
            {
                view.show_lookahead(cx);
            }
        })
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
                let change = ValueChange::Set(*lookahead);
                view.device.change("Change lookahead", change, set, cx);
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
            device: DeviceEdit::new(session, limiter, Limiter::AUTOMATION, cx),
            frame,
            lookahead,
            history: LimiterHistory::new(CEILING_KNOB.range()),
            _metering: every_poll(cx, Self::read_meters),
        }
    }

    /// Puts the lookahead of the record in its select, after any change of the record: a pick,
    /// an undo or an outside edit.
    fn show_lookahead(&mut self, cx: &mut Context<Self>) {
        let device = &self.device;
        let state = device.session.read(cx).project().state(&device.instance);
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
        let device = &self.device;
        let (project, id) = (device.session.read(cx).project(), device.instance.id());
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
}

impl DeviceView for LimiterView {
    type State = LimiterState;

    fn device(&mut self) -> &mut DeviceEdit<LimiterState> {
        &mut self.device
    }
}

impl Render for LimiterView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.device.state(cx) else {
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
                    view.device.change(CEILING_KNOB.undo_label, change, set, cx);
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
                    .top(self.device.knob(GAIN_KNOB, &state, cx))
                    .bottom(self.device.knob(RELEASE_KNOB, &state, cx)),
            )
            .column(
                Column::new()
                    .top(self.device.knob(CEILING_KNOB, &state, cx))
                    .bottom(lookahead),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lookahead_has_a_row_of_the_select() {
        for lookahead in Lookahead::ALL {
            let value = lookahead_value(lookahead);
            assert_eq!(value, lookahead.milliseconds().to_string());
        }
    }
}
