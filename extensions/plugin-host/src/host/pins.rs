//! The pins of a record, both ways, in one place: [`Plugins::follow_pins`].
//!
//! A pin the record changed is sent to the plugin. A pin the plugin changed itself is written to
//! the record, as one undo step per turn of a knob. The host remembers what it last sent and read
//! of each pin, so a value on its way to the plugin is not taken for a change of the plugin's,
//! and a value the plugin rounds as it takes it is not written back. A pin an automation lane
//! moves is not read at all: the lane plays into the plugin on the audio thread, and what it
//! plays is never the composer's edit. Which pins a lane holds the audio side says itself
//! ([`crate::processor::LanedPins`]), before the plugin hears the lane and until it has played
//! the record value again, so a lane value is never taken for the plugin's own change.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use sound_core::{InstanceId, Project, ProjectEdit as Edit, ProjectError};

use super::{Hosted, Plugins, record_of};
use crate::PluginRecord;
use crate::backend::{Hand, ParameterChange};
use crate::parameters::playable;
use crate::processor::{AutomatedPin, AutomatedPins};

/// How long the pins of a plugin that says nothing of a hand must be quiet before what it
/// changed is one undo step: CLAP, whose gesture events come out of a block this host does not
/// read, and a plugin that moves a parameter itself. Long enough for the pauses of a slow hand,
/// short enough that two turns are two steps.
const QUIET: Duration = Duration::from_millis(500);

/// The same for a VST 3 hand that never lets go: a plugin that sends `beginEdit` without its
/// `endEdit` still gets its turn written.
const HELD_QUIET: Duration = Duration::from_secs(5);

/// A lane value the plugin's own window was shown, and the value it showed then. `None` where
/// the host shows the window nothing, as for CLAP.
#[derive(Copy, Clone, Debug)]
pub(super) struct Shown {
    sent: f64,
    seen: Option<f64>,
}

/// A pin, as the host last sent it or read it.
#[derive(Copy, Clone, Debug)]
pub(super) struct PinState {
    /// The value of the record the plugin was sent, or the plugin's own value the record was
    /// given. A record that holds another one was changed by someone else, and that is sent.
    record: f64,
    /// What the plugin said it was the last time it was asked. `None` from a send until the
    /// plugin has played it: the first value read then is the plugin taking it, perhaps
    /// rounded, and not a change of its own. A hand on the same knob in that moment is taken
    /// for the plugin taking the value, and is not written.
    plugin: Option<f64>,
}

impl PinState {
    fn sent(value: f64) -> Self {
        Self {
            record: value,
            plugin: None,
        }
    }

    /// What the plugin says now. `Some` when it moved the parameter itself, to a value the
    /// record does not hold yet: that goes into the record.
    fn read(&mut self, now: f64) -> Option<f64> {
        let before = self.plugin.replace(now);
        let moved = before.is_some_and(|before| !same(before, now)) && !same(now, self.record);
        if moved {
            self.record = now;
        }
        moved.then_some(now)
    }
}

/// Two values bit for bit, so a value that is not a number counts as one value and not as a
/// change at every poll.
fn same(one: f64, other: f64) -> bool {
    one.to_bits() == other.to_bits()
}

/// What one plugin changed of its pins itself since the last poll, and where the hand is.
struct PinsMoved {
    changes: Vec<ParameterChange>,
    hand: Hand,
}

/// What the plugin of one record changed of its pins, as one undo step that is still open.
pub(super) struct PinGesture {
    edit: Edit,
    /// When the plugin last changed one, for [`QUIET`].
    changed: Instant,
    /// The record as the turn last wrote it. A record that is not this any more was written
    /// by someone else since: deleted, given another plugin, an undo, or another pin. That
    /// write is the last, and its step already starts from before the turn, so the turn ends
    /// with no step of its own: finishing it would be a second step for the same change, and
    /// cancelling it would undo the newer write.
    written: Option<PluginRecord>,
}

impl PinGesture {
    /// Whether someone else wrote the record since the turn last did.
    fn overtaken(&self, project: &Project, id: &InstanceId) -> bool {
        record_of(project, id) != self.written.as_ref()
    }

    /// Whether the turn is over, by what the plugin says of the hand, or else by time.
    fn is_over(&self, hand: Hand, now: Instant) -> bool {
        let quiet = now.saturating_duration_since(self.changed);
        match hand {
            Hand::Held => quiet >= HELD_QUIET,
            Hand::LetGo => true,
            Hand::Unknown => quiet >= QUIET,
        }
    }
}

impl Hosted {
    /// The pins of `record` whose value the plugin takes, in the order of their ids, and kept
    /// as the pins the next start of the plugin gets. An automation lane may move those whose
    /// parameter [`Parameter::takes_lane`].
    pub(super) fn automated(&mut self, record: &PluginRecord) -> Vec<AutomatedPin> {
        let mut pins = Vec::new();
        if !record.parameters.is_empty() {
            let parameters = self.parameters();
            let all = record.parameters.iter();
            pins = all
                .filter_map(|(id, pin)| {
                    let parameter = parameters.get(id).filter(|it| it.takes(pin.value))?;
                    Some(AutomatedPin {
                        id: *id,
                        takes_lane: parameter.takes_lane_at(pin),
                        minimum: parameter.minimum,
                        maximum: parameter.maximum,
                        record: pin.value,
                    })
                })
                .collect();
        }
        self.lanes = AutomatedPins::new(&pins);
        pins
    }

    /// The generation of the pins a lane holds, and each with the value it plays, as the
    /// audio side wrote them last. `None` when blocks kept writing while it was read.
    fn held(&self) -> Option<(u64, BTreeMap<u32, f64>)> {
        let (generation, held) = self.laned.read()?;
        Some((generation, held.into_iter().collect()))
    }

    /// Where the format needs the host for it, shows the plugin's own window the value each
    /// pin in `held` plays, and the record value again once its lane lets go. A window that
    /// shows something else by then was turned by the composer since: it is left as it is, and
    /// the next read of the pin takes that as an edit.
    fn follow_lanes(&mut self, held: &BTreeMap<u32, f64>) {
        let mut shown = BTreeMap::new();
        for (id, value) in held {
            let was = self.shown.get(id).copied();
            let seen = match was {
                Some(was) if same(was.sent, *value) => was.seen,
                _ => {
                    let change = ParameterChange {
                        id: *id,
                        value: *value,
                    };
                    // As the window took it, perhaps rounded: what a release compares with.
                    let shows = self.plugin.show(change);
                    shows.then(|| self.plugin.value(*id).unwrap_or(*value))
                }
            };
            let sent = *value;
            shown.insert(*id, Shown { sent, seen });
        }
        let released = self.shown.iter().filter(|(id, _)| !held.contains_key(id));
        for (id, was) in released {
            // A window that follows the processor by itself needs nothing. Its first read is
            // the plugin taking the record value again, as for every pin a lane held.
            let (Some(state), Some(seen)) = (self.pins.get_mut(id), was.seen) else {
                continue;
            };
            let now = self.plugin.value(*id);
            if now.is_some_and(|now| same(now, seen)) {
                let record = ParameterChange {
                    id: *id,
                    value: state.record,
                };
                self.plugin.show(record);
            } else {
                state.plugin = Some(seen);
            }
        }
        self.shown = shown;
    }

    /// The record to the plugin: every pin that is new, or whose value is not the one the host
    /// last sent or wrote, was changed by someone else and is sent. A pin that moves nothing is
    /// not sent and not followed; the behaviour reports it.
    pub(super) fn send_pins(&mut self, record: &PluginRecord) {
        if record.parameters.is_empty() {
            self.pins.clear();
            return;
        }
        self.parameters();
        let Self {
            parameters: Some(parameters),
            plugin,
            pins,
            ..
        } = self
        else {
            return;
        };
        let playable: Vec<ParameterChange> = playable(parameters, &record.parameters).collect();
        pins.retain(|id, _| playable.iter().any(|change| change.id == *id));
        for change in playable {
            if !pins
                .get(&change.id)
                .is_some_and(|state| same(state.record, change.value))
            {
                plugin.send(change);
                pins.insert(change.id, PinState::sent(change.value));
            }
        }
    }

    /// The plugin to the record: what the plugin changed of its pins itself since the last
    /// read, once every value sent is in what it says.
    ///
    /// A pin a lane holds is not read, and its first read once the lane is gone is the plugin
    /// taking its record value again, not a change of its own. The audio side writes a pin held
    /// before the plugin hears its lane, and free only after the plugin played its record value
    /// again, so a pin held in the table as read before or after the plugin's values is left
    /// out. A lane that came or went in between moves the generation of the table, and then
    /// nothing is written this round: the next poll reads again.
    fn read_pins(&mut self) -> Vec<ParameterChange> {
        let before = self.held();
        let mut values = Vec::new();
        if self.plugin.sent_values_played() {
            for id in self.pins.keys() {
                if let Some(now) = self.plugin.value(*id) {
                    values.push((*id, now));
                }
            }
        }
        let after = self.held();
        let laned: BTreeSet<u32> = match (&before, &after) {
            (Some((first, before)), Some((last, after))) if first == last => {
                before.keys().chain(after.keys()).copied().collect()
            }
            // A lane came or went while the plugin was read, or blocks kept writing all the
            // while: what was read may be a lane's.
            _ => {
                if let Some((_, after)) = &after {
                    self.follow_lanes(after);
                }
                return Vec::new();
            }
        };
        for (id, state) in &mut self.pins {
            if laned.contains(id) {
                state.plugin = None;
            }
        }
        if let Some((_, after)) = &after {
            self.follow_lanes(after);
        }
        let mut changes = Vec::new();
        for (id, now) in values {
            if laned.contains(&id) {
                continue;
            }
            if let Some(state) = self.pins.get_mut(&id)
                && let Some(value) = state.read(now)
            {
                changes.push(ParameterChange { id, value });
            }
        }
        changes
    }
}

/// The pins of the record of `id` that the plugin of `hosted` takes, as its record has them now.
pub(super) fn pins_of(hosted: &Hosted, project: &Project, id: &InstanceId) -> Vec<ParameterChange> {
    let (Some(record), Some(parameters)) = (record_of(project, id), &hosted.parameters) else {
        return Vec::new();
    };
    playable(parameters, &record.parameters).collect()
}

/// The name of the undo step of a turn of a knob: the pin it began with, or else the plugin.
fn turn_label(record: Option<&PluginRecord>, pin: u32) -> String {
    let name = record.map(|record| match record.parameters.get(&pin) {
        Some(pin) if !pin.name.is_empty() => pin.name.as_str(),
        _ => record.plugin_id.as_str(),
    });
    format!("Change {}", name.unwrap_or("plugin"))
}

/// What a host that loads no plugin says a pin takes a lane for: every pin, with any value. It
/// cannot ask the plugin, and it reports no problem of a pin either, so `--inspect` does not
/// report a lane on a pin that plays. A range this wide still works out a place on it.
pub(super) fn listed_lanes(record: &PluginRecord) -> Vec<AutomatedPin> {
    let widest = f64::from(f32::MAX / 2.0);
    let pins = record.parameters.iter();
    let pins = pins.map(|(id, pin)| AutomatedPin {
        id: *id,
        takes_lane: true,
        minimum: -widest,
        maximum: widest,
        record: pin.value,
    });
    pins.collect()
}

impl Plugins {
    /// The pins of every plugin, both ways: what a record changed goes to its plugin, and what
    /// a plugin changed itself goes to its record, one undo step for each turn of a knob. Call
    /// it after [`Self::poll`], as often. It needs the project mutably for the second way only,
    /// which is an edit like one of the window; a host of a project open read-only does
    /// nothing of that.
    pub fn follow_pins(&self, project: &mut Project) -> Vec<ProjectError> {
        self.follow_pins_at(project, Instant::now())
    }

    /// [`Self::follow_pins`] with the time given, so a test can move it past [`QUIET`].
    pub fn follow_pins_at(&self, project: &mut Project, now: Instant) -> Vec<ProjectError> {
        // One: with the table borrowed, every pin a record changed goes to its plugin, and
        // what each plugin changed itself is read. No record is written here: a write runs the
        // behaviour of the record, which asks this table.
        let mut moved = Vec::new();
        {
            let mut table = self.0.table.borrow_mut();
            for (id, hosted) in &mut table.loaded {
                // An entry whose record is gone or names another plugin is the poll's to let go.
                let Some(record) = record_of(project, id).filter(|record| hosted.holds(record))
                else {
                    continue;
                };
                hosted.send_pins(record);
                let changes = match self.0.writes_state {
                    true => hosted.read_pins(),
                    // A project open read-only writes nothing, so its host reads nothing.
                    false => Vec::new(),
                };
                let hand = hosted.plugin.hand();
                moved.push((id.clone(), PinsMoved { changes, hand }));
            }
        }
        // Two: what the plugins changed goes into their records, and a turn of a knob that is
        // over ends its undo step. Gestures are taken out while records are written, because a
        // write runs a behaviour, and put back after.
        let gestures = std::mem::take(&mut *self.0.gestures.borrow_mut());
        let (overtaken, mut gestures): (BTreeMap<_, _>, BTreeMap<_, _>) = gestures
            .into_iter()
            .partition(|(id, gesture)| gesture.overtaken(project, id));
        for gesture in overtaken.into_values() {
            project.abandon(gesture.edit);
        }
        let mut errors = Vec::new();
        for (id, moved) in &moved {
            let Some(first) = moved.changes.first() else {
                continue;
            };
            let Some(instance) = project.resolve::<PluginRecord>(id) else {
                continue;
            };
            let gesture = gestures.entry(id.clone()).or_insert_with(|| PinGesture {
                edit: project.begin(&turn_label(project.state(&instance), first.id)),
                changed: now,
                written: None,
            });
            gesture.changed = now;
            let written = project.update(&mut gesture.edit, &instance, |record| {
                for change in &moved.changes {
                    if let Some(pin) = record.parameters.get_mut(&change.id) {
                        pin.value = change.value;
                    }
                }
            });
            gesture.written = record_of(project, id).cloned();
            errors.extend(written.err());
        }
        // A turn ends when the plugin says the hand let go, or else by time. One whose plugin
        // went with its record unchanged, as a load that failed, ends as it stands.
        let hands: BTreeMap<&InstanceId, Hand> =
            moved.iter().map(|(id, moved)| (id, moved.hand)).collect();
        let mut open = BTreeMap::new();
        for (id, gesture) in gestures {
            match hands.get(&id) {
                Some(hand) if !gesture.is_over(*hand, now) => {
                    open.insert(id, gesture);
                }
                _ => errors.extend(project.finish(gesture.edit).err()),
            }
        }
        *self.0.gestures.borrow_mut() = open;
        errors
    }

    /// Ends every turn of a knob that is still open, as one undo step each, and writes its
    /// record. For a project that closes: the headless loop calls it through [`Self::close`],
    /// and the window as its session goes.
    pub fn end_turns(&self, project: &mut Project) -> Vec<ProjectError> {
        let gestures = std::mem::take(&mut *self.0.gestures.borrow_mut());
        let mut errors = Vec::new();
        for (id, gesture) in gestures {
            match gesture.overtaken(project, &id) {
                true => project.abandon(gesture.edit),
                false => errors.extend(project.finish(gesture.edit).err()),
            }
        }
        errors
    }
}
