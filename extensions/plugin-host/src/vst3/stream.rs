//! An `IBStream` over bytes in memory: how a plugin's state is read and written.
//!
//! VST 3 gives and takes state through a stream the host provides. Ours is a growing buffer,
//! so reading a state asset and writing one are the same object.
//!
//! What every call does is Steinberg's own `MemoryStream`
//! (`vst3_public_sdk/source/common/memorystream.cpp`), which is the stream plugins are written
//! against, and this file follows it call for call:
//!
//! - `seek` may go past the end. The SDK only clamps a stream over memory it does not own, and
//!   ours always owns its bytes. A plugin that leaves room for a header, writes its payload and
//!   then seeks back to fill the header in is doing something the format allows, and a stream
//!   that clamped the seek would put its payload at byte zero for the header to overwrite.
//! - `write` past the end grows the buffer, so the gap a seek left is really there. The SDK
//!   leaves those bytes as whatever the allocation held; ours are zeros, which is the same
//!   thing for a plugin that fills them in and a better one for a plugin that does not.
//! - `read` past the end gives nothing and leaves the position at the end, as the SDK does.
//! - All four answer `kResultTrue`, and a null pointer where the SDK would dereference one is
//!   `kInvalidArgument` instead, which is what the SDK itself does where it checks at all.

use std::cell::RefCell;

use vst3::Steinberg::{
    IBStream, IBStream_::IStreamSeekMode_, IBStreamTrait, int32, int64, kInvalidArgument,
    kOutOfMemory, kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComWrapper};

use super::MAX_STATE;

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
        // A position past the end comes back to it, as the SDK's own stream does.
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
        // A write after a seek past the end grows the buffer, with zeros where the seek left a
        // gap. Nothing this host writes or reads is anywhere near [`MAX_STATE`], so a length
        // above it is a plugin that asked for more than a state can be, and the SDK's own
        // answer to an allocation it cannot make is what it gets.
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
        let from = match mode as u32 {
            IStreamSeekMode_::kIBSeekSet => 0,
            IStreamSeekMode_::kIBSeekCur => inner.position as int64,
            IStreamSeekMode_::kIBSeekEnd => length,
            _ => return kInvalidArgument,
        };
        // Past the end is allowed and a later write fills the gap, which is what the SDK's own
        // stream does and what a plugin that writes its header last needs. Before the first
        // byte is not: the SDK reads such a position back out of its own buffer, which is
        // nothing a host may do, and no plugin asks for it.
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

    /// The calls as a plugin makes them. Each one is `unsafe` in the API and safe here: every
    /// pointer is to something on this stack that outlives the call.
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

    /// What a plugin that leaves room for a header does: seek past the end, write the payload,
    /// come back and fill the header in. A stream that clamped the seek would write the payload
    /// at byte zero and then overwrite it with the header, which is a state that is not the
    /// plugin's.
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

    /// The gap a seek leaves is really there, and it is zeros.
    #[test]
    fn a_write_past_the_end_grows_the_stream_and_leaves_zeros_behind_it() {
        let held = MemoryStream::writing();
        let stream = &*held;
        assert_eq!(seek(stream, 3), 3);
        assert_eq!(write(stream, b"x"), kResultOk);
        assert_eq!(stream.written(), b"\0\0\0x");
    }

    /// Reading past the end gives nothing and comes back to the end, as Steinberg's own stream
    /// does. Before the first byte is refused instead of read out of somebody else's memory.
    #[test]
    fn reading_past_the_end_gives_nothing_and_a_seek_before_the_start_is_refused() {
        let held = MemoryStream::reading(b"abc");
        let stream = &*held;
        assert_eq!(seek(stream, 8), 8);
        assert_eq!(read(stream, 4), b"");
        // The position came back to the end, so the next read is from there.
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

    /// A plugin that asks for more than a state can be gets the answer the format has for an
    /// allocation a host cannot make, and this process allocates nothing.
    #[test]
    fn a_write_of_more_than_a_state_can_hold_is_refused_and_allocates_nothing() {
        let held = MemoryStream::writing();
        let stream = &*held;
        assert_eq!(seek(stream, MAX_STATE as int64), MAX_STATE as int64);
        assert_eq!(write(stream, b"x"), kOutOfMemory);
        assert!(stream.written().is_empty());
    }
}
