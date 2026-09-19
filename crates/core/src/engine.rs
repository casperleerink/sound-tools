//! The audio side of the engine: the slot table, the current schedule and `process_block`.
//!
//! Everything here runs on the audio thread. It allocates nothing and frees nothing. Whatever
//! it stops using travels back to the control side inside the batch that replaced it.

use std::any::Any;

use rtsan_standalone::nonblocking;

use crate::graph::Schedule;
use crate::processor::{
    AudioInputs, AudioOutputs, EventInputs, EventOutputs, MAX_BLOCK, ProcessContext, Processor,
};

/// A [`Processor`] without its `Update` type, so one table can hold every kind.
pub(crate) trait ErasedProcessor: Send {
    fn update(&mut self, update: &mut dyn Any);
    fn process(&mut self, context: &mut ProcessContext<'_>);
}

impl<P: Processor> ErasedProcessor for P {
    fn update(&mut self, update: &mut dyn Any) {
        if let Some(update) = update.downcast_mut::<P::Update>() {
            Processor::update(self, update);
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        Processor::process(self, context);
    }
}

pub(crate) type Slot = Option<Box<dyn ErasedProcessor>>;

/// One step of an edit. The audio thread applies it by swapping, so the value it replaces
/// ends up inside the command and rides back to the control thread with the batch.
pub(crate) enum Command {
    /// A larger slot table. The old table comes back empty.
    GrowSlots(Vec<Slot>),
    /// `Some` inserts a processor, `None` removes one.
    SetSlot {
        slot: usize,
        processor: Slot,
    },
    Update {
        slot: usize,
        update: Box<dyn Any + Send>,
    },
    SetSchedule(Box<Schedule>),
}

/// All commands of one edit. They apply at the start of the same block.
pub(crate) type Batch = Vec<Command>;

/// Counters the audio thread publishes after every `process_block` call. They only grow, so
/// reading the latest value never misses a report.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineStatus {
    /// Calls to `process_block`. One per device callback.
    pub blocks: u64,
    /// Engine time: frames processed since the engine was created.
    pub frames: u64,
    pub batches_applied: u64,
    /// Events dropped because an event buffer was full.
    pub event_overflows: u64,
    /// Times an edit had to wait a block because the return ring was full.
    pub return_ring_full: u64,
}

pub struct Engine {
    channels: usize,
    slots: Vec<Slot>,
    schedule: Box<Schedule>,
    commands: rtrb::Consumer<Batch>,
    returns: rtrb::Producer<Batch>,
    status: EngineStatus,
    status_writer: triple_buffer::Input<EngineStatus>,
}

impl Engine {
    pub(crate) fn from_parts(
        channels: usize,
        slot_count: usize,
        schedule: Schedule,
        commands: rtrb::Consumer<Batch>,
        returns: rtrb::Producer<Batch>,
        status_writer: triple_buffer::Input<EngineStatus>,
    ) -> Self {
        Self {
            channels,
            slots: std::iter::repeat_with(|| None).take(slot_count).collect(),
            schedule: Box::new(schedule),
            commands,
            returns,
            status: EngineStatus::default(),
            status_writer,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Renders the next frames into `output`, interleaved by channel. The device callback,
    /// offline rendering and tests all call this. Samples past the last whole frame are zeroed.
    #[nonblocking]
    pub fn process_block(&mut self, output: &mut [f32]) {
        // SAFETY: Rust formally assumes the default float environment. This sets only the
        // flush-to-zero bits and restores them when the closure returns. Those bits change
        // results for denormal values only, and nothing in the engine depends on denormal
        // results. Audio software does this as a rule: denormals make float math many times
        // slower, which shows up as dropouts in decaying tails.
        unsafe {
            no_denormals::no_denormals(|| {
                let whole = output.len() - output.len() % self.channels.max(1);
                let (frames, rest) = output.split_at_mut(whole);
                rest.fill(0.0);
                for sub_block in frames.chunks_mut(MAX_BLOCK * self.channels.max(1)) {
                    self.apply_batches();
                    self.run(sub_block);
                }
                self.status.blocks += 1;
                self.status_writer.write(self.status);
            });
        }
    }

    /// One batch in gives one batch out, so a batch is only taken when the return ring has room.
    /// Otherwise the batch waits in the command ring for a later block. Nothing is dropped here.
    fn apply_batches(&mut self) {
        while !self.commands.is_empty() {
            if self.returns.is_full() {
                self.status.return_ring_full += 1;
                return;
            }
            let Ok(mut batch) = self.commands.pop() else {
                return;
            };
            for command in &mut batch {
                match command {
                    Command::GrowSlots(table) => {
                        for (new, old) in table.iter_mut().zip(&mut self.slots) {
                            std::mem::swap(new, old);
                        }
                        std::mem::swap(table, &mut self.slots);
                    }
                    Command::SetSlot { slot, processor } => {
                        if let Some(current) = self.slots.get_mut(*slot) {
                            std::mem::swap(current, processor);
                        }
                    }
                    Command::Update { slot, update } => {
                        if let Some(Some(processor)) = self.slots.get_mut(*slot) {
                            processor.update(update.as_mut());
                        }
                    }
                    Command::SetSchedule(schedule) => {
                        std::mem::swap(schedule, &mut self.schedule);
                    }
                }
            }
            self.status.batches_applied += 1;
            // Cannot fail: this thread is the only producer and the ring had room above. If it
            // ever did fail, leaking is the realtime-safe choice over dropping here.
            if let Err(rtrb::PushError::Full(batch)) = self.returns.push(batch) {
                std::mem::forget(batch);
            }
        }
    }

    /// Runs the schedule once for a sub-block of at most `MAX_BLOCK` frames.
    fn run(&mut self, output: &mut [f32]) {
        let channels = self.channels.max(1);
        let frames = output.len() / channels;
        let Schedule {
            steps,
            audio_outputs,
            audio_scratch,
            event_outputs,
            event_inputs,
            device_sources,
        } = &mut *self.schedule;

        for step in steps.iter() {
            for (input, sources) in audio_scratch.iter_mut().zip(&step.audio_sources) {
                input.fill(0.0);
                for source in sources.iter().filter_map(|index| audio_outputs.get(*index)) {
                    for (sum, sample) in input.iter_mut().zip(source) {
                        *sum += sample;
                    }
                }
            }
            let step_event_inputs = event_inputs
                .get_mut(step.event_inputs.clone())
                .unwrap_or_default();
            for (input, sources) in step_event_inputs.iter_mut().zip(&step.event_sources) {
                input.clear();
                for source in sources.iter().filter_map(|index| event_outputs.get(*index)) {
                    input.merge_from(source.as_ref());
                }
                self.status.event_overflows += input.take_overflow();
            }
            let step_audio_outputs = audio_outputs
                .get_mut(step.audio_outputs.clone())
                .unwrap_or_default();
            step_audio_outputs
                .iter_mut()
                .for_each(|buffer| buffer.fill(0.0));
            let step_event_outputs = event_outputs
                .get_mut(step.event_outputs.clone())
                .unwrap_or_default();
            step_event_outputs
                .iter_mut()
                .for_each(|buffer| buffer.clear());

            if let Some(Some(processor)) = self.slots.get_mut(step.slot) {
                processor.process(&mut ProcessContext {
                    frames,
                    start_frame: self.status.frames,
                    audio_inputs: AudioInputs {
                        buffers: audio_scratch
                            .get(..step.audio_sources.len())
                            .unwrap_or_default(),
                        frames,
                    },
                    audio_outputs: AudioOutputs {
                        buffers: &mut *step_audio_outputs,
                        frames,
                    },
                    event_inputs: EventInputs {
                        buffers: step_event_inputs,
                    },
                    event_outputs: EventOutputs {
                        buffers: &mut *step_event_outputs,
                        frames,
                    },
                });
            }
            for buffer in step_event_outputs {
                self.status.event_overflows += buffer.take_overflow();
            }
        }

        output.fill(0.0);
        for (channel, sources) in device_sources.iter().enumerate().take(channels) {
            for source in sources.iter().filter_map(|index| audio_outputs.get(*index)) {
                let samples = output.iter_mut().skip(channel).step_by(channels);
                for (sum, sample) in samples.zip(source) {
                    *sum += sample;
                }
            }
        }
        self.status.frames += frames as u64;
    }
}
