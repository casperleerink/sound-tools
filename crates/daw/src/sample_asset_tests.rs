use super::*;
use std::io::Cursor;

pub(crate) struct TestDirectory(pub PathBuf);

impl TestDirectory {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sound-daw-sample-{}-{}",
            std::process::id(),
            NEXT_IMPORT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    pub fn assets(&self) {
        fs::create_dir(self.0.join("assets")).unwrap();
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

pub(crate) fn float_wav(channels: u16, sample_rate: u32, samples: &[f32]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(
            &mut bytes,
            hound::WavSpec {
                channels,
                sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }
    bytes.into_inner()
}

#[test]
fn decodes_pcm_depths_and_float_channels_at_original_rate() {
    for bits in [16, 24, 32] {
        for channels in [1, 2] {
            let mut bytes = Cursor::new(Vec::new());
            {
                let mut writer = hound::WavWriter::new(
                    &mut bytes,
                    hound::WavSpec {
                        channels,
                        sample_rate: 44_100,
                        bits_per_sample: bits,
                        sample_format: hound::SampleFormat::Int,
                    },
                )
                .unwrap();
                let half = 1i32 << (bits - 2);
                for sample in [half, -half, 0, half] {
                    writer.write_sample(sample).unwrap();
                }
                writer.finalize().unwrap();
            }
            let sample = SampleAsset::decode_wav(bytes.get_ref()).unwrap();
            assert_eq!(sample.sample_rate(), 44_100);
            if channels == 1 {
                assert_eq!(sample.frames(), &[[0.5; 2], [-0.5; 2], [0.0; 2], [0.5; 2]]);
            } else {
                assert_eq!(sample.frames(), &[[0.5, -0.5], [0.0, 0.5]]);
            }
        }
    }
    let mono = SampleAsset::decode_wav(&float_wav(1, 22_050, &[0.25, -0.5])).unwrap();
    assert_eq!(mono.sample_rate(), 22_050);
    assert_eq!(mono.frames(), &[[0.25; 2], [-0.5; 2]]);
    let stereo = SampleAsset::decode_wav(&float_wav(2, 96_000, &[0.25, -0.5])).unwrap();
    assert_eq!(stereo.frames(), &[[0.25, -0.5]]);
}

#[test]
fn rejects_invalid_wavs() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.01, -1.01] {
        assert!(SampleAsset::decode_wav(&float_wav(1, 48_000, &[value])).is_err());
    }
    for rate in [0u32, 384_001] {
        let mut bytes = float_wav(1, 48_000, &[0.5]);
        bytes[24..28].copy_from_slice(&rate.to_le_bytes());
        assert!(SampleAsset::decode_wav(&bytes).is_err());
    }
    assert!(SampleAsset::decode_wav(&float_wav(3, 48_000, &[0.5; 3])).is_err());
    assert!(SampleAsset::decode_wav(&float_wav(1, 48_000, &[])).is_err());
    assert!(SampleAsset::decode_wav(b"not a wav").is_err());
    let bytes = float_wav(2, 48_000, &[0.5; 4]);
    assert!(SampleAsset::decode_wav(&bytes[..bytes.len() - 1]).is_err());
    let mut pcm8 = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(
            &mut pcm8,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 8,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        writer.write_sample(0i8).unwrap();
        writer.finalize().unwrap();
    }
    assert!(SampleAsset::decode_wav(pcm8.get_ref()).is_err());
}

#[test]
fn import_is_unique_and_portable_without_the_original() {
    let directory = TestDirectory::new();
    directory.assets();
    let original = directory.0.join("external.wav");
    let bytes = float_wav(2, 48_000, &[0.25, -0.5]);
    fs::write(&original, &bytes).unwrap();
    let first = import_sample(&directory.0, &original).unwrap();
    let second = import_sample(&directory.0, &original).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        fs::read(directory.0.join("assets").join(&first)).unwrap(),
        bytes
    );
    fs::remove_file(original).unwrap();
    let moved = directory.0.join("moved-project");
    fs::create_dir(&moved).unwrap();
    fs::rename(directory.0.join("assets"), moved.join("assets")).unwrap();
    assert_eq!(
        SampleAsset::load(&moved, &first).unwrap().frames(),
        &[[0.25, -0.5]]
    );
    assert_eq!(fs::read_dir(moved.join("assets")).unwrap().count(), 2);
}

#[test]
fn invalid_import_does_not_publish_an_asset() {
    let directory = TestDirectory::new();
    directory.assets();
    let source = directory.0.join("bad.wav");
    fs::write(&source, float_wav(1, 48_000, &[f32::NAN])).unwrap();
    assert!(import_sample(&directory.0, &source).is_err());
    assert!(import_sample(&directory.0, &directory.0).is_err());
    assert_eq!(fs::read_dir(directory.0.join("assets")).unwrap().count(), 0);
}

#[test]
fn rejects_invalid_asset_paths() {
    let directory = TestDirectory::new();
    directory.assets();
    for asset in [
        "",
        "/sample.wav",
        "../sample.wav",
        "a/../sample.wav",
        "./sample.wav",
        "a//b.wav",
        "a/",
        "C:/sample.wav",
        "a\\b.wav",
        "a\0.wav",
    ] {
        assert!(validate_asset_reference(asset).is_err(), "{asset:?}");
        assert!(SampleAsset::load(&directory.0, asset).is_err());
    }
    assert!(SampleAsset::load(&directory.0, "missing.wav").is_err());
    fs::create_dir(directory.0.join("assets/folder")).unwrap();
    assert!(SampleAsset::load(&directory.0, "folder").is_err());
    fs::write(
        directory.0.join("assets/folder/sample.wav"),
        float_wav(1, 48_000, &[0.5]),
    )
    .unwrap();
    assert!(SampleAsset::load(&directory.0, "folder/sample.wav").is_ok());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escapes_and_symlinked_assets_directory() {
    use std::os::unix::fs::symlink;
    let directory = TestDirectory::new();
    directory.assets();
    let external = directory.0.join("external.wav");
    fs::write(&external, float_wav(1, 48_000, &[0.5])).unwrap();
    symlink(&external, directory.0.join("assets/escape.wav")).unwrap();
    symlink(&directory.0, directory.0.join("assets/escape-dir")).unwrap();
    assert!(SampleAsset::load(&directory.0, "escape.wav").is_err());
    assert!(SampleAsset::load(&directory.0, "escape-dir/external.wav").is_err());
    let nested = directory.0.join("project");
    fs::create_dir(&nested).unwrap();
    symlink(directory.0.join("assets"), nested.join("assets")).unwrap();
    assert!(SampleAsset::load(&nested, "escape.wav").is_err());
    assert!(import_sample(&nested, &external).is_err());
}

#[test]
fn concurrent_imports_do_not_overwrite() {
    let directory = TestDirectory::new();
    directory.assets();
    let source = directory.0.join("external.wav");
    let bytes = float_wav(1, 48_000, &[0.5]);
    fs::write(&source, &bytes).unwrap();
    let references = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| import_sample(&directory.0, &source).unwrap()))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<std::collections::BTreeSet<_>>()
    });
    assert_eq!(references.len(), 8);
    for reference in references {
        assert_eq!(
            fs::read(directory.0.join("assets").join(reference)).unwrap(),
            bytes
        );
    }
    assert_eq!(fs::read_dir(directory.0.join("assets")).unwrap().count(), 8);
}
