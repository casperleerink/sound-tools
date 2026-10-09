//! The sounds a `sample` field of a tool names, as Hum reads them: a file under
//! `assets/audio/`, mixed to one channel at the rate of the engine. One cache for the process,
//! by file and rate: a file is read once while it stays the same, a failure too, so a turn of a
//! knob or a save of the tool reads nothing again. In the window a file is read on a thread of
//! its own, as a minute of sound takes about half a second: the tool plays without it until it
//! is there, then its behaviour runs again.

use std::path::PathBuf;
use std::sync::Arc;

use sound_core::{Assets, InstanceId};
use sound_media::{
    AudioAsset, Keep, Loader, SCRATCH_FRAMES, Stamp, engine_frames, load, resampler,
};

/// The longest sample a tool holds: a list of the record is held in memory whole.
const MAX_SECONDS: f64 = 60.0;

/// What a list of a record holds, so only copies of it play: kept for the process.
static SAMPLES: Loader<(PathBuf, u32), Vec<f32>> = Loader::new("sample", Keep::Always);

/// The instances of the project of `assets` whose sample was read since the last call, to run
/// their behaviour again.
pub(crate) fn take_read(assets: &Assets) -> Vec<InstanceId> {
    SAMPLES.take_done(assets)
}

/// The sound of `file`, a file name under `assets/audio/` such as `voice.wav`, at
/// `sample_rate`, one channel. `None` while it is read on its thread for `waiter`. The message
/// says what to do when it cannot be read.
pub(crate) fn sample(
    assets: &Assets,
    file: &str,
    sample_rate: u32,
    waiter: &InstanceId,
) -> Result<Option<Arc<Vec<f32>>>, String> {
    let asset = AudioAsset::new(file).map_err(|error| format!("{file:?}: {error}"))?;
    let path = assets.path(asset.asset_name());
    let read = {
        let (assets, path) = (assets.clone(), path.clone());
        move || {
            let files = vec![Stamp::of(&path)];
            (read(&assets, &asset, sample_rate), files)
        }
    };
    SAMPLES.get((path, sample_rate), (assets, waiter), read)
}

fn read(assets: &Assets, asset: &AudioAsset, sample_rate: u32) -> Result<Vec<f32>, String> {
    let audio = load(assets, asset).map_err(|error| error.to_string())?;
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
        let id = InstanceId::new("sine").unwrap();
        let sound = sample(&assets, "sine.wav", 48_000, &id).unwrap().unwrap();
        // A second at 48 kHz, with its level.
        assert!((sound.len() as i64 - 48_000).abs() <= 1, "{}", sound.len());
        let loudest = sound[1_000..47_000]
            .iter()
            .fold(0.0_f32, |peak, s| peak.max(s.abs()));
        assert!((loudest - 0.5).abs() < 0.01, "{loudest}");
        // Kept: the same list, not read again.
        let again = sample(&assets, "sine.wav", 48_000, &id).unwrap().unwrap();
        assert!(Arc::ptr_eq(&sound, &again));
        // A file that is not there says where it looked.
        let missing = sample(&assets, "gone.wav", 48_000, &id).unwrap_err();
        assert!(missing.contains("gone.wav"), "{missing}");
    }
}
