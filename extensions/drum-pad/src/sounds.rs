//! The sound of each pad, made on the control thread and played from memory: a synthesized
//! sound rendered at the pitch and decay of its pad, or a sample at the pitch of its pad and
//! the rate of the engine. Nothing reads a file or makes a sound on the audio thread.
//!
//! What was made is kept while anything holds it, by what it was made from, so a behaviour
//! that runs again for an edit of a volume or a pan makes nothing again, and two Drum pads with
//! the same kit share its sounds.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};

use sound_core::Assets;
use sound_media::{Audio, MediaError, SCRATCH_FRAMES};

use crate::{Pad, Sound, Source, kit};

/// A sample pad plays at most this much of its file, the longest decay. So a long file costs
/// the memory of this much sound, not of the file.
pub const MAX_SAMPLE_SECONDS: f64 = 10.0;

/// Frames per step of [`Rendered::level`].
const LEVEL_FRAMES: usize = 256;

/// One sound of a pad, at the rate of the engine, ready to play.
pub struct Rendered {
    frames: Box<[[f32; 2]]>,
    /// The loudest sample of each step of [`LEVEL_FRAMES`], as a part of the loudest of all:
    /// how loud the sound is at a place, for the card, with no work on the audio thread.
    levels: Box<[f32]>,
    /// The file a sample came from, held while the sound is, so that the next run of the
    /// behaviour finds it in memory and this sound with it.
    _file: Option<Arc<Audio>>,
}

impl Rendered {
    fn new(frames: Vec<[f32; 2]>, file: Option<Arc<Audio>>) -> Self {
        let loudest = |frames: &[[f32; 2]]| {
            frames
                .iter()
                .flatten()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
        };
        let peak = loudest(&frames);
        let levels = frames
            .chunks(LEVEL_FRAMES)
            .map(|step| match peak > 0.0 {
                true => loudest(step) / peak,
                false => 0.0,
            })
            .collect();
        Self {
            frames: frames.into_boxed_slice(),
            levels,
            _file: file,
        }
    }

    pub fn frames(&self) -> &[[f32; 2]] {
        &self.frames
    }

    /// How loud the sound is around frame `frame`, from 0 to 1 of its loudest. Realtime safe.
    pub fn level(&self, frame: usize) -> f32 {
        self.levels
            .get(frame / LEVEL_FRAMES)
            .copied()
            .unwrap_or(0.0)
    }
}

impl std::fmt::Debug for Rendered {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Rendered")
            .field("frames", &self.frames.len())
            .finish()
    }
}

/// A synthesized sound, by what makes it: the bits of the pitch and the decay are exact.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Synthesized {
    sound: Sound,
    pitch_bits: u32,
    decay_bits: u32,
    rate: u32,
}

static SYNTHESIZED: LazyLock<Mutex<HashMap<Synthesized, Weak<Rendered>>>> =
    LazyLock::new(Mutex::default);

/// A sample, by the file in memory it came from, the bits of its pitch and the rate.
struct Sampled {
    file: Weak<Audio>,
    pitch_bits: u32,
    rate: u32,
    sound: Weak<Rendered>,
}

static SAMPLED: LazyLock<Mutex<Vec<Sampled>>> = LazyLock::new(Mutex::default);

/// The sound of `pad` at `rate`: made the first time and kept while anything holds it. A
/// sample is loaded from `assets/audio/` the first time; a file that is not there, or does not
/// play, is the error.
///
/// Control thread only: this may read a file and does the work of a render.
pub fn render(pad: &Pad, assets: &Assets, rate: u32) -> Result<Arc<Rendered>, MediaError> {
    match &pad.source {
        Source::Sound(sound) => Ok(synthesized(*sound, pad, rate)),
        Source::Sample(asset) => {
            let file = sound_media::load(assets, asset)?;
            Ok(sampled(file, pad.pitch_semitones, rate))
        }
    }
}

fn synthesized(sound: Sound, pad: &Pad, rate: u32) -> Arc<Rendered> {
    let key = Synthesized {
        sound,
        pitch_bits: pad.pitch_semitones.to_bits(),
        decay_bits: pad.decay_ms.to_bits(),
        rate,
    };
    let lock = || SYNTHESIZED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(made) = lock().get(&key).and_then(Weak::upgrade) {
        return made;
    }
    // Made with no lock held, so another thread that asks for another sound does not wait.
    let pitch = semitones_to_ratio(pad.pitch_semitones);
    let decay = f64::from(pad.decay_ms) / 1000.0;
    let made = Arc::new(Rendered::new(
        kit::synthesize(sound, pitch, decay, rate),
        None,
    ));
    let mut made_before = lock();
    made_before.retain(|_, sound| sound.strong_count() > 0);
    made_before.insert(key, Arc::downgrade(&made));
    made
}

/// A sample at `pitch_semitones` and the engine's `rate`. Pitch is speed, as on a sampler:
/// the file is played as if it had been recorded at a rate that many semitones higher, through
/// the resampler of audio clips, which filters what would fold back. So a file at another rate
/// than the engine plays at its own pitch at 0 semitones.
fn sampled(file: Arc<Audio>, pitch_semitones: f32, rate: u32) -> Arc<Rendered> {
    let pitch_bits = pitch_semitones.to_bits();
    let lock = || SAMPLED.lock().unwrap_or_else(PoisonError::into_inner);
    let found = lock().iter().find_map(|made| {
        let same = made
            .file
            .upgrade()
            .is_some_and(|other| Arc::ptr_eq(&other, &file));
        (same && made.pitch_bits == pitch_bits && made.rate == rate)
            .then(|| made.sound.upgrade())
            .flatten()
    });
    if let Some(found) = found {
        return found;
    }
    let ratio = semitones_to_ratio(pitch_semitones);
    let heard_rate = (f64::from(file.sample_rate()) * ratio).round().max(1.0) as u32;
    let resampler = sound_media::resampler(heard_rate, rate);
    let longest = (MAX_SAMPLE_SECONDS * f64::from(rate)).ceil() as u64;
    let length = sound_media::engine_frames(file.frames(), heard_rate, rate).min(longest);
    let mut frames = vec![[0.0_f32; 2]; length as usize];
    let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
    resampler.render(&file, 0, 0, &mut frames, &mut scratch);
    let made = Arc::new(Rendered::new(frames, Some(file.clone())));
    let mut made_before = lock();
    made_before.retain(|made| made.sound.strong_count() > 0);
    made_before.push(Sampled {
        file: Arc::downgrade(&file),
        pitch_bits,
        rate,
        sound: Arc::downgrade(&made),
    });
    made
}

/// Semitones as a ratio of frequencies: 12 is twice as high.
pub fn semitones_to_ratio(semitones: f32) -> f64 {
    (f64::from(semitones) / 12.0).exp2()
}
