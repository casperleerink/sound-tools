//! A file the composer drops or chooses comes into the project off the thread that asks.

use std::sync::Arc;

use runtime::input::{Brought, Importing, import};
use sound_core::Assets;

/// Waits for what the thread brought.
fn brought(importing: &Importing) -> Result<Brought, String> {
    let start = std::time::Instant::now();
    loop {
        if let Some(done) = importing.poll() {
            return done;
        }
        assert!(start.elapsed().as_secs() < 30, "the import did not end");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Half a second of mono 16-bit silence at 48 kHz, as a WAV file.
fn wav() -> Vec<u8> {
    let frames: u32 = 24_000;
    let data = frames * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    bytes.resize(bytes.len() + data as usize, 0);
    bytes
}

#[test]
fn a_sound_is_copied_into_the_assets_and_one_that_does_not_play_is_not() {
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let waker = Arc::new(|| {});

    let path = outside.path().join("My Take (2).WAV");
    std::fs::write(&path, wav()).unwrap();
    let done = brought(&import(&assets, path, waker.clone()));
    assert_eq!(done, Ok(Brought::Sound("my-take-2.wav".to_string())));
    assert!(project.path().join("assets/audio/my-take-2.wav").is_file());

    let broken = outside.path().join("broken.wav");
    std::fs::write(&broken, b"RIFF, but nothing else").unwrap();
    let done = brought(&import(&assets, broken, waker));
    assert!(done.is_err(), "{done:?}");
    assert!(!project.path().join("assets/audio/broken.wav").exists());
}

#[test]
fn a_midi_file_becomes_a_timeline_for_each_part() {
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let assets = Assets::new(project.path());
    let clip: sound_notes::Clip = serde_json::from_value(serde_json::json!({
        "start": 0, "length": 1920, "notes": [{"start": 0, "length": 480, "pitch": 60, "velocity": 100}]
    }))
    .unwrap();
    let bytes = sound_notes::midi_file::write(&clip, "Tune", 120.0, (4, 4)).unwrap();
    let path = outside.path().join("tune.mid");
    std::fs::write(&path, bytes).unwrap();
    let Ok(Brought::Parts(parts)) = brought(&import(&assets, path, Arc::new(|| {}))) else {
        panic!("no parts");
    };
    let [part] = parts.as_slice() else {
        panic!("{parts:?}");
    };
    assert_eq!(part["notes"][0]["pitch"], 60, "{part}");
}
