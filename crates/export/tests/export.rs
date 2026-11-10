#![allow(clippy::unwrap_used)]

use std::path::Path;

use sound_export::{CEILING_DBTP, Depth, Export, Format, Loudness, Report, SampleRate, Settings};
use sound_media::analysis::{Measures, Meter};

const RATE: u32 = 48_000;

/// `seconds` of a 1 kHz sine at `amplitude` with a little noise, from a fixed seed, so the MP3
/// encoder has more than one tone to code.
fn piece(seconds: f64, amplitude: f32) -> Vec<f32> {
    let frames = (seconds * f64::from(RATE)) as usize;
    let mut seed = 0x1234_5678_u32;
    let mut samples = Vec::with_capacity(2 * frames);
    for frame in 0..frames {
        let time = frame as f32 / RATE as f32;
        let tone = amplitude * (std::f32::consts::TAU * 1_000.0 * time).sin();
        for channel in 0..2 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / (1_u32 << 24) as f32 - 0.5;
            samples.push(tone * (1.0 - 0.3 * channel as f32) + amplitude * 0.1 * noise);
        }
    }
    samples
}

fn export(settings: Settings, samples: &[f32], path: &Path) -> anyhow::Result<Report> {
    let mut export = Export::new(settings)?;
    // In pieces, as a render gives them.
    for piece in samples.chunks(2 * 512) {
        export.push(piece)?;
    }
    export.finish(path)
}

fn measure_file(path: &Path) -> Measures {
    let audio = sound_media::Audio::parse(std::fs::read(path).unwrap()).unwrap();
    let mut buffer = vec![[0.0_f32; 2]; audio.frames() as usize];
    audio.read(0, &mut buffer);
    let mut meter = Meter::new(audio.sample_rate(), vec![]);
    meter.push(buffer.as_flattened());
    meter.finish()
}

fn wav(depth: Depth, loudness: Option<Loudness>) -> Settings {
    Settings {
        format: Format::Wav(depth),
        rate: SampleRate::Hz48000,
        loudness,
    }
}

#[test]
fn a_normalized_export_is_at_the_loudness_asked_and_under_the_peak_limit() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("piece.wav");
    let settings = wav(Depth::TwentyFour, Some(Loudness::Streaming));
    let report = export(settings, &piece(6.0, 0.03), &path).unwrap();
    assert_eq!(report.notes, Vec::<String>::new());

    let measures = measure_file(&path);
    let integrated = measures.integrated.unwrap();
    assert!((integrated - -14.0).abs() < 0.1, "{integrated}");
    assert!(measures.true_peak.unwrap().0 <= CEILING_DBTP);
}

#[test]
fn the_peak_limit_holds_a_peaky_export_back_and_says_so() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("piece.wav");
    let mut samples = piece(6.0, 0.03);
    // One loud click, at -0.9 dB.
    samples[2 * 48_000] = 0.9;
    let settings = wav(Depth::TwentyFour, Some(Loudness::Streaming));
    let report = export(settings, &samples, &path).unwrap();
    assert_eq!(report.notes.len(), 1, "{:?}", report.notes);
    assert!(report.notes[0].contains("peak limit"), "{:?}", report.notes);

    let measures = measure_file(&path);
    let peak = measures.true_peak.unwrap().0;
    assert!(peak <= CEILING_DBTP && peak > CEILING_DBTP - 0.1, "{peak}");
    let integrated = measures.integrated.unwrap();
    assert!(integrated < -20.0, "{integrated}");
    assert!((report.loudness.unwrap() - integrated).abs() < 0.1);
}

/// The encoder's delay and padding, from the LAME tag of the first frame of a CBR MP3 of
/// MPEG-1 stereo: the 4 bytes of the header and 32 of side information, then "Info".
fn gapless_tag(mp3: &[u8]) -> (usize, usize) {
    let info = 4 + 32;
    assert_eq!(&mp3[info..info + 4], b"Info");
    let lame = info + 120;
    assert_eq!(&mp3[lame..lame + 4], b"LAME");
    let both = u32::from_be_bytes([0, mp3[lame + 21], mp3[lame + 22], mp3[lame + 23]]);
    ((both >> 12) as usize, (both & 0xFFF) as usize)
}

#[test]
fn an_mp3_decodes_back_to_the_same_piece_of_the_same_length() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("piece.mp3");
    let samples = piece(3.0, 0.3);
    let settings = Settings {
        format: Format::Mp3,
        rate: SampleRate::Hz48000,
        loudness: None,
    };
    export(settings, &samples, &path).unwrap();

    let mp3 = std::fs::read(&path).unwrap();
    let (delay, padding) = gapless_tag(&mp3);
    let mut decoder = rusty_mp3::Mp3Decoder::new();
    decoder.push(&mp3);
    decoder.flush();
    let mut decoded = Vec::new();
    while let Ok(frame) = decoder.next_frame() {
        assert_eq!((frame.sample_rate, frame.channels), (RATE, 2));
        decoded.extend_from_slice(&frame.samples);
    }
    // What a player keeps, as the tag tells it: the first frame, the tag itself, decodes to
    // silence, then the encoder's delay and the decoder's own, and the padding at the end.
    let frame = 1152;
    let start = frame + delay + rusty_mp3::encode::lametag::DECODER_DELAY;
    let end = decoded.len() / 2 - padding + rusty_mp3::encode::lametag::DECODER_DELAY;
    let kept = &decoded[2 * start..2 * end];
    assert_eq!(kept.len(), samples.len());

    let signal: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    let error: f64 = (samples.iter().zip(kept))
        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
        .sum();
    let snr = 10.0 * (signal / error).log10();
    assert!(snr > 20.0, "{snr} dB");
}

#[test]
fn the_same_settings_give_the_same_bytes() {
    let folder = tempfile::tempdir().unwrap();
    let samples = piece(2.0, 0.3);
    let mp3 = Settings {
        format: Format::Mp3,
        rate: SampleRate::Hz44100,
        loudness: Some(Loudness::Broadcast),
    };
    for (name, settings) in [
        ("dithered.wav", wav(Depth::Sixteen, None)),
        ("piece.mp3", mp3),
    ] {
        let (first, second) = (
            folder.path().join(name),
            folder.path().join(format!("2{name}")),
        );
        export(settings, &samples, &first).unwrap();
        export(settings, &samples, &second).unwrap();
        assert_eq!(
            std::fs::read(first).unwrap(),
            std::fs::read(second).unwrap()
        );
    }
}

#[test]
fn a_render_that_goes_wrong_leaves_the_file_that_was_there() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("piece.wav");
    std::fs::write(&path, b"the last export").unwrap();
    let mut samples = piece(1.0, 0.3);
    samples[2 * 24_000] = f32::NAN;
    let error = export(wav(Depth::TwentyFour, None), &samples, &path).unwrap_err();
    assert!(format!("{error}").contains("0.50 s"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), b"the last export");
    // Nothing left behind next to it either.
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
}

#[test]
fn a_loud_export_without_a_loudness_says_it_clips_unless_it_is_float() {
    let folder = tempfile::tempdir().unwrap();
    let samples = piece(1.0, 1.2);
    let clips = export(
        wav(Depth::Sixteen, None),
        &samples,
        &folder.path().join("a.wav"),
    );
    assert!(clips.unwrap().notes[0].contains("clips"));
    let float = export(
        wav(Depth::Float, None),
        &samples,
        &folder.path().join("b.wav"),
    );
    assert_eq!(float.unwrap().notes, Vec::<String>::new());
}
