//! A tap of another app on a real Mac. It needs speakers, `afplay` and leave to record other
//! apps, so it runs only by hand:
//! `cargo nextest run -p sound-core --test app_sound --run-ignored only`.

#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sound_core::{AppSound, InputDevice};

/// A WAV file of a 440 Hz sine at half scale, 16 bit, one channel.
fn sine_wav(seconds: u32) -> Vec<u8> {
    let rate = 48_000u32;
    let samples = (0..rate * seconds).map(|frame| {
        let phase = frame as f32 * 440.0 / rate as f32 * std::f32::consts::TAU;
        (phase.sin() * 0.5 * f32::from(i16::MAX)) as i16
    });
    let data: Vec<u8> = samples.flat_map(i16::to_le_bytes).collect();
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend((rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    wav
}

#[test]
#[ignore = "needs a Mac that plays sound and lets this program record other apps"]
fn a_tap_of_afplay_hears_what_it_plays() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("sine.wav");
    std::fs::write(&file, sine_wav(6)).unwrap();
    // A plain test thread, no executor to block.
    #[allow(clippy::disallowed_methods)]
    let mut afplay = Command::new("/usr/bin/afplay")
        .arg(&file)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    // Until afplay has played sound, Core Audio does not list it.
    std::thread::sleep(Duration::from_millis(500));

    let device = InputDevice::of_apps(&AppSound::Named("afplay".to_string())).unwrap();
    println!(
        "tap: {} Hz, {} channels",
        device.sample_rate(),
        device.channels()
    );
    let (stream, mut reader, _live) = device.start().unwrap();
    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(100));
        reader.read(&mut samples);
    }
    drop(stream);
    afplay.kill().unwrap();
    afplay.wait().unwrap();

    let loudest = samples
        .iter()
        .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()));
    println!("{} samples, loudest {loudest}", samples.len());
    assert!(!samples.is_empty(), "the tap gave no frames");
    assert!(
        loudest > 0.1,
        "the tap is silent: allow this program under Privacy & Security, Screen & System Audio Recording"
    );
}
