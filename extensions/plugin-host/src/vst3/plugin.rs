//! Loading one VST 3 plugin and keeping it: the control side of the backend.
//!
//! A VST 3 plugin is up to two objects. The component makes the sound and owns the state; the
//! controller is the interface side and knows the parameters. They may be one object or two,
//! and when they are two the host joins them with `IConnectionPoint` and passes the messages.
//!
//! The order of the calls is the format's: create, `initialize`, state, bus arrangements,
//! `setupProcessing`, `setActive(true)`. `setProcessing` and `process` are the audio thread's
//! and live in `process.rs`.
//!
//! How the host learns that a plugin changed its own state, which VST 3 has no `mark_dirty`
//! for: the controller tells `IComponentHandler::performEdit` when the composer moves
//! something in the plugin's window, and the component reports what it changed by itself in
//! the block's output parameter changes. Both mark the state to be saved, and the second is
//! also given to the controller, which is how the two halves stay in step.

use std::rc::Rc;
use std::sync::Arc;

use vst3::Steinberg::Vst::{
    BusDirections_, BusInfo, ControllerNumbers_, IAudioProcessor, IAudioProcessorTrait, IComponent,
    IComponent_iid, IComponentTrait, IConnectionPoint, IConnectionPointTrait, IEditController,
    IEditController_iid, IEditControllerTrait, IMidiMapping, IMidiMappingTrait, MediaTypes_,
    ParamID, ProcessSetup, SpeakerArr, SpeakerArrangement, SymbolicSampleSizes_,
};
use vst3::Steinberg::{
    IPluginBaseTrait, TUID, int32, kNotImplemented, kResultFalse, kResultOk, kResultTrue,
};
use vst3::{ComPtr, ComWrapper};

use super::context::{Handler, HostContext, as_handler, as_unknown};
use super::module::Module;
use super::process::{
    EDIT_CAPACITY, ParameterChange, REPORT_CAPACITY, Vst3Processor, process_mode,
};
use super::stream::{MemoryStream, as_stream};
use super::view::Vst3Gui;
use super::{MAX_STATE, class_id_of, refused};
use crate::backend::{LoadedPlugin, Opening, PluginGui, Requests};
use crate::processor::Started;
use crate::scan::ScannedPlugin;
use crate::{PluginProblem, processor::not_ours};

use sound_core::PrepareConfig;

/// What a state asset of a VST 3 plugin holds. VST 3 keeps two states, the component's and the
/// controller's, and a preset file holds both, so this file holds both as well.
const MAGIC: [u8; 4] = *b"SVT3";

/// Loads the plugin `found` names, with `saved` as its own state, and starts it.
pub fn load(
    found: &ScannedPlugin,
    saved: Option<&[u8]>,
    config: PrepareConfig,
) -> Result<Opening, PluginProblem> {
    let plugin_id = found.id.clone();
    let fail = |message: String| PluginProblem::DidNotLoad {
        plugin_id: plugin_id.clone(),
        message,
    };
    let class = class_id_of(&plugin_id).ok_or_else(|| {
        fail("a VST 3 plugin_id is the class id as thirty-two hex digits".to_string())
    })?;
    let module = Module::load(&found.path).map_err(fail)?;
    let context = ComWrapper::new(HostContext);
    let host = as_unknown(&context).ok_or_else(|| fail("the host context".to_string()))?;

    // SAFETY: every call below goes to the plugin the factory made, in the order VST 3 gives,
    // and every pointer either comes from the plugin or outlives the call. A failure after
    // `initialize` is undone by `Joined`, so no plugin is left half-set-up.
    let plugin = unsafe {
        let component: ComPtr<IComponent> = create(module.factory(), &class, &IComponent_iid)
            .ok_or_else(|| {
                fail("the bundle has no plugin with this class id, or it is not a component".into())
            })?;
        let result = not_ours(|| component.initialize(host.as_ptr()));
        if result != kResultOk && result != kResultTrue {
            return Err(refused(&plugin_id, "initialize", result));
        }
        let mut joined = Joined {
            component: component.clone(),
            separate: None,
            controller: None,
            connection: None,
            handler: ComWrapper::new(Handler::default()),
            // `initialize` has been answered, so from here there is something to undo.
            kept: true,
        };

        // The controller: a second class the component names, or the component itself.
        let mut controller_class: TUID = [0; 16];
        let has_own = component.getControllerClassId(&mut controller_class) == kResultOk
            && controller_class != class;
        joined.separate = has_own
            .then(|| {
                create::<IEditController>(module.factory(), &controller_class, &IEditController_iid)
            })
            .flatten();
        if let Some(controller) = &joined.separate {
            let result = not_ours(|| controller.initialize(host.as_ptr()));
            if result != kResultOk && result != kResultTrue {
                return Err(refused(&plugin_id, "the controller's initialize", result));
            }
        }
        joined.controller = joined
            .separate
            .clone()
            .or_else(|| component.cast::<IEditController>());

        if let Some(controller) = &joined.controller
            && let Some(pointer) = as_handler(&joined.handler)
        {
            controller.setComponentHandler(pointer.as_ptr());
        }

        // Two objects talk through their connection points, with messages this host makes.
        joined.connection = joined.separate.as_ref().and_then(|controller| {
            let from = component.cast::<IConnectionPoint>()?;
            let to = controller.cast::<IConnectionPoint>()?;
            from.connect(to.as_ptr());
            to.connect(from.as_ptr());
            Some((from, to))
        });
        let controller = joined.controller.clone();

        if let Some(bytes) = saved {
            read_state(&plugin_id, bytes, &component, controller.as_ref())?;
        }

        let inputs = bus_channels(&component, BusDirections_::kInput as int32);
        let outputs = bus_channels(&component, BusDirections_::kOutput as int32);
        let processor = component
            .cast::<IAudioProcessor>()
            .ok_or_else(|| fail("the plugin makes no audio".to_string()))?;
        if processor.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as int32) != kResultOk {
            return Err(fail("the plugin does not take 32-bit samples".to_string()));
        }
        // A plugin may change its buses while it answers the arrangement, in either
        // direction, so what it has is read again and the buffers are made from that.
        arrange(&processor, &inputs, &outputs);
        let inputs = bus_channels(&component, BusDirections_::kInput as int32);
        let outputs = bus_channels(&component, BusDirections_::kOutput as int32);

        // The first event input is the one that gets the notes, and there is no more than one.
        component.activateBus(
            MediaTypes_::kEvent as int32,
            BusDirections_::kInput as int32,
            0,
            1,
        );
        // The first audio input is the one an effect is played into. A bus that is not active
        // is one the plugin may ignore, so an effect would be silent without this. An
        // instrument with an audio input gets the silence it always got.
        if !inputs.is_empty() {
            component.activateBus(
                MediaTypes_::kAudio as int32,
                BusDirections_::kInput as int32,
                0,
                1,
            );
        }
        component.activateBus(
            MediaTypes_::kAudio as int32,
            BusDirections_::kOutput as int32,
            0,
            1,
        );

        // A render is told to the plugin here and carried in every block below, which is what
        // VST 3 asks: the mode of a `ProcessData` is the mode of the `setupProcessing` it
        // belongs to. A streaming sampler may wait for its samples in an offline render.
        let mode = process_mode(config.offline);
        let mut setup = ProcessSetup {
            processMode: mode,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: sound_core::MAX_BLOCK as int32,
            sampleRate: f64::from(config.sample_rate),
        };
        let result = processor.setupProcessing(&mut setup);
        if result != kResultOk && result != kResultTrue {
            return Err(refused(&plugin_id, "setupProcessing", result));
        }
        let result = not_ours(|| component.setActive(1));
        if result != kResultOk && result != kResultTrue {
            return Err(refused(&plugin_id, "setActive", result));
        }
        // After `setActive`, which is when a plugin's latency is settled.
        let latency = not_ours(|| processor.getLatencySamples());

        let pedal_parameter = pedal_parameter(controller.as_ref());
        // The window side, made here so that nothing but a load ever asks the plugin for a
        // view. A plugin with no edit controller has no window at all.
        let gui = Vst3Gui::new(controller.as_ref(), &plugin_id);
        let live = Arc::new(());
        let (reports, changed) = rtrb::RingBuffer::new(REPORT_CAPACITY);
        // The other way: what the composer changes in the plugin's own window, on its way to
        // the processor. Both rings are made here, so nothing allocates once a block runs.
        let (edited, edits) = rtrb::RingBuffer::new(EDIT_CAPACITY);
        let started = Vst3Processor::new(
            processor.clone(),
            live.clone(),
            &inputs,
            &outputs,
            pedal_parameter,
            reports,
            edits,
            mode,
            latency,
        );
        // The pedal is only missing from a plugin that has somewhere to take notes. A plugin
        // with no event input bus, which is what an ordinary effect is, has no pedal to miss,
        // and this host cannot ask what a record is for.
        let takes_notes = component.getBusCount(
            MediaTypes_::kEvent as int32,
            BusDirections_::kInput as int32,
        ) > 0;
        let notes = match (takes_notes, pedal_parameter) {
            (true, None) => vec![PluginProblem::NoPedal {
                plugin_id: plugin_id.clone(),
            }],
            _ => Vec::new(),
        };
        Opening {
            started: Box::new(started),
            plugin: Box::new(Vst3Plugin {
                plugin_id: plugin_id.clone(),
                _module: module,
                gui,
                joined,
                _context: context,
                changed,
                edited,
                live,
                processor,
                inputs,
                outputs,
                pedal_parameter,
                mode,
            }),
            notes,
        }
    };
    Ok(plugin)
}

/// A plugin that is initialized, with its two halves joined and the host's handler in it.
///
/// It undoes all of that when it is dropped without [`Self::kept`], so a load that fails part
/// of the way through leaves no plugin half-set-up, and [`Vst3Plugin::let_go`] is the same
/// four calls in the same order.
struct Joined {
    component: ComPtr<IComponent>,
    /// The interface side. The component itself when the plugin is one object.
    controller: Option<ComPtr<IEditController>>,
    /// The controller when it is a second object, which is the one that has to be terminated
    /// and disconnected of its own.
    separate: Option<ComPtr<IEditController>>,
    connection: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    /// What the plugin's controller reports to. The plugin holds a pointer to it until the
    /// handler is taken back below, so it must outlive that call.
    handler: ComWrapper<Handler>,
    /// Whether there is still something to undo. It is set once `initialize` has been
    /// answered and taken by [`Self::let_go`], and a [`Vst3Plugin`] whose audio side the
    /// engine still holds clears it instead, because then nothing may be terminated at all.
    kept: bool,
}

impl Joined {
    /// Everything VST 3 asks a host to do when it is done with a plugin, in that order.
    /// Calling it again does nothing.
    fn let_go(&mut self) {
        if !std::mem::take(&mut self.kept) {
            return;
        }
        // SAFETY: every object came from the plugin and is alive, and nothing else holds the
        // audio side: `Vst3Plugin::released` checks that before this runs, and a load that
        // failed never made one.
        unsafe {
            not_ours(|| self.component.setActive(0));
            if let Some((from, to)) = self.connection.take() {
                from.disconnect(to.as_ptr());
                to.disconnect(from.as_ptr());
            }
            // The plugin lets go of the host's handler before the handler can go, whether the
            // controller is a second object or the component itself.
            if let Some(controller) = self.controller.take() {
                controller.setComponentHandler(std::ptr::null_mut());
            }
            if let Some(controller) = &self.separate {
                not_ours(|| controller.terminate());
            }
            not_ours(|| self.component.terminate());
        }
    }
}

impl Drop for Joined {
    fn drop(&mut self) {
        // Only a load that failed between `initialize` and the plugin being made. A plugin
        // that was made has already been let go of, or is one the engine still holds.
        self.let_go();
    }
}

/// One loaded VST 3 plugin, from the control thread.
pub struct Vst3Plugin {
    plugin_id: String,
    /// The bundle this plugin came out of. Nothing unloads one, but a plugin owning its module
    /// says so rather than leaving it to a table somewhere else.
    _module: Rc<Module>,
    /// The plugin's own window. Declared before [`Self::joined`], so that a plugin being
    /// dropped releases its view before anything terminates the object that made it.
    gui: Option<Vst3Gui>,
    joined: Joined,
    /// The plugin holds this for as long as it lives, so it must outlive the plugin.
    _context: ComWrapper<HostContext>,
    /// What the plugin changed by itself while it played.
    changed: rtrb::Consumer<ParameterChange>,
    /// What the composer changed in the plugin's own window, on its way to the processor.
    edited: rtrb::Producer<ParameterChange>,
    /// The audio side holds a second one of these. While it does, this plugin may not be
    /// deactivated: the two ends would be in different hands.
    live: Arc<()>,
    /// What a new audio side is made of when the plugin is started again: the plugin's
    /// processor interface, its buses, the parameter the pedal goes to and the process mode.
    processor: ComPtr<IAudioProcessor>,
    inputs: Vec<usize>,
    outputs: Vec<usize>,
    pedal_parameter: Option<ParamID>,
    mode: int32,
}

impl LoadedPlugin for Vst3Plugin {
    fn poll(&mut self) -> Requests {
        // What the plugin changed by itself goes to its controller, which is how the two
        // halves of a plugin stay in step, and says that the state is to be saved.
        let mut changed = false;
        while let Ok(change) = self.changed.pop() {
            changed = true;
            if let Some(controller) = &self.joined.controller {
                // SAFETY: the controller came from the plugin and is alive.
                unsafe { controller.setParamNormalized(change.id, change.value) };
            }
        }
        if changed {
            self.joined.handler.mark_dirty();
        }
        // The other way: what the composer changed in the plugin's own window goes to the
        // processor, which is the half that makes the sound. `ivsteditcontroller.h` says that
        // is what `IComponentHandler` is for. What the ring has no room for goes back and is
        // sent at the next poll, so a parameter never ends on a value the composer left behind.
        for edit in self.joined.handler.take_edits() {
            if self.edited.push(edit).is_err() {
                self.joined.handler.keep_edit(edit);
            }
        }
        Requests {
            restart: self.joined.handler.take_latency_changed(),
            restart_not_done: self.joined.handler.take_restart_requested(),
            state_is_dirty: self.joined.handler.take_state_is_dirty(),
            midi_mapping_changed: self.joined.handler.take_midi_mapping_changed(),
            // VST 3 has no way for a plugin to close the window it is in: the host owns that
            // window and the plugin only fills it. CLAP's `clap_host_gui.closed` has no
            // counterpart here, so this is always false.
            window_closed: false,
            window_size: self.gui.as_ref().and_then(Vst3Gui::take_wanted_size),
        }
    }

    fn save_state(&mut self) -> Result<Vec<u8>, String> {
        // SAFETY: both objects came from the plugin and are alive, and each stream outlives the
        // call it is given to.
        let component = unsafe {
            asked_for_state("the plugin", |stream| {
                self.joined.component.getState(stream)
            })
        }?;
        // Whatever object the controller is. One object that implements both interfaces does
        // not promise that its two states are the same bytes, and the load gives this part back
        // through `IEditController::setState` whatever the object identity, so the save asks
        // the same way. Only `initialize` and `terminate` depend on the two being separate.
        let controller = match &self.joined.controller {
            // SAFETY: as above.
            Some(controller) => unsafe {
                asked_for_state("the plugin's controller", |stream| {
                    controller.getState(stream)
                })
            }?,
            None => None,
        };
        // A plugin that keeps no state of its own is not written at all, so nothing replaces
        // what is in the asset with a file that says "this plugin holds nothing".
        let Some(component) = component else {
            return Ok(Vec::new());
        };
        write_state(&component, &controller.unwrap_or_default())
    }

    fn gui(&mut self) -> Option<&mut dyn PluginGui> {
        // `None` for a plugin with no edit controller: nothing can make a view then.
        let gui = self.gui.as_mut()?;
        Some(gui)
    }

    fn released(&mut self) -> bool {
        if Arc::get_mut(&mut self.live).is_none() {
            return false;
        }
        // Whatever the plugin still holds for a window goes before it is terminated. Every path
        // that lets a plugin go has already closed its window; this is the one that decides.
        if let Some(gui) = &mut self.gui {
            gui.destroy();
        }
        self.joined.let_go();
        true
    }

    /// `kLatencyChanged`: "The host has to deactivate and reactivate the plug-in, then
    /// afterwards the host could ask for the current latency." The buses and the setup stay
    /// as they were, so a new audio side is made from what the load found.
    fn restart(
        &mut self,
        _config: PrepareConfig,
    ) -> Option<Result<Box<dyn Started>, PluginProblem>> {
        Arc::get_mut(&mut self.live)?;
        let did_not_restart = |call: &str, result: int32| PluginProblem::DidNotRestart {
            plugin_id: self.plugin_id.clone(),
            message: format!("the plugin answered {result} to {call}"),
        };
        // SAFETY: the component came from the plugin and is alive, and nothing processes: the
        // engine gave the audio side back, which stopped it on the audio thread.
        let result = unsafe {
            not_ours(|| self.joined.component.setActive(0));
            not_ours(|| self.joined.component.setActive(1))
        };
        if result != kResultOk && result != kResultTrue {
            return Some(Err(did_not_restart("setActive", result)));
        }
        // SAFETY: as above.
        let latency = unsafe { not_ours(|| self.processor.getLatencySamples()) };
        // New rings: their other ends went with the audio side that came back. A parameter
        // the plugin moved in the moment it was away is saved with its state anyway.
        let (reports, changed) = rtrb::RingBuffer::new(REPORT_CAPACITY);
        let (edited, edits) = rtrb::RingBuffer::new(EDIT_CAPACITY);
        self.changed = changed;
        self.edited = edited;
        let started = Vst3Processor::new(
            self.processor.clone(),
            self.live.clone(),
            &self.inputs,
            &self.outputs,
            self.pedal_parameter,
            reports,
            edits,
            self.mode,
            latency,
        );
        Some(Ok(Box::new(started)))
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        // The project is being torn down. A plugin whose audio side is still in an engine that
        // is going is left as it is, exactly as the CLAP backend leaves one: terminating it
        // while another thread may still be in `process` would be worse than not terminating.
        // `Joined::let_go` does nothing once `kept` is taken, so this decides.
        if Arc::get_mut(&mut self.live).is_none() {
            self.joined.kept = false;
        }
    }
}

/// Makes one object of a class from the factory.
///
/// # Safety
///
/// The factory must be alive.
unsafe fn create<I: vst3::Interface>(
    factory: &ComPtr<vst3::Steinberg::IPluginFactory>,
    class: &TUID,
    interface: &TUID,
) -> Option<ComPtr<I>> {
    use vst3::Steinberg::IPluginFactoryTrait;
    let mut object = std::ptr::null_mut();
    // SAFETY: the caller keeps the contract. Both ids are sixteen bytes, which is what
    // `createInstance` reads, and `object` is written only when the call says it worked.
    unsafe {
        let result =
            not_ours(|| factory.createInstance(class.as_ptr(), interface.as_ptr(), &mut object));
        if result != kResultOk || object.is_null() {
            return None;
        }
        ComPtr::from_raw(object.cast::<I>())
    }
}

/// How many channels each audio bus of one direction has.
///
/// # Safety
///
/// The component must be alive.
unsafe fn bus_channels(component: &ComPtr<IComponent>, direction: int32) -> Vec<usize> {
    // SAFETY: the caller keeps the contract, and `info` is written by the plugin before it is
    // read, which is why it starts zeroed.
    unsafe {
        let count = component.getBusCount(MediaTypes_::kAudio as int32, direction);
        (0..count)
            .map(|index| {
                let mut info: BusInfo = std::mem::zeroed();
                let result =
                    component.getBusInfo(MediaTypes_::kAudio as int32, direction, index, &mut info);
                match result == kResultOk {
                    true => info.channelCount.max(0) as usize,
                    false => 0,
                }
            })
            .collect()
    }
}

/// Tells the plugin what this host gives each bus. The first input and the first output are
/// asked for in stereo, which is what the engine carries. Whatever the plugin answers, what it
/// really has is what its buses say afterwards, which the caller reads again.
///
/// # Safety
///
/// The processor must be alive.
unsafe fn arrange(processor: &ComPtr<IAudioProcessor>, inputs: &[usize], outputs: &[usize]) {
    let mut wanted_in: Vec<SpeakerArrangement> =
        inputs.iter().map(|count| speakers(*count)).collect();
    let mut wanted_out: Vec<SpeakerArrangement> =
        outputs.iter().map(|count| speakers(*count)).collect();
    if let Some(first) = wanted_in.first_mut() {
        *first = SpeakerArr::kStereo;
    }
    if let Some(first) = wanted_out.first_mut() {
        *first = SpeakerArr::kStereo;
    }
    // SAFETY: the caller keeps the contract, and both slices outlive the call.
    unsafe {
        not_ours(|| {
            processor.setBusArrangements(
                wanted_in.as_mut_ptr(),
                wanted_in.len() as int32,
                wanted_out.as_mut_ptr(),
                wanted_out.len() as int32,
            )
        });
    }
}

/// The speaker arrangement of a channel count. Anything this host does not know by name is
/// asked for as the count of low bits, which is what the VST 3 arrangements are.
fn speakers(channels: usize) -> SpeakerArrangement {
    match channels {
        0 => 0,
        1 => SpeakerArr::kMono,
        2 => SpeakerArr::kStereo,
        count => (1_u64 << count) - 1,
    }
}

/// The parameter the plugin maps the sustain pedal to. VST 3 has no MIDI controller event:
/// `IMidiMapping` is how the format says a host sends one, as a parameter change.
///
/// # Safety
///
/// The controller must be alive.
unsafe fn pedal_parameter(controller: Option<&ComPtr<IEditController>>) -> Option<ParamID> {
    let mapping = controller?.cast::<IMidiMapping>()?;
    let mut id: ParamID = 0;
    // SAFETY: the caller keeps the contract, and `id` is written only when the call works.
    let result = unsafe {
        mapping.getMidiControllerAssignment(
            0,
            0,
            ControllerNumbers_::kCtrlSustainOnOff as i16,
            &mut id,
        )
    };
    (result == kResultOk).then_some(id)
}

/// One half of a plugin's state, out of the plugin.
///
/// `Ok(None)` is "this plugin keeps no such state", which VST 3 says with `kNotImplemented` or
/// `kResultFalse` and which a one-object plugin usually says of its controller half. Every
/// other failure code is an error, so a plugin that could not give its state does not have an
/// empty one written over the good file that is already there.
///
/// # Safety
///
/// The call must go to an object that is alive. The stream it is given lives for the call.
unsafe fn asked_for_state(
    whose: &str,
    call: impl FnOnce(*mut vst3::Steinberg::IBStream) -> int32,
) -> Result<Option<Vec<u8>>, String> {
    let written = MemoryStream::writing();
    let stream = as_stream(&written).ok_or("the state stream")?;
    // SAFETY: the caller keeps the contract, and the stream outlives the call.
    let result = not_ours(|| call(stream.as_ptr()));
    if result == kResultOk || result == kResultTrue {
        return Ok(Some(written.written()));
    }
    // The two ways VST 3 has of saying "not mine". Everything else is a failure.
    if result == kNotImplemented || result == kResultFalse {
        return Ok(None);
    }
    Err(format!("{whose} answered {result} to getState"))
}

/// A state asset: the component's state and the controller's, in one file.
///
/// A state that could not be read back is not written: the lengths are four bytes each, so a
/// part above [`MAX_STATE`] would be written with a length that is not its own and the file
/// would be unreadable. The asset that is there is left alone and the composer is told.
fn write_state(component: &[u8], controller: &[u8]) -> Result<Vec<u8>, String> {
    let too_big = |what: &str, length: usize| {
        format!(
            "the {what} state is {length} bytes, more than the {MAX_STATE} a state file holds. The file that is there is left as it is"
        )
    };
    if component.len() > MAX_STATE {
        return Err(too_big("plugin's", component.len()));
    }
    if controller.len() > MAX_STATE {
        return Err(too_big("plugin controller's", controller.len()));
    }
    let mut bytes = Vec::with_capacity(MAGIC.len() + 8 + component.len() + controller.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&(component.len() as u32).to_le_bytes());
    bytes.extend_from_slice(component);
    bytes.extend_from_slice(&(controller.len() as u32).to_le_bytes());
    bytes.extend_from_slice(controller);
    Ok(bytes)
}

/// The two states in a state asset. An error says the file is not one this host wrote.
fn read_parts(bytes: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let rest = bytes
        .strip_prefix(&MAGIC)
        .ok_or("this is not a VST 3 state written by this program")?;
    let (component, rest) = take_part(rest)?;
    let (controller, _) = take_part(rest)?;
    Ok((component, controller))
}

/// One length-prefixed part of a state asset, and what follows it.
fn take_part(rest: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let (length, rest) = rest
        .split_at_checked(4)
        .ok_or("the state file stops in the middle")?;
    let length = u32::from_le_bytes([length[0], length[1], length[2], length[3]]) as usize;
    if length > MAX_STATE {
        return Err("the state file says it is longer than any plugin state".to_string());
    }
    rest.split_at_checked(length)
        .ok_or_else(|| "the state file stops in the middle".to_string())
}

/// Gives the plugin back what it saved: the component's state, the same bytes to the
/// controller so it follows, and then the controller's own.
///
/// # Safety
///
/// Both objects must be alive.
unsafe fn read_state(
    plugin_id: &str,
    bytes: &[u8],
    component: &ComPtr<IComponent>,
    controller: Option<&ComPtr<IEditController>>,
) -> Result<(), PluginProblem> {
    let not_read = |message: String| PluginProblem::StateNotRead {
        plugin_id: plugin_id.to_string(),
        message,
    };
    let (component_bytes, controller_bytes) = read_parts(bytes).map_err(not_read)?;
    let saved = MemoryStream::reading(component_bytes);
    let stream = as_stream(&saved).ok_or_else(|| not_read("the state stream".to_string()))?;
    // A plugin that says it does not take a state is not a failure: a one-object plugin says
    // exactly that of `setComponentState`, because the state is already where it belongs. Any
    // other failure code is one, and the plugin does not load: a plugin that came up with half
    // of its state would write that half over the good file at the next save.
    let took = |whose: &str, call: &str, result: int32| {
        let answered = result == kResultOk || result == kResultTrue;
        let not_mine = result == kNotImplemented || result == kResultFalse;
        match answered || not_mine {
            true => Ok(()),
            false => Err(not_read(format!("{whose} answered {result} to {call}"))),
        }
    };
    // SAFETY: the caller keeps the contract, and the stream outlives every call below.
    unsafe {
        let result = not_ours(|| component.setState(stream.as_ptr()));
        if result != kResultOk && result != kResultTrue {
            return Err(not_read(format!(
                "the plugin answered {result} to setState"
            )));
        }
        if let Some(controller) = controller {
            // The controller is given the component's state as well, which is how it shows
            // what the component really holds.
            saved.rewind();
            let result = not_ours(|| controller.setComponentState(stream.as_ptr()));
            took("the plugin's controller", "setComponentState", result)?;
            if !controller_bytes.is_empty() {
                let own = MemoryStream::reading(controller_bytes);
                let own =
                    as_stream(&own).ok_or_else(|| not_read("the state stream".to_string()))?;
                let result = not_ours(|| controller.setState(own.as_ptr()));
                took("the plugin's controller", "setState", result)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(component: &[u8], controller: &[u8]) -> Vec<u8> {
        write_state(component, controller).expect("a state this size is written")
    }

    #[test]
    fn a_state_asset_holds_both_states_and_reads_back_as_it_was_written() {
        let bytes = written(b"component", b"controller");
        assert_eq!(
            read_parts(&bytes),
            Ok((&b"component"[..], &b"controller"[..]))
        );
    }

    #[test]
    fn a_state_asset_of_a_plugin_that_has_only_a_component_reads_back() {
        let bytes = written(b"component", b"");
        assert_eq!(read_parts(&bytes), Ok((&b"component"[..], &b""[..])));
    }

    #[test]
    fn a_file_that_is_not_one_of_ours_is_refused_instead_of_given_to_a_plugin() {
        assert!(read_parts(b"nonsense").is_err());
        assert!(read_parts(b"SVT3\xff\xff\xff\x0f").is_err());
        assert!(read_parts(b"SVT3\x08\x00\x00\x00ab").is_err());
    }

    /// A state longer than the four bytes its length is written in would be read back as
    /// another state, or as nothing. It is refused before the asset is touched, so what the
    /// plugin saved last is still there.
    #[test]
    fn a_state_too_long_to_be_read_back_is_refused_instead_of_written() {
        // Half a gigabyte of zeros, which the system gives as pages it never has to fill in
        // because nothing here writes into them. The length is what is checked.
        let long = vec![0_u8; MAX_STATE + 1];
        let error = write_state(&long, b"").expect_err("a state this long is refused");
        assert!(error.contains("left as it is"), "{error}");
        let error = write_state(b"component", &long).expect_err("a controller state is too");
        assert!(error.contains("controller"), "{error}");
    }

    #[test]
    fn a_stereo_bus_is_asked_for_by_name_and_a_wide_one_by_its_channels() {
        assert_eq!(speakers(1), SpeakerArr::kMono);
        assert_eq!(speakers(2), SpeakerArr::kStereo);
        assert_eq!(speakers(4), 0b1111);
        assert_eq!(speakers(0), 0);
    }
}
