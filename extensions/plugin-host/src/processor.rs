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
//! are down, the same expansion of `AllOff` and the same bound on how many events one block may
//! carry. A backend only says how one event is written down, through [`Started::push`].
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

use sound_core::{
    AudioInput, AudioOutput, CHANNELS, EventInput, Ports, PrepareConfig, ProcessContext, Processor,
    Timed,
};
use sound_notes::{NoteEvent, Pedal};

/// How many events one block can carry into the plugin. An `AllOff` alone can be 129 of them.
/// Anything above this is counted and dropped, never allocated.
pub const EVENT_CAPACITY: usize = 512;

/// MIDI channel 1, controller 64: the sustain pedal. The value goes through as it was played.
pub const SUSTAIN_CONTROLLER: u8 = 64;

/// One thing to tell the plugin, at a frame offset in the block. This is the note contract with
/// `AllOff` already expanded into the keys that are really down.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PluginEvent {
    On { key: u8, velocity: u8 },
    Off { key: u8 },
    Pedal(Pedal),
}

/// A plugin that is loaded and started, seen from the audio thread. One implementation per
/// format.
///
/// No call of this trait may allocate, lock or make a system call in our own code. A call into
/// the plugin itself is wrapped in [`not_ours`].
pub trait Started: Send {
    /// Whether the sustain pedal reaches this plugin. A plugin that offers no way to receive it
    /// gets the notes and not the pedal, and its record says so.
    fn takes_pedal(&self) -> bool;

    /// A new block: everything the last one carried is forgotten.
    fn begin_block(&mut self);

    /// One event for this block. `false` says there was no room, which the caller counts.
    fn push(&mut self, offset: u32, event: PluginEvent) -> bool;

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
pub fn not_ours<T>(call: impl FnOnce() -> T) -> T {
    let _disabled = rtsan_standalone::ScopedDisabler::default();
    call()
}

/// Copies our one stereo port into the channels of a plugin's first audio input port.
///
/// A plugin that takes one channel gets the left one, which is where a processor that makes
/// one signal puts it. A plugin that takes more than two gets silence in the rest, as it did
/// before effects existed. A plugin with no audio input takes nothing: what came before it in
/// the chain is lost, and what it plays takes its place.
pub fn copy_in(channels: &mut [Vec<f32>], frames: usize, input: [&[f32]; CHANNELS]) {
    for (index, channel) in channels.iter_mut().enumerate() {
        match input.get(index) {
            Some(samples) => channel[..frames].copy_from_slice(&samples[..frames]),
            None => channel[..frames].fill(0.0),
        }
    }
}

/// Copies the channels a plugin wrote into our one stereo port. A plugin with one channel is
/// heard on both, as every processor that makes one signal.
pub fn copy_out(channels: &[Vec<f32>], frames: usize, left: &mut [f32], right: &mut [f32]) {
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

/// The processor an instance of the plugin tool keeps. It is silent until the control side
/// sends it a plugin, and silent again when it is sent `None`.
pub struct HostedPlugin {
    plugin: Option<Box<dyn Started>>,
    /// Whether the plugin's own `run` failed. It is left silent instead of called again.
    failed: bool,
    /// The keys this processor has sent a note on for and no note off yet, so an `AllOff` ends
    /// exactly those. A plugin need not understand a note off that matches every key.
    keys_down: [bool; 128],
}

/// What the control side sends: the plugin to play, or nothing. The one that was there rides
/// back to the control thread inside the update and is dropped there.
pub type HostedUpdate = Option<Box<dyn Started>>;

impl HostedPlugin {
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const AUDIO: AudioOutput = AudioOutput::new(0);

    /// A processor with no plugin. It makes no sound.
    pub fn silent() -> Self {
        Self {
            plugin: None,
            failed: false,
            keys_down: [false; 128],
        }
    }
}

impl Processor for HostedPlugin {
    type Update = HostedUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .event_input(Self::NOTES)
            .audio_output(Self::AUDIO)
    }

    fn prepare(&mut self, _config: &PrepareConfig) {}

    fn update(&mut self, update: &mut HostedUpdate) {
        // The plugin that was here goes back inside the update and is dropped on the control
        // thread. It stops here, while this is still the audio thread. Nothing heap-allocated
        // is dropped here.
        if let Some(leaving) = self.plugin.as_deref_mut() {
            leaving.stop();
        }
        std::mem::swap(&mut self.plugin, update);
        self.keys_down = [false; 128];
        self.failed = false;
    }

    /// The engine is handing this processor back. The plugin stops here, on the audio thread,
    /// so that the control thread only ever deactivates one that is already stopped.
    fn leaving(&mut self) {
        if let Some(plugin) = self.plugin.as_deref_mut() {
            plugin.stop();
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let events = context.event_inputs.get(Self::NOTES);
        let input = context.audio_inputs.get(Self::INPUT);
        let [left, right] = context.audio_outputs.get(Self::AUDIO);
        let frames = context.frames;
        let plugin = self.plugin.as_deref_mut().filter(|_| !self.failed);
        let Some(plugin) = plugin else {
            // No plugin, or one that failed: the slot passes what it is given through. For an
            // instrument that is the silence of an input nothing reaches, and for an effect it
            // is the track playing on through a slot whose plugin is missing.
            left[..frames].copy_from_slice(&input[0][..frames]);
            right[..frames].copy_from_slice(&input[1][..frames]);
            return;
        };
        // More events in one block than the plugin's buffer holds. Counted, never allocated.
        for _ in 0..translate(plugin, events, &mut self.keys_down) {
            context.event_outputs.count_dropped();
        }
        if !plugin.run(frames, input, &mut left[..frames], &mut right[..frames]) {
            self.failed = true;
        }
    }
}

/// Fills the plugin's own event buffer from one block of the note contract. Returns how many
/// events did not fit.
fn translate(
    plugin: &mut dyn Started,
    events: &[Timed<NoteEvent>],
    keys_down: &mut [bool; 128],
) -> u64 {
    plugin.begin_block();
    let mut dropped = 0;
    for timed in events {
        let time = timed.offset as u32;
        match timed.event {
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
            NoteEvent::Pedal(pedal) => {
                if plugin.takes_pedal() && !plugin.push(time, PluginEvent::Pedal(pedal)) {
                    dropped += 1;
                }
            }
            // The contract's "release everything". Both formats have a note off that matches
            // every key, and not every plugin handles one, so the exact keys go out instead.
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
                if plugin.takes_pedal() && !plugin.push(time, PluginEvent::Pedal(Pedal::UP)) {
                    dropped += 1;
                }
            }
        }
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;
    use sound_notes::{Pitch, Velocity};

    /// A plugin that takes `room` events and refuses the rest, so a test can say what the
    /// wrapper does with an event that did not fit.
    struct Full {
        room: usize,
        taken: Vec<(u32, PluginEvent)>,
    }

    impl Started for Full {
        fn takes_pedal(&self) -> bool {
            true
        }

        fn begin_block(&mut self) {
            self.taken.clear();
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

    fn on(key: u8) -> Timed<NoteEvent> {
        Timed {
            offset: 0,
            event: NoteEvent::On {
                pitch: Pitch::new(key).expect("a pitch"),
                velocity: Velocity::new(100).expect("a velocity"),
            },
        }
    }

    fn off(key: u8) -> Timed<NoteEvent> {
        Timed {
            offset: 0,
            event: NoteEvent::Off {
                pitch: Pitch::new(key).expect("a pitch"),
            },
        }
    }

    fn all_off() -> Timed<NoteEvent> {
        Timed {
            offset: 0,
            event: NoteEvent::AllOff,
        }
    }

    /// A note off that did not fit leaves its key down, so the `AllOff` of a stop still ends
    /// it. Forgetting the key here is a note that sounds for ever.
    #[test]
    fn a_note_off_that_did_not_fit_is_still_ended_by_all_off() {
        let mut plugin = Full {
            room: 1,
            taken: Vec::new(),
        };
        let mut keys_down = [false; 128];
        assert_eq!(translate(&mut plugin, &[on(60)], &mut keys_down), 0);
        assert_eq!(translate(&mut plugin, &[off(60)], &mut keys_down), 0);
        // Now with no room: the note off is counted and the key stays down.
        assert_eq!(translate(&mut plugin, &[on(60)], &mut keys_down), 0);
        plugin.room = 0;
        assert_eq!(translate(&mut plugin, &[off(60)], &mut keys_down), 1);
        plugin.room = 8;
        assert_eq!(translate(&mut plugin, &[all_off()], &mut keys_down), 0);
        assert_eq!(
            plugin.taken,
            [
                (0, PluginEvent::Off { key: 60 }),
                (0, PluginEvent::Pedal(Pedal::UP)),
            ]
        );
    }

    /// A note on that did not fit never reached the plugin, so an `AllOff` must not send a
    /// note off for a note the plugin never started.
    #[test]
    fn a_note_on_that_did_not_fit_is_not_ended_by_all_off() {
        let mut plugin = Full {
            room: 0,
            taken: Vec::new(),
        };
        let mut keys_down = [false; 128];
        assert_eq!(translate(&mut plugin, &[on(60)], &mut keys_down), 1);
        plugin.room = 8;
        assert_eq!(translate(&mut plugin, &[all_off()], &mut keys_down), 0);
        assert_eq!(plugin.taken, [(0, PluginEvent::Pedal(Pedal::UP))]);
    }
}
