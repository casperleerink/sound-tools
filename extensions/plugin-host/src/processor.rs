//! The engine processor around a plugin's audio processor.
//!
//! The plugin's own handle stays on the control thread, see `host.rs`. What comes here is the
//! audio processor, which CLAP allows on the audio thread. This wrapper translates the note
//! contract into CLAP events, calls the plugin and copies its first output port into our one
//! stereo port.
//!
//! Nothing here allocates, locks or makes a system call. Every buffer is made when the plugin
//! is loaded, and the plugin is given [`OutputEvents::void`], so it has nowhere to make this
//! thread grow a buffer either. What the plugin does inside its own calls is not ours: the
//! realtime sanitizer is switched off around each of those calls and around nothing else.
//!
//! CLAP wants `start_processing` and `stop_processing` on the audio thread. This wrapper is the
//! only place that has one, so it stops the plugin before it lets it go: on a swap in
//! [`Processor::update`] and on [`Processor::leaving`], which is the engine handing the
//! processor back. [`Drop`] is the last resort, for the engine itself being torn down, when
//! there is no audio thread left to do it on.

use clack_host::events::Match;
use clack_host::events::event_types::{MidiEvent, NoteOffEvent, NoteOnEvent};
use clack_host::prelude::*;
use sound_core::{
    AudioOutput, EventInput, MAX_BLOCK, Ports, PrepareConfig, ProcessContext, Processor, Timed,
};
use sound_notes::{NoteEvent, Pedal};

use crate::host::SoundToolsHost;

/// How many CLAP events one block can carry into the plugin. An `AllOff` alone can be 129 of
/// them. Anything above this is counted and dropped, never allocated.
const EVENT_CAPACITY: usize = 512;

/// MIDI channel 1, controller 64: the sustain pedal. The value goes through as it was played.
const SUSTAIN_CONTROLLER: u8 = 64;

/// Which events the plugin's note port takes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// CLAP note events. The sustain pedal still needs MIDI, see [`Loaded::pedal_reaches_plugin`].
    Clap,
    Midi,
}

/// A plugin that is loaded and started, with every buffer its process call needs.
pub struct Loaded {
    audio: PluginAudioProcessor<SoundToolsHost>,
    dialect: Dialect,
    /// Whether the note port takes MIDI, which is the only way to send the sustain pedal.
    pub pedal_reaches_plugin: bool,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    /// Silence for the first audio input port of the plugin, one buffer per channel. Empty
    /// when the plugin takes no audio in, which is the usual case for an instrument.
    input_channels: Vec<Vec<f32>>,
    /// The first audio output port of the plugin, one buffer per channel.
    output_channels: Vec<Vec<f32>>,
    input_events: EventBuffer,
    /// The plugin's process call failed. It is left silent instead of called again.
    failed: bool,
}

impl Loaded {
    pub fn new(
        audio: PluginAudioProcessor<SoundToolsHost>,
        dialect: Dialect,
        pedal_reaches_plugin: bool,
        input_channel_count: usize,
        output_channel_count: usize,
    ) -> Self {
        let buffers = |count: usize| (0..count).map(|_| vec![0.0; MAX_BLOCK]).collect();
        Self {
            audio,
            dialect,
            pedal_reaches_plugin,
            input_ports: AudioPorts::with_capacity(input_channel_count.max(1), 1),
            output_ports: AudioPorts::with_capacity(output_channel_count.max(1), 1),
            input_channels: buffers(input_channel_count),
            output_channels: buffers(output_channel_count),
            input_events: EventBuffer::with_capacity(EVENT_CAPACITY),
            failed: false,
        }
    }

    /// Stops the plugin's processing. Audio thread only, as CLAP asks, and only from the two
    /// places that have one. Calling it again does nothing.
    fn stop(&mut self) {
        // The plugin's own `stop_processing` runs in here.
        not_ours(|| self.audio.ensure_processing_stopped());
    }
}

/// Everything a plugin does inside its own code. The realtime sanitizer is switched off for
/// exactly the call and nothing around it: what a plugin allocates is its business, what this
/// crate allocates is a bug.
fn not_ours<T>(call: impl FnOnce() -> T) -> T {
    let _disabled = rtsan_standalone::ScopedDisabler::default();
    call()
}

impl Drop for Loaded {
    fn drop(&mut self) {
        // Reached only when the engine itself is gone, which ends the audio thread before this
        // runs. Every other way out of the engine stops the plugin there first.
        self.stop();
    }
}

/// The processor an instance of the plugin tool keeps. It is silent until the control side
/// sends it a plugin, and silent again when it is sent `None`.
pub struct HostedPlugin {
    plugin: Option<Box<Loaded>>,
    /// The keys this processor has sent a note on for and no note off yet, so an `AllOff`
    /// ends exactly those. A plugin need not understand a note off that matches every key.
    keys_down: [bool; 128],
}

/// What the control side sends: the plugin to play, or nothing. The one that was there rides
/// back to the control thread inside the update and is dropped there.
pub type HostedUpdate = Option<Box<Loaded>>;

impl HostedPlugin {
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const AUDIO: AudioOutput = AudioOutput::new(0);

    /// A processor with no plugin. It makes no sound.
    pub fn silent() -> Self {
        Self {
            plugin: None,
            keys_down: [false; 128],
        }
    }
}

impl Processor for HostedPlugin {
    type Update = HostedUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
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
        let [left, right] = context.audio_outputs.get(Self::AUDIO);
        let frames = context.frames;
        let Some(plugin) = self.plugin.as_deref_mut() else {
            return;
        };
        if plugin.failed {
            return;
        }
        // More events in one block than the plugin's buffer holds. Counted, never allocated.
        for _ in 0..translate(plugin, events, &mut self.keys_down) {
            context.event_outputs.count_dropped();
        }
        if !run(plugin, frames) {
            plugin.failed = true;
            return;
        }
        let channels = &plugin.output_channels;
        match channels.len() {
            0 => {}
            // One channel: the same signal on both, as every processor that makes one signal.
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
}

/// Fills the plugin's input event buffer from one block of the note contract. Returns how many
/// events did not fit.
fn translate(plugin: &mut Loaded, events: &[Timed<NoteEvent>], keys_down: &mut [bool; 128]) -> u64 {
    plugin.input_events.clear();
    let mut room = EVENT_CAPACITY;
    let mut dropped = 0;
    for timed in events {
        let time = timed.offset as u32;
        match timed.event {
            NoteEvent::On { pitch, velocity } => {
                let key = pitch.number();
                if room == 0 {
                    dropped += 1;
                    continue;
                }
                room -= 1;
                keys_down[usize::from(key)] = true;
                push_note_on(plugin, time, key, velocity.value());
            }
            NoteEvent::Off { pitch } => {
                let key = pitch.number();
                if room == 0 {
                    dropped += 1;
                    continue;
                }
                room -= 1;
                keys_down[usize::from(key)] = false;
                push_note_off(plugin, time, key);
            }
            NoteEvent::Pedal(pedal) => {
                if !plugin.pedal_reaches_plugin {
                    continue;
                }
                if room == 0 {
                    dropped += 1;
                    continue;
                }
                room -= 1;
                push_pedal(plugin, time, pedal);
            }
            // The contract's "release everything". CLAP has a note off that matches every key,
            // but not every plugin handles it, so the exact keys go out instead.
            NoteEvent::AllOff => {
                for key in 0..128_u8 {
                    if !keys_down[usize::from(key)] {
                        continue;
                    }
                    if room == 0 {
                        dropped += 1;
                        continue;
                    }
                    room -= 1;
                    keys_down[usize::from(key)] = false;
                    push_note_off(plugin, time, key);
                }
                if plugin.pedal_reaches_plugin && room > 0 {
                    room -= 1;
                    push_pedal(plugin, time, Pedal::UP);
                }
            }
        }
    }
    dropped
}

fn push_note_on(plugin: &mut Loaded, time: u32, key: u8, velocity: u8) {
    match plugin.dialect {
        Dialect::Clap => {
            let pckn = Pckn::new(0_u16, 0_u16, u16::from(key), Match::All);
            let event = NoteOnEvent::new(time, pckn, f64::from(velocity) / 127.0);
            plugin.input_events.push(&event);
        }
        Dialect::Midi => {
            let event = MidiEvent::new(time, 0, [0x90, key, velocity]);
            plugin.input_events.push(&event);
        }
    }
}

fn push_note_off(plugin: &mut Loaded, time: u32, key: u8) {
    match plugin.dialect {
        Dialect::Clap => {
            let pckn = Pckn::new(0_u16, 0_u16, u16::from(key), Match::All);
            // CLAP's note off carries a release velocity. The note contract has none, so the
            // usual half value goes out.
            let event = NoteOffEvent::new(time, pckn, 0.5);
            plugin.input_events.push(&event);
        }
        Dialect::Midi => {
            let event = MidiEvent::new(time, 0, [0x80, key, 64]);
            plugin.input_events.push(&event);
        }
    }
}

/// The pedal always goes as raw MIDI, with its value: CLAP note events have no sustain.
fn push_pedal(plugin: &mut Loaded, time: u32, pedal: Pedal) {
    let event = MidiEvent::new(time, 0, [0xB0, SUSTAIN_CONTROLLER, pedal.value()]);
    plugin.input_events.push(&event);
}

/// Calls the plugin for one block. Returns whether it worked.
fn run(plugin: &mut Loaded, frames: usize) -> bool {
    let Loaded {
        audio,
        input_ports,
        output_ports,
        input_channels,
        output_channels,
        input_events,
        ..
    } = plugin;

    let inputs = if input_channels.is_empty() {
        InputAudioBuffers::empty()
    } else {
        input_ports.with_input_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                input_channels
                    .iter_mut()
                    .map(|channel| InputChannel::constant(&mut channel[..frames])),
            ),
        }])
    };
    let mut outputs = if output_channels.is_empty() {
        OutputAudioBuffers::empty()
    } else {
        output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(
                output_channels
                    .iter_mut()
                    .map(|channel| &mut channel[..frames]),
            ),
        }])
    };
    let input = input_events.as_input();
    // Nothing reads what a plugin sends out: MIDI from a plugin is not built. A void list
    // takes every event and keeps none, so a plugin that sends thousands grows nothing here.
    let mut output = OutputEvents::void();

    let started = match not_ours(|| audio.ensure_processing_started()) {
        Ok(started) => started,
        Err(_) => return false,
    };
    not_ours(|| started.process(&inputs, &mut outputs, &input, &mut output, None, None)).is_ok()
}
