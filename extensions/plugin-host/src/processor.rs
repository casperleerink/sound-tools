//! The engine processor around a plugin's audio processor, and the note contract it speaks.
//!
//! The plugin's own handle stays on the control thread, see `host.rs`. What comes here is the
//! part every format allows on the audio thread: CLAP's audio processor, VST 3's
//! `IAudioProcessor`. This wrapper translates the note contract into what that format takes,
//! gives the plugin our one stereo input port, calls it and copies its first output port into
//! our one stereo output port.
//!
//! One wrapper serves an instrument and an effect, because one record does: nothing here knows
//! which slot it is in. An instrument's input is connected to nothing and is silent, which is
//! what every audio input of a plugin got before effects existed. A slot with no plugin passes
//! its input to its output unchanged, so a missing effect is a slot the sound goes through and
//! not a track that goes silent.
//!
//! The translation is here and not in a backend: both formats need the same list of keys that
//! are down, the same memory of where the pedal and the wheels stand, the same expansion of
//! `AllOff` and the same bound on how many events one block may carry. A backend only says how
//! one event is written down, through [`Started::push`].
//!
//! So are the automation lanes of the pins: a lane sends its value every block, which goes to
//! the plugin as a parameter change, and a pin that hears nothing in a block goes back to its
//! record value, the rule every device of the core follows. The plugin smooths its parameters
//! itself, so nothing here ramps.
//!
//! Nothing here allocates, locks or makes a system call. Every buffer is made when the plugin
//! is loaded. What the plugin does inside its own calls is not ours: the realtime sanitizer is
//! switched off for exactly those calls and nothing around them ([`not_ours`]).
//!
//! Both formats want processing started and stopped on the thread that processes. This wrapper
//! is the only place that has one, so it stops the plugin before it lets it go: on a swap in
//! [`Processor::update`] and on [`Processor::leaving`], which is the engine handing the
//! processor back. The backend's own `Drop` is the last resort, for the engine itself being
//! torn down, when there is no audio thread left to do it on.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering, fence};

use sound_core::{
    AudioInput, AudioOutput, Automation, CHANNELS, EventInput, MAX_AUTOMATED, Ports, PrepareConfig,
    ProcessContext, Processor, Timed,
};
use sound_notes::{Amount, Bend, NoteEvent, Pedal};

/// How many events one block can carry into the plugin. An `AllOff` alone can be 132 of them.
/// Anything above this is counted and dropped, never allocated.
pub(crate) const EVENT_CAPACITY: usize = 512;

/// One thing to tell the plugin, at a frame offset in the block. This is the note contract with
/// `AllOff` already expanded into the keys that are really down and the controls that moved.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum PluginEvent {
    On { key: u8, velocity: u8 },
    Off { key: u8 },
    Control(Control),
}

/// A control of the whole instrument and where it stands: the sustain pedal, a wheel or the key
/// pressure. Each format has one way in for each of them, which a plugin may not offer, see
/// [`Started::takes`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Control {
    Pedal(Pedal),
    Bend(Bend),
    ModWheel(Amount),
    Pressure(Amount),
}

impl Control {
    /// Every control at rest, one of each in the order of [`Self::index`]. It is where a plugin
    /// stands before it hears any of them, and where `AllOff` puts it back.
    pub(crate) const REST: [Self; 4] = [
        Self::Pedal(Pedal::UP),
        Self::Bend(Bend::MIDDLE),
        Self::ModWheel(Amount::NONE),
        Self::Pressure(Amount::NONE),
    ];

    /// Which control this is, as its place in [`Self::REST`], for a table with one entry each.
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Pedal(_) => 0,
            Self::Bend(_) => 1,
            Self::ModWheel(_) => 2,
            Self::Pressure(_) => 3,
        }
    }

    /// Where it stands as MIDI sends it: 0 to 127, and for the bend 0 to 16383 with the middle
    /// at 8192.
    pub(crate) fn midi_value(self) -> u16 {
        match self {
            Self::Pedal(pedal) => u16::from(pedal.value()),
            Self::Bend(bend) => (i32::from(bend.value()) + 8192) as u16,
            Self::ModWheel(amount) | Self::Pressure(amount) => u16::from(amount.value()),
        }
    }
}

/// A plugin that is loaded and started, seen from the audio thread. One implementation per
/// format.
///
/// No call of this trait may allocate, lock or make a system call in our own code. A call into
/// the plugin itself is wrapped in [`not_ours`].
pub(crate) trait Started: Send {
    /// Whether this control reaches the plugin; only which control it is counts, not where it
    /// stands. A plugin that offers no way to receive one gets the notes and not that control.
    /// For the pedal its record says so.
    fn takes(&self, control: Control) -> bool;

    /// How many frames late the plugin plays, as it said when it was activated.
    fn latency(&self) -> u32;

    /// A new block: everything the last one carried is forgotten.
    fn begin_block(&mut self);

    /// One event for this block. `false` says there was no room, which the caller counts.
    fn push(&mut self, offset: u32, event: PluginEvent) -> bool;

    /// The value of the parameter `id` from an automation lane, at the first frame of this
    /// block. Called after [`Self::begin_block`] and before any event, so it comes after a value
    /// the host sent for the same parameter and is the one the block ends on. Room for
    /// [`MAX_AUTOMATED`] of them is kept on top of the events. `false` says there was no room.
    fn automate(&mut self, id: u32, value: f64) -> bool;

    /// Runs the plugin for `frames` frames with `input` on its first audio input port, and
    /// writes its first output port into `left` and `right`. `false` says the plugin failed
    /// and is not to be called again.
    fn run(
        &mut self,
        frames: usize,
        input: [&[f32]; CHANNELS],
        left: &mut [f32],
        right: &mut [f32],
    ) -> bool;

    /// Stops the plugin's processing. The audio thread only, as both formats ask, and only from
    /// the places here that have one. Calling it again does nothing.
    fn stop(&mut self);
}

/// Everything a plugin does inside its own code. The realtime sanitizer is switched off for
/// exactly the call and nothing around it: what a plugin allocates is its business, what this
/// crate allocates is a bug.
pub(crate) fn not_ours<T>(call: impl FnOnce() -> T) -> T {
    let _disabled = rtsan_standalone::ScopedDisabler::default();
    call()
}

/// What a slot plays when no plugin plays it: what it was given, unchanged. Nothing here
/// allocates.
fn pass_through(input: [&[f32]; CHANNELS], left: &mut [f32], right: &mut [f32], frames: usize) {
    left[..frames].copy_from_slice(&input[0][..frames]);
    right[..frames].copy_from_slice(&input[1][..frames]);
}

/// Copies our one stereo port into the channels of a plugin's first audio input port.
///
/// A plugin that takes one channel gets the left one, which is where a processor that makes
/// one signal puts it. A plugin that takes more than two gets silence in the rest, as it did
/// before effects existed. A plugin with no audio input takes nothing: what came before it in
/// the chain is lost, and what it plays takes its place.
pub(crate) fn copy_in(channels: &mut [Vec<f32>], frames: usize, input: [&[f32]; CHANNELS]) {
    for (index, channel) in channels.iter_mut().enumerate() {
        match input.get(index) {
            Some(samples) => channel[..frames].copy_from_slice(&samples[..frames]),
            None => channel[..frames].fill(0.0),
        }
    }
}

/// Copies the channels a plugin wrote into our one stereo port. A plugin with one channel is
/// heard on both, as every processor that makes one signal.
pub(crate) fn copy_out(channels: &[Vec<f32>], frames: usize, left: &mut [f32], right: &mut [f32]) {
    match channels.len() {
        0 => {}
        1 => {
            left.copy_from_slice(&channels[0][..frames]);
            right.copy_from_slice(&channels[0][..frames]);
        }
        _ => {
            left.copy_from_slice(&channels[0][..frames]);
            right.copy_from_slice(&channels[1][..frames]);
        }
    }
}

/// A pin of the record as the audio side knows it: whether a lane may move it, its range,
/// which a lane value is held to, and its value in the record, which it goes back to when its
/// lane lets go. In the format's own units.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct AutomatedPin {
    pub id: u32,
    /// Whether a lane may move it: the plugin says a host may automate it and it is not
    /// stepped.
    pub takes_lane: bool,
    pub minimum: f64,
    pub maximum: f64,
    pub record: f64,
}

/// The pins of a record: first those a lane may move, in the order of the index of an
/// [`Automation`] event, which is the order of the numbers the behaviour named, then the rest.
/// The rest are here for their record value: a pin that stops taking a lane, because the plugin
/// changed its parameters, goes back to it all the same. A fixed array, so it is copied in an
/// update and nothing is dropped on the audio thread.
#[derive(Copy, Clone, Debug)]
pub(crate) struct AutomatedPins {
    pins: [AutomatedPin; MAX_AUTOMATED],
    count: usize,
    /// How many of them take a lane, at the front.
    laned: usize,
}

impl AutomatedPins {
    pub(crate) const NONE: Self = Self {
        pins: [AutomatedPin {
            id: 0,
            takes_lane: false,
            minimum: 0.0,
            maximum: 0.0,
            record: 0.0,
        }; MAX_AUTOMATED],
        count: 0,
        laned: 0,
    };

    /// The first [`MAX_AUTOMATED`] of `pins`, which is as many as a record holds, those that
    /// take a lane first in the order given.
    pub(crate) fn new(pins: &[AutomatedPin]) -> Self {
        let mut automated = Self::NONE;
        let laned = pins.iter().filter(|pin| pin.takes_lane);
        let rest = pins.iter().filter(|pin| !pin.takes_lane);
        for (place, pin) in automated.pins.iter_mut().zip(laned.chain(rest)) {
            *place = *pin;
            automated.count += 1;
            automated.laned += usize::from(pin.takes_lane);
        }
        automated
    }

    /// The pin a lane of this index moves.
    fn get(&self, index: usize) -> Option<&AutomatedPin> {
        self.pins[..self.laned].get(index)
    }

    fn find(&self, id: u32) -> Option<&AutomatedPin> {
        self.pins[..self.count].iter().find(|pin| pin.id == id)
    }
}

/// Pins and the value of each, at most one of each pin, in a fixed array.
#[derive(Copy, Clone, Debug)]
struct PinValues {
    pins: [(u32, f64); MAX_AUTOMATED],
    count: usize,
}

impl PinValues {
    const NONE: Self = Self {
        pins: [(0, 0.0); MAX_AUTOMATED],
        count: 0,
    };

    fn as_slice(&self) -> &[(u32, f64)] {
        &self.pins[..self.count]
    }

    fn contains(&self, id: u32) -> bool {
        self.as_slice().iter().any(|(pin, _)| *pin == id)
    }

    /// Notes the value of `id`, over the one it had. Only the pins of one record are noted,
    /// which never fill it.
    fn insert(&mut self, id: u32, value: f64) {
        if let Some(place) = self.pins[..self.count]
            .iter_mut()
            .find(|(pin, _)| *pin == id)
        {
            place.1 = value;
            return;
        }
        if let Some(place) = self.pins.get_mut(self.count) {
            *place = (id, value);
            self.count += 1;
        }
    }
}

/// The pins whose value in the plugin may be a lane's, with that value, for the main thread: it
/// reads no such pin into the record, and shows the lane value in the plugin's own window where
/// the format needs the host to. A block writes a pin in before the plugin hears its lane, and
/// takes it out only after the plugin played its record value again, so a plugin value that is
/// a lane's is never read while the pin is out of it.
///
/// A sequence lock of atomics: the audio side never waits, and the main thread reads again
/// when a block wrote while it read. The generation goes up whenever the pins it holds change,
/// not their values, so the main thread can tell that a lane came or went while it read the
/// plugin, even when it came and went between two reads of this.
pub(crate) struct LanedPins {
    sequence: AtomicU64,
    generation: AtomicU64,
    count: AtomicUsize,
    ids: [AtomicU32; MAX_AUTOMATED],
    values: [AtomicU64; MAX_AUTOMATED],
}

impl LanedPins {
    pub(crate) fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            count: AtomicUsize::new(0),
            ids: std::array::from_fn(|_| AtomicU32::new(0)),
            values: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// The audio side, the only one that writes, so it reads what it wrote last as it is.
    fn write(&self, held: &PinValues) {
        let count = self.count.load(Ordering::Relaxed).min(MAX_AUTOMATED);
        let ids = &self.ids[..count];
        let same_pins = count == held.count
            && held
                .as_slice()
                .iter()
                .all(|(pin, _)| ids.iter().any(|id| id.load(Ordering::Relaxed) == *pin));
        let sequence = self.sequence.load(Ordering::Relaxed);
        // Odd while it writes, so a read in between knows.
        self.sequence
            .store(sequence.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        let places = self.ids.iter().zip(&self.values);
        for ((id, value), (pin, played)) in places.zip(held.as_slice()) {
            id.store(*pin, Ordering::Relaxed);
            value.store(played.to_bits(), Ordering::Relaxed);
        }
        self.count.store(held.count, Ordering::Relaxed);
        if !same_pins {
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
        self.sequence
            .store(sequence.wrapping_add(2), Ordering::Release);
    }

    /// The main thread: the generation, and each pin a lane holds with the value it plays.
    /// `None` when blocks kept writing while it read.
    pub(crate) fn read(&self) -> Option<(u64, Vec<(u32, f64)>)> {
        for _ in 0..8 {
            let before = self.sequence.load(Ordering::Acquire);
            if before % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let count = self.count.load(Ordering::Relaxed).min(MAX_AUTOMATED);
            let places = self.ids.iter().zip(&self.values).take(count);
            let held = places.map(|(id, value)| {
                let value = f64::from_bits(value.load(Ordering::Relaxed));
                (id.load(Ordering::Relaxed), value)
            });
            let held: Vec<(u32, f64)> = held.collect();
            let generation = self.generation.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if self.sequence.load(Ordering::Relaxed) == before {
                return Some((generation, held));
            }
        }
        None
    }
}

/// A started plugin and what its audio side tells the main thread of its lanes.
pub(crate) struct Playing {
    pub started: Box<dyn Started>,
    pub laned: Arc<LanedPins>,
}

/// The processor an instance of the plugin tool keeps. It is silent until the control side
/// sends it a plugin, and silent again when it is sent `None`.
pub(crate) struct HostedPlugin {
    plugin: Option<Playing>,
    /// Whether the plugin's own `run` failed. It is left silent instead of called again.
    failed: bool,
    /// The keys this processor has sent a note on for and no note off yet, so an `AllOff` ends
    /// exactly those. A plugin need not understand a note off that matches every key.
    keys_down: [bool; 128],
    /// Where each control stands as the plugin last heard it, by [`Control::index`], so an
    /// `AllOff` puts back only what moved. Kept when the plugin changes: one started again keeps
    /// what it heard, and telling a new one "at rest" once more costs nothing.
    controls: [Control; 4],
    /// The pins of the record.
    pins: AutomatedPins,
    /// The pins whose value in the plugin is a lane's, with that value. Each goes back to its
    /// record value in the first block in which its lane says nothing.
    held: PinValues,
}

/// What the control side sends.
pub(crate) enum HostedUpdate {
    /// The plugin to play, or nothing, and the pins of its record. The plugin that was there
    /// rides back to the control thread inside the update and is dropped there.
    Plugin(Option<Playing>, AutomatedPins),
    /// The pins of the record, for the plugin that plays: its record changed only its pins.
    Pins(AutomatedPins),
}

impl HostedPlugin {
    pub(crate) const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub(crate) const AUTOMATION: EventInput<Automation> = EventInput::new(1);
    pub(crate) const INPUT: AudioInput = AudioInput::new(0);
    pub(crate) const AUDIO: AudioOutput = AudioOutput::new(0);

    /// A processor with no plugin. It makes no sound.
    pub(crate) fn silent() -> Self {
        Self {
            plugin: None,
            failed: false,
            keys_down: [false; 128],
            controls: Control::REST,
            pins: AutomatedPins::NONE,
            held: PinValues::NONE,
        }
    }
}

impl Processor for HostedPlugin {
    type Update = HostedUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .event_input(Self::NOTES)
            .event_input(Self::AUTOMATION)
            .audio_output(Self::AUDIO)
    }

    fn prepare(&mut self, _config: &PrepareConfig) {}

    fn update(&mut self, update: &mut HostedUpdate) {
        let (plugin, pins) = match update {
            HostedUpdate::Plugin(plugin, pins) => (plugin, pins),
            HostedUpdate::Pins(pins) => {
                self.pins = *pins;
                return;
            }
        };
        // The plugin that was here goes back inside the update and is dropped on the control
        // thread. It stops here, while this is still the audio thread. Nothing heap-allocated
        // is dropped here.
        if let Some(leaving) = &mut self.plugin {
            leaving.started.stop();
        }
        std::mem::swap(&mut self.plugin, plugin);
        self.keys_down = [false; 128];
        self.failed = false;
        self.pins = *pins;
        // A new plugin starts at the values of its record, which the host sends it first, and
        // the main thread hears that no lane holds a pin of it.
        self.held = PinValues::NONE;
        if let Some(playing) = &self.plugin {
            playing.laned.write(&self.held);
        }
    }

    /// The engine is handing this processor back. The plugin stops here, on the audio thread,
    /// so that the control thread only ever deactivates one that is already stopped.
    fn leaving(&mut self) {
        if let Some(plugin) = &mut self.plugin {
            plugin.started.stop();
        }
    }

    /// The plugin's own latency. A slot with no plugin passes its input through and has none.
    /// A plugin that is started again after its latency changed arrives in an update, which is
    /// when the engine reads this.
    fn latency(&self) -> u32 {
        self.plugin
            .as_ref()
            .map_or(0, |plugin| plugin.started.latency())
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let events = context.event_inputs.get(Self::NOTES);
        let lanes = context.event_inputs.get(Self::AUTOMATION);
        let input = context.audio_inputs.get(Self::INPUT);
        let [left, right] = context.audio_outputs.get(Self::AUDIO);
        let frames = context.frames;
        let playing = self.plugin.as_mut().filter(|_| !self.failed);
        let Some(Playing { started, laned }) = playing else {
            // No plugin, or one that failed in an earlier block: the slot passes what it is
            // given through. For an instrument that is the silence of an input nothing
            // reaches, and for an effect it is the track playing on through a slot whose
            // plugin is missing.
            pass_through(input, left, right, frames);
            return;
        };
        let (keys_down, controls) = (&mut self.keys_down, &mut self.controls);
        let mut dropped = 0;
        let played = with_lanes(
            &mut **started,
            lanes,
            &self.pins,
            &mut self.held,
            laned,
            |plugin| {
                // More events in one block than the plugin's buffer holds. Counted, never
                // allocated.
                dropped = translate(plugin, events, keys_down, controls);
                plugin.run(frames, input, &mut left[..frames], &mut right[..frames])
            },
        );
        for _ in 0..dropped {
            context.event_outputs.count_dropped();
        }
        if !played {
            // The block a plugin fails on is the first one it does not play, so the slot
            // passes it through here and not from the next block. A backend writes nothing
            // into the output when it fails, so whatever it left there is the silence the
            // engine gave it, which would be a gap of one block in the chain.
            self.failed = true;
            pass_through(input, left, right, frames);
        }
    }
}

/// One block of a plugin with its lanes: the lanes go in, then `play` puts in the notes and
/// runs the plugin. What [`LanedPins`] says brackets it: every pin a lane holds before or in
/// this block is written held before the plugin hears anything, and only the pins a lane still
/// holds once it has run. A plugin with no lanes writes nothing.
fn with_lanes(
    plugin: &mut dyn Started,
    lanes: &[Timed<Automation>],
    pins: &AutomatedPins,
    held: &mut PinValues,
    laned: &LanedPins,
    play: impl FnOnce(&mut dyn Started) -> bool,
) -> bool {
    plugin.begin_block();
    let before = *held;
    automate(plugin, lanes, pins, held);
    let any = before.count + held.count > 0;
    if any {
        // The pins a lane lets go in this block still hold the lane's value until it runs.
        let mut both = *held;
        for (id, value) in before.as_slice() {
            if !both.contains(*id) {
                both.insert(*id, *value);
            }
        }
        laned.write(&both);
    }
    let played = play(plugin);
    if any {
        laned.write(held);
    }
    played
}

/// Plays the lanes of one block into the plugin: the value of every lane, held to the range of
/// its pin, and the record value of a pin in `held` whose lane said nothing, which is a lane
/// that went. A lane value that does not fit comes again in the next block, as every lane value
/// does; a record value that does not fit stays held and goes in the next block.
fn automate(
    plugin: &mut dyn Started,
    lanes: &[Timed<Automation>],
    pins: &AutomatedPins,
    held: &mut PinValues,
) {
    let mut heard = PinValues::NONE;
    for timed in lanes {
        let Some(pin) = pins.get(usize::from(timed.event.parameter)) else {
            continue;
        };
        // Not `clamp`: it panics on a NaN, and nothing may panic on the audio thread.
        let value = f64::from(timed.event.value)
            .max(pin.minimum)
            .min(pin.maximum);
        plugin.automate(pin.id, value);
        heard.insert(pin.id, value);
    }
    for (id, value) in held.as_slice() {
        if heard.contains(*id) {
            continue;
        }
        // A pin the record no longer holds has no value to go back to: the plugin keeps the
        // one it has, as when a pin no lane moves is taken out.
        if let Some(pin) = pins.find(*id)
            && !plugin.automate(pin.id, pin.record)
        {
            heard.insert(pin.id, *value);
        }
    }
    *held = heard;
}

/// Fills the plugin's own event buffer from one block of the note contract, after
/// [`Started::begin_block`]. Returns how many events did not fit.
fn translate(
    plugin: &mut dyn Started,
    events: &[Timed<NoteEvent>],
    keys_down: &mut [bool; 128],
    controls: &mut [Control; 4],
) -> u64 {
    let mut dropped = 0;
    for timed in events {
        let time = timed.offset as u32;
        let mut control = |control: Control| {
            if !send_control(plugin, time, control, controls) {
                dropped += 1;
            }
        };
        match timed.event {
            NoteEvent::Pedal(pedal) => control(Control::Pedal(pedal)),
            NoteEvent::Bend(bend) => control(Control::Bend(bend)),
            NoteEvent::ModWheel(amount) => control(Control::ModWheel(amount)),
            NoteEvent::Pressure(amount) => control(Control::Pressure(amount)),
            // A key is noted as down only when its event really reached the plugin. A note on
            // that did not fit must not be ended by a later `AllOff`, and a note off that did
            // not fit leaves its key down so that a later `AllOff` does end it.
            NoteEvent::On { pitch, velocity } => {
                let key = pitch.number();
                let event = PluginEvent::On {
                    key,
                    velocity: velocity.value(),
                };
                match plugin.push(time, event) {
                    true => keys_down[usize::from(key)] = true,
                    false => dropped += 1,
                }
            }
            NoteEvent::Off { pitch } => {
                let key = pitch.number();
                match plugin.push(time, PluginEvent::Off { key }) {
                    true => keys_down[usize::from(key)] = false,
                    false => dropped += 1,
                }
            }
            // The contract's "release everything". Both formats have a note off that matches
            // every key, and not every plugin handles one, so the exact keys go out instead,
            // and then every control that is not at rest.
            NoteEvent::AllOff => {
                for key in 0..128_u8 {
                    if !keys_down[usize::from(key)] {
                        continue;
                    }
                    match plugin.push(time, PluginEvent::Off { key }) {
                        true => keys_down[usize::from(key)] = false,
                        false => dropped += 1,
                    }
                }
                for rest in Control::REST {
                    if controls[rest.index()] != rest && !send_control(plugin, time, rest, controls)
                    {
                        dropped += 1;
                    }
                }
            }
        }
    }
    dropped
}

/// Tells the plugin where a control stands, when it takes that control. It is noted as heard
/// only when it really reached the plugin, so a move that did not fit is put back by a later
/// `AllOff` all the same. `false` says there was no room.
fn send_control(
    plugin: &mut dyn Started,
    time: u32,
    control: Control,
    controls: &mut [Control; 4],
) -> bool {
    if !plugin.takes(control) {
        return true;
    }
    let sent = plugin.push(time, PluginEvent::Control(control));
    if sent {
        controls[control.index()] = control;
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use sound_notes::{Pitch, Velocity};

    /// A plugin that takes `room` events and `lane_room` lane values and refuses the rest, so
    /// a test can say what the wrapper does with one that did not fit.
    struct Full {
        room: usize,
        taken: Vec<(u32, PluginEvent)>,
        lane_room: usize,
        automated: Vec<(u32, f64)>,
    }

    impl Started for Full {
        fn takes(&self, _control: Control) -> bool {
            true
        }

        fn latency(&self) -> u32 {
            0
        }

        fn begin_block(&mut self) {
            self.taken.clear();
            self.automated.clear();
        }

        fn automate(&mut self, id: u32, value: f64) -> bool {
            if self.automated.len() == self.lane_room {
                return false;
            }
            self.automated.push((id, value));
            true
        }

        fn push(&mut self, offset: u32, event: PluginEvent) -> bool {
            if self.taken.len() == self.room {
                return false;
            }
            self.taken.push((offset, event));
            true
        }

        fn run(
            &mut self,
            _frames: usize,
            _input: [&[f32]; CHANNELS],
            _left: &mut [f32],
            _right: &mut [f32],
        ) -> bool {
            true
        }

        fn stop(&mut self) {}
    }

    /// The wrapper's memory next to a [`Full`] plugin, one block at a time.
    struct Wrapper {
        plugin: Full,
        keys_down: [bool; 128],
        controls: [Control; 4],
    }

    impl Wrapper {
        fn with_room(room: usize) -> Self {
            Self {
                plugin: Full {
                    room,
                    taken: Vec::new(),
                    lane_room: MAX_AUTOMATED,
                    automated: Vec::new(),
                },
                keys_down: [false; 128],
                controls: Control::REST,
            }
        }

        /// One block of `events`, all at offset 0. Gives how many did not fit.
        fn play(&mut self, events: &[NoteEvent]) -> u64 {
            let events: Vec<_> = events
                .iter()
                .map(|event| Timed {
                    offset: 0,
                    event: *event,
                })
                .collect();
            self.plugin.begin_block();
            translate(
                &mut self.plugin,
                &events,
                &mut self.keys_down,
                &mut self.controls,
            )
        }

        fn taken(&self) -> Vec<PluginEvent> {
            self.plugin.taken.iter().map(|(_, event)| *event).collect()
        }
    }

    /// Two pins: `7` from 0 to 1 at a half in the record, `3` from 20 to 20000 at 1000.
    fn pins() -> AutomatedPins {
        let pin = |id, minimum, maximum, record| AutomatedPin {
            id,
            takes_lane: true,
            minimum,
            maximum,
            record,
        };
        AutomatedPins::new(&[pin(7, 0.0, 1.0, 0.5), pin(3, 20.0, 20_000.0, 1000.0)])
    }

    /// One block of lanes into `plugin`, as `(index, value)`. Gives what reached the plugin.
    fn lanes(
        plugin: &mut Full,
        pins: &AutomatedPins,
        held: &mut PinValues,
        values: &[(u16, f32)],
    ) -> Vec<(u32, f64)> {
        let lanes: Vec<_> = values
            .iter()
            .map(|&(parameter, value)| Timed {
                offset: 0,
                event: Automation { parameter, value },
            })
            .collect();
        plugin.begin_block();
        automate(plugin, &lanes, pins, held);
        plugin.automated.clone()
    }

    fn plugin() -> Full {
        Full {
            room: 8,
            taken: Vec::new(),
            lane_room: MAX_AUTOMATED,
            automated: Vec::new(),
        }
    }

    #[test]
    fn a_lane_reaches_its_pin_held_to_its_range_and_an_index_no_pin_has_is_left_out() {
        let (mut plugin, mut held) = (plugin(), PinValues::NONE);
        let reached = lanes(
            &mut plugin,
            &pins(),
            &mut held,
            &[(0, 0.25), (1, 1e9), (2, 0.5), (0, f32::NAN)],
        );
        assert_eq!(reached, [(7, 0.25), (3, 20_000.0), (7, 0.0)]);
    }

    /// A lane that goes says nothing more, and the pin goes back to its record once. A pin the
    /// record no longer holds keeps the value it has.
    #[test]
    fn a_pin_whose_lane_says_nothing_goes_back_to_its_record_once() {
        let (mut plugin, mut held) = (plugin(), PinValues::NONE);
        lanes(&mut plugin, &pins(), &mut held, &[(0, 0.25), (1, 50.0)]);
        let reached = lanes(&mut plugin, &pins(), &mut held, &[(1, 60.0)]);
        assert_eq!(reached, [(3, 60.0), (7, 0.5)]);
        assert_eq!(
            lanes(&mut plugin, &pins(), &mut held, &[(1, 60.0)]),
            [(3, 60.0)]
        );
        let none = AutomatedPins::NONE;
        assert_eq!(lanes(&mut plugin, &none, &mut held, &[]), []);
        assert_eq!(lanes(&mut plugin, &pins(), &mut held, &[]), []);
    }

    /// The plugin changed its parameters while a lane held a pin, which takes no lane now: it
    /// goes back to its record all the same.
    #[test]
    fn a_pin_that_stops_taking_a_lane_goes_back_to_its_record() {
        let (mut plugin, mut held) = (plugin(), PinValues::NONE);
        lanes(&mut plugin, &pins(), &mut held, &[(0, 0.25)]);
        let stepped = AutomatedPin {
            id: 7,
            takes_lane: false,
            minimum: 0.0,
            maximum: 1.0,
            record: 0.5,
        };
        let now = AutomatedPins::new(&[stepped]);
        assert_eq!(
            lanes(&mut plugin, &now, &mut held, &[(0, 0.75)]),
            [(7, 0.5)]
        );
    }

    /// While the plugin runs a block that starts a lane, and the one in which the lane goes,
    /// the table says the pin is held; after the second, it is free.
    #[test]
    fn a_pin_is_written_held_before_the_plugin_hears_its_lane_and_free_after_its_record() {
        let (mut plugin, mut held) = (plugin(), PinValues::NONE);
        let laned = LanedPins::new();
        let mut block = |values: &[(u16, f32)]| {
            let lanes: Vec<_> = values
                .iter()
                .map(|&(parameter, value)| Timed {
                    offset: 0,
                    event: Automation { parameter, value },
                })
                .collect();
            let mut seen = None;
            with_lanes(&mut plugin, &lanes, &pins(), &mut held, &laned, |_| {
                seen = laned.read().map(|(_, pins)| pins);
                true
            });
            (seen, laned.read().map(|(_, pins)| pins))
        };
        let (during, after) = block(&[(0, 0.25)]);
        assert_eq!(during, Some(vec![(7, 0.25)]));
        assert_eq!(after, Some(vec![(7, 0.25)]));
        let (during, after) = block(&[]);
        assert_eq!(during, Some(vec![(7, 0.25)]));
        assert_eq!(after, Some(vec![]));
    }

    /// The generation moves when a lane comes or goes, and not while it only moves: a poll that
    /// sees it move while it reads the plugin knows a lane value may be in what it read.
    #[test]
    fn the_generation_moves_when_the_held_pins_change_and_not_with_their_values() {
        let (mut plugin, mut held) = (plugin(), PinValues::NONE);
        let laned = LanedPins::new();
        let mut block = |values: &[(u16, f32)]| {
            let lanes: Vec<_> = values
                .iter()
                .map(|&(parameter, value)| Timed {
                    offset: 0,
                    event: Automation { parameter, value },
                })
                .collect();
            with_lanes(&mut plugin, &lanes, &pins(), &mut held, &laned, |_| true);
            laned.read().map(|(generation, _)| generation)
        };
        let start = block(&[]);
        let came = block(&[(0, 0.25)]);
        assert_ne!(came, start);
        assert_eq!(block(&[(0, 0.5)]), came);
        let second = block(&[(0, 0.5), (1, 400.0)]);
        assert_ne!(second, came);
        let went = block(&[]);
        assert_ne!(went, second);
        assert_eq!(block(&[]), went);
    }

    /// A record value with no room in its block is not forgotten: the pin would stay on the
    /// last value of a lane that is gone.
    #[test]
    fn a_record_value_that_did_not_fit_goes_in_the_next_block() {
        let (mut plugin, mut held) = (plugin(), PinValues::NONE);
        lanes(&mut plugin, &pins(), &mut held, &[(0, 0.25)]);
        plugin.lane_room = 0;
        assert_eq!(lanes(&mut plugin, &pins(), &mut held, &[]), []);
        plugin.lane_room = MAX_AUTOMATED;
        assert_eq!(lanes(&mut plugin, &pins(), &mut held, &[]), [(7, 0.5)]);
    }

    fn on(key: u8) -> NoteEvent {
        NoteEvent::On {
            pitch: Pitch::new(key).expect("a pitch"),
            velocity: Velocity::new(100).expect("a velocity"),
        }
    }

    fn off(key: u8) -> NoteEvent {
        NoteEvent::Off {
            pitch: Pitch::new(key).expect("a pitch"),
        }
    }

    fn bend(value: i16) -> Bend {
        Bend::new(value).expect("a bend")
    }

    /// A note off that did not fit leaves its key down, so the `AllOff` of a stop still ends
    /// it. Forgetting the key here is a note that sounds for ever.
    #[test]
    fn a_note_off_that_did_not_fit_is_still_ended_by_all_off() {
        let mut wrapper = Wrapper::with_room(1);
        assert_eq!(wrapper.play(&[on(60)]), 0);
        assert_eq!(wrapper.play(&[off(60)]), 0);
        // Now with no room: the note off is counted and the key stays down.
        assert_eq!(wrapper.play(&[on(60)]), 0);
        wrapper.plugin.room = 0;
        assert_eq!(wrapper.play(&[off(60)]), 1);
        wrapper.plugin.room = 8;
        assert_eq!(wrapper.play(&[NoteEvent::AllOff]), 0);
        assert_eq!(wrapper.taken(), [PluginEvent::Off { key: 60 }]);
    }

    /// A note on that did not fit never reached the plugin, so an `AllOff` must not send a
    /// note off for a note the plugin never started.
    #[test]
    fn a_note_on_that_did_not_fit_is_not_ended_by_all_off() {
        let mut wrapper = Wrapper::with_room(0);
        assert_eq!(wrapper.play(&[on(60)]), 1);
        wrapper.plugin.room = 8;
        assert_eq!(wrapper.play(&[NoteEvent::AllOff]), 0);
        assert_eq!(wrapper.taken(), []);
    }

    /// `AllOff` puts back exactly the controls that are not at rest, after the keys. One that
    /// moved and came back needs nothing.
    #[test]
    fn all_off_puts_back_only_the_controls_that_moved() {
        let mut wrapper = Wrapper::with_room(16);
        let moved = [
            on(60),
            NoteEvent::Pedal(Pedal::new(127).expect("a pedal")),
            NoteEvent::Bend(bend(-4000)),
            NoteEvent::ModWheel(Amount::new(90).expect("an amount")),
            NoteEvent::ModWheel(Amount::NONE),
        ];
        assert_eq!(wrapper.play(&moved), 0);
        assert_eq!(wrapper.play(&[NoteEvent::AllOff]), 0);
        assert_eq!(
            wrapper.taken(),
            [
                PluginEvent::Off { key: 60 },
                PluginEvent::Control(Control::Pedal(Pedal::UP)),
                PluginEvent::Control(Control::Bend(Bend::MIDDLE)),
            ]
        );
        // Everything is at rest now, so a second one sends nothing.
        assert_eq!(wrapper.play(&[NoteEvent::AllOff]), 0);
        assert_eq!(wrapper.taken(), []);
    }

    /// A wheel move that did not fit never reached the plugin, so the plugin stands where it
    /// heard it last, and `AllOff` puts back that.
    #[test]
    fn a_wheel_move_that_did_not_fit_is_not_noted_as_heard() {
        let mut wrapper = Wrapper::with_room(1);
        assert_eq!(wrapper.play(&[NoteEvent::Bend(bend(8191))]), 0);
        wrapper.plugin.room = 0;
        assert_eq!(wrapper.play(&[NoteEvent::Bend(Bend::MIDDLE)]), 1);
        wrapper.plugin.room = 8;
        assert_eq!(wrapper.play(&[NoteEvent::AllOff]), 0);
        assert_eq!(
            wrapper.taken(),
            [PluginEvent::Control(Control::Bend(Bend::MIDDLE))]
        );
    }

    #[test]
    fn the_bend_goes_as_fourteen_bits_with_the_middle_at_8192() {
        assert_eq!(Control::Bend(bend(-8192)).midi_value(), 0);
        assert_eq!(Control::Bend(Bend::MIDDLE).midi_value(), 8192);
        assert_eq!(Control::Bend(bend(8191)).midi_value(), 16383);
    }

    #[test]
    fn every_control_at_rest_has_its_own_place() {
        for (index, rest) in Control::REST.into_iter().enumerate() {
            assert_eq!(rest.index(), index);
        }
    }
}
