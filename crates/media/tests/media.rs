#![allow(clippy::unwrap_used)]
//! Reading WAV, AIFF and FLAC, playing at another rate, and importing into a project.

use std::f64::consts::TAU;
use std::path::Path;
use std::sync::Arc;

use sound_core::Assets;
use sound_media::{
    Audio, AudioAsset, Container, Encoding, Info, MediaError, Resampler, SCRATCH_FRAMES,
    SampleLoop, varispeed,
};

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

/// One second of a sine of `hz` at half of full scale in a float WAV at `file_rate`, played
/// at `engine_rate` in blocks of 64 frames from its start.
fn played(folder: &Path, file_rate: u32, engine_rate: u32, hz: f64) -> Vec<[f32; 2]> {
    let sine: Vec<[f64; 2]> = (0..file_rate as usize)
        .map(|frame| [0.5 * (TAU * hz * frame as f64 / f64::from(file_rate)).sin(); 2])
        .collect();
    let path = folder.join(format!("sine-{file_rate}-{hz}.wav"));
    wav(&path, file_rate, 2, (32, hound::SampleFormat::Float), &sine);
    let audio = Audio::parse(std::fs::read(&path).unwrap()).unwrap();
    // One second of the file is one second of the engine.
    let frames = sound_media::engine_frames(audio.frames(), file_rate, engine_rate);
    assert_eq!(frames, u64::from(engine_rate));
    let resampler = Resampler::new(file_rate, engine_rate);
    let mut out = vec![[0.0_f32; 2]; frames as usize];
    let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
    for (index, block) in out.chunks_mut(64).enumerate() {
        resampler.render(&audio, 0, index as u64 * 64, block, &mut scratch);
    }
    out
}

/// Away from the two ends, where the filter reaches past the file: the level of the sine of
/// `hz` in the output against the half of full scale that went in, and the worst difference
/// from that sine at the engine rate in the right phase, both in dB.
fn measure(out: &[[f32; 2]], engine_rate: u32, hz: f64) -> (f64, f64) {
    let rate = f64::from(engine_rate);
    let middle = (engine_rate / 10) as usize..(engine_rate - engine_rate / 10) as usize;
    let (mut sine, mut cosine) = (0.0, 0.0);
    for frame in middle.clone() {
        let angle = TAU * hz * frame as f64 / rate;
        sine += f64::from(out[frame][0]) * angle.sin();
        cosine += f64::from(out[frame][0]) * angle.cos();
    }
    let count = middle.len() as f64;
    let level = 2.0 * (sine * sine + cosine * cosine).sqrt() / count;
    let worst = middle
        .map(|frame| {
            let expected = 0.5 * (TAU * hz * frame as f64 / rate).sin();
            (f64::from(out[frame][0]) - expected).abs()
        })
        .fold(0.0, f64::max);
    (20.0 * (level / 0.5).log10(), 20.0 * (worst / 0.5).log10())
}

#[test]
fn a_file_at_another_rate_plays_at_its_pitch_and_length_and_flat_to_twenty_kilohertz() {
    let folder = tempfile::tempdir().unwrap();
    for (file_rate, engine_rate) in [(44_100, 48_000), (48_000, 44_100), (96_000, 48_000)] {
        for hz in [
            100.0, 1_000.0, 5_000.0, 10_000.0, 15_000.0, 19_000.0, 20_000.0,
        ] {
            let out = played(folder.path(), file_rate, engine_rate, hz);
            let (level, worst) = measure(&out, engine_rate, hz);
            println!(
                "{file_rate} Hz file at {engine_rate} Hz, {hz} Hz: level {level:+.4} dB, worst error {worst:.1} dB under the sine"
            );
            assert!(
                level.abs() < 0.01,
                "{file_rate} to {engine_rate}, {hz} Hz: {level} dB"
            );
            assert!(
                worst < -80.0,
                "{file_rate} to {engine_rate}, {hz} Hz: {worst} dB"
            );
        }
    }
    // Below 43 kHz the pass band ends at 95 % of the Nyquist frequency.
    let out = played(folder.path(), 22_050, 48_000, 9_000.0);
    let (level, worst) = measure(&out, 48_000, 9_000.0);
    println!("22050 Hz file at 48000 Hz, 9000 Hz: level {level:+.4} dB, worst error {worst:.1} dB");
    assert!(level.abs() < 0.01 && worst < -80.0, "{level} {worst}");
}

#[test]
fn nothing_above_the_nyquist_frequency_of_the_engine_folds_back() {
    let folder = tempfile::tempdir().unwrap();
    // Tones a 48 kHz file can hold and a 44.1 kHz engine cannot: they must go, not fold down.
    for hz in [22_100.0, 22_500.0, 23_000.0, 23_500.0] {
        let out = played(folder.path(), 48_000, 44_100, hz);
        let middle = &out[4_410..39_690];
        let power: f64 = middle
            .iter()
            .map(|frame| f64::from(frame[0]).powi(2))
            .sum::<f64>();
        let rms = (power / middle.len() as f64).sqrt();
        let decibels = 20.0 * (rms / (0.5 / 2.0_f64.sqrt())).log10();
        println!("{hz} Hz in a 48 kHz file at 44.1 kHz: what is left is {decibels:.1} dB");
        assert!(decibels < -90.0, "{hz} Hz: {decibels} dB");
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

/// A float WAV of a sine of `hz` at half of full scale, `seconds` long, at `rate`.
fn sine_file(folder: &Path, rate: u32, hz: f64, seconds: f64) -> Audio {
    let frames = (seconds * f64::from(rate)) as usize;
    let sine: Vec<[f64; 2]> = (0..frames)
        .map(|frame| [0.5 * (TAU * hz * frame as f64 / f64::from(rate)).sin(); 2])
        .collect();
    let path = folder.join(format!("sine-{rate}-{hz}-{seconds}.wav"));
    wav(&path, rate, 2, (32, hound::SampleFormat::Float), &sine);
    Audio::parse(std::fs::read(&path).unwrap()).unwrap()
}

/// The frequency of a steady sine, from the first and the last of its rising zero crossings,
/// each placed between two frames on a straight line.
fn frequency(samples: &[f32], rate: f64) -> f64 {
    let crossings: Vec<f64> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(frame, pair)| {
            let (a, b) = (f64::from(pair[0]), f64::from(pair[1]));
            frame as f64 + a / (a - b)
        })
        .collect();
    let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
    (crossings.len() - 1) as f64 * rate / (last - first)
}

#[test]
fn varispeed_plays_a_file_at_its_own_speed_sample_for_sample() {
    let folder = tempfile::tempdir().unwrap();
    let audio = sine_file(folder.path(), 48_000, 441.0, 0.5);
    let mut expected = vec![[0.0_f32; 2]; 1_000];
    audio.read(5_000, &mut expected);
    let mut out = vec![[0.0_f32; 2]; 1_000];
    let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
    for (index, block) in out.chunks_mut(64).enumerate() {
        let position = 5_000.0 + (index * 64) as f64;
        varispeed().render(&audio, position, 1.0, block, &mut scratch);
    }
    assert_eq!(out, expected);
}

/// A sine played at the steps of keys from four octaves down to four up, also from a file at
/// another rate than the engine: each comes out at its frequency and its level.
#[test]
fn varispeed_plays_a_sine_at_the_frequency_of_its_step_and_at_its_level() {
    let folder = tempfile::tempdir().unwrap();
    let engine_rate = 48_000.0;
    for file_rate in [48_000_u32, 44_100, 96_000] {
        let audio = sine_file(folder.path(), file_rate, 440.0, 4.0);
        for semitones in [-48, -31, -12, -7, -1, 0, 1, 5, 12, 19, 24, 36, 48] {
            let pitch = 2.0_f64.powf(f64::from(semitones) / 12.0);
            let step = pitch * f64::from(file_rate) / engine_rate;
            // Half a second, or as much as the file holds from its second frame on.
            let frames = (engine_rate / 2.0).min((audio.frames() as f64 - 64.0) / step) as usize;
            let mut out = vec![[0.0_f32; 2]; frames];
            let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
            for (index, block) in out.chunks_mut(64).enumerate() {
                let position = 32.0 + step * (index * 64) as f64;
                varispeed().render(&audio, position, step, block, &mut scratch);
            }
            let left: Vec<f32> = out.iter().map(|frame| frame[0]).collect();
            let measured = frequency(&left, engine_rate);
            let expected = 440.0 * pitch;
            let cents = 1200.0 * (measured / expected).log2();
            let peak = left
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
            let level = 20.0 * f64::from(peak / 0.5).log10();
            println!(
                "{file_rate} Hz file, {semitones:+} semitones: {measured:.3} Hz for {expected:.3} Hz ({cents:+.4} cents), level {level:+.3} dB"
            );
            assert!(cents.abs() < 0.01, "{file_rate} {semitones}: {cents} cents");
            assert!(level.abs() < 0.05, "{file_rate} {semitones}: {level} dB");
        }
    }
}

/// The level of what a sine of `file_hz` in a file at `file_rate`, played at `step` into an
/// engine at 48 kHz, comes out as, in dB against the sine: the whole output, so for a tone
/// above the Nyquist frequency of the output it is what folds back.
fn played_level(folder: &Path, file_rate: u32, file_hz: f64, step: f64) -> f64 {
    let audio = sine_file(folder, file_rate, file_hz, 2.0);
    let frames = ((audio.frames() as f64 - 2_000.0) / step).min(48_000.0) as usize;
    let mut out = vec![[0.0_f32; 2]; frames];
    let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
    for (index, block) in out.chunks_mut(64).enumerate() {
        let position = 1_000.0 + step * (index * 64) as f64;
        varispeed().render(&audio, position, step, block, &mut scratch);
    }
    let middle = &out[frames / 10..frames - frames / 10];
    let power: f64 = middle.iter().map(|frame| f64::from(frame[0]).powi(2)).sum();
    let rms = (power / middle.len() as f64).sqrt();
    20.0 * (rms / (0.5 / 2.0_f64.sqrt())).log10()
}

/// Above a step of 1 the kernel follows the output: a tone that lands above the Nyquist
/// frequency of the engine is gone, not folded back; one under 16 kHz keeps its level.
#[test]
fn varispeed_folds_nothing_back_above_a_step_of_one() {
    let folder = tempfile::tempdir().unwrap();
    // (the rate of the file, the step, where the tone lands in the output)
    let cases: [(u32, f64, &[f64]); 4] = [
        (
            48_000,
            1.06,
            &[10_000.0, 16_000.0, 20_000.0, 23_000.0, 24_500.0, 25_000.0],
        ),
        (
            48_000,
            2.0,
            &[10_000.0, 16_000.0, 20_000.0, 25_000.0, 30_000.0, 40_000.0],
        ),
        (
            48_000,
            4.0,
            &[10_000.0, 16_000.0, 25_000.0, 40_000.0, 60_000.0, 80_000.0],
        ),
        // A file at 96 kHz at its root: a step of 2.
        (
            96_000,
            2.0,
            &[10_000.0, 16_000.0, 20_000.0, 25_000.0, 30_000.0, 40_000.0],
        ),
    ];
    for (file_rate, step, lands) in cases {
        let ratio = f64::from(file_rate) / 48_000.0;
        for out_hz in lands {
            // The tone in the file that lands there at this step.
            let file_hz = out_hz / step * ratio;
            let level = played_level(folder.path(), file_rate, file_hz, step);
            let what = match *out_hz > 24_000.0 {
                true => format!("folds back to {:.0} Hz at", 48_000.0 - (out_hz % 48_000.0)),
                false => "comes out at".to_string(),
            };
            println!(
                "{file_rate} Hz file, step {step}: {file_hz:.0} Hz in the file lands at {out_hz:.0} Hz, {what} {level:+.1} dB"
            );
            if *out_hz > 24_000.0 {
                assert!(level < -70.0, "{file_rate} {step} {out_hz}: {level} dB");
            } else if *out_hz <= 16_000.0 {
                assert!(
                    level.abs() < 0.05,
                    "{file_rate} {step} {out_hz}: {level} dB"
                );
            }
        }
    }
}

#[test]
fn varispeed_renders_a_block_on_its_own_as_in_a_run() {
    let folder = tempfile::tempdir().unwrap();
    let audio = sine_file(folder.path(), 44_100, 1_234.5, 1.0);
    let step = 1.37;
    let mut scratch = vec![[0.0_f32; 2]; SCRATCH_FRAMES];
    let mut whole = vec![[0.0_f32; 2]; 4_000];
    varispeed().render(&audio, 100.25, step, &mut whole, &mut scratch);
    let mut piece = vec![[0.0_f32; 2]; 37];
    varispeed().render(
        &audio,
        100.25 + 1_234.0 * step,
        step,
        &mut piece,
        &mut scratch,
    );
    for (piece, whole) in piece.iter().zip(&whole[1_234..1_271]) {
        assert!((piece[0] - whole[0]).abs() < 1e-6, "{piece:?} {whole:?}");
    }
    // A step far beyond what the scratch holds in a block still reads every frame, each a
    // filtered sample of the file, never more than it holds.
    let mut fast = vec![[0.0_f32; 2]; 64];
    varispeed().render(&audio, 0.0, 700.0, &mut fast, &mut scratch);
    assert!(
        fast.iter()
            .all(|frame| frame[0].is_finite() && frame[0].abs() <= 0.55)
    );
    assert!(fast.iter().any(|frame| frame[0] != 0.0));
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
    assert_eq!(first.asset.to_string(), "my-take-2.wav");
    let copied = project.path().join("assets/audio/my-take-2.wav");
    assert_eq!(
        std::fs::read(&copied).unwrap(),
        std::fs::read(&source).unwrap()
    );

    let second = sound_media::import(&assets, &source).unwrap();
    assert_eq!(second.asset.to_string(), "my-take-2-2.wav");
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

/// Takes every permission off a file or folder, or gives them back, so a test can tell
/// whether it is read again: a file that cannot be read must not be what an answer came from.
fn lock(path: &Path, locked: bool) {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = match (locked, path.is_dir()) {
        (true, true) => 0o555,
        (true, false) => 0o000,
        (false, _) => 0o755,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_file_that_does_not_play_is_read_once_until_it_changes() {
    let project = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let path = project.path().join("assets/audio/notes.wav");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not audio").unwrap();
    let asset = AudioAsset::new("notes.wav").unwrap();
    assert!(matches!(
        sound_media::load(&assets, &asset),
        Err(MediaError::Format { .. })
    ));
    // Unreadable now, and asked again: the answer is the one kept, not a failed read.
    lock(&path, true);
    let again = sound_media::load(&assets, &asset);
    let info = sound_media::info(&assets, &asset);
    lock(&path, false);
    assert!(matches!(again, Err(MediaError::Format { .. })), "{again:?}");
    assert!(matches!(info, Err(MediaError::Format { .. })), "{info:?}");
    // Changed, it is read again.
    wav(&path, 48_000, 2, (16, hound::SampleFormat::Int), &frames());
    assert_eq!(sound_media::load(&assets, &asset).unwrap().frames(), 5);
}

#[test]
fn what_a_file_is_stays_known_when_nothing_holds_it() {
    let project = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let path = project.path().join("assets/audio/voice.wav");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    wav(&path, 44_100, 2, (16, hound::SampleFormat::Int), &frames());
    let asset = AudioAsset::new("voice.wav").unwrap();
    drop(sound_media::load(&assets, &asset).unwrap());
    lock(&path, true);
    let info = sound_media::info(&assets, &asset);
    lock(&path, false);
    let info = info.unwrap();
    assert_eq!(
        (info.frames, info.sample_rate, info.channels),
        (5, 44_100, 2)
    );
}

#[test]
fn an_imported_file_is_read_once() {
    let project = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("riff.wav");
    wav(
        &source,
        48_000,
        2,
        (24, hound::SampleFormat::Int),
        &frames(),
    );
    let imported = sound_media::import(&assets, &source).unwrap();
    let copied = project.path().join("assets/audio/riff.wav");
    lock(&copied, true);
    let loaded = sound_media::load(&assets, &imported.asset);
    lock(&copied, false);
    assert_eq!(loaded.unwrap().frames(), 5);
}

/// A copy that never becomes a clip does not stay in memory: the cache holds it weakly, and
/// what `import` gave back is the only strong hold.
#[test]
fn an_imported_file_nobody_holds_leaves_memory() {
    let project = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("riff.wav");
    wav(
        &source,
        48_000,
        2,
        (16, hound::SampleFormat::Int),
        &frames(),
    );
    let imported = sound_media::import(&assets, &source).unwrap();
    let held = std::sync::Arc::downgrade(&imported.audio);
    drop(imported);
    assert!(held.upgrade().is_none());
}

/// What a file is comes from its header, whatever the file holds after it, and a header that
/// is cut short is no audio.
#[test]
fn the_header_says_how_long_a_file_is() {
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("long.wav");
    let long: Vec<[f64; 2]> = (0..200_000).map(|_| [0.25, -0.25]).collect();
    wav(&source, 44_100, 2, (24, hound::SampleFormat::Int), &long);
    let info = sound_media::probe(&source).unwrap();
    assert_eq!(
        (info.frames, info.sample_rate, info.channels),
        (200_000, 44_100, 2)
    );
    let text = outside.path().join("notes.wav");
    std::fs::write(&text, "not audio").unwrap();
    assert!(matches!(
        sound_media::probe(&text),
        Err(MediaError::Format { .. })
    ));
    let gone = outside.path().join("gone.wav");
    assert!(matches!(
        sound_media::probe(&gone),
        Err(MediaError::Missing { .. })
    ));
}

#[test]
fn a_failed_import_leaves_nothing_in_the_project() {
    let project = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let folder = project.path().join("assets/audio");
    std::fs::create_dir_all(&folder).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("riff.wav");
    wav(
        &source,
        48_000,
        2,
        (16, hound::SampleFormat::Int),
        &frames(),
    );
    // A folder that takes no file, as a full disk takes none.
    lock(&folder, true);
    let imported = sound_media::import(&assets, &source);
    lock(&folder, false);
    assert!(
        matches!(imported, Err(MediaError::Io { .. })),
        "{imported:?}"
    );
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
}

/// A 16-bit stereo WAV at 48 kHz whose `data` chunk says it is empty, then `after`.
fn empty_data_then(after: &[u8]) -> Vec<u8> {
    let mut body = b"WAVE".to_vec();
    body.extend(b"fmt ");
    body.extend(16_u32.to_le_bytes());
    body.extend(1_u16.to_le_bytes());
    body.extend(2_u16.to_le_bytes());
    body.extend(48_000_u32.to_le_bytes());
    body.extend((48_000_u32 * 4).to_le_bytes());
    body.extend(4_u16.to_le_bytes());
    body.extend(16_u16.to_le_bytes());
    body.extend(b"data");
    body.extend(0_u32.to_le_bytes());
    body.extend(after);
    let mut file = b"RIFF".to_vec();
    file.extend((body.len() as u32).to_le_bytes());
    file.extend(body);
    file
}

#[test]
fn a_data_chunk_of_no_length_is_empty_when_a_chunk_follows_and_streamed_when_none_does() {
    let mut list = b"LIST".to_vec();
    list.extend(8_u32.to_le_bytes());
    list.extend(b"INFOabcd");
    let audio = Audio::parse(empty_data_then(&list)).unwrap();
    assert_eq!(audio.frames(), 0);
    // A writer that streams leaves 0 and writes the samples after it.
    let samples: Vec<u8> = [1000_i16, -1000, 2000, -2000]
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let audio = Audio::parse(empty_data_then(&samples)).unwrap();
    assert_eq!(audio.frames(), 2);
}

/// The header of a file is read in parts. A chunk after the empty data that is longer than
/// the first part still makes a file with no samples, as it does when the whole file is read.
#[test]
fn a_data_chunk_of_no_length_followed_by_a_long_chunk_probes_as_it_loads() {
    let mut list = b"LIST".to_vec();
    let body = vec![b'a'; 100_000];
    list.extend((body.len() as u32).to_le_bytes());
    list.extend(body);
    let bytes = empty_data_then(&list);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("empty.wav");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(sound_media::probe(&path).unwrap().frames, 0);
    assert_eq!(Audio::parse(bytes).unwrap().frames(), 0);
}

/// The FLAC files in `tests/fixtures/`, made with `flac -8 --no-padding --no-seektable` from
/// 300 frames of WAV of these samples: (name, bits, channels, rate).
const FLAC_FIXTURES: [(&str, u16, u16, u32); 2] =
    [("stereo-16", 16, 2, 44_100), ("mono-24", 24, 1, 48_000)];

/// The sample of a FLAC fixture: a hash of its place, wrapped into the range of `bits`, so
/// the encoder can predict little of it.
fn fixture_sample(frame: u64, channel: u64, bits: u16) -> i32 {
    let full = 1_u64 << bits;
    let value = (frame * 2_654_435_761 + channel * 104_729) % full;
    (value as i64 - (full / 2) as i64) as i32
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}.flac"))
}

#[test]
fn a_flac_file_reads_as_the_samples_of_the_wav_it_was_made_from() {
    let folder = tempfile::tempdir().unwrap();
    for (name, bits, channels, rate) in FLAC_FIXTURES {
        let path = folder.path().join(format!("{name}.wav"));
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: bits,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for frame in 0..300 {
            for channel in 0..u64::from(channels) {
                writer
                    .write_sample(fixture_sample(frame, channel, bits))
                    .unwrap();
            }
        }
        writer.finalize().unwrap();
        let (wav, expected) = read_all(&path);
        let (flac, read) = read_all(&fixture(name));
        assert_eq!(flac.container(), Container::Flac);
        assert_eq!(flac.encoding(), wav.encoding(), "{name}");
        assert_eq!(flac.info().frames, 300, "{name}");
        assert_eq!(read, expected, "{name}");
        // It holds its decoded samples.
        assert_eq!(flac.memory(), 300 * usize::from(bits / 8 * channels));
    }
}

#[test]
fn the_header_of_a_flac_file_says_what_it_is() {
    for (name, _, channels, rate) in FLAC_FIXTURES {
        let path = fixture(name);
        let probed = sound_media::probe(&path).unwrap();
        let expected = Info {
            frames: 300,
            channels,
            sample_rate: rate,
            container: Container::Flac,
        };
        assert_eq!(probed, expected, "{name}");
        let (audio, _) = read_all(&path);
        assert_eq!(audio.info(), expected, "{name}");
    }
}

#[test]
fn an_imported_flac_file_is_copied_as_it_is() {
    let project = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let source = fixture("stereo-16");
    let imported = sound_media::import(&assets, &source).unwrap();
    assert_eq!(imported.asset.to_string(), "stereo-16.flac");
    let copied = project.path().join("assets/audio/stereo-16.flac");
    assert_eq!(
        std::fs::read(&copied).unwrap(),
        std::fs::read(&source).unwrap()
    );
    assert_eq!(
        sound_media::load(&assets, &imported.asset)
            .unwrap()
            .frames(),
        300
    );
}

/// The body of a `smpl` chunk with these loops: (type, start, end).
fn smpl(loops: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut body = vec![0; 28];
    body.extend((loops.len() as u32).to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    for (index, (kind, start, end)) in loops.iter().enumerate() {
        for field in [index as u32, *kind, *start, *end, 0, 0] {
            body.extend(field.to_le_bytes());
        }
    }
    body
}

/// A WAV file of 300 frames, with `smpl` after its data chunk when it has `loops`.
fn wav_with_loops(path: &Path, loops: Option<&[(u32, u32, u32)]>) -> Vec<u8> {
    let silence = vec![[0.0; 2]; 300];
    wav(path, 44_100, 2, (16, hound::SampleFormat::Int), &silence);
    let mut bytes = std::fs::read(path).unwrap();
    if let Some(loops) = loops {
        let body = smpl(loops);
        bytes.extend(b"smpl");
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
        let riff = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        std::fs::write(path, &bytes).unwrap();
    }
    bytes
}

#[test]
fn a_wav_file_gives_the_first_forward_loop_of_its_smpl_chunk_after_its_data() {
    const FORWARD: u32 = 0;
    const BACKWARD: u32 = 2;
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("loop.wav");
    let cases: [(Option<&[(u32, u32, u32)]>, Option<SampleLoop>); 6] = [
        (None, None),
        (
            Some(&[(FORWARD, 10, 299)]),
            Some(SampleLoop {
                start: 10,
                end: 299,
            }),
        ),
        (
            Some(&[(BACKWARD, 0, 5), (FORWARD, 20, 100), (FORWARD, 30, 40)]),
            Some(SampleLoop {
                start: 20,
                end: 100,
            }),
        ),
        // A one-frame loop is a loop.
        (
            Some(&[(FORWARD, 7, 7)]),
            Some(SampleLoop { start: 7, end: 7 }),
        ),
        // An end past the last frame, or before the start, is no loop.
        (Some(&[(FORWARD, 10, 300)]), None),
        (Some(&[(FORWARD, 50, 40)]), None),
    ];
    for (loops, expected) in cases {
        let bytes = wav_with_loops(&path, loops);
        let audio = Audio::parse(bytes).unwrap();
        assert_eq!(audio.frames(), 300, "{loops:?}");
        assert_eq!(audio.sample_loop(), expected, "{loops:?}");
        // The chunk after the samples changes nothing of what the header says.
        assert_eq!(sound_media::probe(&path).unwrap().frames, 300, "{loops:?}");
    }
}
