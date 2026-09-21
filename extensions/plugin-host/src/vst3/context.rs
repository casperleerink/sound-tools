//! What the host is, from the plugin's side: the application it runs in, the messages its two
//! halves send each other, and the handler its controller reports changes to.
//!
//! A VST 3 plugin is often two objects, a component and a controller, that talk through
//! `IConnectionPoint` with `IMessage` objects the host makes. A plugin asks for one through
//! `IHostApplication::createInstance`, so a host that answers `kNotImplemented` there breaks
//! every plugin whose halves talk. These are the smallest objects that answer.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_void};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use vst3::Steinberg::Vst::{
    IAttributeList, IAttributeList_iid, IAttributeListTrait, IComponentHandler,
    IComponentHandlerTrait, IHostApplication, IHostApplicationTrait, IMessage, IMessage_iid,
    IMessageTrait, ParamID, ParamValue, RestartFlags_, String128, TChar,
};
use vst3::Steinberg::{
    FUnknown, TUID, int32, int64, kInvalidArgument, kNoInterface, kNotImplemented, kResultOk,
    tresult, uint32,
};
use vst3::{Class, ComPtr, ComWrapper};

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
pub struct HostMessage {
    identifier: RefCell<Option<CString>>,
    attributes: ComWrapper<HostAttributes>,
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl Default for HostMessage {
    fn default() -> Self {
        Self {
            identifier: RefCell::new(None),
            attributes: ComWrapper::new(HostAttributes::default()),
        }
    }
}

/// The values of one message, by name.
#[derive(Default)]
pub struct HostAttributes {
    values: RefCell<BTreeMap<CString, Attribute>>,
}

impl Class for HostAttributes {
    type Interfaces = (IAttributeList,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> *const std::ffi::c_char {
        // The plugin reads this until it sets another id or the message goes, and the message
        // owns the string, so the pointer stays valid for exactly as long as it may be read.
        match &*self.identifier.borrow() {
            Some(identifier) => identifier.as_ptr(),
            None => std::ptr::null(),
        }
    }

    unsafe fn setMessageID(&self, id: *const std::ffi::c_char) {
        // SAFETY: the caller gives a C string or nothing.
        let identifier = (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned());
        *self.identifier.borrow_mut() = identifier;
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
}

impl IAttributeListTrait for HostAttributes {
    unsafe fn setInt(&self, id: *const std::ffi::c_char, value: int64) -> tresult {
        let Some(key) = Self::key(id) else {
            return kInvalidArgument;
        };
        self.values.borrow_mut().insert(key, Attribute::Int(value));
        kResultOk
    }

    unsafe fn getInt(&self, id: *const std::ffi::c_char, value: *mut int64) -> tresult {
        let (Some(key), false) = (Self::key(id), value.is_null()) else {
            return kInvalidArgument;
        };
        match self.values.borrow().get(&key) {
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
        self.values
            .borrow_mut()
            .insert(key, Attribute::Float(value));
        kResultOk
    }

    unsafe fn getFloat(&self, id: *const std::ffi::c_char, value: *mut f64) -> tresult {
        let (Some(key), false) = (Self::key(id), value.is_null()) else {
            return kInvalidArgument;
        };
        match self.values.borrow().get(&key) {
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
        self.values.borrow_mut().insert(key, Attribute::Text(text));
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
        let values = self.values.borrow();
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
        self.values
            .borrow_mut()
            .insert(key, Attribute::Bytes(bytes.to_vec()));
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
        let values = self.values.borrow();
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
/// plugin's own window, and a plugin that wants to be started again.
///
/// VST 3 has no `mark_dirty`. A parameter edit is the change a host learns about, so an edit
/// is what marks the state to be saved. See `plugin.rs`.
#[derive(Default)]
pub struct Handler {
    state_is_dirty: AtomicBool,
    restart_requested: AtomicBool,
    /// How many edits are open (`beginEdit` without `endEdit`). Only for the log of the test
    /// plugin and to keep the pair balanced; nothing of the host depends on it.
    open_edits: AtomicI32,
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

    pub fn take_restart_requested(&self) -> bool {
        self.restart_requested.swap(false, Ordering::AcqRel)
    }
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        self.open_edits.fetch_add(1, Ordering::AcqRel);
        kResultOk
    }

    unsafe fn performEdit(&self, _id: ParamID, _value_normalized: ParamValue) -> tresult {
        // The parameter belongs to the plugin, not to the project: it is saved in the plugin's
        // own state asset and is never an undo step.
        self.state_is_dirty.store(true, Ordering::Release);
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        self.open_edits.fetch_sub(1, Ordering::AcqRel);
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        // Values changing is not a restart: it means the state the host holds is stale.
        if flags & RestartFlags_::kParamValuesChanged as int32 != 0 {
            self.state_is_dirty.store(true, Ordering::Release);
        }
        let rest = flags & !(RestartFlags_::kParamValuesChanged as int32);
        if rest != 0 {
            self.restart_requested.store(true, Ordering::Release);
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
