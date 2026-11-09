//! Loading one VST 3 plugin and keeping it: the control side of the backend.
//!
//! The order of the calls is the format's: create, `initialize`, state, bus arrangements,
//! `setupProcessing`, `setActive(true)`.
//!
//! VST 3 has no `mark_dirty`. The state is marked to be saved when the controller calls
//! `performEdit`, and when the component reports a change in a block's output parameter
//! changes, which also goes to the controller so the two halves stay in step.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use vst3::Steinberg::Vst::{
    BusDirections_, BusInfo, IAudioProcessor, IAudioProcessorTrait, IComponent, IComponent_iid,
    IComponentTrait, IConnectionPoint, IConnectionPointTrait, IEditController, IEditController_iid,
    IEditControllerTrait, IMidiMapping, IMidiMappingTrait, MediaTypes_, ParamID, ParamValue,
    ParameterInfo, ParameterInfo_::ParameterFlags_, ProcessSetup, SpeakerArr, SpeakerArrangement,
    SymbolicSampleSizes_,
};
use vst3::Steinberg::{IPluginBaseTrait, TUID, int32, kNotImplemented, kResultFalse, kResultOk};
use vst3::{ComPtr, ComWrapper};

use super::context::{Handler, HostContext, as_handler, as_unknown};
use super::module::Module;
use super::parameters;
use super::process::{
    CONTROLLER_NUMBERS, ControlTargets, Report, Vst3Processor, controller_number, process_mode,
};
use super::stream::{MemoryStream, as_stream};
use super::view::Vst3Gui;
use super::{MAX_STATE, class_id_of, refused};
use crate::backend::{Hand, LoadedPlugin, Opening, ParameterChange, PluginGui, Requests};
use crate::parameters::{Parameter, by_id, playable};
use crate::processor::{Control, Laned, Player, PluginAudio};
use crate::scan::ScannedPlugin;
use crate::{Pin, PluginProblem, processor::not_ours};

use sound_core::PrepareConfig;
use sound_notes::Pedal;

/// The start of a state asset, which holds both the component's and the controller's state,
/// as a VST 3 preset file does.
const MAGIC: [u8; 4] = *b"SVT3";

/// Every parameter a host may set of the plugin `found` names, without preparing it for audio.
pub(crate) fn read_parameters(found: &ScannedPlugin) -> Result<Vec<Parameter>, PluginProblem> {
    let initialized = initialize(found)?;
    Ok(match &initialized.joined.controller {
        // SAFETY: the controller came from the plugin and is alive until `initialized` goes.
        Some(controller) => unsafe { parameters::of_controller(controller) },
        None => Vec::new(),
    })
}

/// A plugin that is made and initialized, with what it needs to stay alive. The field order is
/// the drop order: the plugin goes before its context and its bundle.
struct Initialized {
    joined: Joined,
    context: ComWrapper<HostContext>,
    module: Rc<Module>,
}

/// Makes the plugin `found` names and initializes it and its controller, which runs its code.
fn initialize(found: &ScannedPlugin) -> Result<Initialized, PluginProblem> {
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

    // SAFETY: every call goes to the plugin the factory made, in the order VST 3 gives, and
    // every pointer comes from the plugin or outlives the call. `Joined` undoes a failure.
    unsafe {
        let component: ComPtr<IComponent> = create(module.factory(), &class, &IComponent_iid)
            .ok_or_else(|| {
                fail("the bundle has no plugin with this class id, or it is not a component".into())
            })?;
        let result = not_ours(|| component.initialize(host.as_ptr()));
        if result != kResultOk {
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
            if result != kResultOk {
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

        joined.connection = joined.separate.as_ref().and_then(|controller| {
            let from = component.cast::<IConnectionPoint>()?;
            let to = controller.cast::<IConnectionPoint>()?;
            from.connect(to.as_ptr());
            to.connect(from.as_ptr());
            Some((from, to))
        });
        Ok(Initialized {
            joined,
            context,
            module,
        })
    }
}

/// Loads the plugin `found` names, with `saved` as its own state and then `pins`, and starts it.
pub(crate) fn load(
    found: &ScannedPlugin,
    saved: Option<&[u8]>,
    config: PrepareConfig,
    pins: &BTreeMap<u32, Pin>,
) -> Result<Opening, PluginProblem> {
    let plugin_id = found.id.clone();
    let fail = |message: String| PluginProblem::DidNotLoad {
        plugin_id: plugin_id.clone(),
        message,
    };
    let initialized = initialize(found)?;
    let component = initialized.joined.component.clone();
    let controller = initialized.joined.controller.clone();

    // SAFETY: as in `initialize`.
    let plugin = unsafe {
        if let Some(bytes) = saved {
            read_state(&plugin_id, bytes, &component, controller.as_ref())?;
        }
        // VST 3 has no call that gives the processor a value outside a block, so the pins go to
        // the controller before activation: a plugin whose halves share values sets up its
        // smoothing from them. One that keeps its own hears them in its first block.
        if let Some(controller) = controller.as_ref().filter(|_| !pins.is_empty()) {
            let parameters = by_id(parameters::of_controller(controller));
            for change in playable(&parameters, pins) {
                controller.setParamNormalized(change.id, change.value);
            }
        }

        let processor = component
            .cast::<IAudioProcessor>()
            .ok_or_else(|| fail("the plugin makes no audio".to_string()))?;
        if processor.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as int32) != kResultOk {
            return Err(fail("the plugin does not take 32-bit samples".to_string()));
        }
        let buses = prepare_buses(&component, &processor);

        // A streaming sampler may wait for its samples in an offline render.
        let mode = process_mode(config.offline);
        let mut setup = ProcessSetup {
            processMode: mode,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: sound_core::MAX_BLOCK as int32,
            sampleRate: f64::from(config.sample_rate),
        };
        let result = processor.setupProcessing(&mut setup);
        if result != kResultOk {
            return Err(refused(&plugin_id, "setupProcessing", result));
        }
        let result = not_ours(|| component.setActive(1));
        if result != kResultOk {
            return Err(refused(&plugin_id, "setActive", result));
        }
        // Only settled after `setActive`.
        let latency = not_ours(|| processor.getLatencySamples());

        let targets = ControlTargets::new(|number| mapped_parameter(controller.as_ref(), number));
        let targets = Arc::new(targets);
        // What the processor holds as far as the host knows; `kParamValuesChanged` is answered
        // against these.
        let values = controller
            .as_ref()
            .map(|controller| parameter_values(controller, &BTreeMap::new()))
            .unwrap_or_default();
        let gui = Vst3Gui::new(controller.as_ref(), &plugin_id);
        let live = Arc::new(());
        let (started, ends) = Vst3Processor::new(
            processor.clone(),
            live.clone(),
            &buses.inputs,
            &buses.outputs,
            targets.clone(),
            initialized.joined.handler.clone(),
            mode,
            latency,
        );
        // Only a plugin that takes notes misses a pedal; an effect has no event input.
        let notes = match (buses.takes_notes, targets.get(Control::Pedal(Pedal::UP))) {
            (true, None) => vec![PluginProblem::NoPedal {
                plugin_id: plugin_id.clone(),
            }],
            _ => Vec::new(),
        };
        initialized.joined.handler.forget_restarts();
        Opening {
            started: Box::new(started),
            plugin: Box::new(Vst3Plugin {
                plugin_id: plugin_id.clone(),
                _module: initialized.module,
                gui,
                joined: initialized.joined,
                _context: initialized.context,
                changed: ends.changed,
                edited: ends.edited,
                edits_pushed: 0,
                pushed_at: BTreeMap::new(),
                live,
                processor,
                targets,
                takes_notes: buses.takes_notes,
                values,
                mode,
            }),
            notes,
        }
    };
    Ok(plugin)
}

/// A plugin that is initialized, with its two halves joined and the host's handler in it. A
/// load that fails part of the way through drops it, which undoes all of that.
struct Joined {
    component: ComPtr<IComponent>,
    /// The component itself when the plugin is one object.
    controller: Option<ComPtr<IEditController>>,
    /// The controller when it is a second object, which must be terminated on its own.
    separate: Option<ComPtr<IEditController>>,
    connection: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    /// The plugin holds a pointer to this until `let_go` takes it back.
    handler: ComWrapper<Handler>,
    /// Whether there is still something to undo. A [`Vst3Plugin`] whose audio side the engine
    /// still holds clears it, because then nothing may be terminated.
    kept: bool,
}

impl Joined {
    /// What VST 3 asks a host to do when it is done with a plugin, in order. Calling it again
    /// does nothing.
    fn let_go(&mut self) {
        if !std::mem::take(&mut self.kept) {
            return;
        }
        // SAFETY: every object came from the plugin and is alive, and nothing holds the audio
        // side: `Vst3Plugin::released` checks that, and a failed load never made one.
        unsafe {
            not_ours(|| self.component.setActive(0));
            if let Some((from, to)) = self.connection.take() {
                from.disconnect(to.as_ptr());
                to.disconnect(from.as_ptr());
            }
            // The plugin lets go of the handler before the handler can go.
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
        self.let_go();
    }
}

/// One loaded VST 3 plugin, from the control thread.
pub(super) struct Vst3Plugin {
    plugin_id: String,
    _module: Rc<Module>,
    /// Declared before [`Self::joined`], so a plugin being dropped releases its view before
    /// anything terminates the object that made it.
    gui: Option<Vst3Gui>,
    joined: Joined,
    /// The plugin holds this for as long as it lives.
    _context: ComWrapper<HostContext>,
    changed: rtrb::Consumer<Report>,
    edited: rtrb::Producer<ParameterChange>,
    /// How many edits went into `edited`, and for each parameter how many had by its latest
    /// one. A report the processor made before it took that many is older than the edit.
    edits_pushed: u64,
    pushed_at: BTreeMap<ParamID, u64>,
    /// The audio side holds a second one of these. While it does, this plugin may not be
    /// deactivated.
    live: Arc<()>,
    processor: ComPtr<IAudioProcessor>,
    targets: Arc<ControlTargets>,
    /// Whether the plugin has an event input, so that a pedal it no longer maps is worth saying.
    takes_notes: bool,
    /// The value of every parameter the processor was last given or reported, as far as the
    /// host knows. Read-only parameters are left out, because a host never sends one.
    values: BTreeMap<ParamID, ParamValue>,
    mode: int32,
}

impl Vst3Plugin {
    /// What the plugin changed by itself goes to its controller and marks the state dirty.
    fn take_reports(&mut self) {
        let mut changed = false;
        while let Ok(report) = self.changed.pop() {
            changed = true;
            let change = report.change;
            // Skip a report older than the parameter's latest edit, or one whose edit still
            // waits: the controller already shows the edit.
            let edited = self.pushed_at.get(&change.id);
            let older = edited.is_some_and(|edited| report.edits_taken < *edited);
            if older || self.joined.handler.waits(change.id) {
                continue;
            }
            self.values.insert(change.id, change.value);
            if let Some(controller) = &self.joined.controller {
                // SAFETY: the controller came from the plugin and is alive.
                unsafe { controller.setParamNormalized(change.id, change.value) };
            }
        }
        if changed {
            self.joined.handler.mark_dirty();
        }
    }

    /// Every waiting edit goes to the processor as far as the ring has room; the rest waits for
    /// the next poll. An audio side that has gone takes nothing: the plugin is restarting.
    fn push_edits(&mut self) {
        if self.edited.is_abandoned() {
            return;
        }
        for edit in self.joined.handler.take_edits() {
            self.values.insert(edit.id, edit.value);
            match self.edited.push(edit) {
                Ok(()) => {
                    self.edits_pushed += 1;
                    self.pushed_at.insert(edit.id, self.edits_pushed);
                }
                Err(_) => self.joined.handler.keep_edit(edit),
            }
        }
    }

    /// `kParamValuesChanged`: "The host invalidates all caches of parameter values and asks the
    /// edit controller for the current values." Every parameter the controller now shows
    /// differently from what the processor was given is sent as an edit, such as after a preset
    /// the plugin loaded itself. Most plugins say this after their state is read, and then
    /// nothing is sent.
    fn follow_the_controller(&mut self) {
        let Some(controller) = &self.joined.controller else {
            return;
        };
        // SAFETY: the controller came from the plugin and is alive.
        let now = |id| unsafe { controller.getParamNormalized(id) };
        for change in differences(&mut self.values, now) {
            self.joined.handler.keep_edit(change);
        }
    }
}

impl LoadedPlugin for Vst3Plugin {
    fn poll(&mut self) -> Requests {
        self.take_reports();
        // `kParamIDMappingChanged`: listed again before any values are compared, so a new
        // parameter is compared from here on.
        let ids_changed = self.joined.handler.take_ids_changed();
        if ids_changed && let Some(controller) = &self.joined.controller {
            self.values = parameter_values(controller, &self.values);
        }
        if self.joined.handler.take_values_changed() {
            self.follow_the_controller();
        }
        // `kMidiCCAssignmentChanged`: "The host has to rebuild the MIDI-CC => parameter
        // mapping". The audio side follows from the next block.
        let mut pedal_unmapped = false;
        if self.joined.handler.take_midi_mapping_changed() {
            let pedal = controller_number(Control::Pedal(Pedal::UP));
            for number in 0..CONTROLLER_NUMBERS {
                // SAFETY: the controller came from the plugin and is alive.
                let now = unsafe { mapped_parameter(self.joined.controller.as_ref(), number) };
                let before = self.targets.of_number(number);
                self.targets.set(number, now);
                if number != pedal {
                    // Another control leaves its old parameter where it put it, as a knob a
                    // MIDI learn moved stays where it was left.
                    continue;
                }
                pedal_unmapped = self.takes_notes && now.is_none() && before.is_some();
                // A pedal held on the parameter it leaves would stay down there for good.
                if let Some(before) = before
                    && now != Some(before)
                {
                    self.joined.handler.keep_edit(ParameterChange {
                        id: before,
                        value: 0.0,
                    });
                }
            }
        }
        self.push_edits();
        Requests {
            restart: self.joined.handler.take_restart_wanted(),
            reload: self.joined.handler.take_reload_wanted(),
            state_is_dirty: self.joined.handler.take_state_is_dirty(),
            pedal_unmapped,
            window_closed: false,
            window_size: self.gui.as_ref().and_then(Vst3Gui::take_wanted_size),
            parameters_changed: ids_changed | self.joined.handler.take_titles_changed(),
        }
    }

    fn save_state(&mut self) -> Result<Vec<u8>, String> {
        // SAFETY: both objects came from the plugin and are alive, and each stream outlives its
        // call.
        let component = unsafe {
            asked_for_state("the plugin", |stream| {
                self.joined.component.getState(stream)
            })
        }?;
        // Asked even when the controller is the component: one object does not promise its two
        // states are the same bytes, and the load gives this part back through
        // `IEditController::setState` either way.
        let controller = match &self.joined.controller {
            // SAFETY: as above.
            Some(controller) => unsafe {
                asked_for_state("the plugin's controller", |stream| {
                    controller.getState(stream)
                })
            }?,
            None => None,
        };
        // A plugin that keeps no state writes nothing over the asset.
        let Some(component) = component else {
            return Ok(Vec::new());
        };
        write_state(&component, &controller.unwrap_or_default())
    }

    fn gui(&mut self) -> Option<&mut dyn PluginGui> {
        let gui = self.gui.as_mut()?;
        Some(gui)
    }

    /// VST 3 has no answer for an id the plugin does not have: its controller gives a number
    /// for any id.
    fn value(&mut self, id: u32) -> Option<f64> {
        let controller = self.joined.controller.as_ref()?;
        // SAFETY: the controller came from the plugin and is alive.
        Some(unsafe { controller.getParamNormalized(id) })
    }

    fn text(&mut self, id: u32, value: f64) -> Option<String> {
        let controller = self.joined.controller.as_ref()?;
        // SAFETY: the controller came from the plugin and is alive.
        unsafe { parameters::text(controller, id, value) }
    }

    fn parameters(&mut self) -> Vec<Parameter> {
        match &self.joined.controller {
            // SAFETY: the controller came from the plugin and is alive.
            Some(controller) => unsafe { parameters::of_controller(controller) },
            None => Vec::new(),
        }
    }

    /// The controller is told at once, so what it says is the value as the plugin took it, and
    /// the processor gets it in a block, as a window edit does.
    fn send(&mut self, change: ParameterChange) {
        if let Some(controller) = &self.joined.controller {
            // SAFETY: the controller came from the plugin and is alive.
            unsafe { controller.setParamNormalized(change.id, change.value) };
        }
        // In place of a waiting edit of the same parameter, so neither overtakes the other.
        self.joined.handler.put_edit(change);
        self.push_edits();
    }

    /// The known value is updated too, so `kParamValuesChanged` does not send it to the
    /// processor as an edit.
    fn show(&mut self, change: ParameterChange) -> bool {
        let Some(controller) = &self.joined.controller else {
            return false;
        };
        // SAFETY: the controller came from the plugin and is alive.
        unsafe { controller.setParamNormalized(change.id, change.value) };
        self.values.insert(change.id, change.value);
        true
    }

    /// The controller holds every value the moment it is sent, so there is nothing to wait for.
    fn sent_values_played(&mut self) -> bool {
        true
    }

    fn hand(&mut self) -> Hand {
        self.joined.handler.take_hand()
    }

    fn released(&mut self) -> bool {
        if Arc::get_mut(&mut self.live).is_none() {
            return false;
        }
        // The view goes before the plugin is terminated.
        if let Some(gui) = &mut self.gui {
            gui.destroy();
        }
        self.joined.let_go();
        true
    }

    /// `kLatencyChanged`: "The host has to deactivate and reactivate the plug-in, then
    /// afterwards the host could ask for the current latency." `kIoChanged`: "The host has to
    /// deactivate the plug-in, asks the plug-in for its wanted new bus configurations, adapts
    /// its processing graph and reactivate the plug-in." One way for both: the buses are read
    /// again while the plugin is inactive, and a new audio side is made from what it says.
    fn restart(
        &mut self,
        _config: PrepareConfig,
        pins: &[ParameterChange],
        laned: &Laned,
    ) -> Option<Result<Box<dyn PluginAudio>, PluginProblem>> {
        Arc::get_mut(&mut self.live)?;
        // The old audio side's last reports reach the controller before its ring goes.
        self.take_reports();
        let did_not_restart = |call: &str, result: int32| PluginProblem::DidNotRestart {
            plugin_id: self.plugin_id.clone(),
            message: format!("the plugin answered {result} to {call}"),
        };
        let component = &self.joined.component;
        // SAFETY: the component came from the plugin and is alive, and nothing processes: the
        // engine gave the audio side back, which stopped it on the audio thread.
        let (buses, result) = unsafe {
            not_ours(|| component.setActive(0));
            let buses = prepare_buses(component, &self.processor);
            if let Some(controller) = &self.joined.controller {
                for change in pins {
                    controller.setParamNormalized(change.id, change.value);
                }
            }
            (buses, not_ours(|| component.setActive(1)))
        };
        if result != kResultOk {
            return Some(Err(did_not_restart("setActive", result)));
        }
        self.takes_notes = buses.takes_notes;
        // SAFETY: as above.
        let latency = unsafe { not_ours(|| self.processor.getLatencySamples()) };
        self.joined.handler.forget_restarts();
        // New rings: the old ones went with the old audio side, which gave its unplayed edits
        // back to the handler.
        let (started, ends) = Vst3Processor::new(
            self.processor.clone(),
            self.live.clone(),
            &buses.inputs,
            &buses.outputs,
            self.targets.clone(),
            self.joined.handler.clone(),
            self.mode,
            latency,
        );
        self.changed = ends.changed;
        self.edited = ends.edited;
        self.edits_pushed = 0;
        self.pushed_at.clear();
        Some(Ok(Box::new(Player::new(Box::new(started), laned.clone()))))
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        // The project is being torn down. A plugin whose audio side is still in the engine is
        // left as it is: terminating it while another thread may be in `process` is worse.
        if Arc::get_mut(&mut self.live).is_none() {
            self.joined.kept = false;
        }
    }
}

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

/// The channels of every audio bus each way, and whether there is an event input.
struct Buses {
    inputs: Vec<usize>,
    outputs: Vec<usize>,
    takes_notes: bool,
}

/// Asks for stereo on the first audio input and output, reads what the plugin has afterwards,
/// and activates the buses this host plays. VST 3 allows this only while the plugin is
/// inactive.
///
/// # Safety
///
/// Both objects must be alive, and the plugin inactive.
unsafe fn prepare_buses(
    component: &ComPtr<IComponent>,
    processor: &ComPtr<IAudioProcessor>,
) -> Buses {
    // SAFETY: the caller keeps the contract.
    unsafe {
        let inputs = bus_channels(component, BusDirections_::kInput as int32);
        let outputs = bus_channels(component, BusDirections_::kOutput as int32);
        // A plugin may change its buses while it answers the arrangement, so they are read
        // again.
        arrange(processor, &inputs, &outputs);
        let inputs = bus_channels(component, BusDirections_::kInput as int32);
        let outputs = bus_channels(component, BusDirections_::kOutput as int32);

        // Notes go to the first event input only.
        component.activateBus(
            MediaTypes_::kEvent as int32,
            BusDirections_::kInput as int32,
            0,
            1,
        );
        // A plugin may ignore an inactive bus, so an effect would be silent without this.
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
        let takes_notes = component.getBusCount(
            MediaTypes_::kEvent as int32,
            BusDirections_::kInput as int32,
        ) > 0;
        Buses {
            inputs,
            outputs,
            takes_notes,
        }
    }
}

/// Every parameter a host may send (read-only ones are the plugin's to set), with its value
/// from `known`, or from the controller for a parameter not in `known`.
fn parameter_values(
    controller: &ComPtr<IEditController>,
    known: &BTreeMap<ParamID, ParamValue>,
) -> BTreeMap<ParamID, ParamValue> {
    let mut values = BTreeMap::new();
    // SAFETY: the controller came from the plugin and is alive, and `info` starts zeroed in
    // case a failed call leaves it untouched.
    unsafe {
        for index in 0..controller.getParameterCount() {
            let mut info: ParameterInfo = std::mem::zeroed();
            if controller.getParameterInfo(index, &mut info) != kResultOk {
                continue;
            }
            if info.flags & ParameterFlags_::kIsReadOnly as int32 != 0 {
                continue;
            }
            let value = match known.get(&info.id) {
                Some(value) => *value,
                None => controller.getParamNormalized(info.id),
            };
            values.insert(info.id, value);
        }
    }
    values
}

/// Every parameter whose value `now` gives differently from `known`, as an edit, with `known`
/// brought up to date. Compared bit for bit, so a NaN is sent once and not every time.
fn differences(
    known: &mut BTreeMap<ParamID, ParamValue>,
    now: impl Fn(ParamID) -> ParamValue,
) -> Vec<ParameterChange> {
    let mut changes = Vec::new();
    for (id, value) in known {
        let shown = now(*id);
        if shown.to_bits() != value.to_bits() {
            *value = shown;
            changes.push(ParameterChange {
                id: *id,
                value: shown,
            });
        }
    }
    changes
}

/// How many channels each audio bus of one direction has.
///
/// # Safety
///
/// The component must be alive.
unsafe fn bus_channels(component: &ComPtr<IComponent>, direction: int32) -> Vec<usize> {
    // SAFETY: the caller keeps the contract, and `info` starts zeroed in case a failed call
    // leaves it untouched.
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

/// Asks for stereo, which the engine carries, on the first input and output, and for the
/// channels each other bus has now.
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

/// The speaker arrangement of a channel count: an arrangement is a bit per speaker, so an
/// unnamed count is that many low bits, at most 64.
fn speakers(channels: usize) -> SpeakerArrangement {
    match channels {
        0 => 0,
        1 => SpeakerArr::kMono,
        2 => SpeakerArr::kStereo,
        count => u64::MAX >> (64 - count.min(64)),
    }
}

/// The parameter the plugin maps a VST 3 controller number to, through `IMidiMapping`.
///
/// # Safety
///
/// The controller must be alive.
unsafe fn mapped_parameter(
    controller: Option<&ComPtr<IEditController>>,
    number: usize,
) -> Option<ParamID> {
    let number = i16::try_from(number).ok()?;
    let mapping = controller?.cast::<IMidiMapping>()?;
    let mut id: ParamID = 0;
    // SAFETY: the caller keeps the contract, and `id` is written only when the call works.
    let result = unsafe { mapping.getMidiControllerAssignment(0, 0, number, &mut id) };
    (result == kResultOk).then_some(id)
}

/// One half of a plugin's state. `Ok(None)` is "no such state" (`kNotImplemented` or
/// `kResultFalse`, which a one-object plugin often says of its controller). Any other failure
/// is an error, so an empty state is never written over a good file.
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
    if result == kResultOk {
        return Ok(Some(written.written()));
    }
    if result == kNotImplemented || result == kResultFalse {
        return Ok(None);
    }
    Err(format!("{whose} answered {result} to getState"))
}

/// A state asset: the component's state and the controller's, each after a four-byte length.
/// A part above [`MAX_STATE`] could not be read back, so it is refused and the asset that is
/// there is left alone.
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

/// The two states in a state asset.
fn read_parts(bytes: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let rest = bytes
        .strip_prefix(&MAGIC)
        .ok_or("this is not a VST 3 state written by this program")?;
    let (component, rest) = take_part(rest)?;
    let (controller, _) = take_part(rest)?;
    Ok((component, controller))
}

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
    // "Not mine" is fine: a one-object plugin says it of `setComponentState`. Any other failure
    // stops the load, or the next save would write half a state over the good file.
    let took = |whose: &str, call: &str, result: int32| {
        let answered = result == kResultOk;
        let not_mine = result == kNotImplemented || result == kResultFalse;
        match answered || not_mine {
            true => Ok(()),
            false => Err(not_read(format!("{whose} answered {result} to {call}"))),
        }
    };
    // SAFETY: the caller keeps the contract, and the stream outlives every call below.
    unsafe {
        let result = not_ours(|| component.setState(stream.as_ptr()));
        if result != kResultOk {
            return Err(not_read(format!(
                "the plugin answered {result} to setState"
            )));
        }
        if let Some(controller) = controller {
            // The controller reads the component's state too, to show what it holds.
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

    #[test]
    fn a_state_too_long_to_be_read_back_is_refused_instead_of_written() {
        // Zeroed pages the system never has to fill in, because nothing writes into them.
        let long = vec![0_u8; MAX_STATE + 1];
        let error = write_state(&long, b"").expect_err("a state this long is refused");
        assert!(error.contains("left as it is"), "{error}");
        let error = write_state(b"component", &long).expect_err("a controller state is too");
        assert!(error.contains("controller"), "{error}");
    }

    #[test]
    fn a_value_that_is_not_a_number_is_sent_once() {
        let mut known = BTreeMap::from([(1, 0.5), (2, 0.25)]);
        let shown = |id| match id {
            1 => f64::NAN,
            _ => 0.25,
        };
        let first = differences(&mut known, shown);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].id, 1);
        assert!(differences(&mut known, shown).is_empty());
    }

    #[test]
    fn a_stereo_bus_is_asked_for_by_name_and_a_wide_one_by_its_channels() {
        assert_eq!(speakers(1), SpeakerArr::kMono);
        assert_eq!(speakers(2), SpeakerArr::kStereo);
        assert_eq!(speakers(4), 0b1111);
        assert_eq!(speakers(0), 0);
        assert_eq!(speakers(63), u64::MAX >> 1);
        assert_eq!(speakers(64), u64::MAX);
        assert_eq!(speakers(65), u64::MAX);
    }
}
