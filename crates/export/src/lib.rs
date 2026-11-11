//! Turns a render into the file the composer chose: a WAV of 16, 24 or 32-bit float samples or
//! an MP3, at the loudness they chose. `--render` is the one way in, for the window and agents
//! alike, so the choices are command line flags too ([`Settings::parse`]).
//!
//! The render goes to a temporary file first and is measured on the way. The second pass reads
//! it back with the gain of the loudness, and writes the file beside its place, which it then
//! takes. A render that fails leaves the file that was there as it was.

mod settings;

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use sound_media::analysis::{LoudnessMeasures, LoudnessMeter};

pub use settings::{CEILING_DBTP, Depth, Format, Loudness, SampleRate, Settings};

/// Every render is stereo.
const CHANNELS: usize = 2;
/// Frames the second pass reads at once.
const CHUNK_FRAMES: usize = 4096;
const MP3_KBPS: u32 = 320;

/// An export on its way: [`push`](Self::push) the render into it at the settings' rate, then
/// [`finish`](Self::finish).
pub struct Export {
    settings: Settings,
    raw: BufWriter<File>,
    meter: Option<LoudnessMeter>,
    frames: u64,
    peak: f32,
}

/// What an export wrote.
#[derive(Debug)]
pub struct Report {
    pub frames: u64,
    /// The highest sample written, before it is clipped to full scale. 1 is full scale.
    pub peak: f32,
    /// The loudness reached, in LUFS, when one was asked and could be measured.
    pub loudness: Option<f64>,
    /// For the composer, when something is not as they might think.
    pub notes: Vec<String>,
}

impl Export {
    pub fn new(settings: Settings) -> Result<Self> {
        let raw = tempfile::tempfile().context("could not make a file for the render")?;
        Ok(Self {
            settings,
            raw: BufWriter::new(raw),
            meter: settings
                .loudness
                .map(|_| LoudnessMeter::new(settings.rate.hz())),
            frames: 0,
            peak: 0.0,
        })
    }

    /// The rate to render at.
    pub fn sample_rate(&self) -> u32 {
        self.settings.rate.hz()
    }

    /// `samples` of the render, interleaved by channel, after what came before.
    pub fn push(&mut self, samples: &[f32]) -> Result<()> {
        if let Some(at) = samples.iter().position(|sample| !sample.is_finite()) {
            let frame = self.frames + (at / CHANNELS) as u64;
            let seconds = frame as f64 / f64::from(self.sample_rate());
            bail!("the sound broke at {seconds:.2} s, so nothing was exported");
        }
        for sample in samples {
            self.peak = self.peak.max(sample.abs());
            self.raw.write_all(&sample.to_le_bytes())?;
        }
        if let Some(meter) = &mut self.meter {
            meter.push(samples);
        }
        self.frames += (samples.len() / CHANNELS) as u64;
        Ok(())
    }

    /// Writes the file at `path`, over one that is there.
    pub fn finish(self, path: &Path) -> Result<Report> {
        let Self {
            settings,
            raw,
            meter,
            frames,
            peak,
        } = self;
        let mut notes = Vec::new();
        let (gain_db, loudness) = match (settings.loudness, meter.map(LoudnessMeter::finish)) {
            (Some(target), Some(measured)) => normalize(target, measured, &mut notes),
            _ => (0.0, None),
        };
        let gain = 10_f64.powf(gain_db / 20.0) as f32;
        let peak = peak * gain;
        if settings.format != Format::Wav(Depth::Float) && peak > 1.0 {
            notes.push(
                "Louder than full scale in places, so it clips. Choose a loudness to avoid that."
                    .to_string(),
            );
        }
        let mut raw = raw.into_inner().map_err(|error| error.into_error())?;
        raw.seek(SeekFrom::Start(0))?;
        let mut samples = Samples {
            raw: BufReader::new(raw),
            gain,
            bytes: vec![0; CHUNK_FRAMES * CHANNELS * 4],
            buffer: Vec::with_capacity(CHUNK_FRAMES * CHANNELS),
        };
        write_beside(path, |file| match settings.format {
            Format::Wav(depth) => write_wav(file, settings.rate.hz(), depth, &mut samples),
            Format::Mp3 => write_mp3(file, settings.rate.hz(), &mut samples),
        })?;
        Ok(Report {
            frames,
            peak,
            loudness,
            notes,
        })
    }
}

/// The gain in dB that brings `measured` to `target` under the ceiling, and the loudness that
/// reaches, when it can be measured.
fn normalize(
    target: Loudness,
    measured: LoudnessMeasures,
    notes: &mut Vec<String>,
) -> (f64, Option<f64>) {
    // Rounding to whole steps can lift a peak by a hair: aim that much under.
    let ceiling = CEILING_DBTP - 0.01;
    let room = measured
        .true_peak
        .map_or(f64::INFINITY, |peak| ceiling - peak);
    let Some(integrated) = measured.integrated else {
        notes.push(
            "Too short or quiet to measure its loudness: only the peak limit was applied."
                .to_string(),
        );
        return (room.min(0.0), None);
    };
    let wanted = target.lufs() - integrated;
    if room < wanted {
        let reached = integrated + room;
        notes.push(format!(
            "It is at {reached:.1} LUFS, quieter than asked: louder would pass the peak limit."
        ));
        return (room, Some(reached));
    }
    (wanted, Some(target.lufs()))
}

/// The render read back from its temporary file, with the gain applied.
struct Samples {
    raw: BufReader<File>,
    gain: f32,
    bytes: Vec<u8>,
    buffer: Vec<f32>,
}

impl Samples {
    /// The next samples, interleaved by channel; empty at the end.
    fn next(&mut self) -> Result<&mut [f32]> {
        let mut filled = 0;
        while filled < self.bytes.len() {
            match self.raw.read(&mut self.bytes[filled..])? {
                0 => break,
                read => filled += read,
            }
        }
        let gain = self.gain;
        let (samples, _) = self.bytes[..filled].as_chunks::<4>();
        self.buffer.clear();
        (self.buffer).extend(
            samples
                .iter()
                .map(|bytes| f32::from_le_bytes(*bytes) * gain),
        );
        Ok(&mut self.buffer)
    }
}

/// Writes a file next to `path` with `write`, then puts it in `path`'s place, so nothing at
/// `path` changes unless the whole file was written.
fn write_beside(path: &Path, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    let mut builder = tempfile::Builder::new();
    builder.prefix(".export-");
    // As the file it replaces was, or as `File::create` makes a new one: a temporary file is
    // only its owner's to read.
    #[cfg(unix)]
    builder.permissions(match std::fs::metadata(path) {
        Ok(replaced) => replaced.permissions(),
        Err(_) => std::os::unix::fs::PermissionsExt::from_mode(0o644),
    });
    let mut file = builder
        .tempfile_in(folder)
        .with_context(|| format!("could not write in {}", folder.display()))?;
    write(file.as_file_mut())?;
    file.persist(path)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

fn write_wav(file: &mut File, rate: u32, depth: Depth, samples: &mut Samples) -> Result<()> {
    let (bits_per_sample, sample_format) = match depth {
        Depth::Sixteen => (16, hound::SampleFormat::Int),
        Depth::TwentyFour => (24, hound::SampleFormat::Int),
        Depth::Float => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec {
        channels: CHANNELS as u16,
        sample_rate: rate,
        bits_per_sample,
        sample_format,
    };
    let mut writer = hound::WavWriter::new(BufWriter::new(file), spec)?;
    let mut dither = Dither::default();
    loop {
        let chunk = samples.next()?;
        if chunk.is_empty() {
            break;
        }
        for sample in chunk {
            match depth {
                Depth::Float => writer.write_sample(*sample)?,
                Depth::TwentyFour => writer.write_sample(quantize(*sample, 24, 0.0))?,
                Depth::Sixteen => writer.write_sample(quantize(*sample, 16, dither.next()))?,
            }
        }
    }
    writer.finalize()?;
    Ok(())
}

/// `sample` as an integer of `bits`, with `dither` in steps of the last bit added first.
fn quantize(sample: f32, bits: u32, dither: f32) -> i32 {
    let scale = (1_i32 << (bits - 1)) as f32;
    let value = (sample * scale + dither).round();
    value.clamp(-scale, scale - 1.0) as i32
}

/// TPDF dither: the sum of two uniform values of ±½ step, a triangle over ±1 step, from a
/// generator with a fixed seed, so an export is the same file every time.
struct Dither(u64);

impl Default for Dither {
    fn default() -> Self {
        Self(0x9E37_79B9_7F4A_7C15)
    }
}

impl Dither {
    fn next(&mut self) -> f32 {
        self.uniform() + self.uniform() - 1.0
    }

    /// From 0 to 1. xorshift64*.
    fn uniform(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (value >> 40) as f32 / (1_u64 << 24) as f32
    }
}

fn write_mp3(file: &mut File, rate: u32, samples: &mut Samples) -> Result<()> {
    let mut encoder = rusty_mp3::Mp3Encoder::new(rusty_mp3::Mp3EncoderConfig {
        bitrate_kbps: MP3_KBPS,
        vbr_quality: None,
    });
    let mut output = BufWriter::new(&mut *file);
    let mut pushed = false;
    loop {
        let chunk = samples.next()?;
        if chunk.is_empty() && pushed {
            break;
        }
        for sample in chunk.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
        // At least one push, even of nothing, so the encoder knows the format of an empty file.
        encoder
            .push_pcm_f32(chunk, CHANNELS as u16, rate)
            .map_err(|error| anyhow::anyhow!("the MP3 encoder failed: {error:?}"))?;
        pushed = true;
        while let Ok(packet) = encoder.next_packet() {
            output.write_all(&packet)?;
        }
    }
    encoder.finish();
    while let Ok(packet) = encoder.next_packet() {
        output.write_all(&packet)?;
    }
    output.flush()?;
    drop(output);
    // The first frame says how long the piece is, which is known only now, so a player plays
    // it without the encoder's silence at either end.
    if let Some(tag) = encoder.lame_tag_frame() {
        file.seek(SeekFrom::Start(0))?;
        file.write_all(tag)?;
    }
    Ok(())
}
