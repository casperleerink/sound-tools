//! What an extension implements and touches: processors, ports, events and the process context.
//!
//! Extensions never see threads or queues. See `crates/core/README.md` for a walkthrough.

use std::any::{Any, TypeId};
use std::cell::Cell;
use std::marker::PhantomData;

/// Processors never see more frames than this in one `process` call.
pub const MAX_BLOCK: usize = 64;

pub(crate) type AudioBuffer = [f32; MAX_BLOCK];

/// What a processor may rely on for every later `process` call.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PrepareConfig {
    pub sample_rate: u32,
}

/// A unit of realtime work in the engine graph.
pub trait Processor: Send + 'static {
    /// The message the control side sends to this processor: parameter values, a new data
    /// snapshot, or anything else. Use `()` when there is nothing to send.
    type Update: Send + 'static;

    /// The ports of this processor. Called once, on the control thread.
    fn ports(&self) -> Ports;

    /// Control thread. May allocate. Called before the processor reaches the audio thread.
    fn prepare(&mut self, config: &PrepareConfig);

    /// Audio thread, at the start of a block. Realtime safe: no allocation, locks, I/O or logging.
    ///
    /// Copy small values out of `update`. Swap large or shared values (`Box`, `Arc`, `Vec`) with
    /// `std::mem::swap`, never drop them here. The engine carries whatever is left in `update`
    /// back to the control thread and drops it there.
    fn update(&mut self, update: &mut Self::Update);

    /// Audio thread. Realtime safe: no allocation, locks, I/O or logging.
    fn process(&mut self, context: &mut ProcessContext<'_>);
}

/// One block of work for one processor. The engine builds it. Transport info will be added here.
pub struct ProcessContext<'a> {
    /// Frames in this block, from 1 to [`MAX_BLOCK`].
    pub frames: usize,
    /// Engine time of the first frame of this block.
    pub start_frame: u64,
    pub audio_inputs: AudioInputs<'a>,
    pub audio_outputs: AudioOutputs<'a>,
    pub event_inputs: EventInputs<'a>,
    pub event_outputs: EventOutputs<'a>,
}

/// Anything an extension wants to send between processors at a frame position.
/// `Copy` keeps delivery free of allocation and drops on the audio thread.
pub trait Event: Copy + Send + 'static {}
impl<T: Copy + Send + 'static> Event for T {}

/// An event with its frame offset from the start of the block.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Timed<E> {
    pub offset: usize,
    pub event: E,
}

macro_rules! port_handle {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub struct $name(usize);

        impl $name {
            pub const fn new(index: usize) -> Self {
                Self(index)
            }
        }
    };
}

macro_rules! event_port_handle {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        pub struct $name<E> {
            index: usize,
            event: PhantomData<fn() -> E>,
        }

        impl<E> $name<E> {
            pub const fn new(index: usize) -> Self {
                Self {
                    index,
                    event: PhantomData,
                }
            }
        }

        impl<E> Clone for $name<E> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<E> Copy for $name<E> {}
    };
}

port_handle!(
    /// Handle to an audio input port. Indices count from 0 within audio inputs.
    AudioInput
);
port_handle!(
    /// Handle to an audio output port. Indices count from 0 within audio outputs.
    AudioOutput
);
event_port_handle!(
    /// Handle to an event input port carrying `E`. Indices count from 0 within event inputs.
    EventInput
);
event_port_handle!(
    /// Handle to an event output port carrying `E`. Indices count from 0 within event outputs.
    EventOutput
);

/// An input port of any kind, as used in connections.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputPort {
    Audio(usize),
    Events(usize),
}

/// An output port of any kind, as used in connections.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputPort {
    Audio(usize),
    Events(usize),
}

impl From<AudioInput> for InputPort {
    fn from(port: AudioInput) -> Self {
        Self::Audio(port.0)
    }
}

impl<E> From<EventInput<E>> for InputPort {
    fn from(port: EventInput<E>) -> Self {
        Self::Events(port.index)
    }
}

impl From<AudioOutput> for OutputPort {
    fn from(port: AudioOutput) -> Self {
        Self::Audio(port.0)
    }
}

impl<E> From<EventOutput<E>> for OutputPort {
    fn from(port: EventOutput<E>) -> Self {
        Self::Events(port.index)
    }
}

/// The ports a processor declares. Build it from the same handles `process` uses, so the
/// declared event type and the type read in `process` cannot disagree.
#[derive(Clone, Debug, Default)]
pub struct Ports {
    pub(crate) audio_inputs: usize,
    pub(crate) audio_outputs: usize,
    pub(crate) event_inputs: Vec<EventType>,
    pub(crate) event_outputs: Vec<EventType>,
    /// A handle was declared with an index other than the next free one.
    pub(crate) out_of_order: bool,
}

impl Ports {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn audio_input(mut self, port: AudioInput) -> Self {
        self.out_of_order |= port.0 != self.audio_inputs;
        self.audio_inputs += 1;
        self
    }

    pub fn audio_output(mut self, port: AudioOutput) -> Self {
        self.out_of_order |= port.0 != self.audio_outputs;
        self.audio_outputs += 1;
        self
    }

    pub fn event_input<E: Event>(mut self, port: EventInput<E>) -> Self {
        self.out_of_order |= port.index != self.event_inputs.len();
        self.event_inputs.push(EventType::of::<E>());
        self
    }

    pub fn event_output<E: Event>(mut self, port: EventOutput<E>) -> Self {
        self.out_of_order |= port.index != self.event_outputs.len();
        self.event_outputs.push(EventType::of::<E>());
        self
    }
}

/// Runtime identity of an event type. It lets the core check connections and build buffers
/// for a type it does not know.
#[derive(Copy, Clone)]
pub(crate) struct EventType {
    id: TypeId,
    name: &'static str,
    pub(crate) new_buffer: fn(usize) -> Box<dyn ErasedEventBuffer>,
}

impl EventType {
    fn of<E: Event>() -> Self {
        Self {
            id: TypeId::of::<E>(),
            name: std::any::type_name::<E>(),
            new_buffer: |capacity| Box::new(EventBuffer::<E>::new(capacity)),
        }
    }
}

impl PartialEq for EventType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Debug for EventType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name)
    }
}

/// A fixed-capacity, time-sorted event list. Allocated on the control thread with the schedule.
pub(crate) struct EventBuffer<E> {
    events: Vec<Timed<E>>,
    /// `Vec::with_capacity` may reserve more than asked. This keeps the limit exact.
    capacity: usize,
    overflow: u64,
}

impl<E: Event> EventBuffer<E> {
    fn new(capacity: usize) -> Self {
        Self {
            events: Vec::with_capacity(capacity),
            capacity,
            overflow: 0,
        }
    }

    /// Keeps the list sorted by offset. Events with equal offsets keep their arrival order.
    fn insert(&mut self, timed: Timed<E>) {
        if self.events.len() >= self.capacity {
            self.overflow += 1;
            return;
        }
        let position = self
            .events
            .partition_point(|existing| existing.offset <= timed.offset);
        self.events.insert(position, timed);
    }
}

/// An [`EventBuffer`] of a type the core does not know.
pub(crate) trait ErasedEventBuffer: Send {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn clear(&mut self);
    /// Merges the events of `source` into this buffer when both carry the same type.
    fn merge_from(&mut self, source: &dyn ErasedEventBuffer);
    /// Events rejected since the last call because the buffer was full.
    fn take_overflow(&mut self) -> u64;
}

impl<E: Event> ErasedEventBuffer for EventBuffer<E> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clear(&mut self) {
        self.events.clear();
    }

    fn merge_from(&mut self, source: &dyn ErasedEventBuffer) {
        if let Some(source) = source.as_any().downcast_ref::<Self>() {
            for timed in &source.events {
                self.insert(*timed);
            }
        }
    }

    fn take_overflow(&mut self) -> u64 {
        std::mem::take(&mut self.overflow)
    }
}

/// The audio inputs of one block. Unconnected inputs are silent. Several connections to one
/// input arrive summed.
pub struct AudioInputs<'a> {
    pub(crate) buffers: &'a [AudioBuffer],
    pub(crate) frames: usize,
    pub(crate) misuses: &'a Cell<u64>,
}

impl AudioInputs<'_> {
    /// An undeclared port reads as an empty slice and counts in `EngineStatus::port_misuses`.
    pub fn get(&self, port: AudioInput) -> &[f32] {
        let samples = self
            .buffers
            .get(port.0)
            .and_then(|buffer| buffer.get(..self.frames));
        samples.unwrap_or_else(|| misused(self.misuses))
    }
}

/// Counts one use of a handle that matches no declared port, and gives the empty stand-in.
fn misused<T: Default>(misuses: &Cell<u64>) -> T {
    misuses.set(misuses.get() + 1);
    T::default()
}

/// The audio outputs of one block. They start silent.
pub struct AudioOutputs<'a> {
    pub(crate) buffers: &'a mut [AudioBuffer],
    pub(crate) frames: usize,
    pub(crate) misuses: &'a Cell<u64>,
}

impl AudioOutputs<'_> {
    /// An undeclared port gives an empty slice and counts in `EngineStatus::port_misuses`.
    pub fn get(&mut self, port: AudioOutput) -> &mut [f32] {
        let [samples] = self.get_many([port]);
        samples
    }

    /// Several outputs at once, for example left and right in one loop. An undeclared port or
    /// the same port twice gives empty slices and counts in `EngineStatus::port_misuses`.
    pub fn get_many<const N: usize>(&mut self, ports: [AudioOutput; N]) -> [&mut [f32]; N] {
        let frames = self.frames;
        match self.buffers.get_disjoint_mut(ports.map(|port| port.0)) {
            Ok(buffers) => buffers.map(|buffer| buffer.get_mut(..frames).unwrap_or_default()),
            Err(_) => {
                misused::<()>(self.misuses);
                std::array::from_fn(|_| Default::default())
            }
        }
    }
}

/// The event inputs of one block, sorted by offset. Several connections to one input arrive merged.
pub struct EventInputs<'a> {
    pub(crate) buffers: &'a [Box<dyn ErasedEventBuffer>],
    pub(crate) misuses: &'a Cell<u64>,
}

impl EventInputs<'_> {
    /// A handle with an undeclared index or another event type than declared reads as an empty
    /// slice and counts in `EngineStatus::port_misuses`.
    pub fn get<E: Event>(&self, port: EventInput<E>) -> &[Timed<E>] {
        let buffer = self
            .buffers
            .get(port.index)
            .and_then(|buffer| buffer.as_any().downcast_ref::<EventBuffer<E>>());
        match buffer {
            Some(buffer) => &buffer.events,
            None => misused(self.misuses),
        }
    }
}

/// The event outputs of one block.
pub struct EventOutputs<'a> {
    pub(crate) buffers: &'a mut [Box<dyn ErasedEventBuffer>],
    pub(crate) frames: usize,
    pub(crate) misuses: &'a Cell<u64>,
}

impl EventOutputs<'_> {
    /// Sends `event` at `offset` frames into this block. Offsets past the block end land on its
    /// last frame. When the port's buffer is full the event is dropped and counted in
    /// `EngineStatus::event_overflows`. A handle with an undeclared index or another event type
    /// than declared sends nothing and counts in `EngineStatus::port_misuses`.
    pub fn push<E: Event>(&mut self, port: EventOutput<E>, offset: usize, event: E) {
        let offset = offset.min(self.frames.saturating_sub(1));
        let buffer = self
            .buffers
            .get_mut(port.index)
            .and_then(|buffer| buffer.as_any_mut().downcast_mut::<EventBuffer<E>>());
        match buffer {
            Some(buffer) => buffer.insert(Timed { offset, event }),
            None => misused(self.misuses),
        }
    }
}
