//! The plugins this project has loaded, whatever their format.
//!
//! [`Plugins`] lives on the thread the project lives on. Both formats put a plugin's own handle
//! on the application's main thread and allow only its audio side on the audio thread, so this
//! table keeps the handles and hands the audio sides to the engine. Nothing here knows CLAP or
//! VST 3: a format is a [`crate::backend::LoadedPlugin`].
//!
//! The rule for saving: a plugin's state is written to its asset when the plugin says it
//! changed, at the next [`Plugins::poll`] and then at most once a second while it keeps saying
//! so, and always when the plugin goes or the project closes. A crash can lose up to a second
//! of a plugin's own changes, and anything a plugin changed without saying so.
//!
//! The table never decides what the engine gets. [`Plugins::open`] loads a plugin and hands it
//! over every time its record names another plugin or state file, and [`Plugins::poll`] lets
//! go of every entry whose record no longer says what the entry holds. So an edit that the
//! project rejects, which never reaches the engine, leaves nothing behind here either. A record
//! whose pins are all that changed keeps its plugin: [`Plugins::follow_pins`] sends the pins as
//! the record has them at each poll, so a rejected edit is never sent.
//!
//! Pins go both ways in that one place. A pin the record changed is sent to the plugin. A pin
//! the plugin changed itself is written to the record, as one undo step per turn of a knob. The
//! host remembers what it last sent and read of each pin, so a value on its way to the plugin
//! is not taken for a change of the plugin's, and a value the plugin rounds as it takes it is
//! not written back. A pin an automation lane moves is not read at all: the lane plays into the
//! plugin on the audio thread, and what it plays is never the composer's edit. Which pins a lane
//! holds the audio side says itself ([`LanedPins`]), before the plugin hears the lane and until
//! it has played the record value again, so a lane value is never taken for the plugin's own
//! change.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gpui::{Keystroke, WindowHandle, WindowId};
use sound_core::{
    AssetName, Assets, InstanceId, PrepareConfig, Project, ProjectEdit as Edit, ProjectError,
};

use crate::processor::{
    AutomatedPin, AutomatedPins, HostedPlugin, HostedUpdate, LanedPins, Playing,
};

use crate::backend::{Hand, KeyDirection, LoadedPlugin, ParameterChange};
use crate::parameters::{Parameter, ParameterValue, by_id, pin_problem, playable};
use crate::placements::{PlacementStore, Placements};
use crate::scan::{Scan, ScanCache, ScanCommand, ScannedPlugin, scan_folders};
use crate::window::{
    Placement, PluginFrame, PluginWindow, Prepared, WindowOwner, WindowRequest, WindowSize,
};
use crate::{PluginFormat, PluginRecord};

/// How often a plugin that keeps saying its state changed is written. A plugin marks itself
/// dirty on every step of a knob drag, and serializing a sampler's state is not cheap, so the
/// first change is written at once and then at most one write a second. Going or closing
/// writes whatever is left, so nothing is lost by waiting.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);

/// How often the host looks at the plugin folders again while a record names a plugin this
/// machine does not have, so that installing it is all the composer has to do. A look at
/// folders that did not change costs a few milliseconds on a thread of its own.
const LOOK_INTERVAL: Duration = Duration::from_secs(2);

/// What the host tells a plugin about itself.
pub(crate) const HOST_NAME: &str = "Sound Tools";
pub(crate) const HOST_VENDOR: &str = "Sound Tools";
pub(crate) const HOST_URL: &str = "https://github.com/casperleerink/sound-tools";
pub(crate) const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Why a plugin record is not playing. Each becomes one line in `problems.txt`.
///
/// A message says what happens to the slot and not what happens to the track: this host knows
/// no slots. What a missing plugin costs is the owner's rule, which for a track is that an
/// instrument goes silent and an effect lets the sound through. An outside agent read the
/// older wording, which spoke of the track, and called it a disagreement with the docs.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PluginProblem {
    #[error(
        "this machine has no {format} plugin with the id {plugin_id:?}. The record is left as it is and nothing plays through it: a missing instrument is silent, a missing effect lets the sound through unchanged. Install the plugin, or correct `plugin_id`"
    )]
    NotInstalled { format: String, plugin_id: String },
    #[error(
        "the plugins of this machine are still being looked at, so {plugin_id:?} is not there yet. Nothing plays through it until the scan reaches it, which needs nothing of you"
    )]
    StillScanning { plugin_id: String },
    #[error("the plugin {plugin_id:?} did not load: {message}")]
    DidNotLoad { plugin_id: String, message: String },
    #[error(
        "the plugin {plugin_id:?} asked to be started again, because its latency or its buses changed, and did not start: {message}. Nothing plays through it until its record changes"
    )]
    DidNotRestart { plugin_id: String, message: String },
    #[error("the state of the plugin {plugin_id:?} could not be read: {message}")]
    StateNotRead { plugin_id: String, message: String },
    #[error("the state of the plugin {plugin_id:?} could not be saved: {message}")]
    StateNotWritten { plugin_id: String, message: String },
    #[error(
        "the plugin {plugin_id:?} offers the host no way to send the sustain pedal, so the pedal does not reach it. Its notes play"
    )]
    NoPedal { plugin_id: String },
    #[error("the plugin {plugin_id:?} has no window of its own")]
    NoWindow { plugin_id: String },
    #[error("the window of the plugin {plugin_id:?} did not open: {message}")]
    WindowDidNotOpen { plugin_id: String, message: String },
    #[error("where the plugin windows are is not kept on this Mac: {message}")]
    WindowPlaces { message: String },
    #[error(
        "the plugin {plugin_id:?} has no parameter with the id {id} that a host may set, so `parameters.{id}` moves nothing. The rest of the record plays. `sound-tools --plugin-params` lists the ones it has"
    )]
    NoSuchParameter { plugin_id: String, id: u32 },
    #[error("what a plugin changed of the parameters its record holds was not written: {message}")]
    PinsNotWritten { message: String },
    #[error(
        "`parameters.{id}.value` is {value}, outside the range of {name:?} of the plugin {plugin_id:?}, {minimum} to {maximum}, so it moves nothing. The rest of the record plays"
    )]
    OutOfRange {
        plugin_id: String,
        id: u32,
        name: String,
        value: f64,
        minimum: f64,
        maximum: f64,
    },
}

/// What [`Plugins::open`] gives back.
pub(crate) struct Opened {
    pub engine: ForEngine,
    /// What to report about this record every time the behaviour runs, such as a plugin the
    /// sustain pedal cannot reach, or a pin it has no parameter for. These are not failures:
    /// the plugin plays.
    pub notes: Vec<PluginProblem>,
    /// The pins of the record, those an automation lane may move marked, see
    /// [`Hosted::automated`].
    pub lanes: Vec<AutomatedPin>,
}

/// What the behaviour hands the engine.
pub(crate) enum ForEngine {
    /// Nothing: the engine plays on with the plugin it has, which is the one this table holds
    /// for the record. Only the pins of the record changed, and the poll sends those.
    Same,
    /// The audio side of a plugin that was just loaded, or `None` from a host that loads none
    /// ([`Plugins::listing`]), which is a slot that plays nothing and reports nothing.
    Play(Option<Playing>),
}

/// How long the pins of a plugin that says nothing of a hand must be quiet before what it
/// changed is one undo step: CLAP, whose gesture events come out of a block this host does not
/// read, and a plugin that moves a parameter itself. Long enough for the pauses of a slow hand,
/// short enough that two turns are two steps.
const QUIET: Duration = Duration::from_millis(500);

/// The same for a VST 3 hand that never lets go: a plugin that sends `beginEdit` without its
/// `endEdit` still gets its turn written.
const HELD_QUIET: Duration = Duration::from_secs(5);

/// One plugin this project holds, with the record it came from.
///
/// It is `loaded` while a record names it and `retired` once it does not. A retired one is kept
/// until the engine gives its audio side back, because letting it go before that would leave
/// the two ends of one plugin in different hands. It is polled and saved until then, so a
/// plugin that is still playing while it waits does not lose what it changes.
struct Hosted {
    format: PluginFormat,
    plugin_id: String,
    asset: AssetName,
    plugin: Box<dyn LoadedPlugin>,
    /// When its state was last written, for the once-a-second rule.
    last_saved: Option<Instant>,
    /// The plugin said its state changed and it is not written yet. It is kept here and not in
    /// the backend, so that a change the once-a-second rule made wait is written by a later
    /// poll and is never forgotten.
    pending_save: bool,
    /// Whether the plugin has a window at all. A card of a rack reads it on every frame it
    /// draws, and a frame must call into no plugin. CLAP answers it while the plugin loads;
    /// VST 3 cannot be asked without building the plugin's whole interface, so it says yes and
    /// [`Plugins::open_window`] writes the answer here the first time one is asked for.
    has_window: bool,
    /// The plugin's own window, while it is open.
    window: PluginWindow,
    /// What the engine's processors were prepared with, for starting the plugin again.
    config: PrepareConfig,
    /// Where a restart the plugin asked for stands, see [`Plugins::restarts`].
    restart: Restart,
    /// The next run of the behaviour loads the plugin again instead of keeping it: the plugin
    /// asked to be loaded again, or it did not start again after a restart.
    needs_load: bool,
    /// What the load reported that stays true while the plugin plays, which a run of the
    /// behaviour that keeps the plugin reports again.
    notes: Vec<PluginProblem>,
    /// Every parameter a host may set, by id, read from the plugin the first time a pin or the
    /// card needs them and again when the plugin says they changed. Shared, so a card can keep
    /// the list of a plugin with thousands of parameters without a copy per frame.
    parameters: Option<Rc<BTreeMap<u32, Parameter>>>,
    /// Where each pin of the record that moves something stands between the record and the
    /// plugin.
    pins: BTreeMap<u32, PinState>,
    /// The pins the audio side says a lane holds, see [`Hosted::follow_lanes`].
    laned: Arc<LanedPins>,
    /// The pins of the record as the behaviour last ran, which the lane players were given
    /// with them. A plugin started again gets these, so the index of a lane means the same
    /// pin to both until the next run, whatever the plugin says of its parameters meanwhile.
    lanes: AutomatedPins,
    /// What the plugin's own window was last shown of each pin a lane holds.
    shown: BTreeMap<u32, Shown>,
}

/// A lane value the plugin's own window was shown, and the value it showed then. `None` where
/// the host shows the window nothing, as for CLAP.
#[derive(Copy, Clone, Debug)]
struct Shown {
    sent: f64,
    seen: Option<f64>,
}

/// A pin, as the host last sent it or read it.
#[derive(Copy, Clone, Debug)]
struct PinState {
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
struct PinGesture {
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
}

impl PinGesture {
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

/// A plugin that asked to be started again, which is how both formats let a latency change.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Restart {
    Idle,
    /// The plugin asked. The engine is to give its audio side back.
    Asked,
    /// The engine was told to give the audio side back. The first poll that finds it back
    /// starts the plugin again and hands it over.
    Waiting,
}

impl Hosted {
    /// Whether this entry holds what `record` names: the plugin and its state file.
    fn holds(&self, record: &PluginRecord) -> bool {
        record.format == self.format
            && record.plugin_id == self.plugin_id
            && record.asset() == self.asset
    }

    /// Whether a run of the behaviour with `record` may keep this plugin as it plays: the
    /// same plugin, state file and set-up, and nothing asked for a load since. A plugin that is
    /// being started again is kept too: the engine gets it when it has started, and the pins
    /// are sent to it then.
    fn keeps(&self, record: &PluginRecord, config: PrepareConfig) -> bool {
        self.holds(record) && self.config == config && !self.needs_load
    }

    /// Every parameter a host may set, read from the plugin the first time it is asked.
    fn parameters(&mut self) -> &Rc<BTreeMap<u32, Parameter>> {
        let Self {
            parameters, plugin, ..
        } = self;
        parameters.get_or_insert_with(|| Rc::new(listed(plugin.as_mut())))
    }

    /// The pins of `record` whose value the plugin takes, in the order of their ids, and kept
    /// as the pins the next start of the plugin gets. An automation lane may move those whose
    /// parameter [`Parameter::takes_lane`].
    fn automated(&mut self, record: &PluginRecord) -> Vec<AutomatedPin> {
        let mut pins = Vec::new();
        if !record.parameters.is_empty() {
            let parameters = self.parameters();
            let all = record.parameters.iter();
            pins = all
                .filter_map(|(id, pin)| {
                    let parameter = parameters.get(id).filter(|it| it.takes(pin.value))?;
                    Some(AutomatedPin {
                        id: *id,
                        takes_lane: parameter.takes_lane(),
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

    /// What a run of the behaviour reports: the notes of the load, and every pin of `record`
    /// that moves nothing.
    fn notes(&mut self, record: &PluginRecord) -> Vec<PluginProblem> {
        let mut notes = self.notes.clone();
        if !record.parameters.is_empty() {
            let plugin_id = self.plugin_id.clone();
            let parameters = self.parameters();
            let pins = record.parameters.iter();
            notes
                .extend(pins.filter_map(|(id, pin)| pin_problem(&plugin_id, parameters, *id, pin)));
        }
        notes
    }

    /// The record to the plugin: every pin that is new, or whose value is not the one the host
    /// last sent or wrote, was changed by someone else and is sent. A pin that moves nothing is
    /// not sent and not followed; the behaviour reports it.
    fn send_pins(&mut self, record: &PluginRecord) {
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

    /// Whether the record of `id` in the project still says what this entry holds.
    fn matches(&self, project: &Project, id: &InstanceId) -> bool {
        record_of(project, id).is_some_and(|record| self.holds(record))
    }
}

/// The parameters a plugin lists, by id.
fn listed(plugin: &mut dyn LoadedPlugin) -> BTreeMap<u32, Parameter> {
    by_id(plugin.parameters())
}

/// The pins of the record of `id` that the plugin of `hosted` takes, as its record has them now.
fn pins_of(hosted: &Hosted, project: &Project, id: &InstanceId) -> Vec<ParameterChange> {
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

/// The plugin record of `id`, when there is one.
fn record_of<'a>(project: &'a Project, id: &InstanceId) -> Option<&'a PluginRecord> {
    project.state(&project.resolve::<PluginRecord>(id)?)
}

/// What a host that loads no plugin says a pin takes a lane for: every pin, with any value. It
/// cannot ask the plugin, and it reports no problem of a pin either, so `--inspect` does not
/// report a lane on a pin that plays. A range this wide still works out a place on it.
fn listed_lanes(record: &PluginRecord) -> Vec<AutomatedPin> {
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

#[derive(Default)]
struct Table {
    loaded: BTreeMap<InstanceId, Hosted>,
    retired: Vec<Hosted>,
    /// Something a card of a plugin shows changed since whoever draws the rack last asked: a
    /// window opened or closed, or a plugin's parameters or their text.
    card_changed: bool,
    /// Windows whose plugin has gone. Their views are already freed; taking a window down
    /// needs the application, which the moments that find them do not have.
    finished_windows: Vec<WindowHandle<PluginFrame>>,
    /// Where each plugin's window was and whether it was open, read from this machine's
    /// store when the project is first polled, and written back there. See `placements.rs`
    /// and [`Plugins::settle_windows`].
    placements: Placements,
    /// A placement changed since it was last written, or since a write of it failed: a write
    /// that fails is not tried again until a window changes again, so it is said once.
    placements_changed: bool,
    /// When they were last written, for the once-a-second rule a plugin's state has too: a
    /// window that is dragged moves many times a second.
    placements_written: Option<Instant>,
    /// What went wrong while the windows were settled, which has no project to report to. The
    /// next poll gives it.
    window_problems: Vec<PluginProblem>,
}

impl Table {
    /// Says whether the window of `id` is to be open, the next time it can be. Only the
    /// composer decides this: opening a window, closing one, or a plugin's own window that
    /// the composer closed. A window the host takes down because its plugin reloads, or because
    /// the project closes, stays open in here, and comes back.
    fn keep_open(&mut self, id: &InstanceId, open: bool) {
        if let Some(placement) = self.placements.get_mut(id)
            && placement.open != open
        {
            placement.open = open;
            self.placements_changed = true;
        }
    }

    /// Notes where the window of `id` is now.
    fn place(&mut self, id: &InstanceId, placement: Placement) {
        if self.placements.get(id) != Some(&placement) {
            self.placements.insert(id.clone(), placement);
            self.placements_changed = true;
        }
    }
}

/// What this machine has, filled in by whoever scans. Shared with the scan thread, so this is
/// the one place in the host with a lock, and the audio thread never touches it.
#[derive(Default)]
struct Scanning {
    scan: Scan,
    /// Bundles that failed, as one line each. The runtime shows them once.
    notices: Vec<String>,
    /// Goes up whenever the scan learns something, so a poll can tell that a record that was
    /// waiting for a plugin is worth trying again.
    generation: u64,
    /// Whether the host is looking again, see [`Plugins::look_again`].
    looking_again: bool,
}

struct Inner {
    search_paths: Vec<std::path::PathBuf>,
    scanner: ScanCommand,
    cache: ScanCache,
    scanned: Arc<Mutex<Scanning>>,
    /// Whether a scan has run or is running. A host that scans in the background sets it as it
    /// starts, so nothing blocks on the first plugin a project names.
    started: Cell<bool>,
    /// Whether this host scans in the background, which is the window's. Only such a host looks
    /// again while it runs: the others run once and end.
    in_background: Cell<bool>,
    /// When the host last started to look again, for [`LOOK_INTERVAL`].
    last_look: Cell<Option<Instant>>,
    /// Ends the scan thread between bundles when the host goes.
    stop: Arc<AtomicBool>,
    /// The generation the last poll acted on.
    seen: Cell<u64>,
    /// Records whose plugin the scan has not found yet, and the ones that are worth running
    /// again now that it has.
    waiting: RefCell<BTreeSet<InstanceId>>,
    retries: RefCell<Vec<InstanceId>>,
    /// A read-only project (`--inspect`, `--render`) never writes plugin state.
    writes_state: bool,
    /// Whether a record's plugin is loaded at all. `--inspect` does not, see [`Plugins::listing`].
    loads: bool,
    /// The `assets/` folder of the project, from the first plugin that loaded. Kept so that
    /// dropping the host can still save, see [`Drop`].
    assets: RefCell<Option<Assets>>,
    /// The folder of the project, from the first poll. It names the windows, and this machine
    /// keeps their places by it.
    root: RefCell<Option<std::path::PathBuf>>,
    /// Where this machine keeps the plugin windows of every project, next to the scan cache.
    placement_store: PlacementStore,
    table: RefCell<Table>,
    /// What each plugin is changing of its pins, by record, see [`Plugins::follow_pins`]. Kept
    /// out of the table, which a behaviour that a write of a record runs borrows.
    gestures: RefCell<BTreeMap<InstanceId, PinGesture>>,
}

/// The last chance to free what a plugin holds for its window and to save its state. On macOS
/// the application ends without unwinding: GPUI drops the main window and its views, and with
/// them the session and the project, and then the process is gone. Dropping the project drops
/// the registry, the behaviour and this host, so that is the moment. A plugin's window is
/// still standing then, empty, and goes with the application; the runtime ends the application
/// with the main window for exactly that reason. Whoever polls the host must therefore hold
/// it weakly ([`Plugins::downgrade`]), else nothing is saved. [`Plugins::close`] does the same
/// with the project still in hand, and leaves nothing for this.
impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let assets = self.writes_state.then(|| self.assets.get_mut().clone());
        let table = self.table.get_mut();
        for hosted in table.loaded.values_mut().chain(&mut table.retired) {
            // Every plugin's view goes, whether this project writes or not. The windows that
            // held them cannot be taken down from here, and nothing will: this is the project
            // closing, which on macOS is the application quitting.
            // The handle is left where it is: the window goes with the application.
            let _window = hosted.window.give_up(hosted.plugin.gui());
            if let Some(Some(assets)) = &assets
                && let Err(problem) = save(hosted, assets)
            {
                // Nobody is left to tell. The composer at least sees it in the terminal.
                eprintln!("error: {problem}");
            }
        }
        if self.writes_state
            && let Some(root) = self.root.get_mut()
            && let Err(problem) = write_placements(table, &self.placement_store, root, None)
        {
            eprintln!("error: {problem}");
        }
    }
}

/// The plugins of one project. Cheap to clone: every copy is the same table.
///
/// The tool's behaviour keeps one, and so does whoever polls the project. It lives on one
/// thread, like the project.
#[derive(Clone)]
pub struct Plugins(Rc<Inner>);

/// A handle that does not keep the plugins alive. Whoever polls the host holds one of these,
/// so that dropping the project is what ends the host and saves every plugin.
#[derive(Clone)]
pub struct WeakPlugins(Weak<Inner>);

impl WeakPlugins {
    pub fn upgrade(&self) -> Option<Plugins> {
        self.0.upgrade().map(Plugins)
    }
}

impl Plugins {
    pub fn downgrade(&self) -> WeakPlugins {
        WeakPlugins(Rc::downgrade(&self.0))
    }

    /// A host that saves plugin state into the project.
    pub fn new(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, true, true)
    }

    /// A host for a project that is open read-only. It loads plugins and never writes.
    pub fn read_only(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, false, true)
    }

    /// A host that looks a plugin up and never loads one. `runtime --inspect` uses it.
    ///
    /// Loading a plugin runs somebody else's code in this process, and a plugin that only ever
    /// loads and goes can take the process down with it: Crow Hill Origins ends `--inspect`
    /// in a segmentation fault in its own teardown, having never processed a block. Inspecting
    /// prints a project and makes no sound, so it needs no plugin at all.
    ///
    /// It still does everything this side can do without the plugin, so that `--inspect`
    /// reports what it always reported: the scan says whether this machine has the plugin, and
    /// the state asset is read, so a state file that cannot be read is still a problem an
    /// agent sees before playback finds it. What is lost is only what the plugin itself can
    /// say, such as a note port that takes no sustain pedal.
    pub fn listing(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, false, false)
    }

    fn with(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
        writes_state: bool,
        loads: bool,
    ) -> Self {
        let placement_store = cache.placement_store();
        Self(Rc::new(Inner {
            search_paths,
            scanner,
            cache,
            scanned: Arc::new(Mutex::new(Scanning::default())),
            started: Cell::new(false),
            in_background: Cell::new(false),
            last_look: Cell::new(None),
            stop: Arc::new(AtomicBool::new(false)),
            seen: Cell::new(0),
            waiting: RefCell::new(BTreeSet::new()),
            retries: RefCell::new(Vec::new()),
            writes_state,
            loads,
            assets: RefCell::new(None),
            root: RefCell::new(None),
            placement_store,
            table: RefCell::new(Table::default()),
            gestures: RefCell::new(BTreeMap::new()),
        }))
    }

    /// Starts the scan on a thread of its own and comes back at once.
    ///
    /// The window calls this before it opens a project, so that no plugin of this machine is
    /// ever looked at on the thread that draws. A record whose plugin the scan has not reached
    /// yet is reported and played as soon as it turns up, see [`Self::take_retries`].
    /// `--render`, `--inspect` and `--headless` do not call it and wait for the scan the first
    /// time a record needs one.
    pub fn start_scanning(&self) {
        self.0.in_background.set(true);
        if self.0.started.replace(true) {
            return;
        }
        let scanned = self.0.scanned.clone();
        let stop = self.0.stop.clone();
        let (paths, scanner, cache) = (
            self.0.search_paths.clone(),
            self.0.scanner.clone(),
            self.0.cache.clone(),
        );
        // Detached: nothing waits for it. A host that goes sets `stop`, and the thread ends
        // after the bundle it is on, which is bounded by the deadline of one child.
        std::thread::Builder::new()
            .name("plugin-scan".to_string())
            .spawn(move || {
                // Whatever ends this thread, the scan is over: it was stopped, or it panicked
                // inside a bundle. Nothing may wait for a scan that is not running.
                let _over = Over(scanned.clone());
                scan_folders(&paths, &scanner, &cache, &stop, |scan| {
                    publish(&scanned, scan);
                });
            })
            .map_or_else(
                |error| {
                    // A machine that cannot start a thread scans where it stands.
                    eprintln!("error: the plugin scan needs a thread: {error}");
                    self.0.started.set(false);
                },
                |_handle| (),
            );
    }

    /// Every plugin this machine has. The first call pays for the scan unless one is already
    /// running in the background, and then it is what is known so far.
    pub fn scan(&self) -> Scan {
        self.ensure_scan();
        self.known()
    }

    fn known(&self) -> Scan {
        scanning(&self.0.scanned).scan.clone()
    }

    /// A number that goes up whenever the scan learns something, and once more when it ends.
    ///
    /// Whoever draws a picker keeps it and fills the menu again when it changes, because a
    /// picker built while a scan ran holds a part of the list and a line that says so. It
    /// copies nothing, so a poll may ask on every frame.
    pub fn scan_generation(&self) -> u64 {
        scanning(&self.0.scanned).generation
    }

    /// Whether a scan is still running. The picker says so quietly while it is, and whoever
    /// polls asks on every poll, so this copies nothing.
    pub fn scan_is_running(&self) -> bool {
        self.0.started.get() && !scanning(&self.0.scanned).scan.finished
    }

    /// Looks at the plugin folders of this machine again, on a thread of its own, for a plugin
    /// that was installed, updated or removed while the app runs. A bundle whose stamp is the
    /// one the cache has costs no child process, so this is cheap when nothing changed, and
    /// then it changes nothing either: what the host knows, and [`Self::scan_generation`], move
    /// only when the plugins or the failures do. A record waiting for its plugin is tried again
    /// when they move.
    ///
    /// It runs when a picker asks what there is ([`Self::instruments`], [`Self::effects`]), and
    /// at a poll every [`LOOK_INTERVAL`] while a record names a plugin this machine did not
    /// have. Only in a host that scans in the background, once its first scan is over, and one
    /// at a time.
    fn look_again(&self) {
        if !self.0.in_background.get() {
            return;
        }
        {
            let mut scanned = scanning(&self.0.scanned);
            if !scanned.scan.finished || scanned.looking_again {
                return;
            }
            scanned.looking_again = true;
        }
        self.0.last_look.set(Some(Instant::now()));
        let scanned = self.0.scanned.clone();
        let stop = self.0.stop.clone();
        let (paths, scanner, cache) = (
            self.0.search_paths.clone(),
            self.0.scanner.clone(),
            self.0.cache.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("plugin-rescan".to_string())
            .spawn(move || {
                // Whatever ends this thread, it is not looking any more.
                let _done = DoneLooking(scanned.clone());
                let scan = scan_folders(&paths, &scanner, &cache, &stop, |_| {});
                // A scan the host stopped as it went is half a list, not what this machine has.
                if scan.finished {
                    publish_again(&scanned, scan);
                }
            });
        if let Err(error) = spawned {
            eprintln!("error: looking for plugins again needs a thread: {error}");
            scanning(&self.0.scanned).looking_again = false;
        }
    }

    /// Scans if this session has not, and waits for it. Does nothing once a scan has been
    /// started in the background.
    fn ensure_scan(&self) {
        if self.0.started.replace(true) {
            return;
        }
        let scanned = self.0.scanned.clone();
        scan_folders(
            &self.0.search_paths,
            &self.0.scanner,
            &self.0.cache,
            &self.0.stop,
            |scan| publish(&scanned, scan),
        );
    }

    /// Waits for the scan to finish, whoever started it. `--render`, `--inspect` and
    /// `--headless` may block, and this is where they do.
    pub fn wait_for_scan(&self) {
        self.ensure_scan();
        while self.scan_is_running() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Lines about the scan that a person should see once, such as a bundle that crashed.
    pub fn take_notices(&self) -> Vec<String> {
        std::mem::take(&mut scanning(&self.0.scanned).notices)
    }

    /// Every instrument this machine has, of every format, in one line each, for a picker. It
    /// scans on the first call of the session, as loading a plugin does.
    ///
    /// A picker asks when it is filled, which is when a track panel opens, so this is also
    /// where the host looks again for a plugin installed while the app runs. What that finds
    /// fills the picker again through [`Self::scan_generation`].
    pub fn instruments(&self) -> Vec<ScannedPlugin> {
        let mut instruments = self.scan().plugins;
        self.look_again();
        instruments.retain(ScannedPlugin::is_instrument);
        instruments
    }

    /// Every effect this machine has, for the picker that adds one to a rack. A plugin decides
    /// which list it is in by what it declares; nothing checks that it is true, and a record
    /// written by hand may name any plugin in any slot. Looks again, as [`Self::instruments`].
    pub fn effects(&self) -> Vec<ScannedPlugin> {
        let mut effects = self.scan().plugins;
        self.look_again();
        effects.retain(ScannedPlugin::is_effect);
        effects
    }

    /// The name the maker gave the plugin with this id, when this machine has it. `None` says
    /// the plugin is missing, which is what the card of a record shows.
    ///
    /// A card asks on every frame it draws, so this scans nothing and copies one name.
    pub fn installed_name(&self, format: PluginFormat, plugin_id: &str) -> Option<String> {
        let scanned = scanning(&self.0.scanned);
        Some(scanned.scan.find(format, plugin_id)?.name.clone())
    }

    /// What this machine knows of the plugin with this id, for the line on its card. `None`
    /// when it is missing.
    pub fn installed(&self, format: PluginFormat, plugin_id: &str) -> Option<ScannedPlugin> {
        scanning(&self.0.scanned)
            .scan
            .find(format, plugin_id)
            .cloned()
    }

    /// What the parameter `parameter` of this record's plugin is now, with the plugin's own
    /// text for it. `None` when no plugin is loaded for the record, or the plugin cannot say.
    ///
    /// It calls into the plugin, so it belongs to the main thread and not to drawing a frame.
    /// It gives up rather than wait for a table that a plugin's own call has borrowed.
    pub fn parameter_value(&self, id: &InstanceId, parameter: u32) -> Option<ParameterValue> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        let plugin = &mut table.loaded.get_mut(id)?.plugin;
        let value = plugin.value(parameter)?;
        Some(ParameterValue {
            value,
            text: plugin.text(parameter, value),
        })
    }

    /// Every parameter a host may set of this record's plugin, by id. `None` when no plugin is
    /// loaded for the record. The same list until the plugin says its parameters changed, so a
    /// card can tell a new list by its pointer.
    ///
    /// The first call asks the plugin, so it belongs to the main thread and not to drawing a
    /// frame. It gives up rather than wait for a table that a plugin's own call has borrowed.
    pub fn parameters(&self, id: &InstanceId) -> Option<Rc<BTreeMap<u32, Parameter>>> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        Some(table.loaded.get_mut(id)?.parameters().clone())
    }

    /// The plugin's own text for `value` of the parameter `parameter`, such as `1.2 kHz`, which
    /// a card shows under the control of a pin. `None` when no plugin is loaded for the record or
    /// the plugin gives no text. It calls into the plugin, as [`Self::parameter_value`].
    pub fn parameter_text(&self, id: &InstanceId, parameter: u32, value: f64) -> Option<String> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        table.loaded.get_mut(id)?.plugin.text(parameter, value)
    }

    /// Loads the plugin the record names and gives it to the caller for the engine.
    ///
    /// It loads every time the record names another plugin, another state file, or the plugin
    /// asked to be loaded again, and on every run whose processor is new, which is a record
    /// that came back. Then nothing here has to guess what the engine holds: the caller hands
    /// over a plugin, and an edit the project rejects simply never reaches the engine. A run
    /// whose record changed only its pins keeps the plugin as it plays, when `processor_kept`
    /// says the engine still has it: reloading a sampler for a turn of a knob would stop its
    /// sound for seconds.
    ///
    /// Whatever this instance held goes first, saved and waiting to be let go of, so a failure
    /// below leaves no entry behind and the record and the engine agree: silence.
    pub(crate) fn open(
        &self,
        id: &InstanceId,
        record: &PluginRecord,
        assets: &Assets,
        config: PrepareConfig,
        processor_kept: bool,
    ) -> Result<Opened, PluginProblem> {
        // Kept for the drop of this host, which is the last moment a plugin can be saved.
        *self.0.assets.borrow_mut() = Some(assets.clone());
        self.0.waiting.borrow_mut().remove(id);
        {
            let mut table = self.0.table.borrow_mut();
            if let Some(hosted) = table.loaded.get_mut(id)
                && processor_kept
                && hosted.keeps(record, config)
            {
                return Ok(Opened {
                    engine: ForEngine::Same,
                    notes: hosted.notes(record),
                    lanes: hosted.automated(record),
                });
            }
            if let Some(hosted) = table.loaded.remove(id) {
                // The same plugin loading again, which is a reload or another state file, gets
                // its window back where it was. Another plugin in the record does not get the
                // window of the one it replaces.
                if hosted.format != record.format || hosted.plugin_id != record.plugin_id {
                    table.keep_open(id, false);
                }
                retire(hosted, &mut table, self.0.writes_state.then_some(assets));
            }
        }
        let asset = record.asset();
        match self.load(id, record, &asset, assets, config) {
            Ok(opened) => Ok(opened),
            Err(problem) => {
                // A plugin the scan has not reached yet is worth trying again when it has, and
                // one this machine does not have when a look again finds it: the composer may
                // install it while the app runs. The poll looks again while one waits.
                if matches!(
                    problem,
                    PluginProblem::StillScanning { .. } | PluginProblem::NotInstalled { .. }
                ) {
                    self.0.waiting.borrow_mut().insert(id.clone());
                }
                Err(problem)
            }
        }
    }

    fn load(
        &self,
        id: &InstanceId,
        record: &PluginRecord,
        asset: &AssetName,
        assets: &Assets,
        config: PrepareConfig,
    ) -> Result<Opened, PluginProblem> {
        self.ensure_scan();
        let scanned = self.known();
        let found = match scanned.find(record.format, &record.plugin_id) {
            Some(found) => found.clone(),
            None if !scanned.finished => {
                return Err(PluginProblem::StillScanning {
                    plugin_id: record.plugin_id.clone(),
                });
            }
            None => {
                return Err(PluginProblem::NotInstalled {
                    format: record.format.name().to_string(),
                    plugin_id: record.plugin_id.clone(),
                });
            }
        };
        // Nothing checks here what the plugin says it is. One record serves an instrument slot
        // and an effect slot, and this host knows no slots: a track decides what it wires a
        // record to. What a plugin declares is what a picker offers it for, and that is not the
        // same question: Spectral Freeze on the machine this was written on declares itself an
        // instrument and is an effect.
        // Empty bytes are a state file that was made to reserve its name, which is how a plugin
        // the window puts on a track gets one, and that the plugin has not written into yet.
        let saved = assets
            .read(asset)
            .map_err(|error| PluginProblem::StateNotRead {
                plugin_id: record.plugin_id.clone(),
                message: error.to_string(),
            })?
            .filter(|bytes| !bytes.is_empty());
        // A host that only lists stops here, after everything this side can check without the
        // plugin: the scan has the plugin and its state file can be read. Nothing of the
        // plugin's own code runs in this process, so nothing of it can fail here, in its
        // `process`, or in the teardown it never expected. What is left out is only what the
        // plugin itself would have said.
        if !self.0.loads {
            return Ok(Opened {
                engine: ForEngine::Play(None),
                notes: Vec::new(),
                lanes: listed_lanes(record),
            });
        }
        let pins = &record.parameters;
        let opening = match record.format {
            PluginFormat::Clap => crate::clap::load(&found, saved.as_deref(), config, pins),
            PluginFormat::Vst3 => crate::vst3::load(&found, saved.as_deref(), config, pins),
        }?;
        let crate::backend::Opening {
            mut plugin,
            started,
            notes,
        } = opening;
        let has_window = plugin.gui().is_some_and(|gui| gui.is_offered());
        let mut hosted = Hosted {
            format: record.format,
            plugin_id: record.plugin_id.clone(),
            asset: asset.clone(),
            plugin,
            last_saved: None,
            pending_save: false,
            has_window,
            window: PluginWindow::default(),
            config,
            restart: Restart::Idle,
            needs_load: false,
            notes,
            parameters: None,
            pins: BTreeMap::new(),
            laned: Arc::new(LanedPins::new()),
            lanes: AutomatedPins::NONE,
            shown: BTreeMap::new(),
        };
        // The record wins over the state just loaded. The backend gave the plugin every pin
        // before it was activated, where the format lets it. They go again in the first block,
        // for a plugin that took nothing then, which changes nothing for one that did.
        hosted.send_pins(record);
        let notes = hosted.notes(record);
        let lanes = hosted.automated(record);
        let laned = hosted.laned.clone();
        self.0.table.borrow_mut().loaded.insert(id.clone(), hosted);
        Ok(Opened {
            engine: ForEngine::Play(Some(Playing { started, laned })),
            notes,
            lanes,
        })
    }

    /// Asks whoever polls to run the behaviour of `id` again, once.
    fn retry(&self, id: &InstanceId) {
        let mut retries = self.0.retries.borrow_mut();
        if !retries.contains(id) {
            retries.push(id.clone());
        }
    }

    /// Records whose behaviour is worth running again: their plugin was not there when it ran
    /// and may be now, because the scan has learned something since, or their plugin asked to
    /// be unloaded and loaded again. Whoever polls runs their behaviour again, which is what
    /// makes a plugin play and takes its problem away, and what loads a plugin afresh.
    pub fn take_retries(&self) -> Vec<InstanceId> {
        std::mem::take(&mut self.0.retries.borrow_mut())
    }

    /// Whether the plugin of this record has a window of its own to open. `None` says the
    /// record has no plugin loaded at all, which is a plugin that did not load and is already
    /// reported; its card says that instead of offering a window.
    ///
    /// A card asks on every frame it draws. The answer is the one the plugin gave while it
    /// loaded, so this calls into no plugin, and it gives up rather than wait for a table that
    /// a plugin's own call has borrowed.
    pub fn window_offered(&self, id: &InstanceId) -> Option<bool> {
        let table = self.0.table.try_borrow().ok()?;
        table.loaded.get(id).map(|hosted| hosted.has_window)
    }

    /// Whether the window of this record's plugin is open. Read while drawing a card, so it
    /// gives up on a table that a plugin's own call has borrowed, as [`Self::window_offered`].
    pub fn window_is_open(&self, id: &InstanceId) -> bool {
        let Ok(table) = self.0.table.try_borrow() else {
            return false;
        };
        table
            .loaded
            .get(id)
            .is_some_and(|hosted| hosted.window.is_open())
    }

    /// Opens the plugin's own window, or brings the one that is open forward, for the composer.
    /// It opens where it was the last time, and the project remembers that it is open.
    pub fn open_window(&self, id: &InstanceId, cx: &mut gpui::App) -> Result<(), PluginProblem> {
        let opened = self.show_window(id, true, cx);
        let mut table = self.0.table.borrow_mut();
        table.keep_open(id, opened.is_ok());
        opened
    }

    /// Opens the window of `id`, taking the keyboard or not. See [`Self::open_window`].
    ///
    /// It is in three steps because the table may not be borrowed while GPUI runs: opening a
    /// window draws, and a card being drawn asks this host what its plugin has.
    fn show_window(
        &self,
        id: &InstanceId,
        focus: bool,
        cx: &mut gpui::App,
    ) -> Result<(), PluginProblem> {
        // One: what the plugin says, with the table borrowed and no GPUI in sight.
        let prepared = {
            let mut table = self.0.table.borrow_mut();
            let placement = table.placements.get(id).copied();
            let Some(hosted) = table.loaded.get_mut(id) else {
                // The record names a plugin this machine does not have, or the load failed.
                // That is reported, and the card shows it instead of offering a window.
                return Ok(());
            };
            let plugin_id = hosted.plugin_id.clone();
            let title = self.title(hosted);
            let no_window = || PluginProblem::NoWindow {
                plugin_id: plugin_id.clone(),
            };
            let Hosted {
                window,
                plugin,
                has_window,
                ..
            } = hosted;
            let prepared = match plugin.gui() {
                Some(gui) => window.prepare(gui),
                None => Err(no_window()),
            };
            // A plugin that turns out to have no window says so once. The card stops offering
            // one for the rest of this session, so the composer is not asked to find out again.
            // A VST 3 plugin is offered a window without being asked, because asking means
            // building its whole interface; this is where the answer arrives instead.
            if matches!(prepared, Err(PluginProblem::NoWindow { .. })) {
                *has_window = false;
            }
            let resizable = window.resizable();
            table.card_changed = true;
            prepared.map(|prepared| (prepared, plugin_id, title, resizable, placement))?
        };
        // Two: the window itself, with nothing borrowed.
        let (prepared, plugin_id, title, resizable, placement) = prepared;
        let size = match prepared {
            Prepared::AlreadyOpen(handle) => {
                return handle
                    .update(cx, |_, window, _| window.activate_window())
                    .map_err(|error| PluginProblem::WindowDidNotOpen {
                        plugin_id,
                        message: error.to_string(),
                    });
            }
            Prepared::Wanted(size) => size,
        };
        let owner = WindowOwner {
            instance: id.clone(),
            plugins: self.downgrade(),
        };
        let request = WindowRequest {
            title: &title,
            size,
            resizable,
            placement,
            focus,
        };
        let opened = crate::window::open_window(&owner, request, cx);
        let (handle, view, closed) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                // The plugin already holds what it needs for a window. Give it back.
                let _window = self.give_up_window(id);
                return Err(PluginProblem::WindowDidNotOpen {
                    plugin_id,
                    message: error.to_string(),
                });
            }
        };
        let placed = handle
            .update(cx, |_, window, cx| crate::window::placement_of(window, cx))
            .ok();
        // Three: the plugin fills it. A window whose plugin went while it opened, or that the
        // plugin refused, waits for the next poll to be taken down.
        let mut table = self.0.table.borrow_mut();
        let Some(hosted) = table.loaded.get_mut(id) else {
            table.finished_windows.push(handle);
            return Ok(());
        };
        let Hosted { window, plugin, .. } = hosted;
        let attached = match plugin.gui() {
            Some(gui) => window.attach(gui, handle, view, closed),
            None => Err((PluginProblem::NoWindow { plugin_id }, handle)),
        };
        match attached {
            Ok(()) => {
                if let Some(placed) = placed {
                    table.place(id, placed);
                }
                Ok(())
            }
            Err((problem, handle)) => {
                table.finished_windows.push(handle);
                Err(problem)
            }
        }
    }

    /// What a plugin's window is called: the plugin and the piece it plays in.
    fn title(&self, hosted: &Hosted) -> String {
        let name = self
            .installed_name(hosted.format, &hosted.plugin_id)
            .unwrap_or_else(|| hosted.plugin_id.clone());
        let root = self.0.root.borrow();
        let folder = root.as_deref().and_then(std::path::Path::file_name);
        match folder {
            Some(folder) => format!("{name} — {}", folder.to_string_lossy()),
            None => name,
        }
    }

    /// Closes the plugin's own window, for the composer. Its sound and its state are untouched,
    /// and the project remembers that it is closed.
    pub fn close_window(&self, id: &InstanceId, cx: &mut gpui::App) {
        self.0.table.borrow_mut().keep_open(id, false);
        self.take_down(id, cx);
    }

    /// Takes the window of `id` down, and leaves what the project remembers of it as it is.
    fn take_down(&self, id: &InstanceId, cx: &mut gpui::App) {
        // Outside the borrow: taking a window down runs GPUI.
        if let Some(handle) = self.give_up_window(id) {
            crate::window::remove(handle, cx);
        }
    }

    /// A key the window of `id` got while the plugin's own view did not have the keyboard.
    /// `true` says the plugin used it. See `backend::PluginGui::key`. The window is checked, as
    /// for a move: a window that is going may still hand on a key.
    pub(crate) fn key(
        &self,
        id: &InstanceId,
        window: WindowId,
        keystroke: &Keystroke,
        direction: KeyDirection,
    ) -> bool {
        let Ok(mut table) = self.0.table.try_borrow_mut() else {
            return false;
        };
        let Some(hosted) = table.loaded.get_mut(id) else {
            return false;
        };
        if !hosted.window.is(window) {
            return false;
        }
        hosted
            .plugin
            .gui()
            .is_some_and(|gui| gui.key(keystroke, direction))
    }

    /// The window of `id` moved or was resized. Where it is now is noted for the project, and
    /// a drag of its edge is given to the plugin. The window this says is checked, because one
    /// that went may still report a move on its way out.
    pub(crate) fn window_bounds_changed(
        &self,
        id: &InstanceId,
        window: WindowId,
        placement: Placement,
        content: WindowSize,
    ) {
        // A move that GPUI reports from inside a call of ours that has the table is noted by
        // that call itself.
        let Ok(mut table) = self.0.table.try_borrow_mut() else {
            return;
        };
        let Some(hosted) = table.loaded.get_mut(id) else {
            return;
        };
        if !hosted.window.is(window) {
            return;
        }
        let Hosted { window, plugin, .. } = hosted;
        if let Some(gui) = plugin.gui() {
            window.resized(gui, content);
        }
        table.place(id, placement);
    }

    /// Frees whatever the plugin of `id` holds for a window and gives back the window it was
    /// in, for the caller to take down.
    #[must_use]
    fn give_up_window(&self, id: &InstanceId) -> Option<WindowHandle<PluginFrame>> {
        let mut table = self.0.table.borrow_mut();
        let hosted = table.loaded.get_mut(id)?;
        let finished = hosted.window.give_up(hosted.plugin.gui());
        table.card_changed = true;
        finished
    }

    /// The window is going, whatever took it down. GPUI tells its observers while it still
    /// holds the window, so this is the moment the plugin lets go of the view it is in, before
    /// that view is released. See `window::open_window`.
    ///
    /// A window this host took down has let go of its view already. One that still has it was
    /// closed by the composer, with its close control, and the project remembers that.
    pub(crate) fn window_was_closed(&self, id: &InstanceId) {
        let mut table = self.0.table.borrow_mut();
        if let Some(hosted) = table.loaded.get_mut(id)
            && hosted.window.give_up(hosted.plugin.gui()).is_some()
        {
            table.card_changed = true;
            table.keep_open(id, false);
        }
    }

    /// The window work that needs the application: taking down the windows of plugins that
    /// have gone, giving a window the size its plugin asked for, and opening the windows the
    /// project remembers as open whose plugin is loaded, which is how they come back when the
    /// project opens and when their plugin reloads. Whoever polls the host calls it after
    /// [`Self::poll`]; the moments that find such a plugin, a record that was deleted, an undo
    /// or a reload, have no application at hand.
    pub fn settle_windows(&self, cx: &mut gpui::App) {
        // Everything is read out first: running GPUI while the table is borrowed would let a
        // card that is drawn ask this host about its plugin.
        let (finished, resize, reopen) = {
            let mut table = self.0.table.borrow_mut();
            let Table {
                loaded,
                retired,
                finished_windows,
                placements,
                ..
            } = &mut *table;
            let reopen: Vec<InstanceId> = loaded
                .iter()
                .filter(|(id, hosted)| {
                    hosted.has_window
                        && !hosted.window.is_open()
                        && placements.get(*id).is_some_and(|placement| placement.open)
                })
                .map(|(id, _)| id.clone())
                .collect();
            let resize: Vec<_> = loaded
                .values_mut()
                .chain(retired)
                .filter_map(|hosted| hosted.window.take_wanted_size())
                .collect();
            (std::mem::take(finished_windows), resize, reopen)
        };
        for (handle, wanted) in resize {
            crate::window::resize(handle, wanted, cx);
        }
        for handle in finished {
            crate::window::remove(handle, cx);
        }
        // A window that comes back by itself leaves the keyboard where it is. The application
        // is waited for while it is becoming active, which it is at the first polls: AppKit
        // gives the keyboard to the front window once it is, and a plugin's window, which
        // floats, would be that window. One that cannot come back is forgotten as open, so it
        // is not tried again at every poll.
        let active = cx.active_window();
        if reopen.is_empty() || (active.is_none() && !cx.windows().is_empty()) {
            return;
        }
        for id in reopen {
            if let Err(problem) = self.show_window(&id, false, cx) {
                let mut table = self.0.table.borrow_mut();
                table.keep_open(&id, false);
                table.window_problems.push(problem);
            }
        }
        // A plugin may take the keyboard as it is shown, which Six Sines does. It goes back to
        // the window that had it.
        if let Some(active) = active
            && cx.active_window() != Some(active)
        {
            active
                .update(cx, |_, window, _| window.activate_window())
                .ok();
        }
    }

    /// Frees the view of every plugin window and takes the windows down. The application
    /// calls it as it quits, before anything of it is torn down, so that no plugin is left
    /// holding a view of a window that is going.
    pub fn close_all_windows(&self, cx: &mut gpui::App) {
        let open: Vec<InstanceId> = {
            let table = self.0.table.borrow();
            let open = table.loaded.iter();
            open.filter(|(_, hosted)| hosted.window.is_open())
                .map(|(id, _)| id.clone())
                .collect()
        };
        // The project remembers them as open: they come back when it opens again.
        for id in open {
            self.take_down(&id, cx);
        }
    }

    /// Whether any plugin's window opened or closed, or a plugin said its parameters or their
    /// text changed, since the last call. Whoever polls asks, so the cards are drawn again: one
    /// says "Open window" or "Close window", and reads its parameters and their text again.
    pub fn take_card_change(&self) -> bool {
        std::mem::take(&mut self.0.table.borrow_mut().card_changed)
    }

    /// Saves the state of every plugin, whether it said so or not, and lets them all go.
    ///
    /// Call it when the project closes. A plugin that changes its state without telling the
    /// host is saved here all the same. Nothing is written when the bytes are the ones already
    /// in the project, so a session that changed nothing leaves no diff. A turn of a knob that
    /// is still open ends here, so its record is written too.
    pub fn close(&self, project: &mut Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        for error in self.end_turns(project) {
            problems.push(PluginProblem::PinsNotWritten {
                message: error.to_string(),
            });
        }
        let project = &*project;
        let mut table = self.0.table.borrow_mut();
        if self.0.writes_state
            && let Some(root) = self.0.root.borrow().as_deref()
            && let Err(problem) =
                write_placements(&mut table, &self.0.placement_store, root, Some(project))
        {
            problems.push(problem);
        }
        let Table {
            loaded,
            retired,
            card_changed,
            finished_windows,
            ..
        } = &mut *table;
        let assets = self.0.writes_state.then(|| project.assets());
        for hosted in loaded.values_mut().chain(&mut *retired) {
            // Every window closes with the project, whether it writes or not, and the project
            // remembers the ones that were open.
            if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
                finished_windows.push(handle);
                *card_changed = true;
            }
            if let Some(assets) = assets
                && let Err(problem) = save(hosted, assets)
            {
                problems.push(problem);
            }
        }
        retired.extend(std::mem::take(loaded).into_values());
        problems
    }

    /// Main-thread work for every plugin this host holds: the callbacks they asked for, the
    /// state they said changed, and letting go of the ones no record names any more. A plugin
    /// that asked to be started again is noted here and started by [`Self::send_restarts`].
    ///
    /// Call it as often as the project is polled.
    pub fn poll(&self, project: &Project) -> Vec<PluginProblem> {
        self.poll_at(project, Instant::now())
    }

    /// Moves every restart a plugin asked for one step on, see [`Self::restarts`]. Call it
    /// after [`Self::poll`], which is what notes that one asked. It needs the project
    /// mutably only to hand the engine the plugin's audio side, which is not an edit: nothing
    /// is written and there is no undo step.
    pub fn send_restarts(&self, project: &mut Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        // Outside the borrow of the table: the engine is the project's.
        let updates = self.restarts(project, &mut problems);
        for (id, plugin_id, update) in updates {
            if let Err(error) = project.send::<HostedPlugin>(&id, crate::PROCESSOR, update) {
                problems.push(PluginProblem::DidNotRestart {
                    plugin_id,
                    message: error.to_string(),
                });
            }
        }
        problems
    }

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

    /// A plugin that asked to be started again goes in two steps, one poll or more apart.
    /// First the engine is told to give its audio side back, which stops it on the audio
    /// thread. Once the engine has, the plugin is deactivated, activated again and handed back
    /// with the latency it says it has now, and the engine compensates that from the block it
    /// arrives in. In between the slot plays what an empty one does: silence for an
    /// instrument, the sound going through unchanged for an effect.
    ///
    /// Gives what to send the engine, for which instance.
    fn restarts(
        &self,
        project: &Project,
        problems: &mut Vec<PluginProblem>,
    ) -> Vec<(InstanceId, String, HostedUpdate)> {
        let mut table = self.0.table.borrow_mut();
        let mut updates = Vec::new();
        for (id, hosted) in &mut table.loaded {
            match hosted.restart {
                Restart::Idle => {}
                Restart::Asked => {
                    hosted.restart = Restart::Waiting;
                    let none = HostedUpdate::Plugin(None, AutomatedPins::NONE);
                    updates.push((id.clone(), hosted.plugin_id.clone(), none));
                }
                Restart::Waiting => match hosted
                    .plugin
                    .restart(hosted.config, &pins_of(hosted, project, id))
                {
                    // The engine has not given it back yet.
                    None => {}
                    Some(Ok(started)) => {
                        hosted.restart = Restart::Idle;
                        // What was on its way to the old audio side went with it, and the
                        // record may have changed meanwhile: every pin goes again, as the
                        // record has it, ahead of the first block of the new audio side.
                        hosted.pins.clear();
                        if let Some(record) = record_of(project, id) {
                            hosted.send_pins(record);
                        }
                        let laned = hosted.laned.clone();
                        let playing = Playing { started, laned };
                        let update = HostedUpdate::Plugin(Some(playing), hosted.lanes);
                        updates.push((id.clone(), hosted.plugin_id.clone(), update));
                    }
                    Some(Err(problem)) => {
                        // Silent until its record changes, which loads it.
                        hosted.restart = Restart::Idle;
                        hosted.needs_load = true;
                        problems.push(problem);
                    }
                },
            }
        }
        updates
    }

    /// [`Self::poll`] with the time given, so a test can move it.
    pub fn poll_at(&self, project: &Project, now: Instant) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        self.remember_the_windows(project, &mut problems);
        self.note_what_the_scan_found(project);
        // A record waits for a plugin this machine did not have. Once the first scan is over,
        // that is a plugin the composer may be installing right now.
        let looked = self.0.last_look.get();
        if !self.0.waiting.borrow().is_empty()
            && looked.is_none_or(|looked| now.saturating_duration_since(looked) >= LOOK_INTERVAL)
        {
            self.look_again();
        }
        let mut table = self.0.table.borrow_mut();
        problems.append(&mut table.window_problems);
        let Table {
            loaded,
            retired,
            card_changed,
            finished_windows,
            ..
        } = &mut *table;
        let assets = self.0.writes_state.then(|| project.assets());
        // Windows that close for good, see `Table::keep_open`. Noted after the loop, which
        // has the table in pieces.
        let mut closed_for_good = Vec::new();

        // Everything the records no longer say. A record that is gone, that is no longer a
        // plugin, or that names another plugin or another state file than the entry holds:
        // the last of those is an edit the project rolled back after this host had loaded it.
        let stale: Vec<InstanceId> = loaded
            .iter()
            .filter(|(id, hosted)| !hosted.matches(project, id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(mut hosted) = loaded.remove(&id) {
                // The window of a plugin that is going goes with it: a record that was deleted
                // from a file or by an undo leaves no window behind, and an undo that brings
                // the record back does not bring the window.
                if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
                    finished_windows.push(handle);
                    *card_changed = true;
                }
                closed_for_good.push(id.clone());
                // Saved on the way out, so undo of a delete brings the plugin back as it
                // sounded and not as it was last written.
                if let Some(assets) = assets
                    && let Err(problem) = save(&mut hosted, assets)
                {
                    problems.push(problem);
                }
                retired.push(hosted);
            }
        }

        // Retired plugins are served too: one that is still playing, because the engine has
        // not given its audio side back yet, must not miss a callback or lose a change.
        let serving = loaded.iter_mut().map(|(id, hosted)| (Some(id), hosted));
        for (id, hosted) in serving.chain(retired.iter_mut().map(|hosted| (None, hosted))) {
            let requests = hosted.plugin.poll();
            if let Some(wanted) = requests.window_size {
                hosted.window.wants_size(wanted);
            }
            // The plugin closed its own window, by its title bar or by losing it. This host
            // keeps no window that is not shown.
            if requests.window_closed
                && let Some(handle) = hosted.window.give_up(hosted.plugin.gui())
            {
                finished_windows.push(handle);
                *card_changed = true;
                if let Some(id) = id {
                    closed_for_good.push(id.clone());
                }
            }
            // A plugin that asks to be deactivated and activated again, which is how its
            // latency changes. A retired plugin is marked as well, but only loaded ones are
            // started again, see `Self::restarts`.
            if requests.restart && hosted.restart == Restart::Idle {
                hosted.restart = Restart::Asked;
            }
            // A plugin that asks to be unloaded and loaded again gets exactly what a record
            // that changed gets: whoever polls runs its behaviour again, which saves this one
            // on its way out and loads its record from that state. A retired plugin is going
            // anyway.
            if requests.reload
                && let Some(id) = id
            {
                hosted.needs_load = true;
                self.retry(id);
            }
            // The plugin has other parameters, or other names or text for them. The pins of the
            // record are checked again, by a run of its behaviour that keeps the plugin: that is
            // where what a pin cannot move is reported. A list nobody read yet is read when a pin
            // or a card needs it. The list is a new one even when it is equal, because the
            // plugin's text, which it does not hold, may have changed: a card tells by that.
            if requests.parameters_changed
                && let Some(id) = id
                && let Some(before) = hosted.parameters.take()
            {
                let parameters = Rc::new(listed(hosted.plugin.as_mut()));
                if parameters != before {
                    self.retry(id);
                }
                hosted.parameters = Some(parameters);
                *card_changed = true;
            }
            // The plugin now maps its sustain pedal to nothing, so the pedal stops reaching
            // it. The same line a plugin gets that never mapped one.
            if requests.pedal_unmapped {
                problems.push(PluginProblem::NoPedal {
                    plugin_id: hosted.plugin_id.clone(),
                });
            }
            hosted.pending_save |= requests.state_is_dirty;
            let due = hosted
                .last_saved
                .is_none_or(|last| now.duration_since(last) >= SAVE_INTERVAL);
            if let Some(assets) = assets
                && hosted.pending_save
                && due
            {
                hosted.last_saved = Some(now);
                if let Err(problem) = save(hosted, assets) {
                    problems.push(problem);
                }
            }
        }

        // A plugin may only go once the engine has given its audio side back. Until then
        // letting it go would leave the two ends of one plugin in different hands.
        retired.retain_mut(|hosted| !hosted.plugin.released());
        for id in closed_for_good {
            table.keep_open(&id, false);
        }

        // Where the windows are, at most once a second while one is dragged. The project
        // closing writes whatever is left.
        let due = table
            .placements_written
            .is_none_or(|last| now.duration_since(last) >= SAVE_INTERVAL);
        if self.0.writes_state
            && table.placements_changed
            && due
            && let Some(root) = self.0.root.borrow().as_deref()
        {
            table.placements_written = Some(now);
            let store = &self.0.placement_store;
            if let Err(problem) = write_placements(&mut table, store, root, Some(project)) {
                problems.push(problem);
            }
        }
        problems
    }

    /// The first poll of a project reads where its plugin windows were. Every host reads it,
    /// and only the one that has windows opens any, see [`Self::settle_windows`].
    fn remember_the_windows(&self, project: &Project, problems: &mut Vec<PluginProblem>) {
        if self.0.root.borrow().is_some() {
            return;
        }
        let root = project.root().to_path_buf();
        match self.0.placement_store.read(&root) {
            Ok(placements) => self.0.table.borrow_mut().placements = placements,
            Err(message) => problems.push(PluginProblem::WindowPlaces { message }),
        }
        *self.0.root.borrow_mut() = Some(root);
    }

    /// Records that were waiting for a plugin the scan had not reached. When it has learned
    /// something since the last poll, their behaviours are worth running again.
    fn note_what_the_scan_found(&self, project: &Project) {
        if self.0.waiting.borrow().is_empty() {
            return;
        }
        // A record that is gone, or no longer a plugin, waits for nothing, and the host does
        // not look again for it.
        self.0
            .waiting
            .borrow_mut()
            .retain(|id| project.resolve::<PluginRecord>(id).is_some());
        let generation = scanning(&self.0.scanned).generation;
        if self.0.seen.replace(generation) == generation {
            return;
        }
        let known = self.known();
        let mut waiting = self.0.waiting.borrow_mut();
        let mut retries = self.0.retries.borrow_mut();
        waiting.retain(|id| {
            // A record that is gone, or no longer a plugin, waits for nothing.
            let Some(instance) = project.resolve::<PluginRecord>(id) else {
                return false;
            };
            let Some(record) = project.state(&instance) else {
                return false;
            };
            let found = known.find(record.format, &record.plugin_id).is_some();
            if found || known.finished {
                if !retries.contains(id) {
                    retries.push(id.clone());
                }
                return false;
            }
            true
        });
    }
}

/// What the scan has found so far.
///
/// A scan thread that panicked leaves what it had. Nothing of ours can panic while it holds
/// this lock, so going on with it is only so that a project still opens.
fn scanning(scanned: &Mutex<Scanning>) -> MutexGuard<'_, Scanning> {
    scanned
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Says the scan is over, however its thread ended.
struct Over(Arc<Mutex<Scanning>>);

impl Drop for Over {
    fn drop(&mut self) {
        let mut held = scanning(&self.0);
        held.scan.finished = true;
        held.generation += 1;
    }
}

/// Ends a look again, however its thread ended.
struct DoneLooking(Arc<Mutex<Scanning>>);

impl Drop for DoneLooking {
    fn drop(&mut self) {
        scanning(&self.0).looking_again = false;
    }
}

/// Puts what a look again found where the host can read it, when it is not what the host
/// already knew. Only then is the change counted, so a look that found nothing new fills no
/// picker again and tries no waiting record again, and nothing looks again because of it.
fn publish_again(scanned: &Mutex<Scanning>, scan: Scan) {
    let mut held = scanning(scanned);
    if held.scan.plugins == scan.plugins && held.scan.failures == scan.failures {
        return;
    }
    for failure in &scan.failures {
        if !held.scan.failures.contains(failure) {
            held.notices.push(format!(
                "{} could not be scanned: {}",
                failure.path.display(),
                failure.message
            ));
        }
    }
    held.notices.extend(scan.cache_error.clone());
    held.scan = scan;
    held.generation += 1;
}

/// Puts what the scan has found where the host can read it, and counts the change so that a
/// record that is waiting for a plugin is tried again.
fn publish(scanned: &Mutex<Scanning>, scan: &Scan) {
    let mut held = scanning(scanned);
    let said = held.scan.failures.len().min(scan.failures.len());
    for failure in &scan.failures[said..] {
        held.notices.push(format!(
            "{} could not be scanned: {}",
            failure.path.display(),
            failure.message
        ));
    }
    // Said once, when the scan is over: only the last call carries it.
    held.notices.extend(scan.cache_error.clone());
    held.scan = scan.clone();
    held.generation += 1;
}

/// Keeps where the plugin windows are in this machine's store, when they changed since the
/// last write. A window whose record is gone is forgotten on the way, when the project is at
/// hand. A write that fails is said once and not tried again until a window changes again.
fn write_placements(
    table: &mut Table,
    store: &PlacementStore,
    root: &std::path::Path,
    project: Option<&Project>,
) -> Result<(), PluginProblem> {
    if !std::mem::take(&mut table.placements_changed) {
        return Ok(());
    }
    if let Some(project) = project {
        table
            .placements
            .retain(|id, _| project.resolve::<PluginRecord>(id).is_some());
    }
    store
        .write(root, &table.placements)
        .map_err(|message| PluginProblem::WindowPlaces { message })
}

/// Saves a plugin that is going, when the project is one that writes, and puts its handle
/// where it waits for the engine to give the audio side back.
fn retire(mut hosted: Hosted, table: &mut Table, assets: Option<&Assets>) {
    // A record that now names another plugin takes the window of the old one with it.
    if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
        table.finished_windows.push(handle);
        table.card_changed = true;
    }
    if let Some(assets) = assets
        && let Err(problem) = save(&mut hosted, assets)
    {
        // The caller is inside a behaviour and has no way to report. The composer at least
        // sees it in the terminal.
        eprintln!("error: {problem}");
    }
    table.retired.push(hosted);
}

/// Writes what the plugin says its state is, into the asset its record names. Bytes that are
/// already there are not written again, so a session that changed nothing leaves no diff.
fn save(hosted: &mut Hosted, assets: &Assets) -> Result<(), PluginProblem> {
    let fail = |message: String| PluginProblem::StateNotWritten {
        plugin_id: hosted.plugin_id.clone(),
        message,
    };
    let bytes = hosted.plugin.save_state().map_err(fail)?;
    // Cleared only once the bytes are where they belong, so a write that failed is tried again
    // at a later poll instead of being forgotten.
    let written = || {
        if bytes.is_empty() {
            return Ok(());
        }
        let there = assets
            .read(&hosted.asset)
            .map_err(|error| fail(error.to_string()))?;
        if there.as_deref() == Some(bytes.as_slice()) {
            return Ok(());
        }
        assets
            .write(&hosted.asset, &bytes)
            .map_err(|error| fail(error.to_string()))
    };
    let result = written();
    if result.is_ok() {
        hosted.pending_save = false;
    }
    result
}
