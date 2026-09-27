//! WAV and AIFF files, read whole into memory and kept as their own bytes.
//!
//! A file is parsed once, on the control side: where its samples start, how they are encoded,
//! how many channels and frames it has and at what rate. The samples themselves are not
//! converted. They stay as the bytes of the file, so a file costs its size on disk and no
//! more, and [`Audio::read`] turns the frames a block needs into `f32` on the audio thread.

use std::fmt;

/// The lowest and highest sample rate of a file this reads. The top bounds the window of file
/// frames one block of the fastest resampling reads, see `crate::resample`.
pub const SAMPLE_RATES: (u32, u32) = (8_000, 384_000);

/// Why the bytes of a file are no audio this reads.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("it is not a WAV or AIFF file")]
    NotAudio,
    #[error("it is a {0} file, and only WAV and AIFF with plain samples are read")]
    Unsupported(String),
    #[error("it is damaged: {0}")]
    Damaged(&'static str),
    #[error("its sample rate is {0} Hz, and only {min} to {max} Hz is read", min = SAMPLE_RATES.0, max = SAMPLE_RATES.1)]
    SampleRate(u32),
}

/// How one sample is stored.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// 8-bit WAV: unsigned, silence at 128.
    U8,
    /// 8-bit AIFF: signed.
    I8,
    I16Le,
    I16Be,
    I24Le,
    I24Be,
    I32Le,
    I32Be,
    F32Le,
    F32Be,
    F64Le,
    F64Be,
}

impl Encoding {
    /// Bytes per sample.
    pub fn size(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::I16Le | Self::I16Be => 2,
            Self::I24Le | Self::I24Be => 3,
            Self::I32Le | Self::I32Be | Self::F32Le | Self::F32Be => 4,
            Self::F64Le | Self::F64Be => 8,
        }
    }

    /// Signed integers of this many bytes, by byte order.
    fn signed(size: usize, little_endian: bool) -> Option<Self> {
        Some(match (size, little_endian) {
            (1, _) => Self::I8,
            (2, true) => Self::I16Le,
            (2, false) => Self::I16Be,
            (3, true) => Self::I24Le,
            (3, false) => Self::I24Be,
            (4, true) => Self::I32Le,
            (4, false) => Self::I32Be,
            _ => return None,
        })
    }

    fn float(size: usize, little_endian: bool) -> Option<Self> {
        Some(match (size, little_endian) {
            (4, true) => Self::F32Le,
            (4, false) => Self::F32Be,
            (8, true) => Self::F64Le,
            (8, false) => Self::F64Be,
            _ => return None,
        })
    }
}

/// The container of a file.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Container {
    Wav,
    Aiff,
}

impl Container {
    /// The usual file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Aiff => "aiff",
        }
    }
}

impl fmt::Display for Container {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Wav => "WAV",
            Self::Aiff => "AIFF",
        })
    }
}

/// What a file is, without its samples: enough to know how long it plays.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Info {
    pub frames: u64,
    pub channels: u16,
    pub sample_rate: u32,
    pub container: Container,
}

impl Info {
    /// How long the file plays at its own rate.
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate)
    }
}

/// One audio file in memory: its bytes as they are on disk, and where its samples are.
///
/// Immutable. The control side shares it with the audio thread through an `Arc`, inside a
/// snapshot, so it is never dropped there.
pub struct Audio {
    bytes: Vec<u8>,
    /// Where the first sample starts in `bytes`.
    data: usize,
    frames: u64,
    channels: u16,
    sample_rate: u32,
    encoding: Encoding,
    container: Container,
}

impl fmt::Debug for Audio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Audio")
            .field("container", &self.container)
            .field("encoding", &self.encoding)
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("frames", &self.frames)
            .finish()
    }
}

impl Audio {
    /// Reads the layout of a WAV or AIFF file from its bytes and keeps them.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, FormatError> {
        let layout = checked_layout(&bytes)?;
        let frames = frames_of(&layout, bytes.len());
        Ok(Self {
            bytes,
            data: layout.data,
            frames,
            channels: layout.channels,
            sample_rate: layout.sample_rate,
            encoding: layout.encoding,
            container: layout.container,
        })
    }

    pub fn info(&self) -> Info {
        Info {
            frames: self.frames,
            channels: self.channels,
            sample_rate: self.sample_rate,
            container: self.container,
        }
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn container(&self) -> Container {
        self.container
    }

    /// How long the file plays at its own rate.
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate)
    }

    /// The bytes this file takes in memory, which is its size on disk.
    pub fn memory(&self) -> usize {
        self.bytes.len()
    }

    /// The whole file as it is on disk.
    pub(crate) fn file_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The frames from `start` on, left and right, into `out`. A mono file plays on both
    /// channels, and a file with more than two plays its first two. A frame before the start
    /// or after the end of the file is silence.
    ///
    /// Realtime safe: no allocation, lock or system call.
    pub fn read(&self, start: i64, out: &mut [[f32; 2]]) {
        let frames = i64::try_from(self.frames).unwrap_or(i64::MAX);
        let count = i64::try_from(out.len()).unwrap_or(i64::MAX);
        // The part of `out` that lies inside the file.
        let first = (-start).clamp(0, count);
        let last = (frames - start).clamp(first, count);
        let (before, rest) = out.split_at_mut(first as usize);
        let (inside, after) = rest.split_at_mut((last - first) as usize);
        before.fill([0.0; 2]);
        after.fill([0.0; 2]);
        if inside.is_empty() {
            return;
        }
        let frame_size = self.encoding.size() * usize::from(self.channels);
        let from = self.data + (start + first) as usize * frame_size;
        let Some(bytes) = self.bytes.get(from..from + inside.len() * frame_size) else {
            inside.fill([0.0; 2]);
            return;
        };
        // The second channel is the first of a mono file.
        let right = if self.channels > 1 {
            self.encoding.size()
        } else {
            0
        };
        match self.encoding {
            Encoding::U8 => decode(bytes, frame_size, right, inside, |[byte]: [u8; 1]| {
                (f32::from(byte) - 128.0) / 128.0
            }),
            Encoding::I8 => decode(bytes, frame_size, right, inside, |bytes: [u8; 1]| {
                f32::from(i8::from_le_bytes(bytes)) / 128.0
            }),
            Encoding::I16Le => decode(bytes, frame_size, right, inside, |bytes| {
                f32::from(i16::from_le_bytes(bytes)) / 32_768.0
            }),
            Encoding::I16Be => decode(bytes, frame_size, right, inside, |bytes| {
                f32::from(i16::from_be_bytes(bytes)) / 32_768.0
            }),
            Encoding::I24Le => decode(bytes, frame_size, right, inside, |[a, b, c]: [u8; 3]| {
                (i32::from_le_bytes([0, a, b, c]) >> 8) as f32 / 8_388_608.0
            }),
            Encoding::I24Be => decode(bytes, frame_size, right, inside, |[a, b, c]: [u8; 3]| {
                (i32::from_be_bytes([a, b, c, 0]) >> 8) as f32 / 8_388_608.0
            }),
            Encoding::I32Le => decode(bytes, frame_size, right, inside, |bytes| {
                (f64::from(i32::from_le_bytes(bytes)) / 2_147_483_648.0) as f32
            }),
            Encoding::I32Be => decode(bytes, frame_size, right, inside, |bytes| {
                (f64::from(i32::from_be_bytes(bytes)) / 2_147_483_648.0) as f32
            }),
            Encoding::F32Le => decode(bytes, frame_size, right, inside, f32::from_le_bytes),
            Encoding::F32Be => decode(bytes, frame_size, right, inside, f32::from_be_bytes),
            Encoding::F64Le => decode(bytes, frame_size, right, inside, |bytes| {
                f64::from_le_bytes(bytes) as f32
            }),
            Encoding::F64Be => decode(bytes, frame_size, right, inside, |bytes| {
                f64::from_be_bytes(bytes) as f32
            }),
        }
    }
}

/// Decodes the first and the `right`-th sample of every frame. One loop per encoding, so the
/// compiler sees a plain loop with nothing to decide inside it.
fn decode<const SIZE: usize>(
    bytes: &[u8],
    frame_size: usize,
    right: usize,
    out: &mut [[f32; 2]],
    sample: impl Fn([u8; SIZE]) -> f32,
) {
    let at = |frame: &[u8], offset: usize| {
        let bytes = frame.get(offset..).and_then(<[u8]>::first_chunk::<SIZE>);
        bytes.map_or(0.0, |bytes| sample(*bytes))
    };
    for (frame, out) in bytes.chunks_exact(frame_size).zip(out) {
        *out = [at(frame, 0), at(frame, right)];
    }
}

/// The layout of a file from its first bytes, with what this reads checked: the one place
/// that decides whether a file plays, for [`Audio::parse`] and [`probe`] alike.
fn checked_layout(bytes: &[u8]) -> Result<Layout, FormatError> {
    let layout = match bytes.get(..4) {
        Some(b"RIFF") => wav(bytes)?,
        Some(b"FORM") => aiff(bytes)?,
        Some(b"RF64") => return Err(FormatError::Unsupported("RF64".to_string())),
        _ => return Err(FormatError::NotAudio),
    };
    let (min, max) = SAMPLE_RATES;
    if !(min..=max).contains(&layout.sample_rate) {
        return Err(FormatError::SampleRate(layout.sample_rate));
    }
    if layout.channels == 0 {
        return Err(FormatError::Damaged("it says it has no channels"));
    }
    Ok(layout)
}

/// How many whole frames a file of `size` bytes holds.
fn frames_of(layout: &Layout, size: usize) -> u64 {
    let frame_size = layout.encoding.size() * usize::from(layout.channels);
    let available = size.saturating_sub(layout.data);
    // A header may say more than the file holds, for example after a recording that was
    // cut off, and a writer that streams may leave the length at its largest. What is
    // there is what plays.
    let length = layout.length.unwrap_or(available).min(available);
    (length / frame_size.max(1)) as u64
}

/// Why the header of a file could not be read.
pub(crate) enum ProbeError {
    Io(std::io::Error),
    Format(FormatError),
}

/// What a file is, from its header: the start of the file, and more of it only while its
/// chunks go on past what was read. Never its samples, so a long file costs no more than a
/// short one.
pub(crate) fn probe(path: &std::path::Path) -> Result<Info, ProbeError> {
    use std::io::{Read, Seek};
    let mut file = std::fs::File::open(path).map_err(ProbeError::Io)?;
    let size = file.metadata().map_err(ProbeError::Io)?.len();
    let mut wanted = 64 * 1024_u64;
    loop {
        file.rewind().map_err(ProbeError::Io)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(wanted)
            .read_to_end(&mut bytes)
            .map_err(ProbeError::Io)?;
        match checked_layout(&bytes) {
            Ok(layout) => {
                let frames = frames_of(&layout, usize::try_from(size).unwrap_or(usize::MAX));
                return Ok(Info {
                    frames,
                    channels: layout.channels,
                    sample_rate: layout.sample_rate,
                    container: layout.container,
                });
            }
            // A chunk it needs may lie further on, after a long one it does not.
            Err(FormatError::Damaged(_)) if (bytes.len() as u64) < size => wanted *= 8,
            Err(error) => return Err(ProbeError::Format(error)),
        }
    }
}

/// Where the samples of a file are and how they are stored.
struct Layout {
    data: usize,
    /// The length of the sample data the header gives, when it gives a usable one.
    length: Option<usize>,
    channels: u16,
    sample_rate: u32,
    encoding: Encoding,
    container: Container,
}

/// The chunks of a RIFF or IFF file after its 12-byte header: id, start of the body, length of
/// the body as the header says it.
fn chunks(bytes: &[u8], little_endian: bool) -> impl Iterator<Item = ([u8; 4], usize, usize)> {
    let mut at = 12_usize;
    std::iter::from_fn(move || {
        let header = bytes.get(at..at.checked_add(8)?)?;
        let id = *header.first_chunk::<4>()?;
        let size = *header.get(4..)?.first_chunk::<4>()?;
        let size = if little_endian {
            u32::from_le_bytes(size)
        } else {
            u32::from_be_bytes(size)
        } as usize;
        let body = at + 8;
        // Bodies are padded to an even length.
        at = body.saturating_add(size).saturating_add(size % 2);
        Some((id, body, size))
    })
}

/// Whether a whole chunk of a RIFF file starts at `at`: an id of four letters, digits or
/// spaces, and a body that fits in the file.
fn chunk_at(bytes: &[u8], at: usize) -> bool {
    let Some(header) = bytes.get(at..at.saturating_add(8)) else {
        return false;
    };
    let id = header.get(..4).unwrap_or_default();
    let named = id
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b' ');
    let size = u32_at(bytes, at + 4, true).unwrap_or(u32::MAX) as usize;
    named && at.saturating_add(8).saturating_add(size) <= bytes.len()
}

fn u16_at(bytes: &[u8], at: usize, little_endian: bool) -> Option<u16> {
    let bytes = *bytes.get(at..)?.first_chunk::<2>()?;
    Some(if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

fn u32_at(bytes: &[u8], at: usize, little_endian: bool) -> Option<u32> {
    let bytes = *bytes.get(at..)?.first_chunk::<4>()?;
    Some(if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

fn wav(bytes: &[u8]) -> Result<Layout, FormatError> {
    if bytes.get(8..12) != Some(b"WAVE") {
        return Err(FormatError::NotAudio);
    }
    let mut format = None;
    let mut data = None;
    for (id, body, size) in chunks(bytes, true) {
        match &id {
            b"fmt " => format = Some(body),
            b"data" => {
                data = Some((body, size));
                break;
            }
            _ => {}
        }
    }
    let damaged = || FormatError::Damaged("its format chunk is cut short");
    let format = format.ok_or(FormatError::Damaged("it has no format chunk"))?;
    let (data, length) = data.ok_or(FormatError::Damaged("it has no data chunk"))?;
    let mut tag = u16_at(bytes, format, true).ok_or_else(damaged)?;
    let channels = u16_at(bytes, format + 2, true).ok_or_else(damaged)?;
    let sample_rate = u32_at(bytes, format + 4, true).ok_or_else(damaged)?;
    let block_align = u16_at(bytes, format + 12, true).ok_or_else(damaged)?;
    let bits = u16_at(bytes, format + 14, true).ok_or_else(damaged)?;
    if tag == WAVE_FORMAT_EXTENSIBLE {
        // The first two bytes of the sub-format GUID are the format tag it stands for.
        tag = u16_at(bytes, format + 24, true).ok_or_else(damaged)?;
    }
    // The container of a sample is the block of a frame over its channels. A sample of 20 valid
    // bits sits at the top of 3 bytes, so reading the container is exact.
    let size = match channels {
        0 => usize::from(bits).div_ceil(8),
        channels => usize::from(block_align) / usize::from(channels),
    };
    let encoding = match tag {
        WAVE_FORMAT_PCM if size == 1 => Some(Encoding::U8),
        WAVE_FORMAT_PCM => Encoding::signed(size, true),
        WAVE_FORMAT_IEEE_FLOAT => Encoding::float(size, true),
        other => {
            return Err(FormatError::Unsupported(format!(
                "WAV (format {other:#06x})"
            )));
        }
    };
    let encoding = encoding.ok_or_else(|| {
        FormatError::Unsupported(format!("WAV of {bits}-bit samples in {size} bytes"))
    })?;
    // Writers that stream leave the length at its largest, or at zero with nothing after it.
    // A zero followed by another chunk is a file with no samples.
    let streamed = length == u32::MAX as usize || (length == 0 && !chunk_at(bytes, data));
    let length = (!streamed).then_some(length);
    Ok(Layout {
        data,
        length,
        channels,
        sample_rate,
        encoding,
        container: Container::Wav,
    })
}

fn aiff(bytes: &[u8]) -> Result<Layout, FormatError> {
    let compressed = match bytes.get(8..12) {
        Some(b"AIFF") => false,
        Some(b"AIFC") => true,
        _ => return Err(FormatError::NotAudio),
    };
    let mut common = None;
    let mut sound = None;
    for (id, body, size) in chunks(bytes, false) {
        match &id {
            b"COMM" => common = Some(body),
            b"SSND" => sound = Some((body, size)),
            _ => {}
        }
    }
    let damaged = || FormatError::Damaged("its common chunk is cut short");
    let common = common.ok_or(FormatError::Damaged("it has no common chunk"))?;
    let (sound, sound_size) = sound.ok_or(FormatError::Damaged("it has no sound chunk"))?;
    let channels = u16_at(bytes, common, false).ok_or_else(damaged)?;
    let frames = u32_at(bytes, common + 2, false).ok_or_else(damaged)?;
    let bits = u16_at(bytes, common + 6, false).ok_or_else(damaged)?;
    let rate = bytes
        .get(common + 8..)
        .and_then(<[u8]>::first_chunk::<10>)
        .ok_or_else(damaged)?;
    let sample_rate = extended_to_u32(*rate);
    let size = usize::from(bits).div_ceil(8);
    let kind = match compressed {
        true => *bytes
            .get(common + 18..)
            .and_then(<[u8]>::first_chunk::<4>)
            .ok_or_else(damaged)?,
        false => *b"NONE",
    };
    let encoding = match &kind {
        b"NONE" | b"twos" => Encoding::signed(size, false),
        b"sowt" => Encoding::signed(size, true),
        b"in24" => Some(Encoding::I24Be),
        b"in32" => Some(Encoding::I32Be),
        b"fl32" | b"FL32" => Some(Encoding::F32Be),
        b"fl64" | b"FL64" => Some(Encoding::F64Be),
        other => {
            let name = String::from_utf8_lossy(other);
            return Err(FormatError::Unsupported(format!(
                "AIFF-C (compression {name:?})"
            )));
        }
    };
    let encoding =
        encoding.ok_or_else(|| FormatError::Unsupported(format!("AIFF of {bits}-bit samples")))?;
    let offset = u32_at(bytes, sound, false)
        .ok_or(FormatError::Damaged("its sound chunk is cut short"))? as usize;
    let data = sound + 8 + offset;
    let from_frames = frames as usize * encoding.size() * usize::from(channels);
    let length = from_frames.min(sound_size.saturating_sub(8 + offset));
    Ok(Layout {
        data,
        length: Some(length),
        channels,
        sample_rate,
        encoding,
        container: Container::Aiff,
    })
}

/// The 80-bit extended float an AIFF file saves its sample rate in, as a whole number of
/// hertz: a sign and 15 bits of exponent, then 64 bits of mantissa with its leading one.
fn extended_to_u32(bytes: [u8; 10]) -> u32 {
    let [high, low, mantissa @ ..] = bytes;
    let exponent = i32::from(u16::from_be_bytes([high, low]) & 0x7FFF) - 16_383;
    let mantissa = u64::from_be_bytes(mantissa);
    if high & 0x80 != 0 || !(0..64).contains(&exponent) {
        return 0;
    }
    let value = mantissa >> (63 - exponent);
    u32::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_rate_of_an_aiff_file_reads_as_whole_hertz() {
        // 44100 and 48000 as AIFF writers save them.
        let rate_44100 = [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0];
        let rate_48000 = [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0];
        assert_eq!(extended_to_u32(rate_44100), 44_100);
        assert_eq!(extended_to_u32(rate_48000), 48_000);
    }

    #[test]
    fn bytes_that_are_no_audio_say_so() {
        assert_eq!(
            Audio::parse(b"hello world, no audio".to_vec()).err(),
            Some(FormatError::NotAudio)
        );
        assert_eq!(
            Audio::parse(b"RIFF\0\0\0\0WAVE".to_vec()).err(),
            Some(FormatError::Damaged("it has no format chunk"))
        );
    }
}
