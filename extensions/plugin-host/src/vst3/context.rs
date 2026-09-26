//! What the host is, from the plugin's side: the application it runs in, the messages its two
//! halves send each other, and the handler its controller reports changes to.
//!
//! A VST 3 plugin is often two objects, a component and a controller, that talk through
//! `IConnectionPoint` with `IMessage` objects the host makes. A plugin asks for one through
//! `IHostApplication::createInstance`, so a host that answers `kNotImplemented` there breaks
//! every plugin whose halves talk. These are the smallest objects that answer.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use vst3::Steinberg::Vst::{
    IAttributeList, IAttributeList_iid, IAttributeListTrait, IComponentHandler,
    IComponentHandlerTrait, IHostApplication, IHostApplicationTrait, IMessage, IMessage_iid,
    IMessageTrait, ParamID, ParamValue, RestartFlags, RestartFlags_, String128, TChar,
};
use vst3::Steinberg::{
    FUnknown, TUID, int32, int64, kInvalidArgument, kNoInterface, kNotImplemented, kResultOk,
    tresult, uint32,
};
use vst3::{Class, ComPtr, ComWrapper};

use super::process::ParameterChange;
use crate::host::HOST_NAME;

/// The application the plugin runs in. A plugin gets it as the context of `initialize`.
pub struct HostContext;

impl Class for HostContext {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostContext {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave a buffer of 128 UTF-16 units.
        unsafe { write_utf16(HOST_NAME, &mut *name) };
        kResultOk
    }

    /// The only object a plugin may ask a host to make is a message, with its attribute list.
    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave three valid pointers.
        unsafe {
            let wanted = *cid;
            if wanted != IMessage_iid && wanted != IAttributeList_iid {
                return kNoInterface;
            }
            if *iid == IMessage_iid {
                let message = ComWrapper::new(HostMessage::default());
                let Some(pointer) = message.to_com_ptr::<IMessage>() else {
                    return kNoInterface;
                };
                *obj = pointer.into_raw().cast();
                return kResultOk;
            }
            if *iid == IAttributeList_iid {
                let attributes = ComWrapper::new(HostAttributes::default());
                let Some(pointer) = attributes.to_com_ptr::<IAttributeList>() else {
                    return kNoInterface;
                };
                *obj = pointer.into_raw().cast();
                return kResultOk;
            }
            kNoInterface
        }
    }
}

/// The interface pointer of the host context, for `initialize`.
pub fn as_unknown(context: &ComWrapper<HostContext>) -> Option<ComPtr<FUnknown>> {
    context.to_com_ptr()
}

/// One value in a message between a plugin's two halves.
enum Attribute {
    Int(int64),
    Float(f64),
    Text(Vec<TChar>),
    Bytes(Vec<u8>),
}

/// A message the two halves of a plugin pass each other. Nothing here is read by the host: it
/// only has to hold what was put in it and give it back.
///
/// The two halves of a plugin are wired to each other, so `notify` arrives on whatever thread
/// the half that sent it was on, and a message may be kept and read on another. Hence a lock
/// and not a cell: nothing of this is on the audio thread, and a cell that was borrowed twice
/// would end the process from inside a call of the plugin's.
pub struct HostMessage {
    identifier: Mutex<Option<CString>>,
    attributes: ComWrapper<HostAttributes>,
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl Default for HostMessage {
    fn default() -> Self {
        Self {
            identifier: Mutex::new(None),
            attributes: ComWrapper::new(HostAttributes::default()),
        }
    }
}

/// The values of one message, by name. Locked, for the reason [`HostMessage`] gives.
#[derive(Default)]
pub struct HostAttributes {
    values: Mutex<BTreeMap<CString, Attribute>>,
}

impl Class for HostAttributes {
    type Interfaces = (IAttributeList,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> *const std::ffi::c_char {
        // The plugin reads this until it sets another id or the message goes, and the message
        // owns the string, so the pointer stays valid for exactly as long as it may be read.
        match self.identifier.lock() {
            Ok(held) => match &*held {
                Some(identifier) => identifier.as_ptr(),
                None => std::ptr::null(),
            },
            Err(_) => std::ptr::null(),
        }
    }

    unsafe fn setMessageID(&self, id: *const std::ffi::c_char) {
        // SAFETY: the caller gives a C string or nothing.
        let identifier = (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned());
        if let Ok(mut held) = self.identifier.lock() {
            *held = identifier;
        }
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // The caller does not own this reference, which is what `IMessage::getAttributes`
        // says, so nothing is added here. The list lives as long as the message.
        match self.attributes.as_com_ref::<IAttributeList>() {
            Some(list) => list.as_ptr(),
            None => std::ptr::null_mut(),
        }
    }
}

impl HostAttributes {
    fn key(id: *const std::ffi::c_char) -> Option<CString> {
        // SAFETY: the caller gives a C string.
        (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned())
    }

    /// The values, whatever happened to whoever held the lock before.
    fn values(&self) -> std::sync::MutexGuard<'_, BTreeMap<CString, Attribute>> {
        self.values.lock().unwrap_or_else(|held| held.into_inner())
    }
}

impl IAttributeListTrait for HostAttributes {
    unsafe fn setInt(&self, id: *const std::ffi::c_char, value: int64) -> tresult {
        let Some(key) = Self::key(id) else {
            return kInvalidArgument;
        };
        self.values().insert(key, Attribute::Int(value));
        kResultOk
    }

    unsafe fn getInt(&self, id: *const std::ffi::c_char, value: *mut int64) -> tresult {
        let (Some(key), false) = (Self::key(id), value.is_null()) else {
            return kInvalidArgument;
        };
        match self.values().get(&key) {
            // SAFETY: the caller gave a place to write.
            Some(Attribute::Int(found)) => unsafe {
                *value = *found;
                kResultOk
            },
            _ => kNotImplemented,
        }
    }

    unsafe fn setFloat(&self, id: *const std::ffi::c_char, value: f64) -> tresult {
        let Some(key) = Self::key(id) else {
            return kInvalidArgument;
        };
        self.values().insert(key, Attribute::Float(value));
        kResultOk
    }

    unsafe fn getFloat(&self, id: *const std::ffi::c_char, value: *mut f64) -> tresult {
        let (Some(key), false) = (Self::key(id), value.is_null()) else {
            return kInvalidArgument;
        };
        match self.values().get(&key) {
            // SAFETY: the caller gave a place to write.
            Some(Attribute::Float(found)) => unsafe {
                *value = *found;
                kResultOk
            },
            _ => kNotImplemented,
        }
    }

    unsafe fn setString(&self, id: *const std::ffi::c_char, string: *const TChar) -> tresult {
        let (Some(key), false) = (Self::key(id), string.is_null()) else {
            return kInvalidArgument;
        };
        // SAFETY: the caller gives a zero-terminated UTF-16 string.
        let text = unsafe { read_utf16(string) };
        self.values().insert(key, Attribute::Text(text));
        kResultOk
    }

    unsafe fn getString(
        &self,
        id: *const std::ffi::c_char,
        string: *mut TChar,
        size_in_bytes: uint32,
    ) -> tresult {
        let (Some(key), false) = (Self::key(id), string.is_null()) else {
            return kInvalidArgument;
        };
        let values = self.values();
        let Some(Attribute::Text(text)) = values.get(&key) else {
            return kNotImplemented;
        };
        let units = (size_in_bytes as usize) / std::mem::size_of::<TChar>();
        if units == 0 {
            return kInvalidArgument;
        }
        // SAFETY: the caller says the buffer holds `units` UTF-16 units, and one is kept for
        // the zero that ends the string.
        unsafe {
            let taken = text.len().min(units - 1);
            std::ptr::copy_nonoverlapping(text.as_ptr(), string, taken);
            *string.add(taken) = 0;
        }
        kResultOk
    }

    unsafe fn setBinary(
        &self,
        id: *const std::ffi::c_char,
        data: *const c_void,
        size_in_bytes: uint32,
    ) -> tresult {
        let (Some(key), false) = (Self::key(id), data.is_null()) else {
            return kInvalidArgument;
        };
        // SAFETY: the caller says `data` holds `size_in_bytes` bytes.
        let bytes =
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size_in_bytes as usize) };
        self.values().insert(key, Attribute::Bytes(bytes.to_vec()));
        kResultOk
    }

    unsafe fn getBinary(
        &self,
        id: *const std::ffi::c_char,
        data: *mut *const c_void,
        size_in_bytes: *mut uint32,
    ) -> tresult {
        let (Some(key), false, false) = (Self::key(id), data.is_null(), size_in_bytes.is_null())
        else {
            return kInvalidArgument;
        };
        let values = self.values();
        let Some(Attribute::Bytes(bytes)) = values.get(&key) else {
            return kNotImplemented;
        };
        // SAFETY: the caller gave two places to write. The bytes belong to this message, and
        // `IAttributeList::getBinary` lends them for as long as the message lives.
        unsafe {
            *data = bytes.as_ptr().cast();
            *size_in_bytes = bytes.len() as uint32;
        }
        kResultOk
    }
}

/// What the plugin's controller tells the host: a parameter the composer changed in the
/// plugin's own window, and what `restartComponent` asks for.
///
/// `ivsteditcontroller.h` says what this interface is for: "Allow transfer of parameter editing
/// to component (processor) via host and support automation." So an edit is two things here. It
/// marks the state to be saved, because VST 3 has no `mark_dirty`. And it has to reach the
/// plugin's processor, because the controller only knows the value and the processor is what
/// makes the sound: a plugin whose composer turns a knob in its own window hears nothing until
/// the host carries the edit across. See `plugin.rs` for the way over and `process.rs` for the
/// block that carries it.
///
/// Every call here is `[UI-thread]` by the format, which is the thread the host lives on. The
/// lock is for a plugin that does not keep to that: nothing of this is on the audio thread, and
/// a cell borrowed twice would end the process from inside a call of the plugin's.
#[derive(Default)]
pub struct Handler {
    state_is_dirty: AtomicBool,
    /// The plugin asked to be deactivated and activated again: its latency or its buses changed.
    restart_wanted: AtomicBool,
    /// The plugin asked to be unloaded and loaded again.
    reload_wanted: AtomicBool,
    /// The plugin changed the values of its parameters as a whole, as a preset it loaded itself
    /// does, so the processor may no longer hold what the controller shows.
    values_changed: AtomicBool,
    /// The plugin moved its MIDI controller mapping, which is where the sustain pedal goes.
    midi_mapping_changed: AtomicBool,
    /// How many edits are open (`beginEdit` without `endEdit`). Only for the log of the test
    /// plugin and to keep the pair balanced; nothing of the host depends on it.
    open_edits: AtomicI32,
    /// The newest value of every parameter the controller has edited and the processor has not
    /// been given yet. By parameter, so the value a composer left a knob on is never the one
    /// that is dropped: a knob drag is hundreds of edits of one parameter and only the last of
    /// them is the sound.
    edits: Mutex<BTreeMap<ParamID, ParamValue>>,
}

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl Handler {
    /// Whether a parameter changed since the last call.
    pub fn take_state_is_dirty(&self) -> bool {
        self.state_is_dirty.swap(false, Ordering::AcqRel)
    }

    /// The state changed for another reason than an edit of the plugin's own window, such as a
    /// parameter the plugin itself moved while it played.
    pub fn mark_dirty(&self) {
        self.state_is_dirty.store(true, Ordering::Release);
    }

    /// Whether the plugin asked to be deactivated and activated again since the last call.
    pub fn take_restart_wanted(&self) -> bool {
        self.restart_wanted.swap(false, Ordering::AcqRel)
    }

    /// Whether the plugin asked to be unloaded and loaded again since the last call.
    pub fn take_reload_wanted(&self) -> bool {
        self.reload_wanted.swap(false, Ordering::AcqRel)
    }

    /// Whether the plugin said its parameter values changed as a whole since the last call.
    pub fn take_values_changed(&self) -> bool {
        self.values_changed.swap(false, Ordering::AcqRel)
    }

    /// Whether the plugin has moved its MIDI controller mapping since the last call.
    pub fn take_midi_mapping_changed(&self) -> bool {
        self.midi_mapping_changed.swap(false, Ordering::AcqRel)
    }

    /// Every parameter edit that is waiting for the processor, newest value each, and nothing
    /// left behind. The caller is on the host's thread and gives back what would not fit.
    pub fn take_edits(&self) -> Vec<ParameterChange> {
        let mut held = self.edits.lock().unwrap_or_else(|held| held.into_inner());
        std::mem::take(&mut *held)
            .into_iter()
            .map(|(id, value)| ParameterChange { id, value })
            .collect()
    }

    /// Puts an edit back because the processor had no room for it, or never played it. A
    /// newer edit of the same parameter, which the plugin may have made in between, is left as
    /// it is: it is the one the composer means.
    pub fn keep_edit(&self, change: ParameterChange) {
        let mut held = self.edits.lock().unwrap_or_else(|held| held.into_inner());
        held.entry(change.id).or_insert(change.value);
    }
}

/// The `restartComponent` flags whose text in `ivsteditcontroller.h` asks the host to
/// deactivate the plugin, read what changed and activate it again. The host does exactly
/// that, and makes the audio side again from what the plugin says afterwards.
///
/// - `kLatencyChanged`: "The host has to deactivate and reactivate the plug-in, then
///   afterwards the host could ask for the current latency".
/// - `kIoChanged`: "The host has to deactivate the plug-in, asks the plug-in for its wanted
///   new bus configurations, adapts its processing graph and reactivate the plug-in."
const RESTART: RestartFlags = RestartFlags_::kLatencyChanged | RestartFlags_::kIoChanged;

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        self.open_edits.fetch_add(1, Ordering::AcqRel);
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value_normalized: ParamValue) -> tresult {
        // The parameter belongs to the plugin, not to the project: it is saved in the plugin's
        // own state asset and is never an undo step.
        self.state_is_dirty.store(true, Ordering::Release);
        // And it has to reach the processor, which is the half that makes the sound. The host
        // carries it in the parameter changes of the next block, see the type documentation.
        let mut held = self.edits.lock().unwrap_or_else(|held| held.into_inner());
        held.insert(id, value_normalized);
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        self.open_edits.fetch_sub(1, Ordering::AcqRel);
        kResultOk
    }

    /// Every flag of `ivsteditcontroller.h` is noted here and acted on at the next poll, on
    /// the host's thread. ARCHITECTURE.md holds the table of what each one gets.
    ///
    /// - [`RESTART`]: deactivate, read the latency and the buses again, activate.
    /// - `kReloadComponent`, "The host has to unload completely the plug-in (controller/
    ///   processor) and reload it": the plugin is saved and its record is loaded again.
    /// - `kParamValuesChanged`, "The host invalidates all caches of parameter values and asks
    ///   the edit controller for the current values": the values the host last gave the
    ///   processor are compared with the controller's, and what differs is sent. The state is
    ///   saved as well, because it changed.
    /// - `kMidiCCAssignmentChanged`, "The host has to rebuild the MIDI-CC => parameter
    ///   mapping": the sustain pedal's parameter is looked up again.
    /// - Nothing for the rest. `kPrefetchableSupportChanged` asks for a deactivate so that the
    ///   host can read `getPrefetchableSupport`, and this host never processes in prefetch mode.
    ///   `kParamTitlesChanged`, `kNoteExpressionChanged`, `kIoTitlesChanged`,
    ///   `kRoutingInfoChanged`, `kKeyswitchChanged` and `kParamIDMappingChanged` are each about
    ///   a cache a host with a parameter view, a note expression display, bus titles, a routing
    ///   view, a key switch display or parameter automation would drop. This host keeps none of
    ///   those. Splice INSTRUMENT sends `kParamTitlesChanged` and `kParamIDMappingChanged`
    ///   while a composer opens its window.
    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        let asks = |flag: RestartFlags| flags & flag != 0;
        if asks(RestartFlags_::kParamValuesChanged) {
            self.values_changed.store(true, Ordering::Release);
            self.state_is_dirty.store(true, Ordering::Release);
        }
        if asks(RestartFlags_::kMidiCCAssignmentChanged) {
            self.midi_mapping_changed.store(true, Ordering::Release);
        }
        if flags & RESTART != 0 {
            self.restart_wanted.store(true, Ordering::Release);
        }
        if asks(RestartFlags_::kReloadComponent) {
            self.reload_wanted.store(true, Ordering::Release);
        }
        kResultOk
    }
}

/// The interface pointer of the handler, for `setComponentHandler`.
pub fn as_handler(handler: &ComWrapper<Handler>) -> Option<ComPtr<IComponentHandler>> {
    handler.to_com_ptr()
}

/// Writes `text` into a fixed UTF-16 buffer of the VST 3 API, with the zero that ends it.
fn write_utf16(text: &str, buffer: &mut [TChar]) {
    let mut written = 0;
    for (unit, place) in text.encode_utf16().zip(buffer.iter_mut()) {
        *place = unit;
        written += 1;
    }
    match buffer.get_mut(written) {
        Some(place) => *place = 0,
        None => {
            if let Some(last) = buffer.last_mut() {
                *last = 0;
            }
        }
    }
}

/// Reads a zero-terminated UTF-16 string a plugin gave us, without the zero.
///
/// # Safety
///
/// `string` must be a zero-terminated UTF-16 string.
unsafe fn read_utf16(string: *const TChar) -> Vec<TChar> {
    let mut units = Vec::new();
    // SAFETY: the caller keeps the contract, so the zero is reached.
    unsafe {
        let mut at = string;
        while *at != 0 {
            units.push(*at);
            at = at.add(1);
        }
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A knob drag is hundreds of edits of one parameter and only the last of them is the
    /// sound the composer left behind, so that is the one the processor is given.
    #[test]
    fn many_edits_of_one_parameter_come_out_as_the_last_value() {
        let handler = Handler::default();
        for value in [1.0, 0.75, 0.5, 0.25] {
            // SAFETY: the call reads nothing but its arguments.
            unsafe { handler.performEdit(7, value) };
        }
        let edits = handler.take_edits();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].id, 7);
        assert!((edits[0].value - 0.25).abs() < f64::EPSILON);
        // And nothing is left behind for the next poll.
        assert!(handler.take_edits().is_empty());
    }

    /// An edit the processor had no room for comes back, and waits. A newer edit of the same
    /// parameter, made while it was away, is the one the composer means and stays.
    #[test]
    fn an_edit_the_processor_had_no_room_for_waits_and_never_overwrites_a_newer_one() {
        let handler = Handler::default();
        // SAFETY: the call reads nothing but its arguments.
        unsafe { handler.performEdit(7, 1.0) };
        let edits = handler.take_edits();
        // The processor was full, and the plugin moved the same knob again in the meantime.
        // SAFETY: as above.
        unsafe { handler.performEdit(7, 0.5) };
        for edit in edits {
            handler.keep_edit(edit);
        }
        let waiting = handler.take_edits();
        assert_eq!(waiting.len(), 1);
        assert!((waiting[0].value - 0.5).abs() < f64::EPSILON);

        // A parameter nothing newer touched comes back as it was.
        // SAFETY: as above.
        unsafe { handler.performEdit(9, 0.25) };
        let edits = handler.take_edits();
        for edit in edits {
            handler.keep_edit(edit);
        }
        let waiting = handler.take_edits();
        assert_eq!(waiting.len(), 1);
        assert!((waiting[0].value - 0.25).abs() < f64::EPSILON);
    }

    /// What the handler noted for one call of `restartComponent`, taken the way a poll takes
    /// it: restart, reload, values changed, state to be saved, pedal mapping moved.
    fn taken(handler: &Handler) -> [bool; 5] {
        [
            handler.take_restart_wanted(),
            handler.take_reload_wanted(),
            handler.take_values_changed(),
            handler.take_state_is_dirty(),
            handler.take_midi_mapping_changed(),
        ]
    }

    /// What Splice INSTRUMENT really sends: the values changed, the titles and the id mapping,
    /// and a `restartComponent(0)` that asks for nothing. Only the values are acted on.
    #[test]
    fn the_flags_splice_instrument_sends_only_ask_for_the_values_again() {
        let handler = Handler::default();
        // SAFETY: a plain call of a method that touches nothing but this handler.
        unsafe {
            handler.restartComponent(RestartFlags_::kParamTitlesChanged);
            handler.restartComponent(
                RestartFlags_::kParamIDMappingChanged | RestartFlags_::kParamValuesChanged,
            );
            handler.restartComponent(0);
        }
        assert_eq!(taken(&handler), [false, false, true, true, false]);
    }

    /// Every flag of the format, each with what ARCHITECTURE.md says this host does about it.
    /// Nothing is left out: the twelve below are the whole `RestartFlags` enum of
    /// `ivsteditcontroller.h`.
    #[test]
    fn every_restart_flag_is_noted_as_the_table_says() {
        // Restart, reload, values changed, state to be saved, pedal mapping moved.
        let nothing = [false; 5];
        let flags: [(RestartFlags, [bool; 5]); 12] = [
            (
                RestartFlags_::kReloadComponent,
                [false, true, false, false, false],
            ),
            (
                RestartFlags_::kIoChanged,
                [true, false, false, false, false],
            ),
            (
                RestartFlags_::kLatencyChanged,
                [true, false, false, false, false],
            ),
            (
                RestartFlags_::kParamValuesChanged,
                [false, false, true, true, false],
            ),
            (
                RestartFlags_::kMidiCCAssignmentChanged,
                [false, false, false, false, true],
            ),
            (RestartFlags_::kPrefetchableSupportChanged, nothing),
            (RestartFlags_::kParamTitlesChanged, nothing),
            (RestartFlags_::kNoteExpressionChanged, nothing),
            (RestartFlags_::kIoTitlesChanged, nothing),
            (RestartFlags_::kRoutingInfoChanged, nothing),
            (RestartFlags_::kKeyswitchChanged, nothing),
            (RestartFlags_::kParamIDMappingChanged, nothing),
        ];
        let mut seen = 0_i32;
        for (flag, noted) in flags {
            assert_eq!(seen & flag, 0, "flag {flag} is in the list twice");
            seen |= flag;
            let handler = Handler::default();
            // SAFETY: as above.
            unsafe { handler.restartComponent(flag) };
            assert_eq!(taken(&handler), noted, "flag {flag}");
        }
        // The whole enum, 1 to 1 << 11, and nothing else is in it.
        assert_eq!(seen, (1 << 12) - 1);
    }

    /// One flag of a combination is enough, whichever it is.
    #[test]
    fn a_combination_is_read_flag_by_flag() {
        let handler = Handler::default();
        let flags = RestartFlags_::kParamTitlesChanged
            | RestartFlags_::kIoChanged
            | RestartFlags_::kReloadComponent
            | RestartFlags_::kParamValuesChanged;
        // SAFETY: as above.
        unsafe { handler.restartComponent(flags) };
        assert_eq!(taken(&handler), [true, true, true, true, false]);
    }

    #[test]
    fn a_name_that_is_longer_than_the_buffer_still_ends_with_a_zero() {
        let mut buffer = [1_u16; 4];
        write_utf16("abcdef", &mut buffer);
        assert_eq!(buffer, [b'a' as u16, b'b' as u16, b'c' as u16, 0]);
    }

    #[test]
    fn a_name_that_fits_is_written_whole() {
        let mut buffer = [1_u16; 8];
        write_utf16("hi", &mut buffer);
        assert_eq!(&buffer[..3], &[b'h' as u16, b'i' as u16, 0]);
    }
}
