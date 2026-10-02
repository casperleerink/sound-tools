//! The sound of each pad, made away from the thread that draws and played from memory: a
//! synthesized sound rendered at the pitch and decay of its pad, or a sample at the pitch of its
//! pad and the rate of the engine. Nothing reads a file or makes a sound on the audio thread, and
//! nothing that makes a sound runs inside an edit.
//!
//! The behaviour asks for the sound each pad needs ([`sound_for`]). A sound that was made before
//! and is still held somewhere is there at once, so an edit of a volume or a pan makes nothing
//! again, and two Drum pads with the same kit share their sounds. Any other is made by one thread
//! of its own, the latest ask of each pad winning, while the pad keeps the sound it had. When it
//! is made, [`take_ready`] names the instance, and running its behaviour again
//! (`Project::rebind`) puts the sound in its kit: from the next hit. [`wait_for_sounds`] waits for
//! every sound that was asked for, which an offline render does before each block.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, Weak};
use std::time::SystemTime;

use sound_core::{Assets, InstanceId};
use sound_media::{AudioAsset, MediaError, SCRATCH_FRAMES};

use crate::{Pad, Sound, Source, kit};

/// A sample pad plays at most this much of its file, the longest decay. What is kept of a
/// sample is its render and nothing of its file, so a long file costs the memory of this much
/// sound at the rate of the engine: 3.8 MB at 48 kHz. The file itself is read whole while the
/// render is made, then let go of.
pub const MAX_SAMPLE_SECONDS: f64 = 10.0;

/// The last this long of every sample fades to silence, so a sample whose file ends loud, or
/// that is cut at [`MAX_SAMPLE_SECONDS`], ends without a click: 2 ms, the ramp of an audio clip.
pub const SAMPLE_END_SECONDS: f64 = 0.002;

/// Frames per step of [`Rendered::level`].
const LEVEL_FRAMES: usize = 256;

/// One sound of a pad, at the rate of the engine, ready to play.
pub struct Rendered {
    frames: Box<[[f32; 2]]>,
    /// The loudest sample of each step of [`LEVEL_FRAMES`], as a part of the loudest of all:
    /// how loud the sound is at a place, for the card, with no work on the audio thread.
    levels: Box<[f32]>,
}

impl Rendered {
    fn new(frames: Vec<[f32; 2]>) -> Self {
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

/// What makes a sound, exactly: the bits of the pitch and the decay, and for a sample its file
/// by path, size and modification time, so a file written again under the same name is made
/// again.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Recipe {
    Synthesized {
        sound: Sound,
        pitch_bits: u32,
        decay_bits: u32,
        rate: u32,
    },
    Sampled {
        path: PathBuf,
        length: u64,
        modified: Option<SystemTime>,
        pitch_bits: u32,
        rate: u32,
    },
}

/// One pad of one Drum pad of one project: its `assets/` folder, the instance and the pad.
type Slot = (PathBuf, InstanceId, usize);

/// A sound to make, with what reading its file needs.
struct Job {
    recipe: Recipe,
    pad: Pad,
    assets: Assets,
}

#[derive(Default)]
struct Store {
    /// Every sound made, while anything holds it.
    made: HashMap<Recipe, Weak<Rendered>>,
    /// The recipe each pad asked for last. A sound made for an older ask is not kept for it.
    wanted: HashMap<Slot, Recipe>,
    /// The sound each pad last had in its kit, which it keeps while a new one is made.
    last: HashMap<Slot, Weak<Rendered>>,
    /// Sounds to make, the latest ask of each pad.
    queue: BTreeMap<Slot, Job>,
    /// Sounds being made now.
    making: usize,
    /// Made, or failed, and waiting for the behaviour of their instance to run again. A sound
    /// is held here until then.
    ready: Vec<(Slot, Option<Arc<Rendered>>)>,
    /// A sample that could not be read, for the problem of its pad.
    failed: HashMap<Slot, (Recipe, String)>,
}

static STORE: LazyLock<(Mutex<Store>, Condvar)> = LazyLock::new(Default::default);

fn store() -> MutexGuard<'static, Store> {
    STORE.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a behaviour gets for one pad.
pub(crate) struct ForPad {
    /// What the pad plays now: the sound it asked for when it is there, or the one it had.
    pub sound: Option<Arc<Rendered>>,
    /// Why it is silent, when it is.
    pub problem: Option<String>,
}

/// The sound pad `index` of the Drum pad `instance` should play. Never makes a sound: a sound
/// that is not there yet is asked of the thread that makes them, and until it comes the pad
/// plays the sound it had, if it had one. Reads no file: a sample is looked at by its size and
/// modification time, and its header the first time (`sound_media::info`).
pub(crate) fn sound_for(
    instance: &InstanceId,
    index: usize,
    pad: &Pad,
    assets: &Assets,
    rate: u32,
) -> ForPad {
    let slot = (project_of(assets), instance.clone(), index);
    let recipe = match recipe(pad, assets, rate) {
        Ok(recipe) => recipe,
        Err(error) => {
            let mut store = store();
            store.wanted.remove(&slot);
            store.queue.remove(&slot);
            store.last.remove(&slot);
            return ForPad {
                sound: None,
                problem: Some(error.to_string()),
            };
        }
    };
    let mut store = store();
    store.wanted.insert(slot.clone(), recipe.clone());
    if let Some((failed, message)) = store.failed.get(&slot)
        && *failed == recipe
    {
        let problem = Some(message.clone());
        return ForPad {
            sound: None,
            problem,
        };
    }
    if let Some(sound) = store.made.get(&recipe).and_then(Weak::upgrade) {
        store.queue.remove(&slot);
        store.last.insert(slot, Arc::downgrade(&sound));
        return ForPad {
            sound: Some(sound),
            problem: None,
        };
    }
    let job = Job {
        recipe,
        pad: pad.clone(),
        assets: assets.clone(),
    };
    store.queue.insert(slot.clone(), job);
    STORE.1.notify_all();
    start_maker();
    ForPad {
        sound: store.last.get(&slot).and_then(Weak::upgrade),
        problem: None,
    }
}

/// The Drum pads of the project of `assets` whose sounds were made since the last call, each
/// with those sounds. Run the behaviour of each again (`Project::rebind`) while holding what
/// came with it: that puts the sounds in its kit, and they are let go of after, so the sound of
/// an instance that went away does not stay.
pub fn take_ready(assets: &Assets) -> Vec<(InstanceId, Vec<Arc<Rendered>>)> {
    let project = project_of(assets);
    let mut store = store();
    let (ours, others): (Vec<_>, Vec<_>) = std::mem::take(&mut store.ready)
        .into_iter()
        .partition(|((folder, ..), _)| *folder == project);
    store.ready = others;
    let mut ready: BTreeMap<InstanceId, Vec<Arc<Rendered>>> = BTreeMap::new();
    for ((_, instance, _), sound) in ours {
        ready.entry(instance).or_default().extend(sound);
    }
    ready.into_iter().collect()
}

/// Waits until every sound that was asked for is made. For an offline render, which must play
/// what the record says from its first block, and for tests.
pub fn wait_for_sounds() {
    let (lock, done) = &*STORE;
    let mut store = lock.lock().unwrap_or_else(PoisonError::into_inner);
    while !store.queue.is_empty() || store.making > 0 {
        store = done.wait(store).unwrap_or_else(PoisonError::into_inner);
    }
}

/// Whether any sound is asked for and not made yet. For tests.
pub fn sounds_pending() -> bool {
    let store = store();
    !store.queue.is_empty() || store.making > 0
}

/// Identifies a project by its `assets/` folder: the path of a name inside it.
fn project_of(assets: &Assets) -> PathBuf {
    match AudioAsset::new("drum-pad.wav") {
        Ok(asset) => assets.path(asset.asset_name()),
        Err(_) => PathBuf::new(),
    }
}

fn recipe(pad: &Pad, assets: &Assets, rate: u32) -> Result<Recipe, MediaError> {
    let pitch_bits = pad.pitch_semitones.to_bits();
    match &pad.source {
        Source::Sound(sound) => Ok(Recipe::Synthesized {
            sound: *sound,
            pitch_bits,
            decay_bits: pad.decay_ms.to_bits(),
            rate,
        }),
        Source::Sample(asset) => {
            // Missing, or not a file that plays, is known from its header.
            sound_media::info(assets, asset)?;
            let path = assets.path(asset.asset_name());
            let metadata = std::fs::metadata(&path).map_err(|source| MediaError::Io {
                path: asset.project_path(),
                source,
            })?;
            Ok(Recipe::Sampled {
                path,
                length: metadata.len(),
                modified: metadata.modified().ok(),
                pitch_bits,
                rate,
            })
        }
    }
}

/// Starts the thread that makes sounds, once.
fn start_maker() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("drum-pad sounds".into())
            .spawn(make_sounds);
        // Without the thread nothing is made: the pads keep the sounds they have, and a render
        // that waits for sounds would wait for ever. Say so where it can be seen.
        if let Err(error) = spawned {
            eprintln!("drum-pad: the thread that makes sounds did not start: {error}");
        }
    });
}

fn make_sounds() {
    let (lock, changed) = &*STORE;
    loop {
        let (slot, job) = {
            let mut store = lock.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if let Some(next) = store.queue.pop_first() {
                    store.making += 1;
                    break next;
                }
                store = changed.wait(store).unwrap_or_else(PoisonError::into_inner);
            }
        };
        // Made with no lock held, so an ask from the thread that draws never waits for it.
        let made = make(&job).map(|frames| Arc::new(Rendered::new(frames)));
        let mut store = lock.lock().unwrap_or_else(PoisonError::into_inner);
        store.making -= 1;
        let still_wanted = store.wanted.get(&slot) == Some(&job.recipe);
        match made {
            Ok(sound) => {
                store.made.retain(|_, sound| sound.strong_count() > 0);
                store.made.insert(job.recipe, Arc::downgrade(&sound));
                if still_wanted {
                    store.failed.remove(&slot);
                    store.ready.push((slot, Some(sound)));
                }
            }
            Err(error) => {
                if still_wanted {
                    store
                        .failed
                        .insert(slot.clone(), (job.recipe, error.to_string()));
                    // The behaviour runs again to say so.
                    store.ready.push((slot, None));
                }
            }
        }
        store.last.retain(|_, sound| sound.strong_count() > 0);
        changed.notify_all();
    }
}

/// The frames of one sound.
fn make(job: &Job) -> Result<Vec<[f32; 2]>, MediaError> {
    let pad = &job.pad;
    let (Recipe::Synthesized { rate, .. } | Recipe::Sampled { rate, .. }) = job.recipe;
    match &pad.source {
        Source::Sound(sound) => {
            let pitch = semitones_to_ratio(pad.pitch_semitones);
            let decay = f64::from(pad.decay_ms) / 1000.0;
            Ok(kit::synthesize(*sound, pitch, decay, rate))
        }
        Source::Sample(asset) => sampled(&job.assets, asset, pad.pitch_semitones, rate),
    }
}

/// A sample at `pitch_semitones` and the engine's `rate`. Pitch is speed, as on a sampler:
/// the file is played as if it had been recorded at a rate that many semitones higher, through
/// the resampler of audio clips, which filters what would fold back. So a file at another rate
/// than the engine plays at its own pitch at 0 semitones. Its last [`SAMPLE_END_SECONDS`] fade
/// to silence.
fn sampled(
    assets: &Assets,
    asset: &AudioAsset,
    pitch_semitones: f32,
    rate: u32,
) -> Result<Vec<[f32; 2]>, MediaError> {
    let file = sound_media::load(assets, asset)?;
    let ratio = semitones_to_ratio(pitch_semitones);
    let heard_rate = (f64::from(file.sample_rate()) * ratio).round().max(1.0) as u32;
    let resampler = sound_media::resampler(heard_rate, rate);
    let longest = (MAX_SAMPLE_SECONDS * f64::from(rate)).ceil() as u64;
    let length = sound_media::engine_frames(file.frames(), heard_rate, rate).min(longest);
    let mut frames = vec![[0.0_f32; 2]; length as usize];
    let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
    resampler.render(&file, 0, 0, &mut frames, &mut scratch);
    let ramp = ((SAMPLE_END_SECONDS * f64::from(rate)).round() as usize).max(1);
    for (from_end, frame) in frames.iter_mut().rev().take(ramp).enumerate() {
        // The last frame is silent, the one a ramp before it untouched.
        let level = from_end as f32 / ramp as f32;
        *frame = frame.map(|sample| sample * level);
    }
    Ok(frames)
}

/// Semitones as a ratio of frequencies: 12 is twice as high.
pub fn semitones_to_ratio(semitones: f32) -> f64 {
    (f64::from(semitones) / 12.0).exp2()
}
