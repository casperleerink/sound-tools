//! The track panel: the details of one track, in the panel below the timeline where the note
//! editor also shows. It is a rack: device cards from left to right, in the order the sound
//! goes through them. The instrument of the track first, then its effects.
//!
//! The panel knows no instrument, no effect and no plugin. It asks the view registry for the
//! view of whatever instance sits in each slot of the track and puts it into a card, and it
//! asks the device registry what that instance is called and what else the composer could put
//! there. Both registries are filled by whoever makes the window. A slot that is empty, or
//! whose tool has no view, shows a quiet card that says so, with the same picker on it.
//!
//! How the rack follows the track. The slots come from the record
//! ([`arrangement::device_slots`]), so the panel makes its list again whenever the track record
//! changes or a child of the track is created or deleted. That rebuild keeps the card of every
//! slot that stays, by its slot id, so a knob drag in one card goes on while another card is
//! added or removed next to it.
//!
//! The mixer of the track (gain, pan and mute) is not a device. It is a fixed section at the
//! right end of the row, after the rack and outside what scrolls, and it is the one thing the
//! panel edits itself: those three values are in the track record.

use gpui::{
    AnyView, App, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    SharedString, Window, div, prelude::*, px,
};
use sound_core::{Changes, Instance, InstanceId, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::card::Card;
use sound_ui::components::cell::Cell;
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::toggle::Toggle;
use sound_ui::{ActiveTheme, ControlEdit, DeviceLabel, DeviceOffer, Devices, Session, Slot, Views};

use super::layout::{HEADER_WIDTH, RULER_HEIGHT};
use super::paint::accent;
use crate::TrackState;

/// A knob of the mixer section: what it edits, and what an undo step of it is called.
struct Control {
    field: &'static str,
    label: &'static str,
    undo_label: &'static str,
    range: (f32, f32),
    /// The arc starts at the top, for a value with a middle.
    bipolar: bool,
    set: fn(&mut TrackState, f32),
    get: fn(&TrackState) -> f32,
    readout: fn(f32) -> String,
}

impl Control {
    fn knob_range(&self) -> KnobRange {
        KnobRange::linear(self.range.0, self.range.1)
    }
}

const GAIN: Control = Control {
    field: "gain_db",
    label: "Gain",
    undo_label: "Change gain",
    range: TrackState::GAIN_DB,
    bipolar: false,
    set: |track, value| track.gain_db = value,
    get: |track| track.gain_db,
    readout: |value| format!("{} dB", short(value)),
};

const PAN: Control = Control {
    field: "pan",
    label: "Pan",
    undo_label: "Change pan",
    range: TrackState::PAN,
    bipolar: true,
    set: |track, value| track.pan = value,
    get: |track| track.pan,
    readout: pan_readout,
};

/// The pan as people read it: `C` in the middle, else how far to a side in percent.
fn pan_readout(pan: f32) -> String {
    let percent = short(pan.abs() * 100.);
    if pan < 0. {
        format!("{percent}L")
    } else if pan > 0. {
        format!("{percent}R")
    } else {
        "C".to_string()
    }
}

/// What the panel asks of the view that holds it.
pub enum TrackPanelEvent {
    /// The close control.
    Close,
}

/// What a card says when its slot holds nothing.
const EMPTY_SLOT: &str = "No instrument";
const EMPTY_EFFECT_SLOT: &str = "No effect";

/// What the control at the end of the rack says.
const ADD_EFFECT: &str = "Add effect";

/// What the control that takes an effect off the track is called, and what a test finds it by.
/// It names the slot, because one view draws every card of the rack.
pub fn remove_control(slot: &InstanceId) -> SharedString {
    format!("remove-{}", slot.name()).into()
}

/// The same for the picker of an effect card.
fn effect_picker(slot: &InstanceId) -> SharedString {
    format!("effect-picker-{}", slot.name()).into()
}

/// The menu of offers for a slot, and the quiet lines under them.
///
/// `picks_one` says whether the menu chooses what is in a slot, which is what a card does, or
/// runs a command, which is what the control that adds an effect does. A command leaves no
/// check behind: the menu would otherwise mark the effect that was added last as if the
/// control held it.
fn offer_entries(
    offers: &[DeviceOffer],
    slot: Slot,
    picks_one: bool,
    session: &Entity<Session>,
    cx: &App,
) -> Vec<MenuEntry> {
    let project = session.read(cx).project();
    let label = match slot {
        Slot::Instrument => "Instrument",
        Slot::Effect => "Effect",
    };
    let mut entries = vec![MenuEntry::Group(
        MenuGroup::new()
            .label(label)
            .max_height(320.)
            .items(offers.iter().map(|offer| {
                let item =
                    MenuItem::new(offer.key.clone(), offer.name.clone()).selectable(picks_one);
                // An offer this project cannot load is shown and not taken, with the one
                // edit that would make it work. Enabling an extension while the project
                // runs is refused, and nothing here writes `project.json` for anyone.
                match (offer.is_enabled_in(project), &offer.needs, &offer.detail) {
                    (false, Some(needs), _) => item.disabled(true).description(needed(needs)),
                    (_, _, Some(detail)) => item.description(detail.clone()),
                    _ => item,
                }
            })),
    )];
    // What a source of offers has to say under them: that it is still looking at this
    // machine, and what it owes whoever made what it offers.
    entries.extend(Devices::offer_notes(cx).into_iter().map(MenuEntry::Note));
    entries
}

/// One card of the rack: what is in a slot now, its view when its tool has one, and the picker
/// that says what else could go there.
struct Device {
    slot: InstanceId,
    kind: Slot,
    /// `None` while the slot is empty.
    tool: Option<&'static str>,
    view: Option<AnyView>,
    picker: Entity<DropdownMenu>,
    /// What the picker offers, so that choosing needs no second walk over the registry.
    offers: Vec<DeviceOffer>,
    /// The remove control of an effect card takes one to be a tab stop and show a ring.
    remove_focus: FocusHandle,
}

impl Device {
    fn new(
        session: &Entity<Session>,
        slot: InstanceId,
        kind: Slot,
        window: &mut Window,
        cx: &mut Context<TrackPanel>,
    ) -> Self {
        // Asked for once per card, not per frame: a source that has to look at this machine,
        // as the plugin host does, pays for it here.
        let offers = Devices::offered(kind, cx);
        let entries = offer_entries(&offers, kind, true, session, cx);
        let label = device_label(session, &slot, kind, cx);
        let name = match kind {
            Slot::Instrument => SharedString::from("instrument-picker"),
            Slot::Effect => effect_picker(&slot),
        };
        let picker = cx.new(|cx| {
            let mut picker = DropdownMenu::new(label.name, entries, cx)
                .debug_name(name)
                .trigger(Trigger::Ghost)
                .width(280.);
            // The offer that is already there is marked, so the menu says what a card holds.
            if let Some(key) = label.key {
                picker = picker.selected(key);
            }
            picker
        });
        cx.subscribe(&picker, {
            let slot = slot.clone();
            move |panel: &mut TrackPanel, _, picked: &MenuPicked, cx| {
                panel.choose(&slot, &picked.0, cx);
            }
        })
        .detach();
        Self {
            tool: session.read(cx).project().tool_of(&slot),
            view: Views::view_of(session, &slot, window, cx),
            slot,
            kind,
            picker,
            offers,
            remove_focus: cx.focus_handle().tab_stop(true),
        }
    }
}

/// The one edit that puts an offer this project cannot load within reach.
fn needed(extension: &SharedString) -> String {
    sound_ui::enable_extension(extension)
}

/// What the picker of a slot says and which offer it marks. The device registry answers for a
/// tool it knows; else the name is the tool's own, or that the slot is empty, and no offer is
/// marked.
struct SlotLabel {
    name: SharedString,
    key: Option<SharedString>,
}

fn device_label(session: &Entity<Session>, slot: &InstanceId, kind: Slot, cx: &App) -> SlotLabel {
    if let Some(DeviceLabel { key, name }) = Devices::label_of(session, slot, cx) {
        return SlotLabel {
            name,
            key: Some(key),
        };
    }
    let empty = match kind {
        Slot::Instrument => EMPTY_SLOT,
        Slot::Effect => EMPTY_EFFECT_SLOT,
    };
    let name = match session.read(cx).project().tool_of(slot) {
        Some(tool) => tool.into(),
        None => SharedString::from(empty),
    };
    SlotLabel { name, key: None }
}

pub struct TrackPanel {
    session: Entity<Session>,
    track: Instance<TrackState>,
    devices: Vec<Device>,
    /// The control at the end of the rack that adds an effect. It is made once, with what the
    /// registry offers when the panel opens, like the picker of a card.
    add_effect: Entity<DropdownMenu>,
    /// What the sources of offers had said when the menus of this panel were filled. A source
    /// may learn more while the panel is open: the plugin host looks for the plugins of this
    /// Mac on a thread of its own, so a panel opened at the start of a session holds a part of
    /// the list and the quiet line that says so. Every menu is filled again when this changes.
    offers: u64,
    /// The gesture of a drag of a mixer knob.
    edit: ControlEdit,
    /// Not a tab stop. It tells whether the focus is inside the panel.
    focus_handle: FocusHandle,
    /// The knobs and the mute toggle bring their own. A button takes one to be a tab stop and
    /// show a ring.
    close_focus: FocusHandle,
}

impl EventEmitter<TrackPanelEvent> for TrackPanel {}

impl TrackPanel {
    pub(super) fn new(
        session: Entity<Session>,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe_in(&session, window, |panel, _, event, window, cx| {
            let (ProjectEvent::Created(id) | ProjectEvent::Changed(id) | ProjectEvent::Deleted(id)) =
                event
            else {
                return;
            };
            // The name, the colour and the mixer. The view that holds the panel closes it
            // with its track. A track deleted under a knob drag finishes the gesture and does
            // not cancel it: the delete was the last write.
            if id == panel.track.id() {
                if matches!(event, ProjectEvent::Deleted(_)) {
                    panel.end_drag(cx);
                }
                // The record says which effects the track has and in what order, so a change
                // of it may add, remove or move a slot. The rebuild keeps the card of every
                // slot that stays.
                panel.set_slots(window, cx);
                cx.notify();
            }
            // A record inside the track came or went and has no card: an effect record that
            // arrived before the list that names it, or after it. The slots are read again, so
            // the card shows as soon as both are there.
            if id.parent().as_ref() == Some(panel.track.id())
                && !panel.devices.iter().any(|device| device.slot == *id)
            {
                panel.set_slots(window, cx);
                cx.notify();
            }
            // A slot got another tool, lost its record or got one: from a file or an undo.
            // While the tool stays, the view of the device follows its record by itself.
            let Some(index) = panel.devices.iter().position(|device| device.slot == *id) else {
                return;
            };
            let session = panel.session.clone();
            let kind = panel.devices[index].kind;
            if panel.devices[index].tool != session.read(cx).project().tool_of(id) {
                panel.devices[index] = Device::new(&session, id.clone(), kind, window, cx);
                cx.notify();
                return;
            }
            // The tool stayed but its record changed, and a plugin record carries the name of
            // the card: another plugin id, from a file or an undo, renames it.
            let label = device_label(&session, id, kind, cx);
            let picker = panel.devices[index].picker.clone();
            picker.update(cx, |picker, cx| {
                picker.set_label(label.name, cx);
                if let Some(key) = label.key {
                    picker.set_selected(key, cx);
                }
            });
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|panel, cx| {
            panel.edit.finish(&panel.session, cx);
        })
        .detach();
        // A source of offers may learn more while this panel is open, and every menu of it is
        // filled again when it does. The session notifies on the poll that sees the change.
        cx.observe(&session, |panel, _, cx| panel.refill_menus(cx))
            .detach();
        let add_effect = cx.new(|cx| {
            let offers = Devices::offered(Slot::Effect, cx);
            let entries = offer_entries(&offers, Slot::Effect, false, &session, cx);
            DropdownMenu::new(ADD_EFFECT, entries, cx)
                .debug_name("add-effect")
                .trigger(Trigger::Ghost)
                .width(280.)
        });
        cx.subscribe(&add_effect, |panel: &mut TrackPanel, _, picked, cx| {
            panel.add_effect(&picked.0, cx);
        })
        .detach();
        let offers = Devices::offers_generation(cx);
        let mut panel = Self {
            session,
            track: track.clone(),
            devices: Vec::new(),
            add_effect,
            offers,
            edit: ControlEdit::default(),
            focus_handle: cx.focus_handle(),
            close_focus: cx.focus_handle().tab_stop(true),
        };
        panel.set_track(track, window, cx);
        panel
    }

    pub fn track(&self) -> &Instance<TrackState> {
        &self.track
    }

    /// The view in each card of the rack, left to right. `None` for a card without one.
    pub fn device_views(&self) -> impl Iterator<Item = Option<&AnyView>> {
        self.devices.iter().map(|device| device.view.as_ref())
    }

    /// The picker of each card of the rack, left to right. A snapshot opens one.
    pub fn pickers(&self) -> impl Iterator<Item = &Entity<DropdownMenu>> {
        self.devices.iter().map(|device| &device.picker)
    }

    /// The control at the end of the rack that adds an effect. A snapshot opens it.
    pub fn add_effect_control(&self) -> &Entity<DropdownMenu> {
        &self.add_effect
    }

    /// What each card of the rack is called, left to right: what its picker says.
    pub fn device_names(&self, cx: &App) -> Vec<SharedString> {
        let names = self.devices.iter();
        names
            .map(|device| device.picker.read(cx).label().clone())
            .collect()
    }

    /// Shows another track. A knob drag of the track it leaves ends first, so no gesture of
    /// this panel is ever left open.
    pub fn set_track(
        &mut self,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.end_drag(cx);
        self.track = track;
        self.devices.clear();
        self.set_slots(window, cx);
        cx.notify();
    }

    /// Fills every menu of the panel again when a source of offers has learned something: the
    /// picker of each card and the control that adds an effect, through one path.
    ///
    /// The offers themselves are read here and not while a frame draws, because a source may
    /// have to look at the machine. Nothing is read at all while the number is the one the
    /// menus were filled with, which is every poll but the few that change it.
    fn refill_menus(&mut self, cx: &mut Context<Self>) {
        let offers = Devices::offers_generation(cx);
        if offers == self.offers {
            return;
        }
        self.offers = offers;
        let session = self.session.clone();
        for device in &mut self.devices {
            device.offers = Devices::offered(device.kind, cx);
            let entries = offer_entries(&device.offers, device.kind, true, &session, cx);
            device
                .picker
                .update(cx, |picker, cx| picker.set_entries(entries, cx));
        }
        let effects = Devices::offered(Slot::Effect, cx);
        let entries = offer_entries(&effects, Slot::Effect, false, &session, cx);
        self.add_effect
            .update(cx, |add, cx| add.set_entries(entries, cx));
    }

    /// Reads the slots of the track again and makes the list of cards match them.
    ///
    /// A card that is already there for a slot is kept, whatever its place in the rack, so an
    /// open knob drag in one card goes on while another card is added, removed or moved.
    fn set_slots(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let slots = match crate::device_slots(project, &self.track) {
            Ok(slots) => slots,
            Err(error) => {
                let session = self.session.clone();
                session.update(cx, |session, cx| session.report(error, cx));
                return;
            }
        };
        if self.devices.len() == slots.len()
            && self
                .devices
                .iter()
                .zip(&slots)
                .all(|(had, slot)| &had.slot == slot)
        {
            return;
        }
        let mut kept: Vec<Device> = std::mem::take(&mut self.devices);
        self.devices = slots
            .into_iter()
            .enumerate()
            .map(|(index, slot)| {
                let kind = match index {
                    0 => Slot::Instrument,
                    _ => Slot::Effect,
                };
                match kept.iter().position(|device| device.slot == slot) {
                    Some(had) => kept.remove(had),
                    None => Device::new(&self.session, slot, kind, window, cx),
                }
            })
            .collect();
    }

    /// Puts the effect the composer picked at the end of the chain of the track: the record
    /// and the name in the list are one group, so adding an effect is one undo step.
    fn add_effect(&mut self, key: &SharedString, cx: &mut Context<Self>) {
        self.end_drag(cx);
        let offers = Devices::offered(Slot::Effect, cx);
        let Some(offer) = offers.into_iter().find(|offer| offer.key == *key) else {
            return;
        };
        // The menu does not take a disabled row, and neither does this: the tool would not
        // load, so the edit would be refused and the composer would learn nothing.
        if !offer.is_enabled_in(self.session.read(cx).project()) {
            return;
        }
        let track = self.track.clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let slot = crate::add_effect(project, &mut changes, &track, &offer.name)?;
                offer.write(project, &slot, &mut changes)?;
                project.commit(&format!("Add {}", offer.name), changes)
            });
        });
    }

    /// Takes an effect off the track: the record and its name in the list go in one group, so
    /// undo brings it back where it was, and a plugin as it sounded.
    fn remove_effect(&mut self, slot: &InstanceId, cx: &mut Context<Self>) {
        self.end_drag(cx);
        let name = device_label(&self.session, slot, Slot::Effect, cx).name;
        let (track, slot) = (self.track.clone(), slot.clone());
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                crate::remove_effect(project, &mut changes, &track, &slot)?;
                project.commit(&format!("Remove {name}"), changes)
            });
        });
    }

    /// A callback of a control. It holds the view weakly, as `cx.listener` does, so the
    /// listeners of the last frame keep no closed panel and no open drag alive.
    fn callback<E>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, E, &mut Context<Self>) + 'static,
    ) -> impl Fn(E, &mut Window, &mut App) + 'static {
        let panel = cx.weak_entity();
        move |event, _, cx| {
            panel.update(cx, |panel, cx| f(panel, event, cx)).ok();
        }
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.edit.finish(&self.session, cx);
    }

    /// Puts what the composer picked into the slot, as one undo step. It replaces the whole
    /// record, so undo brings the device that was there back as it was, and a plugin as it
    /// sounded: the host saves one on its way out.
    fn choose(&mut self, slot: &InstanceId, key: &SharedString, cx: &mut Context<Self>) {
        let found = self.devices.iter().find(|device| device.slot == *slot);
        let Some(kind) = found.map(|device| device.kind) else {
            return;
        };
        // What is there already: picking it again would write a fresh record over it, which
        // for a plugin means a new and empty state file.
        if device_label(&self.session, slot, kind, cx).key.as_ref() == Some(key) {
            return;
        }
        self.end_drag(cx);
        let found = self
            .devices
            .iter()
            .find(|device| device.slot == *slot)
            .and_then(|device| device.offers.iter().find(|offer| offer.key == *key));
        let Some(offer) = found.cloned() else {
            return;
        };
        // The menu does not take a disabled row, and neither does this: the tool would not
        // load, so the edit would be refused and the composer would learn nothing.
        if !offer.is_enabled_in(self.session.read(cx).project()) {
            return;
        }
        let slot = slot.clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                offer.write(project, &slot, &mut changes)?;
                project.commit(&format!("Choose {}", offer.name), changes)
            });
        });
    }

    fn on_knob(&mut self, control: &Control, change: ValueChange, cx: &mut Context<Self>) {
        let (session, track) = (&self.session, &self.track);
        self.edit
            .apply(session, track, control.undo_label, change, control.set, cx);
    }

    fn knob(&self, control: &'static Control, track: &TrackState, cx: &mut Context<Self>) -> Knob {
        let value = (control.get)(track);
        Knob::new(control.field)
            .range(control.knob_range())
            .bipolar(control.bipolar)
            .value(value)
            // Both knobs rest at 0: no change of level, and the middle.
            .default_value(0.)
            .label(control.label)
            .readout((control.readout)(value))
            .on_change(Self::callback(cx, move |panel, change, cx| {
                panel.on_knob(control, change, cx)
            }))
    }

    /// The fixed section at the right end: the mixer of the track.
    fn mixer(&self, track: &TrackState, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (title, hairline, peach) = (theme.gray_900, theme.alpha_at(0.05), theme.peach);
        let mute = Toggle::new("mute", "Mute", track.mute)
            .color(peach)
            .on_change(Self::callback(cx, |panel, mute: bool, cx| {
                let label = if mute { "Mute track" } else { "Unmute track" };
                let change = ValueChange::Set(mute);
                let (session, track) = (&panel.session, &panel.track);
                panel.edit.apply(
                    session,
                    track,
                    label,
                    change,
                    |track, mute| track.mute = mute,
                    cx,
                );
            }));
        // In a cell of its own, where the knobs are, as the waveform switch of the synth is. It
        // gets no label under it: it says what it is.
        let mute = Cell::new(mute);

        div()
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(24.))
            .border_l_1()
            .border_color(hairline)
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(title)
                    .child("Mixer"),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(self.knob(&GAIN, track, cx))
                    .child(self.knob(&PAN, track, cx))
                    .child(mute),
            )
    }
}

impl Focusable for TrackPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TrackPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Gone: the view that holds the panel closes it after the same event. Read once,
        // because the mixer section needs the record while it makes its controls.
        let track = self.session.read(cx).project().state(&self.track).cloned();
        let theme = cx.theme();
        let (background, hairline, text, muted) = (
            theme.gray_100,
            theme.alpha_at(0.05),
            theme.gray_900,
            theme.gray_700,
        );
        let (name, dot) = track.as_ref().map_or_else(
            || (String::new(), theme.blue),
            |track| (track.name.clone(), accent(track.colour, theme)),
        );

        // Every card begins with its picker, which says what is in the slot and is how the
        // composer puts something else there. The card holds no other title: one name each.
        // An effect card has the control that takes it off the track next to its name.
        let cards: Vec<_> = self
            .devices
            .iter()
            .map(|device| {
                let mut title = div().flex().items_center().gap(px(4.)).ml(px(-8.));
                title = title.child(device.picker.clone());
                if device.kind == Slot::Effect {
                    let slot = device.slot.clone();
                    // The name of the slot, because every card of the rack is drawn by this
                    // one view: two controls of one id would be one control to GPUI, and the
                    // second effect of a track would not be the one that goes.
                    let name = remove_control(&device.slot);
                    let remove = Button::icon_only(name.clone(), "x")
                        .debug_selector(move || name.to_string())
                        // Quiet until it is wanted, like the close control of the panel.
                        .opacity(0.6)
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Xs)
                        .focus_handle(&device.remove_focus)
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            panel.remove_effect(&slot, cx);
                        }));
                    title = title.child(remove);
                }
                let card = Card::new()
                    .flex_none()
                    .gap(px(12.))
                    // The trigger is 32 px tall and brings its own padding, so the card gives
                    // it 8 px less on the top and the left and its text lands where a card
                    // title is.
                    .pt(px(8.))
                    .child(title);
                let empty = match device.kind {
                    Slot::Instrument => "This track is silent.",
                    Slot::Effect => "This slot has no record.",
                };
                match (&device.view, device.tool) {
                    (Some(view), _) => card.child(view.clone()),
                    (None, tool) => card.child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(muted)
                            .child(match tool {
                                Some(_) => "This tool has no view.",
                                None => empty,
                            }),
                    ),
                }
            })
            .collect();

        // The control that adds an effect, at the end of the rack: the same picker pattern,
        // with what declares itself an effect in it. It is not a card: nothing is in it yet.
        let add_effect = div()
            .flex_none()
            .flex()
            .items_center()
            .h(px(32.))
            .child(self.add_effect.clone());

        let close = Button::icon_only("close-track-panel", "x")
            // Quiet until it is wanted, as in the note editor.
            .opacity(0.6)
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Xs)
            .focus_handle(&self.close_focus)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(TrackPanelEvent::Close)));
        // The header is that of the note editor: the dot and the name of the track in a row
        // as high as a ruler, where the track headers are above, then the close control.
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
                    .top(px(RULER_HEIGHT / 2. - 4.))
                    .size(px(8.))
                    .rounded_full()
                    .bg(dot),
            )
            .child(
                div()
                    .absolute()
                    .left(px(44.))
                    .top(px(RULER_HEIGHT / 2. - 10.))
                    .w(px(HEADER_WIDTH - 44. - 40.))
                    .truncate()
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(text)
                    .child(name),
            )
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .left(px(HEADER_WIDTH - 8. - 24.))
                    .child(close),
            );

        div()
            .size_full()
            .relative()
            .flex()
            .bg(background)
            .track_focus(&self.focus_handle)
            // One hairline above the panel. It is on top and takes no room, as in the note
            // editor, so the header is at the same place in both.
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
            // The rack. The transport floats over the bottom of the panel, so the cards are at
            // the top, clear of it. It scrolls sideways: a card is as wide as its controls,
            // and a narrow window must not cut the last ones off.
            .child(
                div()
                    .id("rack")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_x_scroll()
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .p(px(24.))
                    .children(cards)
                    .child(add_effect),
            )
            // The mixer of the track, at the right end and outside what scrolls.
            .children(track.map(|track| self.mixer(&track, cx)))
    }
}
