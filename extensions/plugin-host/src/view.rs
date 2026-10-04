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
//! the rack, in the frame, because the rack keeps whether a slot is on.
//!
//! The control of a pin follows what the plugin says the parameter is: two steps are a toggle,
//! named steps a dropdown, anything else a knob, which snaps to the steps of a stepped one. What
//! it says is the plugin's own text for the value. A pin the plugin has no parameter for, or
//! whose value is outside the range, says so in its cell; the rest of the card works.
//!
//! The list of parameters and the plugin's text both call into the plugin, which a frame may
//! not do. So the view asks when the session tells it something changed, and draws from what it
//! was told.
//!
//! A plugin this machine does not have shows what is wrong and the id the record names, so a
//! composer can see which plugin to install and an agent can be asked to correct the record.
//! The record itself is untouched, as everywhere else.

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, Div, Entity, FocusHandle, SharedString, Window, div, prelude::*, px,
};
use sound_core::{Instance, MAX_AUTOMATED, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::cell::{CELL_WIDTH, Cell, ROW_HEIGHT};
use sound_ui::components::device_card::{
    BODY_VALUE_LINE, CARD_PADDING, CardFrame, Column, PLAIN_CARD_WIDTH, card_width,
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
use sound_ui::{ActiveTheme, ControlEdit, DeviceLabel, Devices, Session, Views, weak_callback};

use crate::{Parameter, Pin, PluginRecord, WeakPlugins};

/// The room at the left of the body, where the plain card has all of it.
const LEFT_WIDTH: f32 = PLAIN_CARD_WIDTH - 2. * CARD_PADDING;
/// The trigger of the list of parameters, as tall as the button above it.
const TRIGGER_HEIGHT: f32 = 28.;
/// A dropdown spans two cells, so the names of its steps fit, and leaves air on either side.
const DROPDOWN_SPAN: usize = 2;
const DROPDOWN_WIDTH: f32 = DROPDOWN_SPAN as f32 * CELL_WIDTH - 8.;

/// Registers the card of the `plugin` tool and what a rack calls one.
///
/// `plugins` is the host of this session, held weakly: the host must go when the project goes,
/// because that is what saves the state of every plugin.
pub fn register(views: &mut Views, devices: &mut Devices, plugins: WeakPlugins) {
    let for_view = plugins.clone();
    views.register_card(move |session, plugin, frame, window, cx| {
        PluginView::new(for_view.clone(), session, plugin, frame, window, cx)
    });
    devices.describe::<PluginRecord>(move |record| {
        // The name its maker gave it, or the id, which is all that is left of a plugin this
        // machine does not have.
        let installed = plugins
            .upgrade()
            .and_then(|plugins| plugins.installed_name(record.format, &record.plugin_id));
        DeviceLabel {
            key: PluginRecord::offer_key(record.format, &record.plugin_id).into(),
            name: installed.map_or_else(|| record.plugin_id.clone().into(), SharedString::from),
        }
    });
}

/// The control a parameter gets on the card.
enum Control {
    Toggle,
    Dropdown,
    /// A knob, in steps of this much for a stepped parameter.
    Knob(Option<f64>),
}

impl Control {
    fn of(parameter: &Parameter) -> Self {
        let Some(steps) = &parameter.steps else {
            return Self::Knob(None);
        };
        match steps.count {
            2 => Self::Toggle,
            _ if !steps.names.is_empty() => Self::Dropdown,
            count if count > 2 => {
                let step = (parameter.maximum - parameter.minimum) / f64::from(count - 1);
                Self::Knob(Some(step))
            }
            _ => Self::Knob(None),
        }
    }
}

/// What the list of parameters was last filled from: the plugin's list and the name of each
/// pin. A turn of a knob changes neither, so it does not fill the list again.
struct Listed {
    parameters: Rc<BTreeMap<u32, Parameter>>,
    pins: BTreeMap<u32, String>,
}

impl Listed {
    fn is(&self, parameters: &Rc<BTreeMap<u32, Parameter>>, pins: &BTreeMap<u32, String>) -> bool {
        Rc::ptr_eq(&self.parameters, parameters) && &self.pins == pins
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
    /// The plugin's own text for the value of each pin, with the value it is for. `None` when
    /// the plugin gives no text.
    readouts: BTreeMap<u32, (f64, Option<SharedString>)>,
    /// The list that puts a parameter on the card and takes one off.
    menu: Entity<DropdownMenu>,
    listed: Option<Listed>,
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
            view.ask_the_plugin(cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&session, |view, _, event: &ProjectEvent, cx| match event {
            ProjectEvent::Changed(id) if id == view.plugin.id() => {
                view.ask_the_plugin(cx);
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
            menu.set_trigger_height(TRIGGER_HEIGHT, cx);
            menu
        });
        cx.subscribe(&menu, |view, _, MenuPicked(value), cx| {
            view.pin_or_unpin(value, cx);
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
            menu,
            listed: None,
        };
        view.ask_the_plugin(cx);
        view
    }

    /// The list that puts a parameter on the card, for a snapshot that shows it open.
    pub fn parameters_menu(&self) -> &Entity<DropdownMenu> {
        &self.menu
    }

    /// Asks the plugin what the card draws from: its parameters, and its text for the value of
    /// every pin that changed. Only what is new costs a call into the plugin.
    fn ask_the_plugin(&mut self, cx: &mut Context<Self>) {
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
            // Another list may have other text for the same values.
            self.readouts.clear();
            self.parameters = parameters;
        }
        self.readouts.retain(|pin, _| pins.contains_key(pin));
        for (pin, Pin { value, .. }) in &pins {
            let known = self.readouts.get(pin);
            if known.is_some_and(|(read, _)| read.to_bits() == value.to_bits()) {
                continue;
            }
            // A pin that moves nothing says so instead, and its value is not the plugin's to
            // read out.
            let parameter = self.parameters.as_ref().and_then(|list| list.get(pin));
            if !parameter.is_some_and(|parameter| parameter.takes(*value)) {
                continue;
            }
            let text = plugins.parameter_text(&id, *pin, *value);
            self.readouts
                .insert(*pin, (*value, text.map(SharedString::from)));
        }

        let Some(parameters) = self.parameters.clone() else {
            return;
        };
        let names: BTreeMap<u32, String> = pins
            .iter()
            .map(|(pin, Pin { name, .. })| (*pin, name.clone()))
            .collect();
        if self
            .listed
            .as_ref()
            .is_some_and(|listed| listed.is(&parameters, &names))
        {
            return;
        }
        let entries = menu_entries(&parameters, &names);
        self.menu
            .update(cx, |menu, cx| menu.set_entries(entries, cx));
        self.listed = Some(Listed {
            parameters,
            pins: names,
        });
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
        let now = self.plugins.upgrade().and_then(|plugins| {
            let now = plugins.parameter_value(self.plugin.id(), parameter_id)?;
            Some(now.value)
        });
        let pin = Pin {
            name: parameter.name.clone(),
            value: inside(now.unwrap_or(parameter.default), parameter),
        };
        let label = format!("Add {}", parameter.name);
        let add = move |record: &mut PluginRecord, pin| {
            record.parameters.insert(parameter_id, pin);
        };
        let (session, plugin) = (&self.session, &self.plugin);
        self.edit
            .apply(session, plugin, &label, ValueChange::Set(pin), add, cx);
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
    /// the project opens, on this Mac.
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
        let readout = self
            .readouts
            .get(&id)
            .filter(|(read, _)| read.to_bits() == pin.value.to_bits())
            .and_then(|(_, text)| text.clone());
        let (minimum, maximum) = (parameter.minimum, parameter.maximum);
        let element_id = ("pin", u64::from(id));
        let (cell, span) = match Control::of(parameter) {
            Control::Toggle => {
                let on = pin.value >= (minimum + maximum) / 2.;
                let word = if on { "On" } else { "Off" };
                let face = readout.unwrap_or_else(|| word.into());
                let toggle = Toggle::new(element_id, face, on).on_change(weak_callback(
                    cx,
                    move |view, on: bool, cx| {
                        let value = if on { maximum } else { minimum };
                        view.change(id, &label, ValueChange::Set(value), |value| value, cx);
                    },
                ));
                (Cell::new(toggle), 1)
            }
            Control::Dropdown => {
                let steps = parameter.steps.iter().flat_map(|steps| &steps.names);
                let values: Vec<f64> = steps.clone().map(|step| step.value).collect();
                let items = steps
                    .enumerate()
                    .map(|(index, step)| MenuItem::new(index.to_string(), step.name.clone()));
                let picked = match out_of_range {
                    true => None,
                    false => nearest(&values, pin.value),
                };
                let picked = picked.map_or_else(SharedString::default, |index| {
                    SharedString::from(index.to_string())
                });
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
            Control::Knob(step) => {
                let range = KnobRange::linear(minimum as f32, maximum as f32);
                let readout = match out_of_range {
                    true => "Out of range".into(),
                    false => readout.unwrap_or_else(|| short(pin.value as f32).into()),
                };
                let bounds = parameter.clone();
                let knob = Knob::new(element_id)
                    .range(range)
                    .value(pin.value as f32)
                    .default_value(parameter.default as f32)
                    .label(name)
                    .readout(readout)
                    .when_some(step, |knob, step| knob.step(step as f32))
                    .on_change(weak_callback(cx, move |view, change: ValueChange, cx| {
                        let bounds = &bounds;
                        let value = |value: f32| inside(f64::from(value), bounds);
                        view.change(id, &label, change, value, cx);
                    }));
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
/// too. At the most pins, the others cannot be picked and a line says why.
fn menu_entries(
    parameters: &BTreeMap<u32, Parameter>,
    pins: &BTreeMap<u32, String>,
) -> Vec<MenuEntry> {
    let full = pins.len() >= MAX_AUTOMATED;
    let listed = parameters.values().map(|parameter| {
        let pinned = pins.contains_key(&parameter.id);
        MenuItem::new(parameter.id.to_string(), parameter.name.clone())
            .checked(pinned)
            .selectable(false)
            .disabled(full && !pinned)
    });
    let unknown = pins
        .iter()
        .filter(|(id, _)| !parameters.contains_key(id))
        .map(|(id, name)| {
            MenuItem::new(id.to_string(), name.clone())
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

/// A value inside the range of `parameter`. The knob works in single precision, whose ends can
/// fall just outside the range the plugin gave.
fn inside(value: f64, parameter: &Parameter) -> f64 {
    value.max(parameter.minimum).min(parameter.maximum)
}

/// The index of the step value nearest `value`.
fn nearest(values: &[f64], value: f64) -> Option<usize> {
    let distance = |index: &usize| (values[*index] - value).abs();
    (0..values.len()).min_by(|one, other| distance(one).total_cmp(&distance(other)))
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
                "This Mac has no {} plugin with this id. Install it, or pick another.",
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
        // A plugin's view is put in an `NSView` of ours, which Linux does not have. The host
        // itself does not know: its tests open windows without a view on every platform.
        let (has_window, no_window) = if cfg!(target_os = "macos") {
            (has_window, "This plugin has no window of its own.")
        } else {
            (
                false,
                "Plugin windows open on macOS only for now. The plugin plays.",
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
        // cell.
        let pins: Vec<(AnyElement, usize)> = record
            .parameters
            .iter()
            .map(|(pin, value)| self.pin(*pin, value, cx))
            .collect();
        let mut pins = pins.into_iter();
        let mut columns = Vec::new();
        let mut spanned = 0;
        while let Some((top, top_span)) = pins.next() {
            let column = Column::new().top(top);
            let (column, span) = match pins.next() {
                Some((bottom, span)) => (column.bottom(bottom), span.max(top_span)),
                None => (column, top_span),
            };
            spanned += span;
            columns.push(column.span(span));
        }
        let width = match spanned {
            0 => PLAIN_CARD_WIDTH,
            spanned => card_width(LEFT_WIDTH, spanned),
        };
        let mut card = self
            .frame
            .card()
            .w(px(width))
            .display(left(top, Some(detail)));
        for column in columns {
            card = card.column(column);
        }
        card.into_any_element()
    }
}
