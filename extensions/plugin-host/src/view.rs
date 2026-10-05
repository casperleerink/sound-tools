//! The card of a hosted plugin in a rack.
//!
//! A plugin draws its whole interface in a window of its own. The card shows what the record
//! holds of it: at the left, a control that opens that window and closes it again, the list of
//! the plugin's parameters that puts one on the card and takes it off again, and the format and
//! the maker on the value line of the second row, `CLAP · <maker>`. Then a cell for each
//! parameter the record pins, two to a column, so the card grows wider with them and never
//! taller. A dropdown spans two cells, so the names of its steps fit. The rack gives the
//! frame of the card, whose title is the name of the plugin and where another one is picked.
//! Nothing is hidden, so there is no expand. The power and close icons of an effect come from
//! the rack, in the frame, because the rack keeps whether a slot is on. DESIGN.md says what
//! each control of a pin is and does.
//!
//! A stepped parameter is driven by the number of its step, never by its value, so what a
//! control writes is always the plugin's own value of a step.
//!
//! The knob of a pin an automation lane moves shows what the lane plays and does not drag, as
//! every automated knob (DESIGN.md), with the plugin's text for that value.
//!
//! The list of parameters and the plugin's text both call into the plugin, which a frame may
//! not do. So the view asks when the session tells it something changed, and draws from what it
//! was told. A lane that sweeps changes its value every frame, so the text of a value a lane
//! plays is asked for at most 15 times a second per pin, and the knob shows the last text it
//! got.
//!
//! A plugin this machine does not have shows what is wrong and the id the record names, so a
//! composer can see which plugin to install and an agent can be asked to correct the record.
//! The record itself is untouched, as everywhere else.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, Div, Entity, FocusHandle, SharedString, Task, Window, div, prelude::*, px,
};
use sound_core::{Instance, InstanceId, MAX_AUTOMATED, Project, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::cell::{CELL_WIDTH, Cell, ROW_HEIGHT};
use sound_ui::components::device_card::{
    BODY_VALUE_LINE, CARD_PADDING, CardFrame, Column, PLAIN_CARD_WIDTH,
};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::popover::Side;
use sound_ui::components::select::Select;
use sound_ui::components::toggle::Toggle;
use sound_ui::components::tooltip::Tooltip;
use sound_ui::{
    ActiveTheme, ControlEdit, DeviceLabel, Devices, LaneNumber, Lanes, Numbers, Session, Views,
    weak_callback,
};

use crate::parameters::{lane_of_pin, pin_of_lane};
use crate::{Parameter, Pin, PluginRecord, Plugins, Steps, WeakPlugins};

/// The room at the left of the body, where the plain card has all of it.
const LEFT_WIDTH: f32 = PLAIN_CARD_WIDTH - 2. * CARD_PADDING;
/// A dropdown spans two cells, so the names of its steps fit, and leaves air on either side.
const DROPDOWN_SPAN: usize = 2;
const DROPDOWN_WIDTH: f32 = DROPDOWN_SPAN as f32 * CELL_WIDTH - 8.;
/// How often at most the card asks the plugin for its text of a value a lane plays.
const LANE_READ_OUT: Duration = Duration::from_millis(66);

/// Registers the card of the `plugin` tool and what a rack calls one.
///
/// `plugins` is the host of this session, held weakly: the host must go when the project goes,
/// because that is what saves the state of every plugin.
pub fn register(views: &mut Views, devices: &mut Devices, plugins: WeakPlugins) {
    let (for_view, for_label) = (plugins.clone(), plugins.clone());
    views.register_card(move |session, plugin, frame, window, cx| {
        PluginView::new(for_view.clone(), session, plugin, frame, window, cx)
    });
    devices.describe::<PluginRecord>(move |record| {
        // The name its maker gave it, or the id, which is all that is left of a plugin this
        // machine does not have.
        let installed = for_label
            .upgrade()
            .and_then(|plugins| plugins.installed_name(record.format, &record.plugin_id));
        DeviceLabel {
            key: PluginRecord::offer_key(record.format, &record.plugin_id).into(),
            name: installed.map_or_else(|| record.plugin_id.clone().into(), SharedString::from),
        }
    });
    devices.numbers::<PluginRecord>(PluginNumbers(plugins));
}

/// What the lanes of a track need of the parameters of a plugin: a lane names a pin by its
/// path, `parameters.12.value`, and reads out a value in the plugin's own text, as the card does.
/// A lane can be added for any parameter that takes one, as for a knob of a built-in device:
/// one that is not pinned yet is pinned at what it plays now, in the same undo step.
struct PluginNumbers(WeakPlugins);

impl Numbers<PluginRecord> for PluginNumbers {
    fn name(
        &self,
        _: &Project,
        _: &InstanceId,
        record: &PluginRecord,
        field: &str,
    ) -> Option<String> {
        let pin = record.parameters.get(&pin_of_lane(field)?)?;
        (!pin.name.is_empty()).then(|| pin.name.clone())
    }

    fn text(
        &self,
        _: &Project,
        id: &InstanceId,
        _: &PluginRecord,
        field: &str,
        value: f32,
    ) -> Option<String> {
        let plugins = self.0.upgrade()?;
        plugins.parameter_text(id, pin_of_lane(field)?, f64::from(value))
    }

    fn lane_numbers(&self, _: &Project, id: &InstanceId, record: &PluginRecord) -> Vec<LaneNumber> {
        let parameters = self.0.upgrade().and_then(|plugins| plugins.parameters(id));
        let Some(parameters) = parameters else {
            return Vec::new();
        };
        let numbers = parameters.values().filter(|it| can_take_lane(record, it));
        numbers
            .map(|parameter| LaneNumber {
                field: lane_of_pin(parameter.id).into(),
                name: parameter.name.clone().into(),
            })
            .collect()
    }

    fn take(
        &self,
        _: &Project,
        id: &InstanceId,
        record: &PluginRecord,
        field: &str,
    ) -> Option<(PluginRecord, f32)> {
        let plugins = self.0.upgrade()?;
        let parameter_id = pin_of_lane(field)?;
        let parameters = plugins.parameters(id)?;
        let parameter = parameters.get(&parameter_id)?;
        if record.parameters.contains_key(&parameter_id) || !can_take_lane(record, parameter) {
            return None;
        }
        let pin = pin_now(&plugins, id, parameter);
        let value = pin.value as f32;
        let mut record = record.clone();
        record.parameters.insert(parameter_id, pin);
        Some((record, value))
    }
}

/// Whether a lane can be added for `parameter` of the plugin of `record`: pinned, its pin takes
/// one as the host plays it; else it takes one and the record has room to pin it.
fn can_take_lane(record: &PluginRecord, parameter: &Parameter) -> bool {
    match record.parameters.get(&parameter.id) {
        Some(pin) => parameter.takes_lane_at(pin),
        None => parameter.takes_lane() && record.parameters.len() < MAX_AUTOMATED,
    }
}

/// A pin of `parameter` at what the plugin of `id` plays now, or at its default when it cannot
/// say: on a step for a stepped one, inside its range for the rest.
fn pin_now(plugins: &Plugins, id: &InstanceId, parameter: &Parameter) -> Pin {
    let now = plugins.parameter_value(id, parameter.id);
    let now = now.map_or(parameter.default, |now| now.value);
    let value = match &parameter.steps {
        Some(steps) => parameter.step_value(steps, steps.index(now)),
        None => inside(now, parameter.minimum, parameter.maximum),
    };
    Pin {
        name: parameter.name.clone(),
        value,
    }
}

/// The control a parameter gets on the card.
enum Control<'a> {
    /// Two steps.
    Toggle(&'a Steps),
    /// Steps that all have a name.
    Dropdown(&'a Steps),
    /// A knob over the steps.
    Stepped(&'a Steps),
    Knob,
}

impl<'a> Control<'a> {
    fn of(parameter: &'a Parameter) -> Self {
        match &parameter.steps {
            Some(steps) if steps.count == 2 => Self::Toggle(steps),
            Some(steps) if steps.count > 2 && steps.all_named() => Self::Dropdown(steps),
            Some(steps) if steps.count > 2 => Self::Stepped(steps),
            _ => Self::Knob,
        }
    }

    fn is_knob(&self) -> bool {
        matches!(self, Self::Stepped(_) | Self::Knob)
    }
}

pub struct PluginView {
    session: Entity<Session>,
    plugin: Instance<PluginRecord>,
    plugins: WeakPlugins,
    /// The title and the close icon the rack gives the card.
    frame: CardFrame,
    /// The button takes one to be a tab stop and to show a focus ring.
    window_focus: FocusHandle,
    /// The drag of the knob of a pin, and the steps of every control on the card.
    edit: ControlEdit,
    /// Every parameter of the plugin, as the host last gave it. `None` while no plugin is
    /// loaded for the record.
    parameters: Option<Rc<BTreeMap<u32, Parameter>>>,
    /// The plugin's own text for the value each pin plays, with the value it is for. `None`
    /// when the plugin gives no text.
    readouts: BTreeMap<u32, (f64, Option<SharedString>)>,
    /// What the automation lanes of the track play into the pins.
    lanes: Entity<Lanes<PluginRecord>>,
    /// When the card last asked for the text of the value a lane plays, by pin, and the ask
    /// that waits for [`LANE_READ_OUT`] to pass.
    lane_asked: BTreeMap<u32, Instant>,
    read_out_later: Option<Task<()>>,
    /// The list that puts a parameter on the card and takes one off.
    menu: Entity<DropdownMenu>,
    /// The pins an automation lane moves, as the list last showed them: those do not come off.
    laned: BTreeSet<u32>,
    /// The pin whose knob is being dragged, and what its values meant when the drag began. A
    /// drag whose knob goes away, because the pin was taken off or became another control,
    /// sends no end, so the card ends it. So it does when the plugin gives the knob other
    /// values, whose numbers the open drag would read wrongly.
    dragged: Option<(u32, Mapping)>,
}

/// How the travel of the knob of a pin maps to values of the plugin.
#[derive(PartialEq)]
struct Mapping {
    minimum: f64,
    maximum: f64,
    steps: Option<Steps>,
}

impl Mapping {
    /// `None` for a parameter that is not a knob.
    fn of(parameter: &Parameter) -> Option<Self> {
        Control::of(parameter).is_knob().then(|| Self {
            minimum: parameter.minimum,
            maximum: parameter.maximum,
            steps: parameter.steps.clone(),
        })
    }
}

impl PluginView {
    pub fn new(
        plugins: WeakPlugins,
        session: Entity<Session>,
        plugin: Instance<PluginRecord>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Every notify of the session, not only a change of this record: a plugin's window can
        // also close by itself, and a plugin can load after the card was made. The poll that
        // saw either notifies.
        cx.observe(&session, |view, _, cx| {
            view.ask_the_plugin(false, cx);
            view.end_a_drag_whose_knob_changed(cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&session, |view, _, event: &ProjectEvent, cx| match event {
            ProjectEvent::Changed(id) if id == view.plugin.id() => {
                view.ask_the_plugin(true, cx);
                view.end_a_drag_whose_knob_changed(cx);
                cx.notify();
            }
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.plugin.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let menu = cx.new(|cx| {
            let mut menu = DropdownMenu::new("Parameters", Vec::new(), cx)
                .searchable("Search", cx)
                .trigger(Trigger::Subtle)
                .side(Side::Top)
                .width(260.)
                .max_height(320.)
                .debug_name("plugin-parameters");
            menu.set_trigger_height(ButtonSize::Sm.height(), cx);
            menu
        });
        cx.subscribe(&menu, |view, _, MenuPicked(value), cx| {
            view.pin_or_unpin(value, cx);
        })
        .detach();
        let lanes = Lanes::follow_named(&session, plugin.id(), cx);
        cx.observe(&lanes, |view, _, cx| {
            view.read_out(cx);
            view.fill_menu(false, cx);
        })
        .detach();
        let mut view = Self {
            session,
            plugin,
            plugins,
            frame,
            window_focus: cx.focus_handle().tab_stop(true),
            edit: ControlEdit::default(),
            parameters: None,
            readouts: BTreeMap::new(),
            lanes,
            lane_asked: BTreeMap::new(),
            read_out_later: None,
            menu,
            laned: BTreeSet::new(),
            dragged: None,
        };
        view.ask_the_plugin(true, cx);
        view
    }

    /// The list that puts a parameter on the card, for a snapshot that shows it open.
    pub fn parameters_menu(&self) -> &Entity<DropdownMenu> {
        &self.menu
    }

    /// Asks the plugin what the card draws from: its parameters, and its text for the value of
    /// every pin that changed. Only what is new costs a call into the plugin. The list of
    /// parameters is filled again when the record changed or the plugin's list did.
    fn ask_the_plugin(&mut self, record_changed: bool, cx: &mut Context<Self>) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let record = self.session.read(cx).project().state(&self.plugin);
        let Some(pins) = record.map(|record| record.parameters.clone()) else {
            return;
        };
        let id = self.plugin.id().clone();
        let parameters = plugins.parameters(&id);
        let same_list = match (&parameters, &self.parameters) {
            (Some(now), Some(before)) => Rc::ptr_eq(now, before),
            (now, before) => now.is_none() && before.is_none(),
        };
        if !same_list {
            // Another list may have other text for the same values: the host makes a new one
            // also when only the plugin's text changed.
            self.readouts.clear();
            self.parameters = parameters;
        }
        self.readouts.retain(|pin, _| pins.contains_key(pin));
        self.read_out(cx);

        self.fill_menu(record_changed || !same_list, cx);
    }

    /// Fills the list of parameters again, when `changed` says what it shows did or the pins a
    /// lane moves are others now.
    fn fill_menu(&mut self, changed: bool, cx: &mut Context<Self>) {
        let (Some(parameters), Some(record)) = (
            &self.parameters,
            self.session.read(cx).project().state(&self.plugin),
        ) else {
            return;
        };
        let lanes = self.lanes.read(cx);
        let pins = record.parameters.keys();
        let laned: BTreeSet<u32> = pins
            .filter(|pin| lanes.is_automated(&lane_of_pin(**pin)))
            .copied()
            .collect();
        if !changed && laned == self.laned {
            return;
        }
        let entries = menu_entries(parameters, &record.parameters, &laned);
        self.laned = laned;
        self.menu
            .update(cx, |menu, cx| menu.set_entries(entries, cx));
    }

    /// Asks the plugin for its text for the value each pin plays now, the record's or a lane's,
    /// where that is not the value of the text the card has. The text of a lane value is asked
    /// for at most once in [`LANE_READ_OUT`] per pin, whatever asks; one that has to wait is
    /// asked for when that has passed.
    fn read_out(&mut self, cx: &mut Context<Self>) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let Some(record) = self.session.read(cx).project().state(&self.plugin) else {
            return;
        };
        let lanes = self.lanes.read(cx);
        let now = Instant::now();
        let mut wait: Option<Duration> = None;
        self.lane_asked
            .retain(|pin, _| record.parameters.contains_key(pin));
        for (pin, Pin { value, .. }) in &record.parameters {
            let lane = lanes.value(&lane_of_pin(*pin));
            let value = lane.map_or(*value, f64::from);
            let known = self.readouts.get(pin);
            if known.is_some_and(|(read, _)| read.to_bits() == value.to_bits()) {
                continue;
            }
            // A pin that moves nothing says so instead, and its value is not the plugin's to
            // read out.
            let parameter = self.parameters.as_ref().and_then(|list| list.get(pin));
            if !parameter.is_some_and(|parameter| parameter.takes(value)) {
                continue;
            }
            if lane.is_some() {
                let since = self.lane_asked.get(pin).map(|at| now.duration_since(*at));
                if let Some(left) = since.and_then(|since| LANE_READ_OUT.checked_sub(since))
                    && !left.is_zero()
                {
                    wait = Some(wait.map_or(left, |wait| wait.min(left)));
                    continue;
                }
                self.lane_asked.insert(*pin, now);
            }
            let text = plugins.parameter_text(self.plugin.id(), *pin, value);
            self.readouts
                .insert(*pin, (value, text.map(SharedString::from)));
        }
        if let Some(wait) = wait
            && self.read_out_later.is_none()
        {
            self.read_out_later = Some(cx.spawn(async move |view, cx| {
                cx.background_executor().timer(wait).await;
                // A card that is gone has nothing to read out.
                view.update(cx, |view, cx| {
                    view.read_out_later = None;
                    view.read_out(cx);
                    cx.notify();
                })
                .ok();
            }));
        }
    }

    /// Ends the drag of a knob that is no longer on the card or maps to other values now. See
    /// [`Self::dragged`].
    fn end_a_drag_whose_knob_changed(&mut self, cx: &mut Context<Self>) {
        let Some((dragged, began)) = &self.dragged else {
            return;
        };
        let record = self.session.read(cx).project().state(&self.plugin);
        let pinned = record.is_some_and(|record| record.parameters.contains_key(dragged));
        let now = self.mapping(*dragged);
        if !pinned || now.as_ref() != Some(began) {
            self.dragged = None;
            self.edit.finish(&self.session, cx);
        }
    }

    /// How the knob of the pin `id` maps to values now, when it is a knob.
    fn mapping(&self, id: u32) -> Option<Mapping> {
        let parameter = self.parameters.as_ref()?.get(&id)?;
        Mapping::of(parameter)
    }

    /// A row of the list was picked: a pinned parameter comes off the card, another goes on it
    /// at the value the plugin has now. One undo step either way.
    fn pin_or_unpin(&mut self, value: &SharedString, cx: &mut Context<Self>) {
        let Ok(parameter_id) = value.parse::<u32>() else {
            return;
        };
        let Some(record) = self.session.read(cx).project().state(&self.plugin) else {
            return;
        };
        if let Some(pin) = record.parameters.get(&parameter_id) {
            if self.laned.contains(&parameter_id) {
                return;
            }
            let label = format!("Remove {}", pin.name);
            let remove = move |record: &mut PluginRecord, ()| {
                record.parameters.remove(&parameter_id);
            };
            let (session, plugin) = (&self.session, &self.plugin);
            self.edit
                .apply(session, plugin, &label, ValueChange::Set(()), remove, cx);
            return;
        }
        if record.parameters.len() >= MAX_AUTOMATED {
            return;
        }
        let parameters = self.parameters.as_ref();
        let Some(parameter) = parameters.and_then(|list| list.get(&parameter_id)) else {
            return;
        };
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let pin = pin_now(&plugins, self.plugin.id(), parameter);
        let label = format!("Add {}", parameter.name);
        let add = move |record: &mut PluginRecord, pin| {
            record.parameters.insert(parameter_id, pin);
        };
        let (session, plugin) = (&self.session, &self.plugin);
        self.edit
            .apply(session, plugin, &label, ValueChange::Set(pin), add, cx);
    }

    /// Notes which knob a drag is on, see [`Self::dragged`].
    fn dragged_to(&mut self, id: u32, change: &ValueChange) {
        match change {
            ValueChange::Drag(_) if self.dragged.as_ref().is_some_and(|(on, _)| *on == id) => {}
            ValueChange::Drag(_) => self.dragged = self.mapping(id).map(|mapping| (id, mapping)),
            ValueChange::DragEnd | ValueChange::DragCancel | ValueChange::Set(_) => {
                self.dragged = None;
            }
        }
    }

    /// A change of the control of the pin `id`, as one gesture or one step.
    fn change<V>(
        &mut self,
        id: u32,
        label: &str,
        change: ValueChange<V>,
        value: impl FnOnce(V) -> f64,
        cx: &mut Context<Self>,
    ) {
        let set = move |record: &mut PluginRecord, changed: V| {
            if let Some(pin) = record.parameters.get_mut(&id) {
                pin.value = value(changed);
            }
        };
        let (session, plugin) = (&self.session, &self.plugin);
        self.edit.apply(session, plugin, label, change, set, cx);
    }

    /// Opens the plugin's own window, or closes the one that is open. Not an edit: nothing of
    /// the project changes and there is no undo step. The host remembers it for the next time
    /// the project opens, on this computer.
    fn toggle_window(&mut self, cx: &mut Context<Self>) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let id = self.plugin.id().clone();
        if plugins.window_is_open(&id) {
            plugins.close_window(&id, cx);
            cx.notify();
            return;
        }
        if let Err(problem) = plugins.open_window(&id, cx) {
            self.session
                .update(cx, |session, cx| session.report(problem, cx));
        }
        cx.notify();
    }

    /// The cell of the pin `id`, and how many columns of cells it spans.
    fn pin(&self, id: u32, pin: &Pin, cx: &mut Context<Self>) -> (AnyElement, usize) {
        let parameter = self.parameters.as_ref().and_then(|list| list.get(&id));
        let Some(parameter) = parameter else {
            return (missing(id, pin, cx), 1);
        };
        let name = SharedString::from(parameter.name.clone());
        let label = format!("Change {name}");
        let out_of_range = !parameter.takes(pin.value);
        // Only a knob over any value takes a lane, so only a knob shows one.
        let lane = self.lanes.read(cx).value(&lane_of_pin(id)).map(f64::from);
        let value = lane.unwrap_or(pin.value);
        // The text of what a lane plays may be a few frames old, see `LANE_READ_OUT`.
        let readout = self
            .readouts
            .get(&id)
            .filter(|(read, _)| lane.is_some() || read.to_bits() == value.to_bits())
            .and_then(|(_, text)| text.clone());
        let (minimum, maximum) = (parameter.minimum, parameter.maximum);
        // Every step the control can write, by its number, as the plugin's own value.
        let step_values = |steps: &Steps| -> Vec<f64> {
            (0..steps.count)
                .map(|index| parameter.step_value(steps, index))
                .collect()
        };
        let element_id = ("pin", u64::from(id));
        let (cell, span) = match Control::of(parameter) {
            Control::Toggle(steps) => {
                let on = steps.index(pin.value) == 1;
                let word = if on { "On" } else { "Off" };
                let face = readout.unwrap_or_else(|| word.into());
                let values = step_values(steps);
                let toggle = Toggle::new(element_id, face, on).on_change(weak_callback(
                    cx,
                    move |view, on: bool, cx| {
                        let value = values[usize::from(on)];
                        view.change(id, &label, ValueChange::Set(value), |value| value, cx);
                    },
                ));
                (Cell::new(toggle), 1)
            }
            Control::Dropdown(steps) => {
                let items = steps
                    .names
                    .iter()
                    .enumerate()
                    .map(|(index, step)| MenuItem::new(index.to_string(), step.name.clone()));
                let picked = match out_of_range {
                    true => SharedString::default(),
                    false => steps.index(pin.value).to_string().into(),
                };
                let values = step_values(steps);
                let select = Select::new(element_id, picked)
                    .entries(vec![MenuEntry::Group(MenuGroup::new().items(items))])
                    .placeholder("None")
                    .trigger_width(DROPDOWN_WIDTH)
                    .menu_width(160.)
                    .on_change(weak_callback(cx, move |view, picked: SharedString, cx| {
                        let index = picked.parse::<usize>().ok();
                        if let Some(value) = index.and_then(|index| values.get(index)) {
                            view.change(id, &label, ValueChange::Set(*value), |value| value, cx);
                        }
                    }));
                (Cell::new(select).span(DROPDOWN_SPAN), DROPDOWN_SPAN)
            }
            control @ (Control::Stepped(_) | Control::Knob) => {
                let readout = match out_of_range {
                    true => "Out of range".into(),
                    false => readout.unwrap_or_else(|| short(value as f32).into()),
                };
                let knob = Knob::new(element_id)
                    .label(name)
                    .readout(readout)
                    .automated(lane.is_some());
                let knob = match control {
                    // The knob goes over the numbers of the steps, one at a time.
                    // A parameter may have millions of steps, so they are worked out one at a
                    // time and not listed.
                    Control::Stepped(steps) => {
                        let last = steps.count - 1;
                        let (value, default) =
                            (steps.index(pin.value), steps.index(parameter.default));
                        let steps = steps.clone();
                        knob.range(KnobRange::linear(0., last as f32))
                            .step(1.)
                            .value(value as f32)
                            .default_value(default as f32)
                            .on_change(weak_callback(cx, move |view, change: ValueChange, cx| {
                                view.dragged_to(id, &change);
                                let steps = &steps;
                                let value = |value: f32| {
                                    let index = (value.round().max(0.) as u32).min(last);
                                    inside(steps.value(index), minimum, maximum)
                                };
                                view.change(id, &label, change, value, cx);
                            }))
                    }
                    _ => knob
                        .range(KnobRange::linear(minimum as f32, maximum as f32))
                        .value(value as f32)
                        .default_value(parameter.default as f32)
                        .on_change(weak_callback(cx, move |view, change: ValueChange, cx| {
                            view.dragged_to(id, &change);
                            let value = |value: f32| inside(f64::from(value), minimum, maximum);
                            view.change(id, &label, change, value, cx);
                        })),
                };
                // The knob is a cell of its own, with its label and readout.
                return (knob.into_any_element(), 1);
            }
        };
        let cell = cell
            .label(name)
            .when(out_of_range, |cell| cell.value("Out of range"));
        (cell.into_any_element(), span)
    }
}

/// The rows of the list of parameters: every parameter of the plugin, with a check on the
/// pinned ones, and after them the pins the plugin has no parameter for, so those can come off
/// too. At the most pins, the others cannot be picked and a line says why. A pin in `laned` an
/// automation lane moves, and it stays: as a knob of a built-in device, its lane would move
/// nothing without it.
fn menu_entries(
    parameters: &BTreeMap<u32, Parameter>,
    pins: &BTreeMap<u32, Pin>,
    laned: &BTreeSet<u32>,
) -> Vec<MenuEntry> {
    let full = pins.len() >= MAX_AUTOMATED;
    let listed = parameters.values().map(|parameter| {
        let pinned = pins.contains_key(&parameter.id);
        let has_lane = laned.contains(&parameter.id);
        let item = MenuItem::new(parameter.id.to_string(), parameter.name.clone())
            .checked(pinned)
            .selectable(false)
            .disabled((full && !pinned) || has_lane);
        if has_lane {
            item.description("Automated. Delete its lane to take it off")
        } else {
            item
        }
    });
    let unknown = pins
        .iter()
        .filter(|(id, _)| !parameters.contains_key(id))
        .map(|(id, pin)| {
            MenuItem::new(id.to_string(), pin.name.clone())
                .description("Not in this plugin")
                .checked(true)
                .selectable(false)
        });
    let mut entries = vec![MenuEntry::Group(
        MenuGroup::new().items(listed.chain(unknown)),
    )];
    if full {
        let note = format!(
            "The card holds {MAX_AUTOMATED} parameters at most. Take one off to add another."
        );
        entries.push(MenuEntry::Note(note.into()));
    }
    entries
}

/// A value inside a range. The knob works in single precision, whose ends can fall just
/// outside the range the plugin gave.
fn inside(value: f64, minimum: f64, maximum: f64) -> f64 {
    value.max(minimum).min(maximum)
}

/// The cell of a pin whose id the plugin does not have: its name from the record, and what is
/// wrong in plain words.
fn missing(id: u32, pin: &Pin, cx: &mut Context<PluginView>) -> AnyElement {
    let peach = cx.theme().peach;
    let explained = format!(
        "This plugin has no parameter with the id {id}, so this moves nothing. Take it off in Parameters."
    );
    let face = div()
        .id(("missing", u64::from(id)))
        // For tests, which find the cell of a pin that moves nothing by its id.
        .debug_selector(move || format!("missing-{id}"))
        .text_size(px(12.))
        .line_height(px(14.))
        .text_color(peach)
        .child("Not found")
        .tooltip(move |_, cx| Tooltip::new(explained.clone()).view(cx));
    Cell::new(face)
        .label(pin.name.clone())
        .value(format!("id {id}"))
        .into_any_element()
}

impl Render for PluginView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(record) = self.session.read(cx).project().state(&self.plugin).cloned() else {
            return div().into_any_element();
        };
        let Some(plugins) = self.plugins.upgrade() else {
            return div().into_any_element();
        };
        let muted = cx.theme().gray_800;
        let line = |text: String| {
            div()
                .text_size(px(12.))
                .line_height(px(14.))
                .text_color(muted)
                .child(text)
        };
        let plain = self.frame.card().w(px(PLAIN_CARD_WIDTH));
        // The left of the body: what is at its top, and the line on the value line of the
        // second row.
        let left = |top: Div, bottom: Option<Div>| {
            div()
                .relative()
                .w(px(LEFT_WIDTH))
                .h(px(ROW_HEIGHT * 2.))
                .child(top)
                .children(bottom.map(|bottom| bottom.absolute().top(px(BODY_VALUE_LINE))))
        };

        let Some(installed) = plugins.installed(record.format, &record.plugin_id) else {
            // Missing. The card is named by the id, which is all that is left of the plugin.
            // The record stays as it is, the track is silent, and `problems.txt` says the
            // same thing to an agent.
            let text = format!(
                "This computer has no {} plugin with this id. Install it, or pick another.",
                record.format.name()
            );
            return plain.child(left(line(text), None)).into_any_element();
        };
        let detail = line(installed.detail());

        let id = self.plugin.id();
        // `None`: this machine has the plugin but it did not load, which is reported already.
        let Some(has_window) = plugins.window_offered(id) else {
            let text = "This plugin did not load, so there is nothing to open. See problems.txt.";
            return plain
                .child(left(line(text.to_string()), Some(detail)))
                .into_any_element();
        };
        // A plugin's view is put in an `NSView` or an `HWND` of ours, and Linux has neither.
        // The host itself does not know: its tests open windows without a view on every
        // platform.
        let (has_window, no_window) = if cfg!(any(target_os = "macos", target_os = "windows")) {
            (has_window, "This plugin has no window of its own.")
        } else {
            (
                false,
                "Plugin windows do not open on Linux yet. The plugin plays.",
            )
        };
        let is_open = plugins.window_is_open(id);
        let label = if is_open {
            "Close window"
        } else {
            "Open window"
        };
        let button = Button::new("plugin-window", label)
            .debug_selector(|| "plugin-window".to_string())
            .variant(ButtonVariant::Subtle)
            .size(ButtonSize::Sm)
            .disabled(!has_window)
            .focus_handle(&self.window_focus)
            .on_click(cx.listener(|view, _, _, cx| view.toggle_window(cx)));
        let top = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(4.))
            .child(button)
            .children((!has_window).then(|| line(no_window.to_string())))
            .children(self.parameters.is_some().then(|| self.menu.clone()));

        // Two pins to a column, in the order of their ids. A column is as wide as its wider
        // cell, and the columns make the card as wide as it is.
        let pins: Vec<(AnyElement, usize)> = record
            .parameters
            .iter()
            .map(|(pin, value)| self.pin(*pin, value, cx))
            .collect();
        let mut pins = pins.into_iter();
        let mut columns = Vec::new();
        while let Some((top, top_span)) = pins.next() {
            let column = Column::new().top(top);
            columns.push(match pins.next() {
                Some((bottom, span)) => column.bottom(bottom).span(span.max(top_span)),
                None => column.span(top_span),
            });
        }
        let card = match columns.is_empty() {
            true => plain,
            false => self.frame.card(),
        };
        let mut card = card.display(left(top, Some(detail)));
        for column in columns {
            card = card.column(column);
        }
        card.into_any_element()
    }
}
