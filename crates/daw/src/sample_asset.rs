use sound_core::{Error, Result};
use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_WAV_BYTES: u64 = 256 * 1024 * 1024;
static NEXT_IMPORT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct SampleAsset {
    sample_rate: u32,
    frames: Vec<[f32; 2]>,
}

impl SampleAsset {
    pub fn load(project_root: &Path, asset: &str) -> Result<Self> {
        let path = resolve_asset(project_root, asset)?;
        Self::decode_wav(&read_wav(&path)?)
    }

    pub fn decode_wav(bytes: &[u8]) -> Result<Self> {
        if bytes.len() as u64 > MAX_WAV_BYTES {
            return Err(Error("WAV exceeds the 256 MiB limit".into()));
        }
        let mut reader = hound::WavReader::new(Cursor::new(bytes))
            .map_err(|error| Error(format!("Invalid WAV: {error}")))?;
        let spec = reader.spec();
        if !(1..=2).contains(&spec.channels)
            || !(1..=384_000).contains(&spec.sample_rate)
            || !matches!(
                (spec.sample_format, spec.bits_per_sample),
                (hound::SampleFormat::Int, 16 | 24 | 32) | (hound::SampleFormat::Float, 32)
            )
        {
            return Err(Error(
                "WAV needs mono/stereo PCM16/24/32 or float32 and a sample rate 1..384000 Hz"
                    .into(),
            ));
        }
        let channels = usize::from(spec.channels);
        let sample_count = reader.len() as usize;
        if sample_count == 0 || !sample_count.is_multiple_of(channels) {
            return Err(Error("WAV needs complete, nonempty frames".into()));
        }
        let declared_bytes = sample_count as u64 * u64::from(spec.bits_per_sample / 8);
        if declared_bytes > bytes.len() as u64 {
            return Err(Error("Truncated WAV sample data".into()));
        }
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(sample_count / channels)
            .map_err(|error| Error(format!("Cannot allocate sample: {error}")))?;
        let mut push = |index: usize, sample: f32| -> Result<()> {
            if !sample.is_finite() || !(-1.0..=1.0).contains(&sample) {
                return Err(Error("WAV samples must be finite and in -1..1".into()));
            }
            if index.is_multiple_of(channels) {
                frames.push([sample; 2]);
            } else {
                frames.last_mut().unwrap()[1] = sample;
            }
            Ok(())
        };
        match spec.sample_format {
            hound::SampleFormat::Float => {
                for (index, sample) in reader.samples::<f32>().enumerate() {
                    push(index, sample.map_err(|error| Error(error.to_string()))?)?;
                }
            }
            hound::SampleFormat::Int => {
                let scale = (1u64 << (spec.bits_per_sample - 1)) as f64;
                for (index, sample) in reader.samples::<i32>().enumerate() {
                    let sample = sample.map_err(|error| Error(error.to_string()))?;
                    if f64::from(sample) < -scale || f64::from(sample) >= scale {
                        return Err(Error("PCM sample outside its bit depth range".into()));
                    }
                    push(index, (f64::from(sample) / scale) as f32)?;
                }
            }
        }
        Ok(Self {
            sample_rate: spec.sample_rate,
            frames,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn frames(&self) -> &[[f32; 2]] {
        &self.frames
    }
}

pub fn validate_asset_reference(asset: &str) -> Result<()> {
    if asset.contains(['\\', ':', '\0'])
        || asset
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(asset).is_absolute()
    {
        return Err(Error(
            "Sample asset must be a relative path inside assets without . or ..".into(),
        ));
    }
    Ok(())
}

fn assets_directory(project_root: &Path) -> Result<PathBuf> {
    let root = project_root.canonicalize()?;
    let expected = root.join("assets");
    let assets = expected.canonicalize()?;
    if assets != expected || !assets.is_dir() {
        return Err(Error(
            "Project assets must be a project-owned directory, not a symlink".into(),
        ));
    }
    Ok(assets)
}

pub fn resolve_asset(project_root: &Path, asset: &str) -> Result<PathBuf> {
    validate_asset_reference(asset)?;
    let assets = assets_directory(project_root)?;
    let path = assets.join(asset).canonicalize()?;
    if !path.starts_with(&assets) || !path.is_file() {
        return Err(Error(
            "Sample asset must be a file inside project assets".into(),
        ));
    }
    Ok(path)
}

fn read_wav(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error("Sample must be a regular WAV file".into()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_WAV_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_WAV_BYTES {
        return Err(Error("WAV exceeds the 256 MiB limit".into()));
    }
    Ok(bytes)
}

pub fn import_sample(project_root: &Path, source: &Path) -> Result<String> {
    let assets = assets_directory(project_root)?;
    let bytes = read_wav(source)?;
    SampleAsset::decode_wav(&bytes)?;
    loop {
        let id = NEXT_IMPORT.fetch_add(1, Ordering::Relaxed);
        let name = format!("sample-{}-{id}.wav", std::process::id());
        let temporary = assets.join(format!(".{name}.tmp"));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let result = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::hard_link(&temporary, assets.join(&name))
        })();
        drop(file);
        fs::remove_file(&temporary)?;
        match result {
            Ok(()) => return Ok(name),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
#[path = "sample_asset_tests.rs"]
pub(crate) mod tests;
