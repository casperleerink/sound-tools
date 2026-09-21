//! A VST 3 instrument for the tests of the plugin host. It is not part of the product.
//!
//! It is the same instrument as `tooling/test-clap-plugin`, in the other format: the sound, the
//! state and the environment variables are all in `test-plugin-support`, so a test reads either
//! render the same way. What is here is the format.
//!
//! It is one object: a component that is also its own edit controller, which VST 3 allows and
//! which is what a small plugin does. Two parameters, so that a host has something to send and
//! something to be told about:
//!
//! - `Transpose`, which the saved state holds. The plugin changes it by itself when the pedal
//!   goes down, and reports it in the block's output parameter changes, which is how a VST 3
//!   plugin tells a host that its state changed. There is no `mark_dirty` in this format.
//! - `Sustain`, which `IMidiMapping` maps MIDI controller 64 to. That is how the format says a
//!   host sends the sustain pedal, and it is what the host under test uses.
//!
//! It has a window, `TestView`, which draws nothing: CI has no display. It answers the calls a
//! host makes of an `IPlugView` and writes each one down with the thread it arrived on, which is
//! what a real plugin would assert, and it can ask its host to resize it.

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use test_plugin_support as support;
use vst3::Steinberg::Vst::{
    BusDirections_, BusInfo, BusInfo_::BusFlags_, BusTypes_, ControllerNumbers_,
    Event_::EventTypes_, IAudioProcessor, IAudioProcessorTrait, IComponent, IComponentHandler,
    IComponentHandlerTrait, IComponentTrait, IEditController, IEditControllerTrait,
    IEventListTrait, IMidiMapping, IMidiMappingTrait, IParamValueQueueTrait,
    IParameterChangesTrait, MediaTypes_, ParamID, ParamValue, ParameterInfo,
    ParameterInfo_::ParameterFlags_, ProcessData, ProcessModes_, ProcessSetup, RoutingInfo,
    SpeakerArr, SpeakerArrangement, String128, SymbolicSampleSizes_, TChar,
};
use vst3::Steinberg::{
    FIDString, FUnknown, IBStream, IBStream_::IStreamSeekMode_, IBStreamTrait, IPlugFrame,
    IPlugFrameTrait, IPlugView, IPlugViewTrait, IPluginBase, IPluginBaseTrait, IPluginFactory,
    IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo_::ClassCardinality_, PClassInfo2, PFactoryInfo, TBool, TUID, ViewRect, int32,
    kInternalError, kInvalidArgument, kNotImplemented, kPlatformTypeNSView, kResultFalse,
    kResultOk, kResultTrue, tresult, uint32,
};
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface, uid};

/// The class id a project record names. Thirty-two hex digits of these sixteen bytes:
/// `534F554E44544F4F4C53544553545430`, which is `SOUNDTOOLSTESTT0` in ASCII.
pub const CLASS_ID: TUID = uid(0x534F554E, 0x44544F4F, 0x4C535445, 0x53545430);

/// The same id as a record holds it. A test writes this into a `plugin_id`.
pub const PLUGIN_ID: &str = "534F554E44544F4F4C53544553545430";

pub const PLUGIN_NAME: &str = "Sound Tools Test Tone";

/// The parameter the state holds, and the one the sustain pedal is mapped to.
const TRANSPOSE: ParamID = 0;
const SUSTAIN: ParamID = 1;

/// How loud the plugin plays, read by the processor out of the block's input parameter changes
/// and nowhere else. It is what the plugin's controller edits through the host, so a host that
/// does not carry an edit from the controller to the processor plays this plugin at full level
/// whatever the composer does in its window. Saved in the component's state.
const LEVEL: ParamID = 2;

/// How many semitones the transpose parameter covers, so that a normalized value is exact.
const TRANSPOSE_RANGE: f64 = 63.0;

/// How many pedal points one block may carry. Fixed, so `process` never allocates.
const PEDAL_POINTS: usize = 32;

/// The level the plugin plays at until its controller state says another, and the one it drops
/// to when it is keeping a controller state and the pedal transposes it. Hundredths.
const FULL_LEVEL: i32 = 100;
const HALF_LEVEL: i32 = 50;

/// The plugin. One object for both halves, which VST 3 allows.
pub struct TestTone {
    audio: RefCell<Audio>,
    /// What the host gave `setComponentHandler`, for telling it about an edit. This plugin's
    /// window has nothing to move in it, so it only keeps it.
    handler: RefCell<Option<ComPtr<IComponentHandler>>>,
    /// The transpose, read by both halves.
    semitones: AtomicI32,
    /// How loud the plugin plays, in hundredths. It is the edit controller's own state, which
    /// is a second state a host must save next to the component's, and this plugin is one
    /// object for both halves so nothing but asking the controller interface finds it.
    level: AtomicI32,
    /// Whether the host has answered this plugin on the main thread. Until it has, a plugin
    /// that was told to wait for one is silent, as a sampler waiting for its samples is.
    answered: AtomicBool,
    /// How loud the plugin plays, in hundredths, as the last `LEVEL` point of a block set it.
    /// The processor writes it and the component's state saves it.
    edit_level: AtomicI32,
    /// Which plugin of this library this is, for the log.
    plugin: u64,
}

/// What only the thread that processes touches.
struct Audio {
    tone: support::Tone,
    processed: u64,
    /// How many parameter changes to send out of every process call.
    reports_out: u32,
    /// Whether to say the output is silent and write nothing into it, from the second block on.
    goes_silent: bool,
    /// Whether this plugin keeps a state in its edit controller as well.
    controller_state: bool,
}

// SAFETY: the component is reached from the main thread and the audio thread, never at once:
// VST 3 puts `process` and `setProcessing` on one thread and everything else on the main
// thread, and a host may not run them together. The transpose, which both read, is an atomic.
unsafe impl Send for TestTone {}
// SAFETY: as above.
unsafe impl Sync for TestTone {}

impl Class for TestTone {
    type Interfaces = (IComponent, IAudioProcessor, IEditController, IMidiMapping);
}

impl TestTone {
    fn new() -> Self {
        Self {
            audio: RefCell::new(Audio {
                tone: support::Tone::new(48_000.0),
                processed: 0,
                reports_out: support::events_out(),
                goes_silent: support::told_to(support::SILENT_VARIABLE),
                controller_state: support::told_to(support::CONTROLLER_STATE_VARIABLE),
            }),
            handler: RefCell::new(None),
            semitones: AtomicI32::new(0),
            level: AtomicI32::new(FULL_LEVEL),
            answered: AtomicBool::new(!support::told_to(support::NEEDS_HOST_VARIABLE)),
            edit_level: AtomicI32::new(support::FULL_EDIT_LEVEL),
            plugin: support::next_plugin(),
        }
    }

    fn log(&self, call: &str) {
        let processed = self.audio.try_borrow().map_or(0, |audio| audio.processed);
        support::log(call, self.plugin, processed);
    }

    /// A knob drag, as the format has it: one `beginEdit`, `count` values on the way down, one
    /// `endEdit`. The last value is `1 / count`, so a host that keeps only the first, or an
    /// average, or none of them is heard.
    fn edit_the_level(&self, handler: &ComPtr<IComponentHandler>, count: u32) {
        support::log("edit_begin", 0, 0);
        // SAFETY: the handler came from the host and is alive; the host keeps it until it
        // takes it back with a null `setComponentHandler`.
        unsafe {
            handler.beginEdit(LEVEL);
            for step in 0..count {
                let value = f64::from(count - step) / f64::from(count);
                support::log("edit_value", 0, 0);
                handler.performEdit(LEVEL, value);
            }
            handler.endEdit(LEVEL);
        }
        support::log("edit_end", 0, 0);
    }
}

/// Every parameter, in the order `getParameterInfo` lists them.
const PARAMETERS: [(ParamID, &str, &str, ParamValue); 3] = [
    (TRANSPOSE, "Transpose", "st", 0.0),
    (SUSTAIN, "Sustain", "", 0.0),
    (LEVEL, "Level", "", 1.0),
];

impl IPluginBaseTrait for TestTone {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        self.log("initialize");
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        self.log("terminate");
        *self.handler.borrow_mut() = None;
        kResultOk
    }
}

impl IComponentTrait for TestTone {
    /// One object for both halves, so there is no second class to name.
    unsafe fn getControllerClassId(&self, _class_id: *mut TUID) -> tresult {
        kNotImplemented
    }

    unsafe fn setIoMode(&self, _mode: int32) -> tresult {
        kNotImplemented
    }

    unsafe fn getBusCount(&self, media: int32, direction: int32) -> int32 {
        let audio_out =
            media == MediaTypes_::kAudio as int32 && direction == BusDirections_::kOutput as int32;
        let event_in =
            media == MediaTypes_::kEvent as int32 && direction == BusDirections_::kInput as int32;
        int32::from(audio_out || event_in)
    }

    unsafe fn getBusInfo(
        &self,
        media: int32,
        direction: int32,
        index: int32,
        bus: *mut BusInfo,
    ) -> tresult {
        // SAFETY: the caller gave a place to write one bus.
        unsafe {
            if bus.is_null() || index != 0 || self.getBusCount(media, direction) == 0 {
                return kInvalidArgument;
            }
            let bus = &mut *bus;
            bus.mediaType = media;
            bus.direction = direction;
            bus.channelCount = if media == MediaTypes_::kAudio as int32 {
                2
            } else {
                1
            };
            bus.busType = BusTypes_::kMain as int32;
            bus.flags = BusFlags_::kDefaultActive as uint32;
            write_utf16(
                if media == MediaTypes_::kAudio as int32 {
                    "Output"
                } else {
                    "Notes"
                },
                &mut bus.name,
            );
        }
        kResultOk
    }

    unsafe fn getRoutingInfo(
        &self,
        _in_info: *mut RoutingInfo,
        _out_info: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        _media: int32,
        _direction: int32,
        _index: int32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, state: TBool) -> tresult {
        self.log(if state == 0 { "deactivate" } else { "activate" });
        if state == 0 {
            if let Ok(mut audio) = self.audio.try_borrow_mut() {
                audio.tone.reset();
            }
        }
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the caller gives a stream that lives for this call.
        let Some(bytes) = (unsafe { read_stream(state) }) else {
            return kInvalidArgument;
        };
        let Some((semitones, edit_level)) = support::load_state(&bytes) else {
            return kResultFalse;
        };
        self.semitones.store(semitones, Ordering::Release);
        self.edit_level.store(edit_level, Ordering::Release);
        if let Ok(mut audio) = self.audio.try_borrow_mut() {
            audio.tone.set_semitones(semitones);
        }
        kResultOk
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        let bytes = support::save_state(
            self.semitones.load(Ordering::Acquire),
            self.edit_level.load(Ordering::Acquire),
        );
        // A plugin that writes its payload first and fills the header in afterwards, which is
        // what a plugin with a chunk length in its header does. The first four bytes are the
        // header here.
        if support::told_to(support::HEADER_LAST_VARIABLE) {
            // SAFETY: the caller gives a stream that lives for this call.
            let written = unsafe {
                seek_stream(state, 4)
                    && write_stream(state, &bytes[4..])
                    && seek_stream(state, 0)
                    && write_stream(state, &bytes[..4])
            };
            return match written {
                true => kResultOk,
                false => kInvalidArgument,
            };
        }
        // SAFETY: the caller gives a stream that lives for this call.
        match unsafe { write_stream(state, &bytes) } {
            true => kResultOk,
            false => kInvalidArgument,
        }
    }
}

impl IAudioProcessorTrait for TestTone {
    unsafe fn setBusArrangements(
        &self,
        _inputs: *mut SpeakerArrangement,
        num_ins: int32,
        outputs: *mut SpeakerArrangement,
        num_outs: int32,
    ) -> tresult {
        // Stereo out and nothing in is the only thing this plugin does.
        // SAFETY: the caller says `outputs` holds `num_outs` arrangements.
        let stereo =
            num_outs == 1 && !outputs.is_null() && unsafe { *outputs } == SpeakerArr::kStereo;
        match num_ins == 0 && stereo {
            true => kResultTrue,
            false => kResultFalse,
        }
    }

    unsafe fn getBusArrangement(
        &self,
        direction: int32,
        index: int32,
        arrangement: *mut SpeakerArrangement,
    ) -> tresult {
        if arrangement.is_null() || index != 0 || direction != BusDirections_::kOutput as int32 {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave a place to write one arrangement.
        unsafe { *arrangement = SpeakerArr::kStereo };
        kResultOk
    }

    unsafe fn canProcessSampleSize(&self, size: int32) -> tresult {
        match size == SymbolicSampleSizes_::kSample32 as int32 {
            true => kResultOk,
            false => kResultFalse,
        }
    }

    unsafe fn getLatencySamples(&self) -> uint32 {
        0
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if setup.is_null() {
            return kInvalidArgument;
        }
        self.log("setupProcessing");
        // SAFETY: the caller gave one setup.
        let mode = unsafe { (*setup).processMode };
        self.log(&format!("mode[{}]", mode_name(mode)));
        // SAFETY: as above.
        let sample_rate = unsafe { (*setup).sampleRate };
        if let Ok(mut audio) = self.audio.try_borrow_mut() {
            let semitones = audio.tone.semitones();
            audio.tone = support::Tone::new(sample_rate as f32);
            audio.tone.set_semitones(semitones);
        }
        kResultOk
    }

    unsafe fn setProcessing(&self, state: TBool) -> tresult {
        self.log(if state == 0 {
            "stop_processing"
        } else {
            "start_processing"
        });
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        if data.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host gives one `ProcessData` whose buffers are as long as `numSamples`
        // and whose lists live for this call, which is what VST 3 promises.
        unsafe {
            let data = &mut *data;
            let Ok(mut audio) = self.audio.try_borrow_mut() else {
                return kResultFalse;
            };
            // The first one names the thread that processes, which the rest of a log is read
            // against. A block says what kind of run it belongs to, which must be the mode the
            // plugin was set up with.
            if audio.processed == 0 {
                support::log("process", self.plugin, 0);
                support::log(
                    &format!("process_mode[{}]", mode_name(data.processMode)),
                    self.plugin,
                    0,
                );
            }
            let frames = data.numSamples.max(0) as usize;
            if data.numOutputs < 1 || data.outputs.is_null() {
                return kResultOk;
            }
            let bus = &mut *data.outputs;
            if bus.numChannels < 2 || bus.__field0.channelBuffers32.is_null() {
                return kResultOk;
            }
            // A plugin that is waiting for its host. It reports a parameter it changed by
            // itself, which is what a VST 3 plugin has instead of CLAP's callback request, and
            // makes no sound until the host gives it back through `setParamNormalized`.
            if !self.answered.load(Ordering::Acquire) {
                audio.processed += 1;
                if let Some(changes) = ComRef::from_raw(data.outputParameterChanges) {
                    let mut index = 0;
                    let id = TRANSPOSE;
                    if let Some(queue) = ComRef::from_raw(changes.addParameterData(&id, &mut index))
                    {
                        queue.addPoint(0, 0.0, &mut index);
                    }
                }
                return kResultOk;
            }
            // What VST 3 lets a plugin do instead of writing zeros. The host must not play what
            // was in its buffer before.
            if audio.goes_silent && audio.processed > 0 {
                bus.silenceFlags = 0b11;
                audio.processed += 1;
                return kResultOk;
            }
            bus.silenceFlags = 0;
            let left = std::slice::from_raw_parts_mut(*bus.__field0.channelBuffers32, frames);
            let right =
                std::slice::from_raw_parts_mut(*bus.__field0.channelBuffers32.add(1), frames);

            // The pedal comes as points of the parameter the MIDI mapping names, and the
            // notes as events. Both lists are in time order, so they are walked together and
            // each one sounds from exactly the frame it carries, as in the CLAP plugin.
            let mut pedal = [(0_i32, 0_u8); PEDAL_POINTS];
            let mut pedal_count = 0;
            if let Some(changes) = ComRef::from_raw(data.inputParameterChanges) {
                for index in 0..changes.getParameterCount() {
                    let Some(queue) = ComRef::from_raw(changes.getParameterData(index)) else {
                        continue;
                    };
                    let id = queue.getParameterId();
                    // How loud to play, which is what the composer edited in the plugin's own
                    // window. The last point of the block is the value the knob was left on.
                    if id == LEVEL {
                        let mut last = None;
                        for point in 0..queue.getPointCount() {
                            let (mut offset, mut value) = (0, 0.0);
                            if queue.getPoint(point, &mut offset, &mut value) == kResultOk {
                                last = Some(value);
                            }
                        }
                        if let Some(value) = last {
                            let level = (value * f64::from(support::FULL_EDIT_LEVEL)).round();
                            self.edit_level.store(level as i32, Ordering::Release);
                        }
                        continue;
                    }
                    if id != SUSTAIN {
                        continue;
                    }
                    for point in 0..queue.getPointCount() {
                        let (mut offset, mut value) = (0, 0.0);
                        if queue.getPoint(point, &mut offset, &mut value) != kResultOk {
                            continue;
                        }
                        if pedal_count == PEDAL_POINTS {
                            break;
                        }
                        pedal[pedal_count] = (offset, (value * 127.0).round() as u8);
                        pedal_count += 1;
                    }
                }
            }

            let events = ComRef::from_raw(data.inputEvents);
            let event_count = events.map_or(0, |events| events.getEventCount());
            let (mut next_event, mut next_pedal, mut played) = (0, 0, 0);
            let mut transposed = false;
            loop {
                let event_at = (next_event < event_count)
                    .then(|| {
                        let mut event = std::mem::zeroed();
                        let events = events?;
                        (events.getEvent(next_event, &mut event) == kResultOk)
                            .then_some((event.sampleOffset.max(0), event))
                    })
                    .flatten();
                let pedal_at = (next_pedal < pedal_count).then(|| pedal[next_pedal]);
                let at = match (event_at, pedal_at) {
                    (None, None) => break,
                    (Some((offset, _)), None) => offset,
                    (None, Some((offset, _))) => offset,
                    (Some((event, _)), Some((point, _))) => event.min(point),
                };
                let at = (at as usize).min(frames);
                // Everything up to here, with the voices and the pedal as they were.
                audio
                    .tone
                    .render(&mut left[played..at], &mut right[played..at]);
                played = at;
                if pedal_at.is_some_and(|(offset, _)| offset as usize <= at) {
                    transposed |= audio.tone.pedal(pedal[next_pedal].1);
                    next_pedal += 1;
                    continue;
                }
                let Some((_, event)) = event_at else {
                    break;
                };
                next_event += 1;
                if event.r#type == EventTypes_::kNoteOnEvent as u16 {
                    let note = event.__field0.noteOn;
                    audio
                        .tone
                        .note_on(note.pitch.clamp(0, 127) as u8, note.velocity);
                } else if event.r#type == EventTypes_::kNoteOffEvent as u16 {
                    let note = event.__field0.noteOff;
                    audio.tone.note_off(Some(note.pitch.clamp(0, 127) as u8));
                }
            }
            if transposed {
                self.semitones
                    .store(audio.tone.semitones(), Ordering::Release);
            }
            audio
                .tone
                .render(&mut left[played..frames], &mut right[played..frames]);
            // The controller's own state, if this plugin keeps one: how loud it plays. A host
            // that lost that part of the state plays this plugin at its full level.
            if audio.controller_state {
                if transposed {
                    self.level.store(HALF_LEVEL, Ordering::Release);
                }
                let level = self.level.load(Ordering::Acquire) as f32 / FULL_LEVEL as f32;
                for sample in left.iter_mut() {
                    *sample *= level;
                }
            }
            // And how loud the last parameter edit of the composer's left it. A host that
            // never carried the edit to this half plays the whole block at the full level.
            let edited =
                self.edit_level.load(Ordering::Acquire) as f32 / support::FULL_EDIT_LEVEL as f32;
            if edited != 1.0 {
                for sample in left.iter_mut() {
                    *sample *= edited;
                }
            }
            audio.processed += 1;

            // What a host is told about: the transpose the plugin changed by itself, and as
            // many more as a test asked for. Whatever the host does with them, it must not
            // grow a buffer on this thread.
            let reports = audio.reports_out;
            if let Some(changes) = ComRef::from_raw(data.outputParameterChanges) {
                let report = |id: ParamID, value: ParamValue| {
                    let mut index = 0;
                    if let Some(queue) = ComRef::from_raw(changes.addParameterData(&id, &mut index))
                    {
                        queue.addPoint(0, value, &mut index);
                    }
                };
                if transposed {
                    let semitones = f64::from(audio.tone.semitones()) / TRANSPOSE_RANGE;
                    report(TRANSPOSE, semitones.clamp(0.0, 1.0));
                }
                for extra in 0..reports {
                    report(TRANSPOSE + 2 + extra, 0.5);
                }
            }
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> uint32 {
        0
    }
}

impl IEditControllerTrait for TestTone {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        // The controller side of one object: the component's state is already where it belongs.
        // SAFETY: the caller gives a stream that lives for this call.
        unsafe { IComponentTrait::setState(self, state) }
    }

    /// The controller's own state, which is the second one a VST 3 host saves. Without the
    /// switch this plugin keeps everything in the component's state and says so.
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        if !support::told_to(support::CONTROLLER_STATE_VARIABLE) {
            return kResultOk;
        }
        // SAFETY: the caller gives a stream that lives for this call.
        let Some(bytes) = (unsafe { read_stream(state) }) else {
            return kInvalidArgument;
        };
        let Some(level) = support::load_controller_state(&bytes) else {
            return kResultFalse;
        };
        self.level.store(level, Ordering::Release);
        kResultOk
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        // A controller that cannot give its state. A host must keep the file it has.
        if support::told_to(support::CONTROLLER_FAILS_VARIABLE) {
            return kInternalError;
        }
        if !support::told_to(support::CONTROLLER_STATE_VARIABLE) {
            // Nothing of its own, which is what a one-object plugin usually says.
            return kNotImplemented;
        }
        let bytes = support::save_controller_state(self.level.load(Ordering::Acquire));
        // SAFETY: the caller gives a stream that lives for this call.
        match unsafe { write_stream(state, &bytes) } {
            true => kResultOk,
            false => kInvalidArgument,
        }
    }

    unsafe fn getParameterCount(&self) -> int32 {
        3
    }

    unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
        let Some((id, title, units, default)) = PARAMETERS.get(index.max(0) as usize).copied()
        else {
            return kInvalidArgument;
        };
        if info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave a place to write one parameter.
        unsafe {
            let info = &mut *info;
            *info = std::mem::zeroed();
            info.id = id;
            write_utf16(title, &mut info.title);
            write_utf16(units, &mut info.units);
            info.stepCount = 0;
            info.defaultNormalizedValue = default;
            info.unitId = 0;
            info.flags = ParameterFlags_::kCanAutomate as int32;
        }
        kResultOk
    }

    unsafe fn getParamStringByValue(
        &self,
        _id: ParamID,
        _value: ParamValue,
        _string: *mut String128,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn getParamValueByString(
        &self,
        _id: ParamID,
        _string: *mut TChar,
        _value: *mut ParamValue,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn normalizedParamToPlain(&self, id: ParamID, value: ParamValue) -> ParamValue {
        match id {
            TRANSPOSE => (value * TRANSPOSE_RANGE).round(),
            _ => value * 127.0,
        }
    }

    unsafe fn plainParamToNormalized(&self, id: ParamID, plain: ParamValue) -> ParamValue {
        match id {
            TRANSPOSE => plain / TRANSPOSE_RANGE,
            _ => plain / 127.0,
        }
    }

    unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
        match id {
            TRANSPOSE => f64::from(self.semitones.load(Ordering::Acquire)) / TRANSPOSE_RANGE,
            LEVEL => {
                f64::from(self.edit_level.load(Ordering::Acquire))
                    / f64::from(support::FULL_EDIT_LEVEL)
            }
            _ => 0.0,
        }
    }

    unsafe fn setParamNormalized(&self, id: ParamID, value: ParamValue) -> tresult {
        // The host giving back what the plugin reported. It is already where it belongs, and
        // it is also the main-thread work a plugin that waits for its host waits for.
        if id == TRANSPOSE {
            support::log("setParamNormalized", self.plugin, 0);
            self.answered.store(true, Ordering::Release);
            let _ = value;
        }
        kResultOk
    }

    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        // SAFETY: the host gives a handler that is alive for this call, and `to_com_ptr`
        // counts the reference this plugin keeps.
        let kept = unsafe { ComRef::from_raw(handler) }.map(|handler| handler.to_com_ptr());
        let given = kept.clone();
        *self.handler.borrow_mut() = kept;
        // What a plugin's own window does when the composer turns a knob: a burst of edits of
        // one parameter through the host, ending on the value the knob was left on. The
        // processor is the half that reads it, so this is what a host has to carry across.
        if let (Some(handler), Some(count)) = (given, support::wanted_edits()) {
            self.edit_the_level(&handler, count);
        }
        kResultOk
    }

    /// The plugin's own window. A host asks for the editor view and puts it in a window of its
    /// own; see `TestView`.
    unsafe fn createView(&self, name: *const c_char) -> *mut IPlugView {
        // A plugin with no window of its own at all, which a host has to say instead of
        // offering one.
        if support::told_to(support::NO_WINDOW_VARIABLE) || name.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: the host gives a C string that lives for this call.
        if unsafe { CStr::from_ptr(name) }.to_bytes() != b"editor" {
            return std::ptr::null_mut();
        }
        support::log("gui_create", 0, 0);
        let view = ComWrapper::new(TestView::default());
        // The view keeps a pointer to itself, without a reference, so that it can name itself
        // in `IPlugFrame::resizeView`. It is only used while the view is alive.
        if let Some(pointer) = view.as_com_ref::<IPlugView>() {
            view.remember_itself(pointer.as_ptr());
        }
        match view.to_com_ptr::<IPlugView>() {
            Some(pointer) => pointer.into_raw(),
            None => std::ptr::null_mut(),
        }
    }
}

/// The plugin's window, in name only: no real view is made, because CI has no display. Every
/// call is written to the log with the thread it came in on, so a test reads exactly what a
/// host did and in what order. What only a real window can show is checked by hand.
struct TestView {
    /// What the host gave `setFrame`, which is what a plugin asks for a resize through.
    frame: RefCell<Option<ComPtr<IPlugFrame>>>,
    /// This object as an `IPlugView`, for naming itself to the frame. No reference is counted:
    /// it is only read while the object is alive, and counting one would keep it alive for ever.
    itself: Cell<*mut IPlugView>,
    /// Whether `attached` was answered, so a test can see that `removed` follows exactly one.
    attached: Cell<bool>,
    /// How big this view is. A real one resizes its own drawing here and nowhere else, which
    /// is what `iplugview.h` asks: "Please only resize the platform representation of the view
    /// when IPlugView::onSize () is called."
    size: Cell<(int32, int32)>,
}

impl Default for TestView {
    fn default() -> Self {
        Self {
            frame: RefCell::new(None),
            itself: Cell::new(std::ptr::null_mut()),
            attached: Cell::new(false),
            size: Cell::new((
                support::WINDOW_WIDTH as int32,
                support::WINDOW_HEIGHT as int32,
            )),
        }
    }
}

impl Class for TestView {
    type Interfaces = (IPlugView,);
}

impl TestView {
    fn remember_itself(&self, pointer: *mut IPlugView) {
        self.itself.set(pointer);
    }

    /// Asks the host for another window size, as a plugin that sizes itself as it opens does.
    /// The frame is in place before `attached`, which is the earliest the format allows.
    fn ask_for_a_resize(&self, width: u32, height: u32) {
        let frame = self.frame.borrow().clone();
        let (Some(frame), false) = (frame, self.itself.get().is_null()) else {
            return;
        };
        support::log("gui_request_resize", 0, 0);
        let mut wanted = ViewRect {
            left: 0,
            top: 0,
            right: width as int32,
            bottom: height as int32,
        };
        // SAFETY: the frame came from the host and is alive, `itself` points at this object,
        // and the rectangle outlives the call. Nothing of this object is borrowed: the host
        // answers `onSize` from inside this call, which is what VST 3 asks of it.
        unsafe { frame.resizeView(self.itself.get(), &mut wanted) };
    }
}

impl Drop for TestView {
    /// The host letting go of the view, which is the last thing a plugin holds for a window.
    fn drop(&mut self) {
        support::log("gui_destroy", 0, 0);
    }
}

impl IPlugViewTrait for TestView {
    unsafe fn isPlatformTypeSupported(&self, r#type: FIDString) -> tresult {
        support::log("gui_is_api_supported", 0, 0);
        if r#type.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host gives a C string that lives for this call.
        let wanted = unsafe { CStr::from_ptr(r#type) };
        // SAFETY: the constant is a static C string.
        let cocoa = unsafe { CStr::from_ptr(kPlatformTypeNSView) };
        match wanted == cocoa {
            true => kResultTrue,
            false => kResultFalse,
        }
    }

    unsafe fn attached(&self, parent: *mut c_void, _type: FIDString) -> tresult {
        support::log("gui_set_parent", 0, 0);
        if parent.is_null() {
            return kInvalidArgument;
        }
        // A plugin that cannot put its view in the parent it was given. A host must leave it
        // alone afterwards, and `removed` is the other half of an `attached` that worked.
        if support::told_to(support::ATTACH_FAILS_VARIABLE) {
            return kInternalError;
        }
        self.attached.set(true);
        // `iplugview.h` on `attached`: "Note that in this call the plug-in could call a
        // IPlugFrame::resizeView ()". This is a plugin that does.
        if let Some((width, height)) = support::wanted_size_in_attached() {
            self.ask_for_a_resize(width, height);
            self.size.set((width as int32, height as int32));
        }
        kResultOk
    }

    unsafe fn removed(&self) -> tresult {
        support::log("gui_removed", 0, 0);
        // A host may only remove a view it attached. A test reads this line to see that it did.
        if !self.attached.replace(false) {
            return kInternalError;
        }
        kResultOk
    }

    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kNotImplemented
    }

    unsafe fn onKeyDown(&self, _key: u16, _code: i16, _modifiers: i16) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyUp(&self, _key: u16, _code: i16, _modifiers: i16) -> tresult {
        kResultFalse
    }

    /// The size the plugin wants to start at. It does not change when the plugin asks for
    /// another one, so a test can see that the host took the size from the request and not
    /// from here.
    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        let (width, height) = self.size.get();
        // SAFETY: the host gave a place to write one rectangle.
        unsafe {
            *size = ViewRect {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
        }
        kResultOk
    }

    unsafe fn onSize(&self, new_size: *mut ViewRect) -> tresult {
        support::log("gui_on_size", 0, 0);
        if new_size.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host gives one rectangle that lives for this call.
        let rect = unsafe { *new_size };
        let told = (rect.right - rect.left, rect.bottom - rect.top);
        let Some((width, height)) = support::wanted_size_in_on_size() else {
            self.size.set(told);
            return kResultOk;
        };
        // A plugin that changes its mind inside the host's answer, every time it is asked and
        // before it has taken any size. A host that lets this call in again is in it for ever:
        // the view it asks about is still the old size, so the host tells it the new one, and
        // the view asks again. The size is taken only once the request is over, which is what
        // makes the window end on it.
        self.ask_for_a_resize(width, height);
        self.size.set((width as int32, height as int32));
        kResultOk
    }

    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        // SAFETY: the host gives a frame that is alive for this call, and `to_com_ptr` counts
        // the reference this view keeps.
        let kept = unsafe { ComRef::from_raw(frame) }.map(|frame| frame.to_com_ptr());
        let had_one = kept.is_some();
        support::log(
            if had_one {
                "gui_set_frame"
            } else {
                "gui_clear_frame"
            },
            0,
            0,
        );
        *self.frame.borrow_mut() = kept;
        // A plugin that sizes itself asks as soon as it has somewhere to ask.
        if had_one && let Some((width, height)) = support::wanted_window_size() {
            self.ask_for_a_resize(width, height);
        }
        kResultOk
    }

    /// This window is not resizable by dragging, which is what the host expects of it.
    unsafe fn canResize(&self) -> tresult {
        kResultFalse
    }

    unsafe fn checkSizeConstraint(&self, _rect: *mut ViewRect) -> tresult {
        kResultFalse
    }
}

/// How a VST 3 plugin says where a MIDI controller goes: to a parameter. Controller 64, the
/// sustain pedal, is the only one this plugin takes.
impl IMidiMappingTrait for TestTone {
    unsafe fn getMidiControllerAssignment(
        &self,
        bus: int32,
        _channel: i16,
        controller: i16,
        id: *mut ParamID,
    ) -> tresult {
        if bus != 0 || controller != ControllerNumbers_::kCtrlSustainOnOff as i16 || id.is_null() {
            return kResultFalse;
        }
        // SAFETY: the caller gave a place to write one parameter id.
        unsafe { *id = SUSTAIN };
        kResultOk
    }
}

/// The factory, which is what `GetPluginFactory` gives a host.
struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory, IPluginFactory2);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave a place to write.
        unsafe {
            let info = &mut *info;
            *info = std::mem::zeroed();
            write_ascii("Sound Tools", &mut info.vendor);
            write_ascii(
                "https://github.com/casperleerink/sound-tools",
                &mut info.url,
            );
            write_ascii("noreply@example.com", &mut info.email);
        }
        kResultOk
    }

    unsafe fn countClasses(&self) -> int32 {
        1
    }

    unsafe fn getClassInfo(&self, index: int32, info: *mut PClassInfo) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave a place to write.
        unsafe {
            let info = &mut *info;
            *info = std::mem::zeroed();
            info.cid = CLASS_ID;
            info.cardinality = ClassCardinality_::kManyInstances as int32;
            write_ascii("Audio Module Class", &mut info.category);
            write_ascii(PLUGIN_NAME, &mut info.name);
        }
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: *const c_char,
        iid: *const c_char,
        object: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || object.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gives two sixteen-byte ids and a place for the object.
        unsafe {
            if *cid.cast::<TUID>() != CLASS_ID {
                return kResultFalse;
            }
            let wanted = *iid.cast::<[u8; 16]>();
            let plugin = ComWrapper::new(TestTone::new());
            let found = pointer_of(&plugin, &wanted);
            match found {
                Some(pointer) => {
                    *object = pointer;
                    kResultOk
                }
                None => kResultFalse,
            }
        }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: int32, info: *mut PClassInfo2) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave a place to write.
        unsafe {
            let info = &mut *info;
            *info = std::mem::zeroed();
            info.cid = CLASS_ID;
            info.cardinality = ClassCardinality_::kManyInstances as int32;
            write_ascii("Audio Module Class", &mut info.category);
            write_ascii(PLUGIN_NAME, &mut info.name);
            write_ascii("Instrument|Synth", &mut info.subCategories);
            write_ascii("Sound Tools", &mut info.vendor);
            write_ascii("0.1.0", &mut info.version);
            write_ascii("VST 3.7.0", &mut info.sdkVersion);
        }
        kResultOk
    }
}

/// The interface of the plugin a host asked for, with its reference counted.
fn pointer_of(plugin: &ComWrapper<TestTone>, wanted: &[u8; 16]) -> Option<*mut c_void> {
    fn of<I: Interface>(plugin: &ComWrapper<TestTone>, wanted: &[u8; 16]) -> Option<*mut c_void>
    where
        TestTone: Class,
    {
        (I::IID == *wanted).then(|| plugin.to_com_ptr::<I>().map(|p| p.into_raw().cast()))?
    }
    of::<IComponent>(plugin, wanted)
        .or_else(|| of::<IAudioProcessor>(plugin, wanted))
        .or_else(|| of::<IEditController>(plugin, wanted))
        .or_else(|| of::<IMidiMapping>(plugin, wanted))
        .or_else(|| of::<IPluginBase>(plugin, wanted))
        .or_else(|| of::<FUnknown>(plugin, wanted))
}

/// What a VST 3 bundle exports. `bundleEntry` gets the `CFBundleRef` of the bundle it is in;
/// this plugin does not need it.
#[unsafe(no_mangle)]
pub extern "C" fn bundleEntry(_bundle: *mut c_void) -> bool {
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn bundleExit() -> bool {
    true
}

/// The one entry a host calls to see what is in a bundle. Everything a test needs a
/// misbehaving plugin for happens here, because this is what a scan makes a bundle do.
#[unsafe(no_mangle)]
pub extern "C" fn GetPluginFactory() -> *mut IPluginFactory {
    support::while_listed("vst3");
    let factory = ComWrapper::new(Factory);
    match factory.to_com_ptr::<IPluginFactory>() {
        Some(pointer) => pointer.into_raw(),
        None => std::ptr::null_mut(),
    }
}

/// Reads everything a stream holds.
///
/// # Safety
///
/// `stream` must be null or a valid `IBStream` for the length of this call.
unsafe fn read_stream(stream: *mut IBStream) -> Option<Vec<u8>> {
    // SAFETY: the caller keeps the contract.
    unsafe {
        let stream = ComRef::from_raw(stream)?;
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let mut read = 0;
            let result = stream.read(chunk.as_mut_ptr().cast(), chunk.len() as int32, &mut read);
            if result != kResultOk || read <= 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read as usize]);
        }
        Some(bytes)
    }
}

/// Moves the write position of a stream, counted from its start.
///
/// # Safety
///
/// `stream` must be null or a valid `IBStream` for the length of this call.
unsafe fn seek_stream(stream: *mut IBStream, to: i64) -> bool {
    // SAFETY: the caller keeps the contract.
    unsafe {
        let Some(stream) = ComRef::from_raw(stream) else {
            return false;
        };
        let mut landed = -1;
        let result = stream.seek(to, IStreamSeekMode_::kIBSeekSet as int32, &mut landed);
        result == kResultOk && landed == to
    }
}

/// What a process mode is called in the log.
fn mode_name(mode: int32) -> &'static str {
    match mode {
        mode if mode == ProcessModes_::kOffline as int32 => "offline",
        mode if mode == ProcessModes_::kPrefetch as int32 => "prefetch",
        _ => "realtime",
    }
}

/// Writes bytes into a stream.
///
/// # Safety
///
/// `stream` must be null or a valid `IBStream` for the length of this call.
unsafe fn write_stream(stream: *mut IBStream, bytes: &[u8]) -> bool {
    // SAFETY: the caller keeps the contract.
    unsafe {
        let Some(stream) = ComRef::from_raw(stream) else {
            return false;
        };
        let mut written = 0;
        let buffer = bytes.as_ptr().cast_mut().cast();
        stream.write(buffer, bytes.len() as int32, &mut written) == kResultOk
    }
}

/// Writes text into a fixed UTF-16 field of the VST 3 API, with the zero that ends it.
fn write_utf16(text: &str, buffer: &mut [TChar]) {
    let mut written = 0;
    for (unit, place) in text.encode_utf16().zip(buffer.iter_mut()) {
        *place = unit;
        written += 1;
    }
    if let Some(place) = buffer.get_mut(written) {
        *place = 0;
    } else if let Some(last) = buffer.last_mut() {
        *last = 0;
    }
}

/// Writes text into a fixed C string field of the VST 3 API.
fn write_ascii(text: &str, buffer: &mut [c_char]) {
    let mut written = 0;
    for (byte, place) in text.bytes().zip(buffer.iter_mut()) {
        *place = byte as c_char;
        written += 1;
    }
    if let Some(place) = buffer.get_mut(written) {
        *place = 0;
    } else if let Some(last) = buffer.last_mut() {
        *last = 0;
    }
}

/// Where `cargo` put this crate's dynamic library, building it first.
pub fn built_library() -> std::path::PathBuf {
    support::built_library("test-vst3-plugin")
}

/// Copies the built library into `folder` as a VST 3 bundle a scan finds, and gives back the
/// bundle.
pub fn install_into(folder: &std::path::Path) -> std::path::PathBuf {
    support::install_bundle(folder, &built_library(), "test-tone", "vst3")
}
