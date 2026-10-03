//! Streamed files: large samples that play from disk instead of from memory.
//!
//! The start of the file is held in memory, [`HEAD_SECONDS`] of it, and the whole file is
//! mapped, so the system reads the rest from disk as it plays and lets go of it again under
//! memory pressure. A note must not wait for the disk on the audio thread, so a thread of its
//! own reads ahead: a processor asks it with [`ReadAhead::ask`] when a note starts, and by the
//! time the note is past the start in memory, the rest is in memory too.
//!
//! A FLAC file cannot be mapped as plain samples, so it is decoded once into a WAV file next
//! to it, `<name>.flac.wav`, which is mapped instead and decoded again only when the FLAC file
//! is newer.
//!
//! Only files nothing writes over in place may be mapped: a mapped file that is cut short ends
//! the process when the missing part is read. The decoded WAV is written under another name and
//! renamed into place, which leaves a map of the old one as it was.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use crate::file::{Audio, Encoding, FormatError, is_flac};

/// How much of a streamed file is held in memory. Far longer than the thread that reads ahead
/// takes to have the next part read, a few milliseconds from an SSD.
pub const HEAD_SECONDS: f64 = 0.25;

/// What went wrong opening a file to stream it.
pub(crate) enum StreamError {
    Io(io::Error),
    Format(FormatError),
}

/// Opens `path` to stream: its start in memory and the rest mapped. A short file is read whole.
pub(crate) fn open(path: &Path) -> Result<Audio, StreamError> {
    let mut start = [0; 4];
    let read = {
        use std::io::Read;
        fs::File::open(path).and_then(|mut file| file.read_exact(&mut start))
    };
    let mapped = match read {
        Ok(()) if is_flac(&start) => decoded(path)?,
        Ok(()) | Err(_) => path.to_path_buf(),
    };
    let file = fs::File::open(&mapped).map_err(StreamError::Io)?;
    // SAFETY: the map is only read, and the files streamed are the instrument library's and
    // the WAV files decoded from it, which nothing writes over in place (see the module).
    let map = unsafe { memmap2::Mmap::map(&file) }.map_err(StreamError::Io)?;
    let audio = Audio::mapped(map, HEAD_SECONDS).map_err(StreamError::Format)?;
    Ok(match audio.is_streamed() {
        true => audio.with_stream_number(NEXT.fetch_add(1, Ordering::Relaxed)),
        false => audio,
    })
}

/// The WAV file decoded from the FLAC file at `path`, decoded now when it is not there or older.
fn decoded(path: &Path) -> Result<PathBuf, StreamError> {
    let name = path.file_name().map(|name| name.to_string_lossy());
    let name = name.unwrap_or_default();
    let target = path.with_file_name(format!("{name}.wav"));
    let modified = |path: &Path| fs::metadata(path).and_then(|metadata| metadata.modified());
    if let (Ok(decoded), Ok(source)) = (modified(&target), modified(path))
        && decoded >= source
    {
        return Ok(target);
    }
    let bytes = fs::read(path).map_err(StreamError::Io)?;
    let audio = Audio::decode_flac(&bytes).map_err(StreamError::Format)?;
    let wav = wav_bytes(&audio).ok_or_else(|| {
        StreamError::Format(FormatError::Unsupported(
            "FLAC of 8-bit samples".to_string(),
        ))
    })?;
    let temporary = path.with_file_name(format!(".{name}.{}.decoding", std::process::id()));
    let written = fs::write(&temporary, wav).and_then(|()| fs::rename(&temporary, &target));
    if let Err(error) = written {
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            // The failure that brought us here is the one to report.
            Err(_) => {}
        }
        return Err(StreamError::Io(error));
    }
    Ok(target)
}

/// A WAV file of the decoded samples of a FLAC file. `None` for 8-bit samples, which WAV holds
/// unsigned, unlike FLAC.
fn wav_bytes(audio: &Audio) -> Option<Vec<u8>> {
    let size: u16 = match audio.encoding() {
        Encoding::I16Le => 2,
        Encoding::I24Le => 3,
        Encoding::I32Le => 4,
        _ => return None,
    };
    let samples = audio.file_bytes();
    let channels = audio.channels();
    let rate = audio.sample_rate();
    let block = size * channels;
    let length = u32::try_from(samples.len()).ok()?;
    let mut wav = Vec::with_capacity(44 + samples.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + length).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    // Plain samples.
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    wav.extend_from_slice(&block.to_le_bytes());
    wav.extend_from_slice(&(size * 8).to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&length.to_le_bytes());
    wav.extend_from_slice(samples);
    Some(wav)
}

/// Every streamed file by a number, so the audio thread names one without holding it.
static STREAMED: LazyLock<Mutex<HashMap<u64, Weak<Audio>>>> = LazyLock::new(Mutex::default);
static NEXT: AtomicU64 = AtomicU64::new(1);

fn streamed() -> MutexGuard<'static, HashMap<u64, Weak<Audio>>> {
    STREAMED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lets the thread that reads ahead find a streamed file by its number.
pub(crate) fn share(audio: &Arc<Audio>) {
    if let Some(number) = audio.stream_number() {
        let mut streamed = streamed();
        streamed.retain(|_, audio| audio.strong_count() > 0);
        streamed.insert(number, Arc::downgrade(audio));
    }
}

/// Requests from the audio thread: a streamed file by its number and the frame a note starts.
type Request = (u64, u64);

/// How many requests one processor may have waiting. More are dropped and counted: their
/// notes then read from the disk as they play.
const WAITING: usize = 256;

/// One processor's way to ask for a streamed file to be read ahead. Made on the control side,
/// with the processor.
pub struct ReadAhead {
    requests: rtrb::Producer<Request>,
}

static QUEUES: LazyLock<Mutex<Vec<rtrb::Consumer<Request>>>> = LazyLock::new(Mutex::default);

impl ReadAhead {
    pub fn new() -> Self {
        let (requests, consumer) = rtrb::RingBuffer::new(WAITING);
        QUEUES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(consumer);
        start_reader();
        Self { requests }
    }

    /// Asks for `audio` to be read from frame `from` on. Nothing for a file in memory.
    ///
    /// Realtime safe: one push into a queue of its own, no allocation, lock or system call.
    pub fn ask(&mut self, audio: &Audio, from: u64) {
        let Some(number) = audio.stream_number() else {
            return;
        };
        match self.requests.push((number, from)) {
            Ok(()) => {}
            // Full: the note reads from the disk as it plays.
            Err(_) => {}
        }
    }
}

impl Default for ReadAhead {
    fn default() -> Self {
        Self::new()
    }
}

/// Starts the thread that reads ahead, once.
fn start_reader() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("sample read-ahead".into())
            .spawn(read_ahead);
        // Without it, streamed notes read from the disk as they play.
        if let Err(error) = spawned {
            eprintln!("sound-media: the thread that reads samples ahead did not start: {error}");
        }
    });
}

/// Takes the requests of every processor and reads what they ask for. The audio thread cannot
/// wake a thread without a system call, so this one looks every millisecond.
fn read_ahead() {
    let mut requests = Vec::new();
    loop {
        {
            let mut queues = QUEUES.lock().unwrap_or_else(PoisonError::into_inner);
            // A processor that went away leaves its queue behind.
            queues.retain(|queue| !queue.is_abandoned() || !queue.is_empty());
            for queue in queues.iter_mut() {
                while let Ok(request) = queue.pop() {
                    requests.push(request);
                }
            }
        }
        for (number, from) in requests.drain(..) {
            let audio = streamed().get(&number).and_then(Weak::upgrade);
            if let Some(audio) = audio {
                audio.read_ahead(from);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
