use super::*;
use sound_runtime::session::Session;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TestProject(PathBuf);

impl TestProject {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sound-app-import-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("assets")).unwrap();
        Self(root)
    }

    fn wav(&self) -> PathBuf {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&38u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&96_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&16_000i16.to_le_bytes());
        let source = self.0.join("original sample.wav");
        fs::write(&source, bytes).unwrap();
        source
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn import_copies_valid_wav_and_keeps_captured_revision_and_tracks() {
    let project = TestProject::new();
    let source = project.wav();
    let original = Arrangement {
        tracks: vec![Track {
            name: "Sampler 1".into(),
            ..Track::default()
        }],
    };
    let Command::SetArrangement {
        arrangement,
        expected_revision,
    } = prepare_import(&project.0, &source, original.clone(), 42).unwrap()
    else {
        panic!("Expected arrangement command")
    };
    assert_eq!(expected_revision, 42);
    assert_eq!(arrangement.tracks[0], original.tracks[0]);
    assert_eq!(arrangement.tracks[1].name, "Sampler 2");
    assert_eq!(arrangement.tracks[1].instrument, "daw.sampler");
    let config = arrangement.tracks[1].sampler.as_ref().unwrap();
    assert_eq!(config.root_key, 60);
    assert_eq!(
        fs::read(project.0.join("assets").join(&config.asset)).unwrap(),
        fs::read(&source).unwrap()
    );
    fs::remove_file(source).unwrap();
    sound_daw::sample_asset::SampleAsset::load(&project.0, &config.asset).unwrap();
    editing::validate(&arrangement).unwrap();
}

#[test]
fn invalid_import_does_not_copy_assets_or_produce_a_command() {
    let project = TestProject::new();
    let source = project.0.join("invalid.wav");
    fs::write(&source, b"not a WAV").unwrap();
    assert!(prepare_import(&project.0, &source, Arrangement::default(), 0).is_err());
    assert!(
        prepare_import(
            &project.0,
            &project.0.join("missing.wav"),
            Arrangement::default(),
            0
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(project.0.join("assets")).unwrap().count(), 0);
}

#[test]
fn import_receipt_rejects_concurrent_edit_without_losing_it() {
    let project = TestProject::new();
    let session = Session::open(project.0.to_str().unwrap()).unwrap();
    let captured = session.snapshot();
    let source = project.wav();
    let command = prepare_import(
        &project.0,
        &source,
        captured.arrangement.clone(),
        captured.revision,
    )
    .unwrap();
    let edited = editing::apply(&captured.arrangement, Edit::AddTrack).unwrap();
    session
        .send(Command::SetArrangement {
            arrangement: edited.clone(),
            expected_revision: captured.revision,
        })
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let mut receipt = Some(session.send(command).unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let error = loop {
        if let Some(result) = poll_pending(&mut receipt) {
            break result.unwrap_err();
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(error.to_string().contains("revision"));
    assert!(receipt.is_none());
    assert_eq!(session.snapshot().arrangement, edited);
    let current = session.snapshot();
    session
        .send(prepare_import(&project.0, &source, current.arrangement, current.revision).unwrap())
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let imported = session.snapshot().arrangement;
    assert_eq!(&imported.tracks[..edited.tracks.len()], &edited.tracks);
    assert_eq!(imported.tracks.len(), edited.tracks.len() + 1);
    drop(session);
    let reopened = Session::open(project.0.to_str().unwrap()).unwrap();
    assert_eq!(reopened.snapshot().arrangement, imported);
}

#[test]
fn target_survives_reordering_and_cycles_handle_empty_lists() {
    let original = Arrangement {
        tracks: vec![
            Track {
                name: "A".into(),
                ..Track::default()
            },
            Track {
                name: "B".into(),
                ..Track::default()
            },
        ],
    };
    let mut reordered = original.clone();
    reordered.tracks.swap(0, 1);
    assert_eq!(reconcile_target(&original, &reordered, 1), 0);
    assert_eq!(reconcile_target(&original, &Arrangement::default(), 1), 0);
    assert_eq!(reconcile_target(&original, &original, 99), 0);
    assert_eq!(cycle(0, 0), 0);
    assert_eq!(cycle(0, 1), 0);
    assert_eq!(cycle(0, 2), 1);
    assert_eq!(cycle(1, 2), 0);
    assert_eq!(cycle(usize::MAX, 2), 0);
}
