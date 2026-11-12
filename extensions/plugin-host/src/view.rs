//! The card of a hosted plugin. The plugin draws its interface in a window of its own, so the
//! display of the card holds the button that opens it, with the list that pins parameters to
//! the card above it; then a cell per pin, two to a column, so the card grows wider and never
//! taller.
//!
//! A stepped parameter is driven by the number of its step, never by its value, so a control
//! always writes one of the plugin's own step values.
//!
//! The list of parameters and the plugin's text both call into the plugin, which drawing may
//! not do. So the view asks when the session says something changed and draws from the answers.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, SharedString, Task, Window, div, prelude::*, px,
};
use sound_core::{Instance, InstanceId, MAX_AUTOMATED, Project, ProjectEvent};
use sound_ui::ControlSize;
use sound_ui::components::button::{Button, ButtonVariant};
use sound_ui::components::cell::{Cell, control_width};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::TextDisplay;
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
    ActiveTheme, DeviceEdit, DeviceLabel, DeviceView, Devices, LaneNumber, Numbers, Session, Views,
    weak_callback,
};
use sound_ui::{TextVariant, Typography};

use crate::parameters::lane_of_pin;
use crate::{Parameter, Pin, PluginProblem, PluginRecord, Plugins, Steps, WeakPlugins};

/// A dropdown spans two cells, so the names of its steps fit.
const DROPDOWN_SPAN: usize = 2;
/// How often at most the card asks the plugin for its text of a value a lane plays, which
/// changes every frame.
const LANE_READ_OUT: Duration = Duration::from_millis(66);

/// `plugins` is held weakly: the host must go when the project goes, because that is what saves
/// the state of every plugin.
pub fn register(views: &mut Views, devices: &mut Devices, plugins: WeakPlugins) {
    let (for_view, for_label) = (plugins.clone(), plugins.clone());
    views.register_card(move |session, plugin, frame, window, cx| {
        PluginView::new(for_view.clone(), session, plugin, frame, window, cx)
    });
    devices.describe::<PluginRecord>(move |record| {
        // A plugin this machine does not have is named by its id.
        let installed = for_label
            .upgrade()
            .and_then(|plugins| plugins.installed_name(record.format, &record.plugin_id));
        DeviceLabel {
            key: PluginRecord::offer_key(record.format, &record.plugin_id).into(),
            name: installed.map_or_else(|| record.plugin_id.clone().into(), SharedString::from),
        }
    });
    let for_window = plugins.clone();
    devices.window::<PluginRecord>(move |id, cx| {
        let plugins = for_window.upgrade()?;
        match plugins.open_window(id, cx) {
            Err(PluginProblem::NoWindow { .. }) => None,
            opened => Some(opened.map_err(Into::into)),
        }
    });
    devices.numbers::<PluginRecord>(PluginNumbers(plugins));
}

/// The parameters of a plugin a lane can move.
struct PluginNumbers(WeakPlugins);

impl Numbers<PluginRecord> for PluginNumbers {
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
}

/// A parameter not pinned yet takes a lane only when the record has room to pin it.
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
    device: DeviceEdit<PluginRecord>,
    plugins: WeakPlugins,
    frame: CardFrame,
    /// Of the window button: a tab stop with a focus ring.
    window_focus: FocusHandle,
    /// As the host last gave them. `None` while no plugin is loaded for the record.
    parameters: Option<Rc<BTreeMap<u32, Parameter>>>,
    /// The plugin's own text for the value each pin plays, with the value it is for. `None`
    /// when the plugin gives no text.
    readouts: BTreeMap<u32, (f64, Option<SharedString>)>,
    /// When the card last asked for the text of a lane's value, by pin.
    lane_asked: BTreeMap<u32, Instant>,
    read_out_later: Option<Task<()>>,
    menu: Entity<DropdownMenu>,
    /// The pins an automation lane moves, as the list last showed them: those do not come off.
    laned: BTreeSet<u32>,
    /// The pin whose knob is being dragged, and how its values mapped when the drag began. A
    /// knob that goes away mid-drag sends no end, so the card ends the drag; so it does when
    /// the mapping changes, which the open drag would read wrongly.
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
        // Every notify, not only a change of this record: a plugin's window can close by
        // itself, and a plugin can load after the card was made.
        cx.observe(&session, |view, _, cx| {
            view.ask_the_plugin(false, cx);
            view.end_a_drag_whose_knob_changed(cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&session, |view, _, event: &ProjectEvent, cx| {
            if let ProjectEvent::Changed(id) = event
                && id == view.device.instance.id()
            {
                view.ask_the_plugin(true, cx);
                view.end_a_drag_whose_knob_changed(cx);
            }
        })
        .detach();
        let menu = cx.new(|cx| {
            DropdownMenu::new("Parameters", Vec::new(), cx)
                .searchable("Search", cx)
                .trigger(Trigger::Select)
                .side(Side::Top)
                .width(260.)
                .max_height(320.)
                .debug_name("plugin-parameters")
        });
        cx.subscribe(&menu, |view, _, MenuPicked(value), cx| {
            view.pin_or_unpin(value, cx);
        })
        .detach();
        let device = DeviceEdit::named(session, plugin, cx);
        cx.observe(&device.lanes, |view, _, cx| {
            view.read_out(cx);
            view.fill_menu(false, cx);
        })
        .detach();
        let mut view = Self {
            device,
            plugins,
            frame,
            window_focus: cx.focus_handle().tab_stop(true),
            parameters: None,
            readouts: BTreeMap::new(),
            lane_asked: BTreeMap::new(),
            read_out_later: None,
            menu,
            laned: BTreeSet::new(),
            dragged: None,
        };
        view.ask_the_plugin(true, cx);
        view
    }

    fn record<'a>(&self, cx: &'a App) -> Option<&'a PluginRecord> {
        let device = &self.device;
        device.session.read(cx).project().state(&device.instance)
    }

    /// Asks the plugin what the card draws from. Only what is new costs a call into the plugin.
    fn ask_the_plugin(&mut self, record_changed: bool, cx: &mut Context<Self>) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let record = self.record(cx);
        let Some(pins) = record.map(|record| record.parameters.clone()) else {
            return;
        };
        let id = self.device.instance.id().clone();
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

    /// Fills the list of parameters again when `changed`, or when the laned pins changed.
    fn fill_menu(&mut self, changed: bool, cx: &mut Context<Self>) {
        let (Some(parameters), Some(record)) = (&self.parameters, self.record(cx)) else {
            return;
        };
        let lanes = self.device.lanes.read(cx);
        let pins = record.parameters.keys();
        let laned: BTreeSet<u32> = pins
            .filter(|pin| lanes.value(&lane_of_pin(**pin)).is_some())
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
    /// where the card has no text for that value yet. A lane's value waits for
    /// [`LANE_READ_OUT`].
    fn read_out(&mut self, cx: &mut Context<Self>) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let Some(record) = self.record(cx) else {
            return;
        };
        let lanes = self.device.lanes.read(cx);
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
            // A pin that moves nothing shows "Out of range" instead.
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
            let text = plugins.parameter_text(self.device.instance.id(), *pin, value);
            self.readouts
                .insert(*pin, (value, text.map(SharedString::from)));
        }
        if let Some(wait) = wait
            && self.read_out_later.is_none()
        {
            self.read_out_later = Some(cx.spawn(async move |view, cx| {
                cx.background_executor().timer(wait).await;
                view.update(cx, |view, cx| {
                    view.read_out_later = None;
                    view.read_out(cx);
                    cx.notify();
                })
                .ok();
            }));
        }
    }

    /// See [`Self::dragged`].
    fn end_a_drag_whose_knob_changed(&mut self, cx: &mut Context<Self>) {
        let Some((dragged, began)) = &self.dragged else {
            return;
        };
        let record = self.record(cx);
        let pinned = record.is_some_and(|record| record.parameters.contains_key(dragged));
        let now = self.mapping(*dragged);
        if !pinned || now.as_ref() != Some(began) {
            self.dragged = None;
            self.device.finish(cx);
        }
    }

    fn mapping(&self, id: u32) -> Option<Mapping> {
        let parameter = self.parameters.as_ref()?.get(&id)?;
        Mapping::of(parameter)
    }

    /// A pinned parameter comes off the card, another goes on it at the value the plugin has now.
    fn pin_or_unpin(&mut self, value: &SharedString, cx: &mut Context<Self>) {
        let Ok(parameter_id) = value.parse::<u32>() else {
            return;
        };
        let Some(record) = self.record(cx) else {
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
            let change = ValueChange::Set(());
            self.device.change(&label, change, remove, cx);
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
        let pin = pin_now(&plugins, self.device.instance.id(), parameter);
        let label = format!("Add {}", parameter.name);
        let add = move |record: &mut PluginRecord, pin| {
            record.parameters.insert(parameter_id, pin);
        };
        self.device.change(&label, ValueChange::Set(pin), add, cx);
    }

    /// See [`Self::dragged`].
    fn dragged_to(&mut self, id: u32, change: &ValueChange) {
        match change {
            ValueChange::Drag(_) if self.dragged.as_ref().is_some_and(|(on, _)| *on == id) => {}
            ValueChange::Drag(_) => self.dragged = self.mapping(id).map(|mapping| (id, mapping)),
            ValueChange::DragEnd | ValueChange::DragCancel | ValueChange::Set(_) => {
                self.dragged = None;
            }
        }
    }

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
        self.device.change(label, change, set, cx);
    }

    /// Not an edit and no undo step: the host remembers the window on this computer only.
    fn toggle_window(&mut self, cx: &mut Context<Self>) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let id = self.device.instance.id().clone();
        if plugins.window_is_open(&id) {
            plugins.close_window(&id, cx);
            cx.notify();
            return;
        }
        if let Err(problem) = plugins.open_window(&id, cx) {
            self.device
                .session
                .update(cx, |session, cx| session.report(problem, cx));
        }
        cx.notify();
    }

    /// The cell of the pin `id`.
    fn pin(&self, id: u32, pin: &Pin, cx: &mut Context<Self>) -> AnyElement {
        let parameter = self.parameters.as_ref().and_then(|list| list.get(&id));
        let Some(parameter) = parameter else {
            return missing(id, pin, cx);
        };
        let name = SharedString::from(parameter.name.clone());
        let label = format!("Change {name}");
        let out_of_range = !parameter.takes(pin.value);
        let lanes = self.device.lanes.read(cx);
        let held = lanes.is_automated(&lane_of_pin(id));
        let lane = lanes.value(&lane_of_pin(id)).map(f64::from);
        let value = lane.unwrap_or(pin.value);
        // The text of what a lane plays may be a few frames old.
        let readout = self
            .readouts
            .get(&id)
            .filter(|(read, _)| lane.is_some() || read.to_bits() == value.to_bits())
            .and_then(|(_, text)| text.clone());
        let (minimum, maximum) = (parameter.minimum, parameter.maximum);
        let step_values = |steps: &Steps| -> Vec<f64> {
            (0..steps.count)
                .map(|index| parameter.step_value(steps, index))
                .collect()
        };
        let element_id = ("pin", u64::from(id));
        let cell = match Control::of(parameter) {
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
                Cell::new(toggle)
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
                    .trigger_width(control_width(DROPDOWN_SPAN))
                    .menu_width(160.)
                    .on_change(weak_callback(cx, move |view, picked: SharedString, cx| {
                        let index = picked.parse::<usize>().ok();
                        if let Some(value) = index.and_then(|index| values.get(index)) {
                            view.change(id, &label, ValueChange::Set(*value), |value| value, cx);
                        }
                    }));
                Cell::new(select).span(DROPDOWN_SPAN)
            }
            control @ (Control::Stepped(_) | Control::Knob) => {
                let readout = match out_of_range {
                    true => "Out of range".into(),
                    false => readout.unwrap_or_else(|| short(value as f32).into()),
                };
                let knob = Knob::new(element_id)
                    .label(name)
                    .readout(readout)
                    .automated(held);
                let knob = match control {
                    // A parameter may have millions of steps, so they are not listed.
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
                return knob.into_any_element();
            }
        };
        cell.label(name)
            .when(out_of_range, |cell| cell.value("Out of range"))
            .into_any_element()
    }
}

/// Every parameter of the plugin, then the pins it has no parameter for, so those can come off
/// too. A laned pin cannot come off, as its lane would move nothing without it.
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

/// The cell of a pin whose id the plugin does not have.
fn missing(id: u32, pin: &Pin, cx: &mut Context<PluginView>) -> AnyElement {
    let peach = cx.theme().peach;
    let explained = format!(
        "This plugin has no parameter with the id {id}, so this moves nothing. Take it off in Parameters."
    );
    let face = div()
        .id(("missing", u64::from(id)))
        // For tests.
        .debug_selector(move || format!("missing-{id}"))
        .text_variant(TextVariant::Label)
        .line_height(px(14.))
        .text_color(peach)
        .child("Not found")
        .tooltip(move |_, cx| Tooltip::new(explained.clone()).view(cx));
    Cell::new(face)
        .label(pin.name.clone())
        .value(format!("id {id}"))
        .into_any_element()
}

impl DeviceView for PluginView {
    type State = PluginRecord;

    fn device(&mut self) -> &mut DeviceEdit<PluginRecord> {
        &mut self.device
    }
}

impl Render for PluginView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted; whatever hosts the view takes it away then.
        let Some(record) = self.record(cx).cloned() else {
            return div().into_any_element();
        };
        let Some(plugins) = self.plugins.upgrade() else {
            return div().into_any_element();
        };
        // The plugin draws itself in a window of its own, so the display of its card is the
        // way there, and says what is wrong when there is none.
        let display = TextDisplay::new("plugin-display");
        let card = self.frame.card();

        let Some(installed) = plugins.installed(record.format, &record.plugin_id) else {
            let text = format!(
                "This computer has no {} plugin with this id. Install it, or pick another.",
                record.format.name()
            );
            return card.display(display.message(text)).into_any_element();
        };
        let display = display.caption(installed.detail());

        let id = self.device.instance.id();
        // The plugin crashed or hung in its own process, and waits for the composer.
        if plugins.has_stopped(id) {
            let button = Button::new("plugin-start-again", "Start again")
                .debug_selector(|| "plugin-start-again".to_string())
                .variant(ButtonVariant::Subtle)
                .size(ControlSize::Md)
                .focus_handle(&self.window_focus)
                .on_click(cx.listener(|view, _, _, cx| {
                    if let Some(plugins) = view.plugins.upgrade() {
                        plugins.start_again(view.device.instance.id());
                    }
                    cx.notify();
                }));
            let text = format!("{} stopped working.", installed.name);
            let display = display.message(text).button(button);
            return card.display(display).into_any_element();
        }
        // `None`: this machine has the plugin but it did not load, which is reported already.
        let Some(has_window) = plugins.window_offered(id) else {
            let text = "This plugin did not load, so there is nothing to open.";
            return card.display(display.message(text)).into_any_element();
        };
        // A plugin's view goes in an `NSView` or an `HWND`, and Linux has neither. The host
        // does not check, so its tests can open windows on every platform.
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
        // A control that does nothing is not shown: with no window the display says why.
        let display = match has_window {
            true => display.button(
                Button::new("plugin-window", label)
                    .debug_selector(|| "plugin-window".to_string())
                    .variant(ButtonVariant::Subtle)
                    .size(ControlSize::Md)
                    .focus_handle(&self.window_focus)
                    .on_click(cx.listener(|view, _, _, cx| view.toggle_window(cx))),
            ),
            false => display.message(no_window),
        };
        let display = match self.parameters.is_some() {
            true => display.control(self.menu.clone()),
            false => display,
        };

        // Two pins to a column, in the order of their ids.
        let mut pins = record
            .parameters
            .iter()
            .map(|(pin, value)| self.pin(*pin, value, cx));
        let mut card = card.display(display);
        while let Some(top) = pins.next() {
            let column = Column::new().top(top);
            card = card.column(match pins.next() {
                Some(bottom) => column.bottom(bottom),
                None => column,
            });
        }
        card.into_any_element()
    }
}
