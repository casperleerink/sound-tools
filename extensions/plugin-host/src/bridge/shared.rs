//! The memory a plugin's process shares with the host: one sub-block on its way in and out, the
//! table of pins a lane holds, and the child's heartbeat.
//!
//! Both processes are the same build (the child checks, see `child.rs`), so the layout is one
//! Rust type. The parent writes Rust values here for the child, but reads back only plain
//! numbers, each held to a bound before it is used: a plugin that writes over this memory can
//! give the parent wrong sound, never undefined behaviour and never work in proportion to a
//! number it chose.

use std::cell::UnsafeCell;
use std::ffi::CString;
use std::fs::File;
use std::mem::MaybeUninit;
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

use sound_core::{Automation, CHANNELS, MAX_BLOCK, Timed};
use sound_notes::{Amount, Bend, NoteEvent, Pedal};

use crate::processor::{AutomatedPins, Control, LanedPins, LanedTable};

/// How many events of each port one sub-block carries across. More than the engine's ports
/// hold as the runtime sets them up (256); an engine set up for more drops the rest, counted.
pub(crate) const ROOM: usize = 4096;

/// The most a sub-block can drop: every note event and every expansion of an `AllOff`, 128 note
/// offs and the four controls.
pub(crate) const MAX_DROPPED: u64 = ROOM as u64 * 132;

/// What the parent asks of the child's audio thread, one byte on the audio socket. The child
/// answers with the same byte once it is done.
pub(crate) const RUN: u8 = b'r';
pub(crate) const STOP: u8 = b's';
pub(crate) const RELEASE: u8 = b'g';

pub(crate) struct Shared {
    /// Moves while the child's main thread turns its loop, see `parent.rs`.
    pub heartbeat: AtomicU64,
    /// What the plugin was last prepared with, for the priority of the child's audio thread.
    pub sample_rate: AtomicU32,
    pub laned: LanedPins,
    /// The block, from the parent.
    pub frames: AtomicU32,
    pub note_count: AtomicU32,
    pub lane_count: AtomicU32,
    /// The next block begins a plugin (`PluginAudio::begin`), with `pins`.
    pub begin: AtomicU32,
    /// `pins` changed since the child last took them.
    pub pins_changed: AtomicU32,
    pins: UnsafeCell<AutomatedPins>,
    notes: UnsafeCell<[MaybeUninit<Timed<NoteEvent>>; ROOM]>,
    lanes: UnsafeCell<[MaybeUninit<Timed<Automation>>; ROOM]>,
    pub input: UnsafeCell<[[f32; MAX_BLOCK]; CHANNELS]>,
    pub output: UnsafeCell<[[f32; MAX_BLOCK]; CHANNELS]>,
    /// Where each control stands, by `Control::index`, as plain numbers both ways.
    pub controls: [AtomicI32; 4],
    /// The answer: whether the plugin played, and how many events did not fit.
    pub played: AtomicU32,
    pub dropped: AtomicU64,
}

// SAFETY: the cells are written by one side only while the other waits for its byte on the
// audio socket, with a fence on each side of it.
unsafe impl Sync for Shared {}

impl Shared {
    /// The parent, before a block: the events and lanes of the block, as many as fit.
    /// Gives how many did not.
    pub(crate) fn write_block(
        &self,
        notes: &[Timed<NoteEvent>],
        lanes: &[Timed<Automation>],
    ) -> u64 {
        // SAFETY: the child reads these only between the parent's byte and its own answer.
        let (into_notes, into_lanes) = unsafe { (&mut *self.notes.get(), &mut *self.lanes.get()) };
        let note_count = notes.len().min(ROOM);
        let lane_count = lanes.len().min(ROOM);
        for (place, timed) in into_notes.iter_mut().zip(notes) {
            place.write(*timed);
        }
        for (place, timed) in into_lanes.iter_mut().zip(lanes) {
            place.write(*timed);
        }
        self.note_count.store(note_count as u32, Ordering::Relaxed);
        self.lane_count.store(lane_count as u32, Ordering::Relaxed);
        (notes.len() - note_count + lanes.len() - lane_count) as u64
    }

    /// The child: the events and lanes the parent wrote for this block.
    ///
    /// # Safety
    ///
    /// Only between the parent's byte and the child's answer, after an acquire fence.
    pub(crate) unsafe fn block(&self) -> (&[Timed<NoteEvent>], &[Timed<Automation>]) {
        let note_count = (self.note_count.load(Ordering::Relaxed) as usize).min(ROOM);
        let lane_count = (self.lane_count.load(Ordering::Relaxed) as usize).min(ROOM);
        // SAFETY: the parent wrote this many, see `write_block`.
        unsafe {
            let notes = &*self.notes.get();
            let lanes = &*self.lanes.get();
            (
                std::slice::from_raw_parts(notes.as_ptr().cast(), note_count),
                std::slice::from_raw_parts(lanes.as_ptr().cast(), lane_count),
            )
        }
    }

    /// The parent, on the audio thread: the pins the child takes at its next block.
    pub(crate) fn write_pins(&self, pins: AutomatedPins, begin: bool) {
        // SAFETY: the child reads them only inside a block, which the parent is not in.
        unsafe { *self.pins.get() = pins };
        self.pins_changed.store(1, Ordering::Relaxed);
        if begin {
            self.begin.store(1, Ordering::Relaxed);
        }
    }

    /// The child, at the start of a block: whether it begins a plugin, and new pins.
    ///
    /// # Safety
    ///
    /// As [`Self::block`].
    pub(crate) unsafe fn take_pins(&self) -> (bool, Option<AutomatedPins>) {
        let begin = self.begin.swap(0, Ordering::Relaxed) == 1;
        let changed = self.pins_changed.swap(0, Ordering::Relaxed) == 1;
        // SAFETY: as above.
        let pins = changed.then(|| unsafe { *self.pins.get() });
        (begin, pins)
    }

    pub(crate) fn write_controls(&self, controls: &[Control; 4]) {
        for (place, control) in self.controls.iter().zip(controls) {
            place.store(i32::from(control_number(*control)), Ordering::Relaxed);
        }
    }

    /// The controls as the child left them. A number that is no control is the one the parent
    /// had.
    pub(crate) fn read_controls(&self, controls: &mut [Control; 4]) {
        for (index, (place, control)) in self.controls.iter().zip(controls).enumerate() {
            if let Some(read) = control_of(index, place.load(Ordering::Relaxed)) {
                *control = read;
            }
        }
    }
}

/// A control as one number: where it stands, with a bend from -8192.
fn control_number(control: Control) -> i16 {
    match control {
        Control::Pedal(pedal) => i16::from(pedal.value()),
        Control::Bend(bend) => bend.value(),
        Control::ModWheel(amount) | Control::Pressure(amount) => i16::from(amount.value()),
        Control::Other { value, .. } => i16::from(value.value()),
    }
}

fn control_of(index: usize, number: i32) -> Option<Control> {
    let small = u8::try_from(number).ok();
    match index {
        0 => Pedal::new(small?).ok().map(Control::Pedal),
        1 => Bend::new(i16::try_from(number).ok()?)
            .ok()
            .map(Control::Bend),
        2 => Amount::new(small?).ok().map(Control::ModWheel),
        3 => Amount::new(small?).ok().map(Control::Pressure),
        _ => None,
    }
}

/// The mapping, open in this process. The memory goes with the last handle.
pub(crate) struct Mapping {
    map: memmap2::MmapRaw,
}

// SAFETY: the mapping is only ever reached as `&Shared`, which is `Sync`.
unsafe impl Send for Mapping {}
// SAFETY: as above.
unsafe impl Sync for Mapping {}

impl Mapping {
    /// Makes a new one, under `name`, which the child opens and the parent then removes.
    pub(crate) fn create(name: &str) -> std::io::Result<Self> {
        Self::open_named(name, libc::O_CREAT | libc::O_EXCL | libc::O_RDWR, true)
    }

    pub(crate) fn open(name: &str) -> std::io::Result<Self> {
        Self::open_named(name, libc::O_RDWR, false)
    }

    fn open_named(name: &str, flags: libc::c_int, sized: bool) -> std::io::Result<Self> {
        let path = CString::new(name).map_err(std::io::Error::other)?;
        // SAFETY: a valid C string; the descriptor is owned by the file below at once.
        let descriptor = unsafe { libc::shm_open(path.as_ptr(), flags, 0o600 as libc::c_uint) };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: a descriptor this process just opened and owns.
        let file = unsafe { File::from_raw_fd(descriptor) };
        if sized {
            file.set_len(std::mem::size_of::<Shared>() as u64)?;
        }
        let map = memmap2::MmapOptions::new()
            .len(std::mem::size_of::<Shared>())
            .map_raw(&file)?;
        // The descriptor closes with the file: no child spawned later inherits it.
        drop(file);
        Ok(Self { map })
    }

    pub(crate) fn remove(name: &str) {
        if let Ok(path) = CString::new(name) {
            // SAFETY: a valid C string. A name that is already gone is no error worth reporting.
            unsafe { libc::shm_unlink(path.as_ptr()) };
        }
    }

    pub(crate) fn shared(&self) -> &Shared {
        // SAFETY: the mapping is as large as `Shared` and page-aligned, and zeroed memory is a
        // valid `Shared`: atomics, numbers, `false`, uninitialized slots.
        unsafe { &*self.map.as_ptr().cast::<Shared>() }
    }
}

impl LanedTable for Mapping {
    fn laned(&self) -> &LanedPins {
        &self.shared().laned
    }
}
