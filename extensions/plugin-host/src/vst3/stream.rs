//! An `IBStream` over bytes in memory, for a plugin's state.
//!
//! It follows Steinberg's own `MemoryStream` (`vst3_public_sdk/source/common/memorystream.cpp`)
//! call for call, because that is the stream plugins are written against. Where the SDK would
//! dereference a null pointer, this answers `kInvalidArgument`.

use std::cell::RefCell;

use vst3::Steinberg::{
    IBStream, IBStream_::IStreamSeekMode_, IBStreamTrait, int32, int64, kInvalidArgument,
    kOutOfMemory, kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComWrapper};

use super::MAX_STATE;

pub(super) struct MemoryStream {
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
    pub(super) fn reading(bytes: &[u8]) -> ComWrapper<Self> {
        ComWrapper::new(Self {
            inner: RefCell::new(Inner {
                bytes: bytes.to_vec(),
                position: 0,
            }),
        })
    }

    /// An empty stream a plugin writes its state into.
    pub(super) fn writing() -> ComWrapper<Self> {
        Self::reading(&[])
    }

    /// What the plugin wrote.
    pub(super) fn written(&self) -> Vec<u8> {
        self.inner.borrow().bytes.clone()
    }

    /// Back to the first byte, for a second reader.
    pub(super) fn rewind(&self) {
        self.inner.borrow_mut().position = 0;
    }
}

/// The interface pointer of a stream, for a call into a plugin.
pub(super) fn as_stream(stream: &ComWrapper<MemoryStream>) -> Option<ComPtr<IBStream>> {
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
        // A position past the end comes back to it, as in the SDK.
        inner.position = inner.position.min(inner.bytes.len());
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
        // Past the end grows the buffer, with zeros in the gap a seek left (the SDK leaves
        // whatever the allocation held). Above [`MAX_STATE`] gets the SDK's answer to an
        // allocation it cannot make.
        let Some(end) = inner
            .position
            .checked_add(count)
            .filter(|end| *end <= MAX_STATE)
        else {
            return kOutOfMemory;
        };
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
        // The bindings make this C enum unsigned on macOS and Linux and signed on Windows.
        let from = match mode {
            mode if mode == IStreamSeekMode_::kIBSeekSet as int32 => 0,
            mode if mode == IStreamSeekMode_::kIBSeekCur as int32 => inner.position as int64,
            mode if mode == IStreamSeekMode_::kIBSeekEnd as int32 => length,
            _ => return kInvalidArgument,
        };
        // Past the end is allowed, as in the SDK: a plugin may leave room for a header, write
        // its payload and seek back. Before the start is refused; the SDK would read outside
        // its buffer.
        let wanted = from.saturating_add(pos);
        if wanted < 0 {
            return kInvalidArgument;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write(stream: &MemoryStream, bytes: &[u8]) -> tresult {
        let mut written = 0;
        // SAFETY: the buffer holds the bytes the call is told it holds.
        unsafe {
            IBStreamTrait::write(
                stream,
                bytes.as_ptr().cast_mut().cast(),
                bytes.len() as int32,
                &mut written,
            )
        }
    }

    fn seek(stream: &MemoryStream, to: int64) -> int64 {
        let mut at = -1;
        // SAFETY: `at` is a place for the position.
        unsafe {
            IBStreamTrait::seek(stream, to, IStreamSeekMode_::kIBSeekSet as int32, &mut at);
        }
        at
    }

    fn read(stream: &MemoryStream, count: usize) -> Vec<u8> {
        let mut bytes = vec![0_u8; count];
        let mut taken = 0;
        // SAFETY: the buffer is as long as the call is told it is.
        unsafe {
            IBStreamTrait::read(
                stream,
                bytes.as_mut_ptr().cast(),
                count as int32,
                &mut taken,
            );
        }
        bytes.truncate(taken.max(0) as usize);
        bytes
    }

    /// A stream that clamped the seek would write the payload at byte zero and then overwrite
    /// it with the header.
    #[test]
    fn a_plugin_may_write_its_header_after_the_payload_it_left_room_for() {
        let held = MemoryStream::writing();
        let stream = &*held;
        assert_eq!(seek(stream, 4), 4);
        assert_eq!(write(stream, b"payload"), kResultOk);
        assert_eq!(seek(stream, 0), 0);
        assert_eq!(write(stream, b"HEAD"), kResultOk);
        assert_eq!(stream.written(), b"HEADpayload");
    }

    #[test]
    fn a_write_past_the_end_grows_the_stream_and_leaves_zeros_behind_it() {
        let held = MemoryStream::writing();
        let stream = &*held;
        assert_eq!(seek(stream, 3), 3);
        assert_eq!(write(stream, b"x"), kResultOk);
        assert_eq!(stream.written(), b"\0\0\0x");
    }

    #[test]
    fn reading_past_the_end_gives_nothing_and_a_seek_before_the_start_is_refused() {
        let held = MemoryStream::reading(b"abc");
        let stream = &*held;
        assert_eq!(seek(stream, 8), 8);
        assert_eq!(read(stream, 4), b"");
        let mut at = -1;
        // SAFETY: `at` is a place for the position.
        unsafe { IBStreamTrait::tell(stream, &mut at) };
        assert_eq!(at, 3);
        let mut landed = -1;
        // SAFETY: as above.
        let result = unsafe {
            IBStreamTrait::seek(
                stream,
                -1,
                IStreamSeekMode_::kIBSeekSet as int32,
                &mut landed,
            )
        };
        assert_eq!(result, kInvalidArgument);
    }

    #[test]
    fn a_write_of_more_than_a_state_can_hold_is_refused_and_allocates_nothing() {
        let held = MemoryStream::writing();
        let stream = &*held;
        assert_eq!(seek(stream, MAX_STATE as int64), MAX_STATE as int64);
        assert_eq!(write(stream, b"x"), kOutOfMemory);
        assert!(stream.written().is_empty());
    }
}
