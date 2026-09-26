#![allow(clippy::unwrap_used)]
//! Reading WAV and AIFF, playing at another rate, and importing into a project.

use std::f64::consts::TAU;
use std::path::Path;
use std::sync::Arc;

use sound_core::Assets;
use sound_media::{Audio, AudioAsset, Encoding, MediaError, Resampler, SCRATCH_FRAMES};

/// A WAV file of these frames, through `hound`, which is not the code under test.
fn wav(
    path: &Path,
    rate: u32,
    channels: u16,
    spec: (u16, hound::SampleFormat),
    frames: &[[f64; 2]],
) {
    let (bits, format) = spec;
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: format,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for frame in frames {
        for sample in &frame[..usize::from(channels)] {
            match format {
                hound::SampleFormat::Float => writer.write_sample(*sample as f32).unwrap(),
                hound::SampleFormat::Int => {
                    let scale = f64::from(1_u32 << (bits - 1));
                    let value = (sample * scale).round().clamp(-scale, scale - 1.0) as i32;
                    writer.write_sample(value).unwrap();
                }
            }
        }
    }
    writer.finalize().unwrap();
}

/// An AIFF or AIFF-C file of big-endian 16-bit, 24-bit, little-endian 16-bit (`sowt`) or
/// 32-bit float samples, written by hand from the specification.
fn aiff(path: &Path, rate: u32, kind: &[u8; 4], bits: u16, frames: &[[f64; 2]]) {
    let compressed = kind != b"NONE";
    let mut samples = Vec::new();
    for frame in frames {
        for sample in frame {
            match (kind, bits) {
                (b"fl32", _) => samples.extend((*sample as f32).to_be_bytes()),
                (b"sowt", 16) => samples.extend(((sample * 32768.0).round() as i16).to_le_bytes()),
                (_, 16) => samples.extend(((sample * 32768.0).round() as i16).to_be_bytes()),
                (_, 24) => {
                    let value = ((sample * 8_388_608.0).round() as i32).to_be_bytes();
                    samples.extend(&value[1..]);
                }
                _ => unreachable!(),
            }
        }
    }
    let mut common = Vec::new();
    common.extend(2_u16.to_be_bytes());
    common.extend((frames.len() as u32).to_be_bytes());
    common.extend(bits.to_be_bytes());
    // The rate as an 80-bit extended float: exponent, then the mantissa with its leading one.
    let exponent = 63 - u64::from(rate).leading_zeros();
    common.extend((16_383 + exponent as u16).to_be_bytes());
    common.extend((u64::from(rate) << (63 - exponent)).to_be_bytes());
    if compressed {
        common.extend(kind);
        common.extend([0, 0]);
    }
    let mut body = Vec::new();
    body.extend(if compressed { b"AIFC" } else { b"AIFF" });
    body.extend(b"COMM");
    body.extend((common.len() as u32).to_be_bytes());
    body.extend(&common);
    body.extend(b"SSND");
    body.extend((samples.len() as u32 + 8).to_be_bytes());
    body.extend([0; 8]);
    body.extend(&samples);
    let mut file = b"FORM".to_vec();
    file.extend((body.len() as u32).to_be_bytes());
    file.extend(body);
    std::fs::write(path, file).unwrap();
}

/// A few frames that reach both ends of the range and differ between the channels.
fn frames() -> Vec<[f64; 2]> {
    vec![
        [0.0, 0.5],
        [0.25, -0.25],
        [-0.5, 0.75],
        [0.999, -1.0],
        [-1.0, 0.125],
    ]
}

fn read_all(path: &Path) -> (Audio, Vec<[f32; 2]>) {
    let audio = Audio::parse(std::fs::read(path).unwrap()).unwrap();
    let mut out = vec![[9.0; 2]; audio.frames() as usize];
    audio.read(0, &mut out);
    (audio, out)
}

fn assert_close(read: &[[f32; 2]], expected: &[[f64; 2]], within: f64, what: &str) {
    assert_eq!(read.len(), expected.len(), "{what}");
    for (index, (read, expected)) in read.iter().zip(expected).enumerate() {
        for channel in 0..2 {
            let error = (f64::from(read[channel]) - expected[channel]).abs();
            assert!(
                error <= within,
                "{what}: frame {index} channel {channel}: {read:?} and {expected:?}"
            );
        }
    }
}

#[test]
fn every_wav_encoding_reads_as_its_samples() {
    let folder = tempfile::tempdir().unwrap();
    let cases = [
        (8, hound::SampleFormat::Int, Encoding::U8, 1.0 / 128.0),
        (
            16,
            hound::SampleFormat::Int,
            Encoding::I16Le,
            1.0 / 32_768.0,
        ),
        (
            24,
            hound::SampleFormat::Int,
            Encoding::I24Le,
            1.0 / 8_388_608.0,
        ),
        (32, hound::SampleFormat::Int, Encoding::I32Le, 1e-7),
        (32, hound::SampleFormat::Float, Encoding::F32Le, 1e-7),
    ];
    for (bits, format, encoding, within) in cases {
        let path = folder.path().join(format!("{bits}-{format:?}.wav"));
        wav(&path, 44_100, 2, (bits, format), &frames());
        let (audio, read) = read_all(&path);
        assert_eq!(audio.encoding(), encoding);
        assert_eq!(
            (audio.channels(), audio.sample_rate(), audio.frames()),
            (2, 44_100, 5)
        );
        assert_close(&read, &frames(), within, &format!("{bits}-bit {format:?}"));
        // It costs its size on disk.
        assert_eq!(
            audio.memory() as u64,
            std::fs::metadata(&path).unwrap().len()
        );
    }
}

#[test]
fn a_mono_file_plays_on_both_channels() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("mono.wav");
    wav(&path, 48_000, 1, (16, hound::SampleFormat::Int), &frames());
    let (audio, read) = read_all(&path);
    assert_eq!(audio.channels(), 1);
    let expected: Vec<[f64; 2]> = frames().iter().map(|frame| [frame[0], frame[0]]).collect();
    assert_close(&read, &expected, 1.0 / 32_768.0, "mono");
}

#[test]
fn every_aiff_encoding_reads_as_its_samples() {
    let folder = tempfile::tempdir().unwrap();
    for (kind, bits, encoding, within) in [
        (b"NONE", 16, Encoding::I16Be, 1.0 / 32_768.0),
        (b"NONE", 24, Encoding::I24Be, 1.0 / 8_388_608.0),
        (b"sowt", 16, Encoding::I16Le, 1.0 / 32_768.0),
        (b"fl32", 32, Encoding::F32Be, 1e-7),
    ] {
        let path = folder.path().join("take.aiff");
        let clamped: Vec<[f64; 2]> = frames()
            .iter()
            .map(|frame| frame.map(|sample| sample.min(0.999)))
            .collect();
        aiff(&path, 96_000, kind, bits, &clamped);
        let (audio, read) = read_all(&path);
        assert_eq!(audio.encoding(), encoding);
        assert_eq!(
            (audio.channels(), audio.sample_rate(), audio.frames()),
            (2, 96_000, 5)
        );
        let name = String::from_utf8_lossy(kind);
        assert_close(&read, &clamped, within, &format!("{name} {bits}"));
    }
}

#[test]
fn frames_outside_the_file_are_silence() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("short.wav");
    wav(
        &path,
        48_000,
        2,
        (32, hound::SampleFormat::Float),
        &frames(),
    );
    let audio = Audio::parse(std::fs::read(&path).unwrap()).unwrap();
    let mut out = [[9.0_f32; 2]; 9];
    audio.read(-2, &mut out);
    assert_eq!(out[..2], [[0.0; 2]; 2]);
    assert_eq!(out[2], [0.0, 0.5]);
    assert_eq!(out[6], [-1.0, 0.125]);
    assert_eq!(out[7..], [[0.0; 2]; 2]);
    audio.read(100, &mut out);
    assert_eq!(out, [[0.0; 2]; 9]);
}

/// The worst difference from a sine of `hz` at the engine rate, over `frames` from `from`.
fn worst_error_from_a_sine(out: &[[f32; 2]], hz: f64, rate: f64, from: usize) -> f64 {
    out.iter()
        .enumerate()
        .skip(from)
        .map(|(frame, sample)| {
            let expected = 0.5 * (TAU * hz * frame as f64 / rate).sin();
            (f64::from(sample[0]) - expected).abs()
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_file_at_another_rate_plays_at_its_pitch_and_length() {
    let folder = tempfile::tempdir().unwrap();
    for (file_rate, hz) in [
        (44_100_u32, 1_000.0),
        (96_000, 1_000.0),
        (44_100, 10_000.0),
        (22_050, 440.0),
    ] {
        let seconds = 1.0;
        let count = (f64::from(file_rate) * seconds) as usize;
        let sine: Vec<[f64; 2]> = (0..count)
            .map(|frame| {
                let value = 0.5 * (TAU * hz * frame as f64 / f64::from(file_rate)).sin();
                [value, value]
            })
            .collect();
        let path = folder.path().join(format!("sine-{file_rate}.wav"));
        wav(&path, file_rate, 2, (32, hound::SampleFormat::Float), &sine);
        let audio = Audio::parse(std::fs::read(&path).unwrap()).unwrap();
        let resampler = Resampler::new(file_rate, 48_000);
        // One second of the file is one second of the engine.
        assert_eq!(resampler.engine_frames(audio.frames()), 48_000);
        let mut out = vec![[0.0_f32; 2]; 48_000];
        let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
        // In blocks of 64, as the engine asks, from frame 0 of the file.
        for (index, block) in out.chunks_mut(64).enumerate() {
            resampler.render(&audio, 0, index as u64 * 64, block, &mut scratch);
        }
        // Away from the two ends, where the filter reaches past the file, it is the sine at
        // the engine rate: the right pitch and the right phase.
        let worst = worst_error_from_a_sine(&out[..47_900], hz, 48_000.0, 100);
        let decibels = 20.0 * (worst / 0.5).log10();
        println!("{file_rate} Hz file, {hz} Hz: worst error {decibels:.1} dB under the sine");
        assert!(
            decibels < -70.0,
            "{file_rate} Hz, {hz} Hz: {decibels:.1} dB"
        );
    }
}

#[test]
fn a_block_renders_the_same_on_its_own_as_in_a_run() {
    let folder = tempfile::tempdir().unwrap();
    let noise: Vec<[f64; 2]> = (0..5_000_u64)
        .map(|frame| {
            let value = ((frame * 7_919) % 1_000) as f64 / 1_000.0 - 0.5;
            [value, -value]
        })
        .collect();
    let path = folder.path().join("noise.wav");
    wav(&path, 44_100, 2, (24, hound::SampleFormat::Int), &noise);
    let audio = Audio::parse(std::fs::read(&path).unwrap()).unwrap();
    let resampler = Resampler::new(44_100, 48_000);
    let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
    let mut whole = vec![[0.0_f32; 2]; 4_000];
    resampler.render(&audio, 100, 0, &mut whole, &mut scratch);
    let mut piece = vec![[0.0_f32; 2]; 37];
    resampler.render(&audio, 100, 1_234, &mut piece, &mut scratch);
    assert_eq!(piece, whole[1_234..1_271]);
}

#[test]
fn an_import_copies_the_file_in_under_a_free_name_and_never_writes_over_one() {
    let project = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("My Take (2).WAV");
    wav(
        &source,
        48_000,
        2,
        (16, hound::SampleFormat::Int),
        &frames(),
    );

    let first = sound_media::import(&assets, &source).unwrap();
    assert_eq!(first.to_string(), "my-take-2.wav");
    let copied = project.path().join("assets/audio/my-take-2.wav");
    assert_eq!(
        std::fs::read(&copied).unwrap(),
        std::fs::read(&source).unwrap()
    );

    let second = sound_media::import(&assets, &source).unwrap();
    assert_eq!(second.to_string(), "my-take-2-2.wav");
    // Nothing else is left in the folder.
    let mut names: Vec<String> = std::fs::read_dir(project.path().join("assets/audio"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["my-take-2-2.wav", "my-take-2.wav"]);

    // A file that does not play is refused, and nothing lands in the project.
    let text = outside.path().join("notes.wav");
    std::fs::write(&text, "not audio").unwrap();
    let refused = sound_media::import(&assets, &text);
    assert!(
        matches!(refused, Err(MediaError::Format { .. })),
        "{refused:?}"
    );
    assert!(!project.path().join("assets/audio/notes.wav").exists());
}

#[test]
fn a_loaded_file_is_shared_and_a_missing_one_says_where_it_should_be() {
    let project = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let asset = AudioAsset::new("voice.wav").unwrap();
    match sound_media::load(&assets, &asset) {
        Err(MediaError::Missing { path }) => assert_eq!(path, "assets/audio/voice.wav"),
        other => panic!("{other:?}"),
    }
    std::fs::create_dir_all(project.path().join("assets/audio")).unwrap();
    let path = project.path().join("assets/audio/voice.wav");
    wav(&path, 48_000, 2, (16, hound::SampleFormat::Int), &frames());
    let first = sound_media::load(&assets, &asset).unwrap();
    let second = sound_media::load(&assets, &asset).unwrap();
    assert!(Arc::ptr_eq(&first, &second));

    // A file replaced under its name is read again.
    wav(
        &path,
        48_000,
        2,
        (16, hound::SampleFormat::Int),
        &frames()[..2],
    );
    let replaced = sound_media::load(&assets, &asset).unwrap();
    assert_eq!(replaced.frames(), 2);
}
