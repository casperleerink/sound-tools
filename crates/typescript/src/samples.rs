//! The sounds a `sample` field of a tool names, as Hum reads them: a file under
//! `assets/audio/`, mixed to one channel at the rate of the engine. Each is read once and kept,
//! so a turn of a knob of the tool does not read it again. In the window a file is read on a
//! thread of its own, as a minute of sound takes about half a second: the tool plays without
//! it until it is there, then its behaviour runs again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use sound_core::{Assets, InstanceId};
use sound_media::{AudioAsset, SCRATCH_FRAMES, engine_frames, load, resampler};

/// The longest sample a tool holds: a list of the record is held in memory whole.
const MAX_SECONDS: f64 = 60.0;

static IN_BACKGROUND: AtomicBool = AtomicBool::new(false);

/// The instances whose sample was read since the last [`take_read`].
static READ: Mutex<Vec<InstanceId>> = Mutex::new(Vec::new());

/// Reads every sample from here on on a thread of its own. The window calls it before the
/// project opens; a render, an inspect and the tests read at once.
pub fn read_in_background() {
    IN_BACKGROUND.store(true, Ordering::Relaxed);
}

/// The instances whose sample was read since the last call, to run their behaviour again.
pub(crate) fn take_read() -> Vec<InstanceId> {
    std::mem::take(&mut *READ.lock().unwrap_or_else(PoisonError::into_inner))
}

/// A sample on its way from its thread, and the instances that wait for it.
#[derive(Default)]
struct Reading {
    read: Option<Result<Vec<f32>, String>>,
    waiting: Vec<InstanceId>,
}

enum Sample {
    Read(Rc<Vec<f32>>),
    Reading(Arc<Mutex<Reading>>),
}

/// The samples read so far, by file and rate.
#[derive(Default, Clone)]
pub(crate) struct Samples(Rc<RefCell<HashMap<(String, u32), Sample>>>);

impl Samples {
    /// The sound of `file`, a file name under `assets/audio/` such as `voice.wav`, at
    /// `sample_rate`, one channel. `None` while it is read on its thread for `waiter`. The
    /// message says what to do when it cannot be read.
    pub(crate) fn get(
        &self,
        assets: &Assets,
        file: &str,
        sample_rate: u32,
        waiter: &InstanceId,
    ) -> Result<Option<Rc<Vec<f32>>>, String> {
        let key = (file.to_string(), sample_rate);
        let mut samples = self.0.borrow_mut();
        let reading = match samples.get(&key) {
            Some(Sample::Read(sound)) => return Ok(Some(sound.clone())),
            Some(Sample::Reading(reading)) => reading.clone(),
            None if !IN_BACKGROUND.load(Ordering::Relaxed) => {
                let sound = Rc::new(read(assets, file, sample_rate)?);
                samples.insert(key, Sample::Read(sound.clone()));
                return Ok(Some(sound));
            }
            None => {
                let reading = Arc::new(Mutex::new(Reading::default()));
                start_reading(&reading, assets.clone(), file.to_string(), sample_rate)?;
                samples.insert(key.clone(), Sample::Reading(reading.clone()));
                reading
            }
        };
        let mut reading = reading.lock().unwrap_or_else(PoisonError::into_inner);
        match reading.read.take() {
            None => {
                reading.waiting.push(waiter.clone());
                Ok(None)
            }
            Some(Ok(sound)) => {
                let sound = Rc::new(sound);
                samples.insert(key, Sample::Read(sound.clone()));
                Ok(Some(sound))
            }
            // Not kept: the file may come or change, and is read again then.
            Some(Err(error)) => {
                samples.remove(&key);
                Err(error)
            }
        }
    }
}

/// Reads a sample on a thread of its own into `reading`, then tells its waiters.
fn start_reading(
    reading: &Arc<Mutex<Reading>>,
    assets: Assets,
    file: String,
    sample_rate: u32,
) -> Result<(), String> {
    let reading = reading.clone();
    std::thread::Builder::new()
        .name("sample".into())
        .spawn(move || {
            let read = read(&assets, &file, sample_rate);
            let mut reading = reading.lock().unwrap_or_else(PoisonError::into_inner);
            reading.read = Some(read);
            let waiting = std::mem::take(&mut reading.waiting);
            READ.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(waiting);
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn read(assets: &Assets, file: &str, sample_rate: u32) -> Result<Vec<f32>, String> {
    let asset = AudioAsset::new(file).map_err(|error| format!("{file:?}: {error}"))?;
    let audio = load(assets, &asset).map_err(|error| error.to_string())?;
    if audio.seconds() > MAX_SECONDS {
        return Err(format!(
            "{} is {:.0} s long; a sample holds up to {MAX_SECONDS:.0} s",
            asset.project_path(),
            audio.seconds()
        ));
    }
    let frames = engine_frames(audio.frames(), audio.sample_rate(), sample_rate);
    let mut stereo = vec![[0.0_f32; 2]; usize::try_from(frames).unwrap_or(0)];
    let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
    resampler(audio.sample_rate(), sample_rate).render(&audio, 0, 0, &mut stereo, &mut scratch);
    Ok(stereo
        .iter()
        .map(|[left, right]| (left + right) * 0.5)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second of a 441 Hz sine at 44.1 kHz, mono.
    fn write_sine(folder: &std::path::Path) {
        let audio = folder.join("assets/audio");
        std::fs::create_dir_all(&audio).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(audio.join("sine.wav"), spec).unwrap();
        for frame in 0..44_100 {
            let phase = frame as f32 * 441.0 / 44_100.0;
            let sample = (phase * std::f32::consts::TAU).sin() * 0.5;
            writer
                .write_sample((sample * f32::from(i16::MAX)) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn a_sample_is_read_at_the_rate_of_the_engine_and_kept() {
        let folder = tempfile::tempdir().unwrap();
        write_sine(folder.path());
        let assets = Assets::new(folder.path());
        let samples = Samples::default();
        let id = InstanceId::new("sine").unwrap();
        let sound = samples
            .get(&assets, "sine.wav", 48_000, &id)
            .unwrap()
            .unwrap();
        // A second at 48 kHz, with its level.
        assert!((sound.len() as i64 - 48_000).abs() <= 1, "{}", sound.len());
        let loudest = sound[1_000..47_000]
            .iter()
            .fold(0.0_f32, |peak, s| peak.max(s.abs()));
        assert!((loudest - 0.5).abs() < 0.01, "{loudest}");
        // Kept: the same list, not read again.
        let again = samples
            .get(&assets, "sine.wav", 48_000, &id)
            .unwrap()
            .unwrap();
        assert!(Rc::ptr_eq(&sound, &again));
        // A file that is not there says where it looked.
        let missing = samples.get(&assets, "gone.wav", 48_000, &id).unwrap_err();
        assert!(missing.contains("gone.wav"), "{missing}");
    }
}
