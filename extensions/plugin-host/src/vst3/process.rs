//! The audio side of a VST 3 plugin: one block in, one block out.
//!
//! VST 3 takes everything of a block in one `ProcessData`: the audio buses, an event list, the
//! parameter changes going in and the ones coming back. All of them are objects the host owns,
//! so all of them are made once, when the plugin loads, and only filled in here. Nothing in
//! this file allocates, locks or makes a system call.
//!
//! The list objects are only ever touched by the thread that calls `process`, which is why
//! they may hold plain cells and still be sent to the audio thread with the processor.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, Event, Event_::EventTypes_, Event__type0,
    IAudioProcessor, IAudioProcessorTrait, IEventList, IEventListTrait, IParamValueQueue,
    IParamValueQueueTrait, IParameterChanges, IParameterChangesTrait, NoteOffEvent, NoteOnEvent,
    ParamID, ParamValue, ProcessData, ProcessModes_, SymbolicSampleSizes_,
};
use vst3::Steinberg::{int32, kInvalidArgument, kResultFalse, kResultOk, kResultTrue, tresult};
use vst3::{Class, ComPtr, ComWrapper};

use crate::processor::{EVENT_CAPACITY, PluginEvent, Started, copy_in, copy_out, not_ours};

/// How many parameters one block may carry, in each direction. The pedal is the only one this
/// host sends; a plugin that reports more than this while it plays loses the rest until the
/// next block, which the composer hears as nothing at all.
const PARAMETER_CAPACITY: usize = 64;

/// How many points one parameter may have in one block.
const POINT_CAPACITY: usize = 32;

/// How many parameter changes wait for the control thread. A plugin that moves its own
/// parameters faster than the host polls loses the oldest, and the state is saved anyway when
/// the plugin goes.
pub const REPORT_CAPACITY: usize = 512;

/// How many edits of the composer's wait for the audio thread. What does not fit stays on the
/// host's thread, by parameter, and goes at the next poll, so no parameter ever ends on a value
/// the composer did not leave it on. See [`crate::vst3::context::Handler`].
pub const EDIT_CAPACITY: usize = 512;

/// One parameter a plugin changed by itself while it played. The control thread gives it to
/// the plugin's controller, which is how the two halves stay in step, and saves the state.
#[derive(Copy, Clone, Debug)]
pub struct ParameterChange {
    pub id: ParamID,
    pub value: ParamValue,
}

/// What a block says it is: a run on a device, or a render. It is the mode of the
/// `setupProcessing` the block belongs to, which is what VST 3 asks of a host.
pub fn process_mode(offline: bool) -> int32 {
    match offline {
        true => ProcessModes_::kOffline as int32,
        false => ProcessModes_::kRealtime as int32,
    }
}

/// A VST 3 plugin that is started, with everything one block needs.
pub struct Vst3Processor {
    processor: ComPtr<IAudioProcessor>,
    /// Keeps the plugin's control side from being terminated while this side is alive. The
    /// control side lets go when it is the last holder.
    _live: Arc<()>,
    events: ComWrapper<HostEventList>,
    events_pointer: *mut IEventList,
    input_changes: ComWrapper<HostParameterChanges>,
    input_changes_pointer: *mut IParameterChanges,
    output_changes: ComWrapper<HostParameterChanges>,
    output_changes_pointer: *mut IParameterChanges,
    input_buses: Buses,
    output_buses: Buses,
    /// The parameter the plugin maps the sustain pedal to, when it maps one. VST 3 has no MIDI
    /// controller event: `IMidiMapping` is the way the format intends, see `plugin.rs`.
    pedal_parameter: Option<ParamID>,
    /// What the plugin changed by itself, on its way to the control thread.
    reports: rtrb::Producer<ParameterChange>,
    /// What the composer changed in the plugin's own window, on its way here. The host's thread
    /// fills it at every poll; this side empties it at the start of every block.
    edits: rtrb::Consumer<ParameterChange>,
    /// Whether the plugin has been told to start processing. It is told here because this is
    /// the thread VST 3 wants that call on.
    processing: bool,
    /// What every block says it is. The same mode the plugin was set up with.
    mode: int32,
    /// What the plugin said its latency was when it was activated.
    latency: u32,
}

// SAFETY: everything a `Vst3Processor` holds is reached from one thread at a time. The engine
// moves a processor between the control thread and the audio thread and never runs it in two
// places at once, which is the contract of `sound_core::Processor`. The plugin's own objects
// are Send by the VST 3 API.
unsafe impl Send for Vst3Processor {}

impl Vst3Processor {
    pub fn new(
        processor: ComPtr<IAudioProcessor>,
        live: Arc<()>,
        input_channels: &[usize],
        output_channels: &[usize],
        pedal_parameter: Option<ParamID>,
        reports: rtrb::Producer<ParameterChange>,
        edits: rtrb::Consumer<ParameterChange>,
        mode: int32,
        latency: u32,
    ) -> Self {
        let events = ComWrapper::new(HostEventList::new());
        let input_changes = ComWrapper::new(HostParameterChanges::new());
        let output_changes = ComWrapper::new(HostParameterChanges::new());
        let pointer = |list: &ComWrapper<HostEventList>| match list.as_com_ref::<IEventList>() {
            Some(list) => list.as_ptr(),
            None => std::ptr::null_mut(),
        };
        let changes = |list: &ComWrapper<HostParameterChanges>| match list
            .as_com_ref::<IParameterChanges>()
        {
            Some(list) => list.as_ptr(),
            None => std::ptr::null_mut(),
        };
        Self {
            processor,
            _live: live,
            events_pointer: pointer(&events),
            events,
            input_changes_pointer: changes(&input_changes),
            input_changes,
            output_changes_pointer: changes(&output_changes),
            output_changes,
            input_buses: Buses::new(input_channels),
            output_buses: Buses::new(output_channels),
            pedal_parameter,
            reports,
            edits,
            processing: false,
            mode,
            latency,
        }
    }
}

impl Drop for Vst3Processor {
    fn drop(&mut self) {
        // Reached only when the engine itself is gone, which ends the audio thread before this
        // runs. Every other way out of the engine stops the plugin there first.
        self.stop();
    }
}

impl Started for Vst3Processor {
    fn takes_pedal(&self) -> bool {
        self.pedal_parameter.is_some()
    }

    fn latency(&self) -> u32 {
        self.latency
    }

    fn begin_block(&mut self) {
        self.events.clear();
        self.input_changes.clear();
        self.output_changes.clear();
        // What the composer changed in the plugin's own window since the last block. VST 3
        // carries an edit to the processor in the block's input parameter changes, and this is
        // the only place a block is built. At the start of the block, because the value was
        // already true before this block began. Popping a ring allocates nothing and locks
        // nothing, and the host's side keeps whatever did not fit.
        while let Ok(edit) = self.edits.pop() {
            if !self.input_changes.add(edit.id, 0, edit.value) {
                // Every queue of this block is taken, which is a plugin with more parameters
                // at once than this host keeps room for. The rest waits for the next block.
                break;
            }
        }
    }

    fn push(&mut self, offset: u32, event: PluginEvent) -> bool {
        match event {
            PluginEvent::On { key, velocity } => self.events.push(note_on(offset, key, velocity)),
            PluginEvent::Off { key } => self.events.push(note_off(offset, key)),
            // A VST 3 plugin has no MIDI controller event. The pedal goes as the parameter the
            // plugin's own MIDI mapping names, with its value as a number from 0 to 1.
            PluginEvent::Pedal(pedal) => match self.pedal_parameter {
                Some(id) => self.input_changes.add(
                    id,
                    offset as int32,
                    f64::from(pedal.value()) / f64::from(u8::MAX >> 1),
                ),
                None => true,
            },
        }
    }

    fn run(
        &mut self,
        frames: usize,
        input: [&[f32]; sound_core::CHANNELS],
        left: &mut [f32],
        right: &mut [f32],
    ) -> bool {
        if !self.processing {
            // VST 3 puts `setProcessing` on the thread that processes, which is this one.
            // SAFETY: the processor came from the plugin and is alive.
            let result = not_ours(|| unsafe { self.processor.setProcessing(1) });
            if result != kResultOk && result != kResultTrue {
                return false;
            }
            self.processing = true;
        }
        // The first bus is what this host plays into; the rest of them get silence, as every
        // audio input of a plugin did before effects existed.
        self.input_buses.clear(frames);
        copy_in(self.input_buses.first_mut(), frames, input);
        // A plugin may say its output is silent (`silenceFlags`) and leave the buffer as it
        // is, so what was in it must not be what a block before wrote.
        self.output_buses.clear(frames);
        let mut data = ProcessData {
            processMode: self.mode,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: frames as int32,
            numInputs: self.input_buses.count() as int32,
            numOutputs: self.output_buses.count() as int32,
            inputs: self.input_buses.as_mut_ptr(),
            outputs: self.output_buses.as_mut_ptr(),
            inputParameterChanges: self.input_changes_pointer,
            outputParameterChanges: self.output_changes_pointer,
            inputEvents: self.events_pointer,
            outputEvents: std::ptr::null_mut(),
            // No transport reaches a plugin yet. A plugin that syncs to the tempo runs free.
            processContext: std::ptr::null_mut(),
        };
        // SAFETY: every pointer in `data` belongs to this processor and outlives the call, and
        // the buffers are as long as `numSamples` says.
        let result = not_ours(|| unsafe { self.processor.process(&mut data) });
        if result != kResultOk && result != kResultTrue {
            return false;
        }
        self.output_changes.report_into(&mut self.reports);
        copy_out(self.output_buses.first(), frames, left, right);
        true
    }

    fn stop(&mut self) {
        if !self.processing {
            return;
        }
        self.processing = false;
        // SAFETY: the processor came from the plugin and is alive.
        not_ours(|| unsafe { self.processor.setProcessing(0) });
    }
}

fn note_on(offset: u32, key: u8, velocity: u8) -> Event {
    Event {
        busIndex: 0,
        sampleOffset: offset as int32,
        ppqPosition: 0.0,
        flags: 0,
        r#type: EventTypes_::kNoteOnEvent as u16,
        __field0: Event__type0 {
            noteOn: NoteOnEvent {
                channel: 0,
                pitch: i16::from(key),
                tuning: 0.0,
                velocity: f32::from(velocity) / 127.0,
                length: 0,
                // VST 3 lets a host number every note so that a plugin can tell two of one
                // pitch apart. The note contract has one note per key, so -1 says "by pitch".
                noteId: -1,
            },
        },
    }
}

fn note_off(offset: u32, key: u8) -> Event {
    Event {
        busIndex: 0,
        sampleOffset: offset as int32,
        ppqPosition: 0.0,
        flags: 0,
        r#type: EventTypes_::kNoteOffEvent as u16,
        __field0: Event__type0 {
            noteOff: NoteOffEvent {
                channel: 0,
                pitch: i16::from(key),
                // The note contract has no release velocity, so the usual half value goes out.
                velocity: 0.5,
                noteId: -1,
                tuning: 0.0,
            },
        },
    }
}

/// The audio buses of one direction: the buffers, the pointers into them the plugin reads, and
/// the `AudioBusBuffers` that names both. Made once, so a block moves no memory about.
struct Buses {
    /// One buffer per channel of every bus, in bus order. Each one is its own allocation, so
    /// the pointers below stay where they are.
    channels: Vec<Vec<f32>>,
    /// Per bus, the pointers to its channels. The `AudioBusBuffers` below point into these,
    /// so they are kept and never touched again.
    _pointers: Vec<Vec<*mut f32>>,
    buses: Vec<AudioBusBuffers>,
    /// The channels of the first bus, for [`Self::first`].
    first: usize,
}

impl Buses {
    fn new(channel_counts: &[usize]) -> Self {
        let total: usize = channel_counts.iter().sum();
        let mut channels: Vec<Vec<f32>> = (0..total)
            .map(|_| vec![0.0; sound_core::MAX_BLOCK])
            .collect();
        let mut pointers = Vec::with_capacity(channel_counts.len());
        let mut at = 0;
        for count in channel_counts {
            let mut bus = Vec::with_capacity(*count);
            for channel in &mut channels[at..at + count] {
                bus.push(channel.as_mut_ptr());
            }
            pointers.push(bus);
            at += count;
        }
        let buses = channel_counts
            .iter()
            .zip(&mut pointers)
            .map(|(count, bus)| AudioBusBuffers {
                numChannels: *count as int32,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: bus.as_mut_ptr(),
                },
            })
            .collect();
        Self {
            channels,
            _pointers: pointers,
            buses,
            first: channel_counts.first().copied().unwrap_or(0),
        }
    }

    fn count(&self) -> usize {
        self.buses.len()
    }

    fn as_mut_ptr(&mut self) -> *mut AudioBusBuffers {
        self.buses.as_mut_ptr()
    }

    /// Silence, before every block. An audio input of a plugin gets it because this host has
    /// no audio to give one; an output gets it so that a plugin that writes nothing is silent.
    fn clear(&mut self, frames: usize) {
        for channel in &mut self.channels {
            channel[..frames].fill(0.0);
        }
    }

    /// The channels of the first bus, which is the one this host plays.
    fn first(&self) -> &[Vec<f32>] {
        &self.channels[..self.first]
    }

    /// The same, to write into: the first input bus is what the host gives the plugin.
    fn first_mut(&mut self) -> &mut [Vec<f32>] {
        &mut self.channels[..self.first]
    }
}

/// The events of one block, as VST 3 takes them.
pub struct HostEventList {
    events: RefCell<Vec<Event>>,
}

// SAFETY: the list belongs to one `Vst3Processor` and is only ever touched from the thread
// that runs it, which the engine never runs in two places at once. The plugin is given a
// pointer to it for the length of one `process` call and no longer.
unsafe impl Send for HostEventList {}
// SAFETY: as above.
unsafe impl Sync for HostEventList {}

impl Class for HostEventList {
    type Interfaces = (IEventList,);
}

impl HostEventList {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::with_capacity(EVENT_CAPACITY)),
        }
    }

    fn clear(&self) {
        self.events.borrow_mut().clear();
    }

    /// `false` says the block is full, which the caller counts. Nothing grows here.
    fn push(&self, event: Event) -> bool {
        let mut events = self.events.borrow_mut();
        if events.len() == EVENT_CAPACITY {
            return false;
        }
        events.push(event);
        true
    }
}

impl IEventListTrait for HostEventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.events.borrow().len() as int32
    }

    unsafe fn getEvent(&self, index: int32, event: *mut Event) -> tresult {
        if event.is_null() || index < 0 {
            return kInvalidArgument;
        }
        let events = self.events.borrow();
        let Some(found) = events.get(index as usize) else {
            return kInvalidArgument;
        };
        // SAFETY: the caller gave a place to write one event.
        unsafe { *event = *found };
        kResultOk
    }

    unsafe fn addEvent(&self, _event: *mut Event) -> tresult {
        // Nothing reads what a plugin sends out: MIDI from a plugin is not built. Refusing
        // here is what keeps this buffer from growing on the audio thread.
        kResultFalse
    }
}

/// The parameter changes of one block, in either direction. Every queue is made once, so a
/// plugin that adds a parameter while it plays grows nothing.
pub struct HostParameterChanges {
    queues: Vec<ComWrapper<HostParameterQueue>>,
    used: Cell<usize>,
}

// SAFETY: as `HostEventList`.
unsafe impl Send for HostParameterChanges {}
// SAFETY: as `HostEventList`.
unsafe impl Sync for HostParameterChanges {}

impl Class for HostParameterChanges {
    type Interfaces = (IParameterChanges,);
}

impl HostParameterChanges {
    fn new() -> Self {
        Self {
            queues: (0..PARAMETER_CAPACITY)
                .map(|_| ComWrapper::new(HostParameterQueue::new()))
                .collect(),
            used: Cell::new(0),
        }
    }

    fn clear(&self) {
        for queue in &self.queues[..self.used.get()] {
            queue.clear();
        }
        self.used.set(0);
    }

    /// Adds one point for `id`, making a queue for it if this block has none. `false` says
    /// there was no room.
    fn add(&self, id: ParamID, offset: int32, value: ParamValue) -> bool {
        let Some(at) = self.queue_for(id) else {
            return false;
        };
        self.queues[at].add(offset, value)
    }

    /// Which queue this block uses for `id`, making one if it has none. `None` says every
    /// queue is taken, which is a plugin with more parameters in one block than this host
    /// keeps room for.
    fn queue_for(&self, id: ParamID) -> Option<usize> {
        let used = self.used.get();
        if let Some(at) = self.queues[..used].iter().position(|queue| queue.is(id)) {
            return Some(at);
        }
        let next = self.queues.get(used)?;
        next.begin(id);
        self.used.set(used + 1);
        Some(used)
    }

    /// Hands what the plugin changed to the control thread. What does not fit is dropped: the
    /// state is saved when the plugin goes in any case.
    fn report_into(&self, reports: &mut rtrb::Producer<ParameterChange>) {
        for queue in &self.queues[..self.used.get()] {
            let Some(value) = queue.last() else {
                continue;
            };
            let change = ParameterChange {
                id: queue.id.get(),
                value,
            };
            let _full = reports.push(change);
        }
    }
}

impl IParameterChangesTrait for HostParameterChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.used.get() as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if index < 0 || index as usize >= self.used.get() {
            return std::ptr::null_mut();
        }
        match self.queues[index as usize].as_com_ref::<IParamValueQueue>() {
            Some(queue) => queue.as_ptr(),
            None => std::ptr::null_mut(),
        }
    }

    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut int32,
    ) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller gives a parameter id.
        let Some(at) = self.queue_for(unsafe { *id }) else {
            return std::ptr::null_mut();
        };
        if !index.is_null() {
            // SAFETY: the caller gave a place for the index.
            unsafe { *index = at as int32 };
        }
        match self.queues[at].as_com_ref::<IParamValueQueue>() {
            Some(queue) => queue.as_ptr(),
            None => std::ptr::null_mut(),
        }
    }
}

/// The points of one parameter in one block.
pub struct HostParameterQueue {
    id: Cell<ParamID>,
    points: RefCell<Vec<(int32, ParamValue)>>,
}

// SAFETY: as `HostEventList`.
unsafe impl Send for HostParameterQueue {}
// SAFETY: as `HostEventList`.
unsafe impl Sync for HostParameterQueue {}

impl Class for HostParameterQueue {
    type Interfaces = (IParamValueQueue,);
}

impl HostParameterQueue {
    fn new() -> Self {
        Self {
            id: Cell::new(0),
            points: RefCell::new(Vec::with_capacity(POINT_CAPACITY)),
        }
    }

    fn is(&self, id: ParamID) -> bool {
        self.id.get() == id
    }

    fn begin(&self, id: ParamID) {
        self.id.set(id);
        self.points.borrow_mut().clear();
    }

    fn clear(&self) {
        self.points.borrow_mut().clear();
    }

    /// Adds one point. When the block has no room left, the newest value takes the place of the
    /// last point instead of being refused, so the value the block ends on is always the one
    /// the composer played. Refusing it would leave a pedal that came up in a block full of
    /// pedal moves holding for ever, `AllOff` and all: the plugin would never hear it go up.
    /// The points in between are what is lost, which is a pedal that moves in smaller steps
    /// than this block could carry.
    ///
    /// It never says no, so nothing above counts a pedal move as an event that was dropped: the
    /// value did reach the plugin, at a frame a little later than it was played.
    fn add(&self, offset: int32, value: ParamValue) -> bool {
        let mut points = self.points.borrow_mut();
        if points.len() == POINT_CAPACITY {
            if let Some(last) = points.last_mut() {
                *last = (offset, value);
            }
            return true;
        }
        points.push((offset, value));
        true
    }

    /// Where the parameter ended up in this block, which is what the controller is told.
    fn last(&self) -> Option<ParamValue> {
        self.points.borrow().last().map(|point| point.1)
    }
}

impl IParamValueQueueTrait for HostParameterQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }

    unsafe fn getPointCount(&self) -> int32 {
        self.points.borrow().len() as int32
    }

    unsafe fn getPoint(
        &self,
        index: int32,
        sample_offset: *mut int32,
        value: *mut ParamValue,
    ) -> tresult {
        if index < 0 || sample_offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        let points = self.points.borrow();
        let Some(point) = points.get(index as usize) else {
            return kInvalidArgument;
        };
        // SAFETY: the caller gave two places to write.
        unsafe {
            *sample_offset = point.0;
            *value = point.1;
        }
        kResultOk
    }

    unsafe fn addPoint(
        &self,
        sample_offset: int32,
        value: ParamValue,
        index: *mut int32,
    ) -> tresult {
        if !self.add(sample_offset, value) {
            return kResultFalse;
        }
        if !index.is_null() {
            // SAFETY: the caller gave a place to write the index.
            unsafe { *index = (self.points.borrow().len() - 1) as int32 };
        }
        kResultOk
    }
}

/// The value a sustain pedal of 0 to 127 becomes for a VST 3 parameter, which is 0 to 1. MIDI
/// controllers are seven bits, so the divisor is 127.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pedal_reaches_a_parameter_as_its_whole_range() {
        let of = |value: u8| f64::from(value) / f64::from(u8::MAX >> 1);
        assert_eq!(of(0), 0.0);
        assert_eq!(of(127), 1.0);
        assert!((of(64) - 0.503_937).abs() < 1e-6);
    }

    #[test]
    fn a_note_on_carries_the_key_and_the_velocity() {
        let event = note_on(17, 60, 127);
        assert_eq!(event.sampleOffset, 17);
        assert_eq!(event.r#type, EventTypes_::kNoteOnEvent as u16);
        // SAFETY: the event was just built as a note on.
        let note = unsafe { event.__field0.noteOn };
        assert_eq!(note.pitch, 60);
        assert_eq!(note.velocity, 1.0);
        assert_eq!(note.noteId, -1);
    }

    #[test]
    fn a_parameter_list_holds_one_queue_per_parameter_and_grows_no_further() {
        let changes = HostParameterChanges::new();
        assert!(changes.add(7, 0, 0.25));
        assert!(changes.add(7, 10, 0.75));
        assert!(changes.add(8, 0, 1.0));
        assert_eq!(changes.used.get(), 2);
        for point in 0..POINT_CAPACITY * 2 {
            assert!(changes.add(7, point as int32, 0.5), "at {point}");
        }
        assert_eq!(changes.queues[0].points.borrow().len(), POINT_CAPACITY);
        changes.clear();
        assert_eq!(changes.used.get(), 0);
    }

    /// The last value of a block is the one that was played last, whether or not the block had
    /// room for every move in it. A pedal that came up in a full block and was refused would
    /// hold for ever.
    #[test]
    fn the_last_value_of_a_full_block_is_the_newest_one_and_not_the_one_before_it() {
        let changes = HostParameterChanges::new();
        for point in 0..POINT_CAPACITY {
            assert!(changes.add(1, point as int32, 0.5));
        }
        assert_eq!(changes.queues[0].last(), Some(0.5));
        // The block is full and the pedal comes up. It is the value the plugin must end on.
        assert!(changes.add(1, POINT_CAPACITY as int32, 0.0));
        assert_eq!(changes.queues[0].last(), Some(0.0));
        assert_eq!(changes.queues[0].points.borrow().len(), POINT_CAPACITY);
        // And at the frame it was played, not the frame of the point it took the place of.
        let points = changes.queues[0].points.borrow();
        assert_eq!(points.last().copied(), Some((POINT_CAPACITY as int32, 0.0)));
    }
}
