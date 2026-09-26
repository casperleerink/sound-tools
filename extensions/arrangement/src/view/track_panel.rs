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
//! The view of a device draws its whole card, from the frame the panel gives it: the picker as
//! its title, and the power and close icons of an effect. Whether an effect is on is saved on
//! its slot in the track record, so the panel edits it.
//!
//! The mixer strip of the track (volume, pan, mute and solo) is not a device. It is in the
//! header column under the name of the track, on the rows of the cards, and the panel edits it
//! itself: those values are in the track record. The meter of the volume shows what the track
//! sends to the master.
//!
//! The panel is 216 pt: 12 above the cards, a card of 192, 12 below. The rack scrolls sideways
//! with two fingers, and a fade at its right edge says when cards go past it.

use gpui::{
    AnyView, App, Bounds, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    ScrollHandle, SharedString, Task, Window, canvas, div, fill, linear_color_stop,
    linear_gradient, prelude::*, px,
};
use sound_core::{Changes, Instance, InstanceId, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::cell::{CONTROL_HEIGHT, ROW_HEIGHT};
use sound_ui::components::device_card::{CARD_HEIGHT, CardFrame, HEADER_HEIGHT, PLAIN_CARD_WIDTH};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::toggle::{self, Toggle};
use sound_ui::components::volume::Volume;
use sound_ui::{
    ActiveTheme, ControlEdit, DeviceLabel, DeviceOffer, Devices, Metering, Session, Slot, Views,
    every_poll, weak_action, weak_callback,
};

use super::layout::HEADER_WIDTH;
use super::paint::accent;
use crate::TrackState;

/// The height of the panel: the cards and 12 pt above and below them.
pub const PANEL_HEIGHT: f32 = CARD_HEIGHT + 2. * RACK_TOP;
/// From the top of the panel to the top of the cards.
pub(super) const RACK_TOP: f32 = 12.;
/// From the header column to the first card.
pub(super) const RACK_LEFT: f32 = 16.;
const CARD_GAP: f32 = 12.;
/// The fade at the right edge of the rack when cards go past it.
const FADE_WIDTH: f32 = 48.;
/// The middle of the title line of the cards, where the name of the track is too.
pub(super) const TITLE_MIDDLE: f32 = RACK_TOP + HEADER_HEIGHT / 2.;
/// The top of the first row of cells, where the mixer strip starts.
pub(super) const ROW_TOP: f32 = RACK_TOP + HEADER_HEIGHT;
/// The left of the volume in the header column, and of the column of pan and mute.
pub(super) const VOLUME_LEFT: f32 = 8.;
const PAN_LEFT: f32 = 84.;
/// Solo right of mute, a toggle and 4 pt of air on.
const SOLO_LEFT: f32 = PAN_LEFT + toggle::LETTER_WIDTH + 4.;

/// What an undo step of the volume is called.
pub(super) const VOLUME_LABEL: &str = "Change volume";
const PAN_LABEL: &str = "Change pan";

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

/// The id of the card of a slot. It names the slot, because two cards of one device, such as two
/// filters, must keep a drag and a focus each.
fn card_id(slot: &InstanceId) -> SharedString {
    format!("card-{}", slot.name()).into()
}

/// What a test finds the close icon of an effect card by, which takes the effect off the track.
pub fn remove_control(slot: &InstanceId) -> String {
    format!("{}-close", card_id(slot))
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
                // An offer this project cannot load is shown and not taken, with why in
                // words. Enabling an extension while the project runs is refused, and the
                // file edit that does it is in the agent docs, not in front of a composer.
                match (offer.is_enabled_in(project), &offer.needs, &offer.detail) {
                    (false, Some(needs), _) => {
                        item.disabled(true).description(needs.reason.clone())
                    }
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
    /// The title and the close icon of the card, which the view of the device draws, or the
    /// panel when the tool has no card.
    frame: CardFrame,
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
        let frame = CardFrame::new(card_id(&slot), picker.clone());
        // An effect comes off the track by the close icon of its card, and is bypassed by its
        // power icon. Both hold the panel weakly, as every callback of a control does. Whether
        // it is on is read from the track record when the card draws.
        let frame = match kind {
            Slot::Instrument => frame,
            Slot::Effect => {
                let (panel, removed, toggled) = (cx.weak_entity(), slot.clone(), slot.clone());
                let (session, name) = (session.clone(), slot.name().to_string());
                let track = slot
                    .parent()
                    .and_then(|track| session.read(cx).project().resolve::<TrackState>(&track));
                let is_on = move |cx: &App| {
                    let project = session.read(cx).project();
                    let track = track.as_ref().and_then(|track| project.state(track));
                    !track
                        .and_then(|track| track.bypassed(&name))
                        .unwrap_or(false)
                };
                let power_panel = panel.clone();
                frame
                    .power(is_on, move |_, cx| {
                        power_panel
                            .update(cx, |panel, cx| panel.toggle_bypass(&toggled, cx))
                            .ok();
                    })
                    .close(move |_, cx| {
                        panel
                            .update(cx, |panel, cx| panel.remove_effect(&removed, cx))
                            .ok();
                    })
            }
        };
        Self {
            tool: session.read(cx).project().tool_of(&slot),
            view: Views::card_of(session, &slot, frame.clone(), window, cx),
            slot,
            kind,
            picker,
            offers,
            frame,
        }
    }
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
    /// The gesture of a drag of the volume or the pan.
    edit: ControlEdit,
    /// Not a tab stop. It tells whether the focus is inside the panel.
    focus_handle: FocusHandle,
    /// The controls of the mixer strip bring their own. A button takes one to be a tab stop
    /// and show a ring.
    close_focus: FocusHandle,
    /// Where the rack is scrolled, and how far it can go, for the fade at its right edge.
    rack_scroll: ScrollHandle,
    /// The meter of the volume: what the track sends to the master.
    metering: Metering,
    _metering: Task<()>,
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
            rack_scroll: ScrollHandle::new(),
            metering: Metering::default(),
            _metering: every_poll(cx, Self::read_meter),
        };
        panel.set_track(track, window, cx);
        panel
    }

    pub fn track(&self) -> &Instance<TrackState> {
        &self.track
    }

    /// One poll of the meter of the volume. Its timer calls it; a snapshot calls it to skip
    /// the wait.
    pub fn read_meter(&mut self, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let peaks = crate::track_peaks(project, self.track.id());
        if self.metering.read(peaks.as_ref()) {
            cx.notify();
        }
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
        self.metering.reset();
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

    /// Bypasses an effect, or turns it on again: one flag on its slot in the track record, as
    /// one undo step named after it.
    fn toggle_bypass(&mut self, slot: &InstanceId, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let Some(mut state) = project.state(&self.track).cloned() else {
            return;
        };
        let Some(effect) = state
            .effects
            .iter_mut()
            .find(|effect| effect.name == slot.name())
        else {
            return;
        };
        effect.bypass = !effect.bypass;
        let name = device_label(&self.session, slot, Slot::Effect, cx).name;
        let label = match effect.bypass {
            true => format!("Turn off {name}"),
            false => format!("Turn on {name}"),
        };
        self.end_drag(cx);
        let track = self.track.clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.set(&track, state);
                project.commit(&label, changes)
            });
        });
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

    /// The mixer strip, in the header column on the rows of the cards: the volume at the left
    /// from the top of the first row to the value line of the second, the pan in the first row
    /// right of it, and mute and solo on the knob line of the second.
    fn mixer_strip(&self, track: &TrackState, cx: &mut Context<Self>) -> [Div; 4] {
        let (peach, yellow) = (cx.theme().peach, cx.theme().yellow);
        let volume = Volume::new("gain_db", track.gain_db)
            .level(self.metering.level())
            .on_clear_clip(weak_action(cx, |panel: &mut Self, cx| {
                panel.metering.clear_clip();
                cx.notify();
            }))
            .on_change(weak_callback(cx, |panel, change: ValueChange, cx| {
                let (session, track) = (&panel.session, &panel.track);
                let set = |track: &mut TrackState, db: f32| track.gain_db = volume_db(db);
                panel
                    .edit
                    .apply(session, track, VOLUME_LABEL, change, set, cx);
            }));
        let pan = Knob::new("pan")
            .range(KnobRange::linear(TrackState::PAN.0, TrackState::PAN.1))
            .bipolar(true)
            .value(track.pan)
            // The middle.
            .default_value(0.)
            .label("Pan")
            .readout(pan_readout(track.pan))
            .on_change(weak_callback(cx, |panel, change, cx| {
                let (session, track) = (&panel.session, &panel.track);
                let set = |track: &mut TrackState, pan| track.pan = pan;
                panel.edit.apply(session, track, PAN_LABEL, change, set, cx);
            }));
        let mute = Toggle::new("mute", "M", track.mute)
            .color(peach)
            .on_change(weak_callback(cx, |panel, mute: bool, cx| {
                let label = if mute { "Mute track" } else { "Unmute track" };
                let change = ValueChange::Set(mute);
                let (session, track) = (&panel.session, &panel.track);
                let set = |track: &mut TrackState, mute| track.mute = mute;
                panel.edit.apply(session, track, label, change, set, cx);
            }));
        let solo = Toggle::new("solo", "S", track.solo)
            .color(yellow)
            .on_change(weak_callback(cx, |panel, solo: bool, cx| {
                let label = if solo { "Solo track" } else { "Unsolo track" };
                let change = ValueChange::Set(solo);
                let (session, track) = (&panel.session, &panel.track);
                let set = |track: &mut TrackState, solo| track.solo = solo;
                panel.edit.apply(session, track, label, change, set, cx);
            }));
        let at = |left: f32, top: f32| div().absolute().left(px(left)).top(px(top));
        // A toggle sits on the line of the middle of a knob.
        let toggle_top = ROW_TOP + ROW_HEIGHT + (CONTROL_HEIGHT - toggle::HEIGHT) / 2.;
        [
            at(VOLUME_LEFT, ROW_TOP).child(volume),
            at(PAN_LEFT, ROW_TOP).child(pan),
            at(PAN_LEFT, toggle_top).child(mute),
            at(SOLO_LEFT, toggle_top).child(solo),
        ]
    }
}

/// The gain a volume control saves: its bottom is `-inf`, which the record keeps as silence,
/// and nothing goes over the top of the record.
pub(super) fn volume_db(db: f32) -> f32 {
    match db.is_nan() {
        true => f32::NEG_INFINITY,
        false => db.min(TrackState::MAX_GAIN_DB),
    }
}

impl Focusable for TrackPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The fade at the right edge of the rack, while cards go past it. It reads the scroll while it
/// paints, after the rack was laid out in the same frame, so it is right after a card came or
/// went and after a resize, not a frame late.
fn fade(scroll: ScrollHandle, color: gpui::Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<gpui::Pixels>, (), window, _| {
            let (scrolled, most) = (-scroll.offset().x, scroll.max_offset().x);
            if most - scrolled < px(1.) {
                return;
            }
            let gradient = linear_gradient(
                90.,
                linear_color_stop(color.opacity(0.), 0.),
                linear_color_stop(color, 1.),
            );
            window.paint_quad(fill(bounds, gradient));
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .w(px(FADE_WIDTH))
    .h_full()
}

impl Render for TrackPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Gone: the view that holds the panel closes it after the same event. Read once,
        // because the mixer strip needs the record while it makes its controls.
        let track = self.session.read(cx).project().state(&self.track).cloned();
        let theme = cx.theme();
        let (background, hairline, text, muted) = (
            theme.gray_100,
            theme.alpha_at(0.05),
            theme.gray_950,
            theme.gray_800,
        );
        let (name, dot) = track.as_ref().map_or_else(
            || (String::new(), theme.blue),
            |track| (track.name.clone(), accent(track.colour, theme)),
        );

        // The view of a device draws its whole card. A slot whose tool has no card gets one
        // from the panel, with the same frame: the picker and the close icon.
        let cards: Vec<_> = self
            .devices
            .iter()
            .map(|device| match &device.view {
                Some(view) => view.clone().into_any_element(),
                None => {
                    let says = match (device.tool, device.kind) {
                        (Some(_), _) => "This tool has no view.",
                        (None, Slot::Instrument) => "This track is silent.",
                        (None, Slot::Effect) => "This slot has no record.",
                    };
                    let line = div()
                        .text_size(px(12.))
                        .line_height(px(14.))
                        .text_color(muted)
                        .child(says);
                    let card = device.frame.card().w(px(PLAIN_CARD_WIDTH));
                    card.child(line).into_any_element()
                }
            })
            .collect();

        // The control that adds an effect, at the end of the rack, on the line of the card
        // titles: the same picker pattern, with what declares itself an effect in it. It is
        // not a card: nothing is in it yet.
        let add_effect = div()
            .flex_none()
            .flex()
            .items_center()
            .h(px(HEADER_HEIGHT))
            .child(self.add_effect.clone());

        let close = Button::icon_only("close-track-panel", "x")
            // Quiet until it is wanted, as in the note editor.
            .opacity(0.6)
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Xs)
            .focus_handle(&self.close_focus)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(TrackPanelEvent::Close)));
        let strip = track
            .as_ref()
            .map(|track| self.mixer_strip(track, cx))
            .into_iter()
            .flatten();
        // The header column: the dot and the name of the track on the line of the card
        // titles, the close icon at its right, and the mixer strip on the rows of the cards.
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
                    .bg(dot),
            )
            .child(
                div()
                    .absolute()
                    .left(px(44.))
                    .top(px(TITLE_MIDDLE - 10.))
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
                    .top(px(TITLE_MIDDLE - 12.))
                    .left(px(HEADER_WIDTH - 8. - 24.))
                    .child(close),
            )
            .children(strip);

        let scroll = self.rack_scroll.clone();
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
            // The rack. Two-finger scroll moves it sideways, and no control in it takes that
            // gesture, so scrolling past a knob never changes a sound.
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .id("rack")
                            .track_scroll(&self.rack_scroll)
                            .size_full()
                            .overflow_x_scroll()
                            .flex()
                            .items_start()
                            .gap(px(CARD_GAP))
                            .pt(px(RACK_TOP))
                            .px(px(RACK_LEFT))
                            .children(cards)
                            .child(add_effect),
                    )
                    .child(fade(scroll, background)),
            )
    }
}
