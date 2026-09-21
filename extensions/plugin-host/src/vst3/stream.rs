//! An `IBStream` over bytes in memory: how a plugin's state is read and written.
//!
//! VST 3 gives and takes state through a stream the host provides. Ours is a growing buffer,
//! so reading a state asset and writing one are the same object.

use std::cell::RefCell;

use vst3::Steinberg::{
    IBStream, IBStream_::IStreamSeekMode_, IBStreamTrait, int32, int64, kInvalidArgument,
    kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComWrapper};

/// A stream of bytes a plugin reads from or writes into.
pub struct MemoryStream {
    inner: RefCell<Inner>,
}

struct Inner {
    bytes: Vec<u8>,
    position: usize,
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl MemoryStream {
    /// A stream a plugin reads its saved state from.
    pub fn reading(bytes: &[u8]) -> ComWrapper<Self> {
        ComWrapper::new(Self {
            inner: RefCell::new(Inner {
                bytes: bytes.to_vec(),
                position: 0,
            }),
        })
    }

    /// An empty stream a plugin writes its state into.
    pub fn writing() -> ComWrapper<Self> {
        Self::reading(&[])
    }

    /// What the plugin wrote.
    pub fn written(&self) -> Vec<u8> {
        self.inner.borrow().bytes.clone()
    }

    /// Back to the first byte, for a second reader. The component's state also goes to the
    /// controller, which reads the same bytes from the start.
    pub fn rewind(&self) {
        self.inner.borrow_mut().position = 0;
    }
}

/// The interface pointer of a stream, for a call into a plugin.
pub fn as_stream(stream: &ComWrapper<MemoryStream>) -> Option<ComPtr<IBStream>> {
    stream.to_com_ptr()
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(
        &self,
        buffer: *mut std::ffi::c_void,
        num_bytes: int32,
        num_bytes_read: *mut int32,
    ) -> tresult {
        if buffer.is_null() || num_bytes < 0 {
            return kInvalidArgument;
        }
        let mut inner = self.inner.borrow_mut();
        let left = inner.bytes.len().saturating_sub(inner.position);
        let taken = left.min(num_bytes as usize);
        // SAFETY: the caller says `buffer` holds `num_bytes` bytes, and `taken` is no more.
        unsafe {
            std::ptr::copy_nonoverlapping(
                inner.bytes[inner.position..].as_ptr(),
                buffer.cast::<u8>(),
                taken,
            );
        }
        inner.position += taken;
        if !num_bytes_read.is_null() {
            // SAFETY: the caller gave a place to write the count.
            unsafe { *num_bytes_read = taken as int32 };
        }
        kResultOk
    }

    unsafe fn write(
        &self,
        buffer: *mut std::ffi::c_void,
        num_bytes: int32,
        num_bytes_written: *mut int32,
    ) -> tresult {
        if buffer.is_null() || num_bytes < 0 {
            return kInvalidArgument;
        }
        let count = num_bytes as usize;
        let mut inner = self.inner.borrow_mut();
        let end = inner.position + count;
        if inner.bytes.len() < end {
            inner.bytes.resize(end, 0);
        }
        let position = inner.position;
        // SAFETY: the caller says `buffer` holds `num_bytes` bytes, and the buffer has room.
        unsafe {
            std::ptr::copy_nonoverlapping(
                buffer.cast::<u8>(),
                inner.bytes[position..].as_mut_ptr(),
                count,
            );
        }
        inner.position = end;
        if !num_bytes_written.is_null() {
            // SAFETY: the caller gave a place to write the count.
            unsafe { *num_bytes_written = num_bytes };
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let mut inner = self.inner.borrow_mut();
        let length = inner.bytes.len() as int64;
        let from = match mode as u32 {
            IStreamSeekMode_::kIBSeekSet => 0,
            IStreamSeekMode_::kIBSeekCur => inner.position as int64,
            IStreamSeekMode_::kIBSeekEnd => length,
            _ => return kInvalidArgument,
        };
        let wanted = from.saturating_add(pos).clamp(0, length);
        inner.position = wanted as usize;
        if !result.is_null() {
            // SAFETY: the caller gave a place to write the position.
            unsafe { *result = wanted };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if pos.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gave a place to write the position.
        unsafe { *pos = self.inner.borrow().position as int64 };
        kResultOk
    }
}
