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

use std::sync::Arc;

use vst3::Steinberg::Vst::{
    BusDirections_, BusInfo, ControllerNumbers_, IAudioProcessor, IAudioProcessorTrait, IComponent,
    IComponent_iid, IComponentTrait, IConnectionPoint, IConnectionPointTrait, IEditController,
    IEditController_iid, IEditControllerTrait, IMidiMapping, IMidiMappingTrait, MediaTypes_,
    ParamID, ProcessModes_, ProcessSetup, SpeakerArr, SpeakerArrangement, SymbolicSampleSizes_,
};
use vst3::Steinberg::{IPluginBaseTrait, TUID, int32, kResultOk, kResultTrue};
use vst3::{ComPtr, ComWrapper};

use super::context::{Handler, HostContext, as_handler, as_unknown};
use super::module::Module;
use super::process::{ParameterChange, REPORT_CAPACITY, Vst3Processor};
use super::stream::{MemoryStream, as_stream};
use super::{class_id_of, refused};
use crate::backend::{LoadedPlugin, Opening, PluginGui, Requests};
use crate::scan::ScannedPlugin;
use crate::{PluginProblem, processor::not_ours};

/// What a state asset of a VST 3 plugin holds. VST 3 keeps two states, the component's and the
/// controller's, and a preset file holds both, so this file holds both as well.
const MAGIC: [u8; 4] = *b"SVT3";

/// The biggest state this host reads back. A plugin that writes more is saved all the same;
/// this only bounds what a file that is not ours can make this process allocate.
const MAX_STATE: usize = 512 * 1024 * 1024;

/// Loads the plugin `found` names, with `saved` as its own state, and starts it.
pub fn load(
    found: &ScannedPlugin,
    saved: Option<&[u8]>,
    sample_rate: u32,
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
    // and every pointer either comes from the plugin or outlives the call. A failure leaves
    // the plugin where it was and this function gives up on it.
    let plugin = unsafe {
        let component: ComPtr<IComponent> = create(module.factory(), &class, &IComponent_iid)
            .ok_or_else(|| {
                fail("the bundle has no plugin with this class id, or it is not a component".into())
            })?;
        let result = not_ours(|| component.initialize(host.as_ptr()));
        if result != kResultOk && result != kResultTrue {
            return Err(refused(&plugin_id, "initialize", result));
        }

        // The controller: a second class the component names, or the component itself.
        let mut controller_class: TUID = [0; 16];
        let has_own = component.getControllerClassId(&mut controller_class) == kResultOk
            && controller_class != class;
        let separate = has_own
            .then(|| {
                create::<IEditController>(module.factory(), &controller_class, &IEditController_iid)
            })
            .flatten();
        if let Some(controller) = &separate {
            let result = not_ours(|| controller.initialize(host.as_ptr()));
            if result != kResultOk && result != kResultTrue {
                return Err(refused(&plugin_id, "the controller's initialize", result));
            }
        }
        let controller = separate
            .clone()
            .or_else(|| component.cast::<IEditController>());

        let handler = ComWrapper::new(Handler::default());
        if let Some(controller) = &controller
            && let Some(pointer) = as_handler(&handler)
        {
            controller.setComponentHandler(pointer.as_ptr());
        }

        // Two objects talk through their connection points, with messages this host makes.
        let connection = separate.as_ref().and_then(|controller| {
            let from = component.cast::<IConnectionPoint>()?;
            let to = controller.cast::<IConnectionPoint>()?;
            from.connect(to.as_ptr());
            to.connect(from.as_ptr());
            Some((from, to))
        });

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
        let outputs = arrange(&processor, &component, &inputs, &outputs);

        // The first event input is the one that gets the notes, and there is no more than one.
        component.activateBus(
            MediaTypes_::kEvent as int32,
            BusDirections_::kInput as int32,
            0,
            1,
        );
        component.activateBus(
            MediaTypes_::kAudio as int32,
            BusDirections_::kOutput as int32,
            0,
            1,
        );

        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: sound_core::MAX_BLOCK as int32,
            sampleRate: f64::from(sample_rate),
        };
        let result = processor.setupProcessing(&mut setup);
        if result != kResultOk && result != kResultTrue {
            return Err(refused(&plugin_id, "setupProcessing", result));
        }
        let result = not_ours(|| component.setActive(1));
        if result != kResultOk && result != kResultTrue {
            return Err(refused(&plugin_id, "setActive", result));
        }

        let pedal_parameter = pedal_parameter(controller.as_ref());
        let live = Arc::new(());
        let (reports, changed) = rtrb::RingBuffer::new(REPORT_CAPACITY);
        let started = Vst3Processor::new(
            processor,
            live.clone(),
            &inputs,
            &outputs,
            pedal_parameter,
            reports,
        );
        let notes = match pedal_parameter {
            Some(_) => Vec::new(),
            None => vec![PluginProblem::NoPedal {
                plugin_id: plugin_id.clone(),
            }],
        };
        Opening {
            started: Box::new(started),
            plugin: Box::new(Vst3Plugin {
                component,
                controller,
                separate,
                connection,
                handler,
                _context: context,
                changed,
                live,
                active: true,
            }),
            notes,
        }
    };
    Ok(plugin)
}

/// One loaded VST 3 plugin, from the control thread.
pub struct Vst3Plugin {
    component: ComPtr<IComponent>,
    /// The interface side. The component itself when the plugin is one object.
    controller: Option<ComPtr<IEditController>>,
    /// The controller when it is a second object, which is the one that has to be terminated
    /// and disconnected of its own.
    separate: Option<ComPtr<IEditController>>,
    connection: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    handler: ComWrapper<Handler>,
    /// The plugin holds this for as long as it lives, so it must outlive the plugin.
    _context: ComWrapper<HostContext>,
    /// What the plugin changed by itself while it played.
    changed: rtrb::Consumer<ParameterChange>,
    /// The audio side holds a second one of these. While it does, this plugin may not be
    /// deactivated: the two ends would be in different hands.
    live: Arc<()>,
    active: bool,
}

impl LoadedPlugin for Vst3Plugin {
    fn poll(&mut self) -> Requests {
        // What the plugin changed by itself goes to its controller, which is how the two
        // halves of a plugin stay in step, and says that the state is to be saved.
        let mut changed = false;
        while let Ok(change) = self.changed.pop() {
            changed = true;
            if let Some(controller) = &self.controller {
                // SAFETY: the controller came from the plugin and is alive.
                unsafe { controller.setParamNormalized(change.id, change.value) };
            }
        }
        if changed {
            self.handler.mark_dirty();
        }
        Requests {
            restart: self.handler.take_restart_requested(),
            state_is_dirty: self.handler.take_state_is_dirty(),
            // A VST 3 plugin has no window before step 5b, so it never closes one and never
            // asks for a size.
            window_closed: false,
            window_size: None,
        }
    }

    fn save_state(&mut self) -> Result<Vec<u8>, String> {
        let component = MemoryStream::writing();
        let stream = as_stream(&component).ok_or("the state stream")?;
        // SAFETY: the component came from the plugin and is alive, and the stream outlives the
        // call.
        let result = unsafe { self.component.getState(stream.as_ptr()) };
        if result != kResultOk && result != kResultTrue {
            return Err(format!("the plugin answered {result} to getState"));
        }
        let mut controller_bytes = Vec::new();
        if let Some(controller) = &self.separate {
            let written = MemoryStream::writing();
            let stream = as_stream(&written).ok_or("the state stream")?;
            // SAFETY: as above.
            let result = unsafe { controller.getState(stream.as_ptr()) };
            if result == kResultOk || result == kResultTrue {
                controller_bytes = written.written();
            }
        }
        Ok(write_state(&component.written(), &controller_bytes))
    }

    fn gui(&mut self) -> Option<&mut dyn PluginGui> {
        // Step 5b puts a VST 3 plugin's own window in one of ours, through `IPlugView`. Until
        // then a card says the plugin has no window of its own, which is the truth here.
        None
    }

    fn released(&mut self) -> bool {
        if Arc::get_mut(&mut self.live).is_none() {
            return false;
        }
        self.let_go();
        true
    }
}

impl Vst3Plugin {
    /// Everything VST 3 asks a host to do when it is done with a plugin, in that order.
    fn let_go(&mut self) {
        if !std::mem::take(&mut self.active) {
            return;
        }
        // SAFETY: every object came from the plugin and is alive, and the audio side has been
        // given back, which is what `released` checks before this runs.
        unsafe {
            not_ours(|| self.component.setActive(0));
            if let Some((from, to)) = self.connection.take() {
                from.disconnect(to.as_ptr());
                to.disconnect(from.as_ptr());
            }
            if let Some(controller) = &self.separate {
                controller.setComponentHandler(std::ptr::null_mut());
                not_ours(|| controller.terminate());
            }
            not_ours(|| self.component.terminate());
        }
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        // The project is being torn down. A plugin whose audio side is still in an engine that
        // is going is left as it is, exactly as the CLAP backend leaves one: terminating it
        // while another thread may still be in `process` would be worse than not terminating.
        if Arc::get_mut(&mut self.live).is_some() {
            self.let_go();
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

/// Tells the plugin what this host gives each bus, and reads back what it settled on. The first
/// output is asked for in stereo, which is what the engine carries.
///
/// # Safety
///
/// Both objects must be alive.
unsafe fn arrange(
    processor: &ComPtr<IAudioProcessor>,
    component: &ComPtr<IComponent>,
    inputs: &[usize],
    outputs: &[usize],
) -> Vec<usize> {
    let mut wanted_in: Vec<SpeakerArrangement> =
        inputs.iter().map(|count| speakers(*count)).collect();
    let mut wanted_out: Vec<SpeakerArrangement> =
        outputs.iter().map(|count| speakers(*count)).collect();
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
        // Whatever the plugin answered, what it really has is what its buses now say. A plugin
        // that refuses stereo out keeps the channel count it had.
        bus_channels(component, BusDirections_::kOutput as int32)
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

/// A state asset: the component's state and the controller's, in one file.
fn write_state(component: &[u8], controller: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(MAGIC.len() + 8 + component.len() + controller.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&(component.len() as u32).to_le_bytes());
    bytes.extend_from_slice(component);
    bytes.extend_from_slice(&(controller.len() as u32).to_le_bytes());
    bytes.extend_from_slice(controller);
    bytes
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
            not_ours(|| controller.setComponentState(stream.as_ptr()));
            if !controller_bytes.is_empty() {
                let own = MemoryStream::reading(controller_bytes);
                if let Some(own) = as_stream(&own) {
                    not_ours(|| controller.setState(own.as_ptr()));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_asset_holds_both_states_and_reads_back_as_it_was_written() {
        let bytes = write_state(b"component", b"controller");
        assert_eq!(
            read_parts(&bytes),
            Ok((&b"component"[..], &b"controller"[..]))
        );
    }

    #[test]
    fn a_state_asset_of_a_plugin_that_has_only_a_component_reads_back() {
        let bytes = write_state(b"component", b"");
        assert_eq!(read_parts(&bytes), Ok((&b"component"[..], &b""[..])));
    }

    #[test]
    fn a_file_that_is_not_one_of_ours_is_refused_instead_of_given_to_a_plugin() {
        assert!(read_parts(b"nonsense").is_err());
        assert!(read_parts(b"SVT3\xff\xff\xff\x0f").is_err());
        assert!(read_parts(b"SVT3\x08\x00\x00\x00ab").is_err());
    }

    #[test]
    fn a_stereo_bus_is_asked_for_by_name_and_a_wide_one_by_its_channels() {
        assert_eq!(speakers(1), SpeakerArr::kMono);
        assert_eq!(speakers(2), SpeakerArr::kStereo);
        assert_eq!(speakers(4), 0b1111);
        assert_eq!(speakers(0), 0);
    }
}
