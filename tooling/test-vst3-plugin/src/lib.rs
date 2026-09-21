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
//! It has no window. Step 5b builds `IPlugView` in the host, and this plugin grows one then.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicI32, Ordering};

use test_plugin_support as support;
use vst3::Steinberg::Vst::{
    BusDirections_, BusInfo, BusInfo_::BusFlags_, BusTypes_, ControllerNumbers_,
    Event_::EventTypes_, IAudioProcessor, IAudioProcessorTrait, IComponent, IComponentHandler,
    IComponentTrait, IEditController, IEditControllerTrait, IEventListTrait, IMidiMapping,
    IMidiMappingTrait, IParamValueQueueTrait, IParameterChangesTrait, MediaTypes_, ParamID,
    ParamValue, ParameterInfo, ParameterInfo_::ParameterFlags_, ProcessData, ProcessSetup,
    RoutingInfo, SpeakerArr, SpeakerArrangement, String128, SymbolicSampleSizes_, TChar,
};
use vst3::Steinberg::{
    FUnknown, IBStream, IBStreamTrait, IPlugView, IPluginBase, IPluginBaseTrait, IPluginFactory,
    IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo_::ClassCardinality_, PClassInfo2, PFactoryInfo, TBool, TUID, int32,
    kInvalidArgument, kNotImplemented, kResultFalse, kResultOk, kResultTrue, tresult, uint32,
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

/// How many semitones the transpose parameter covers, so that a normalized value is exact.
const TRANSPOSE_RANGE: f64 = 63.0;

/// How many pedal points one block may carry. Fixed, so `process` never allocates.
const PEDAL_POINTS: usize = 32;

/// The plugin. One object for both halves, which VST 3 allows.
pub struct TestTone {
    audio: RefCell<Audio>,
    /// What the host gave `setComponentHandler`, for telling it about an edit. This plugin
    /// never opens a window, so it only keeps it.
    handler: RefCell<Option<ComPtr<IComponentHandler>>>,
    /// The transpose, read by both halves.
    semitones: AtomicI32,
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
            }),
            handler: RefCell::new(None),
            semitones: AtomicI32::new(0),
            plugin: support::next_plugin(),
        }
    }

    fn log(&self, call: &str) {
        let processed = self.audio.try_borrow().map_or(0, |audio| audio.processed);
        support::log(call, self.plugin, processed);
    }
}

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
        let Some(semitones) = support::load_state(&bytes) else {
            return kResultFalse;
        };
        self.semitones.store(semitones, Ordering::Release);
        if let Ok(mut audio) = self.audio.try_borrow_mut() {
            audio.tone.set_semitones(semitones);
        }
        kResultOk
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        let bytes = support::save_state(self.semitones.load(Ordering::Acquire));
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
            // against.
            if audio.processed == 0 {
                support::log("process", self.plugin, 0);
            }
            let frames = data.numSamples.max(0) as usize;
            if data.numOutputs < 1 || data.outputs.is_null() {
                return kResultOk;
            }
            let bus = &mut *data.outputs;
            if bus.numChannels < 2 || bus.__field0.channelBuffers32.is_null() {
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
                    if queue.getParameterId() != SUSTAIN {
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

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        // Everything this plugin keeps is in the component's state.
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> int32 {
        2
    }

    unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
        if info.is_null() || !(0..2).contains(&index) {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave a place to write one parameter.
        unsafe {
            let info = &mut *info;
            *info = std::mem::zeroed();
            info.id = if index == 0 { TRANSPOSE } else { SUSTAIN };
            write_utf16(
                if index == 0 { "Transpose" } else { "Sustain" },
                &mut info.title,
            );
            write_utf16(if index == 0 { "st" } else { "" }, &mut info.units);
            info.stepCount = 0;
            info.defaultNormalizedValue = 0.0;
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
            _ => 0.0,
        }
    }

    unsafe fn setParamNormalized(&self, id: ParamID, value: ParamValue) -> tresult {
        // The host giving back what the plugin reported. It is already where it belongs.
        if id == TRANSPOSE {
            support::log("setParamNormalized", self.plugin, 0);
            let _ = value;
        }
        kResultOk
    }

    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        // SAFETY: the host gives a handler that is alive for this call, and `to_com_ptr`
        // counts the reference this plugin keeps.
        let kept = unsafe { ComRef::from_raw(handler) }.map(|handler| handler.to_com_ptr());
        *self.handler.borrow_mut() = kept;
        kResultOk
    }

    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        // No window before step 5b. A host must offer none and say so.
        std::ptr::null_mut()
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
