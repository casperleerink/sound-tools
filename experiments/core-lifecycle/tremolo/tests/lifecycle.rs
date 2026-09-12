use sound_core::{Project, Registry};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tremolo::TremoloState;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tremolo-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn independent_instances_follow_edits_files_undo_and_reopen() {
    let directory = Directory::new();
    let mut registry = Registry::default();
    let tool = tremolo::register(&mut registry);
    let mut project = Project::open(&directory.0, registry).unwrap();
    let first = project
        .create(tool, "first", TremoloState::default())
        .unwrap();
    let second = project
        .create(
            tool,
            "second",
            TremoloState {
                frequency_hz: 660.0,
                ..Default::default()
            },
        )
        .unwrap();
    let second_state = project.state(&second).unwrap().clone();
    let second_bytes = fs::read(directory.0.join("state/second.json")).unwrap();
    project.connect(&first, "audio").unwrap();
    project.set_playing(true);
    let mut samples = [0.0; 256];
    project.render(&mut samples, 48_000.0);
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));

    project
        .edit(&first, "Mute", |state| state.gain = 0.0)
        .unwrap();
    project.render(&mut samples, 48_000.0);
    assert!(
        samples.iter().all(|sample| *sample == 0.0),
        "Only the connected instance should render"
    );
    assert_eq!(project.state(&second).unwrap(), &second_state);
    assert_eq!(
        fs::read(directory.0.join("state/second.json")).unwrap(),
        second_bytes
    );

    let external = TremoloState {
        frequency_hz: 330.0,
        gain: 0.6,
        rate_hz: 9.0,
        depth: 0.9,
    };
    fs::write(
        directory.0.join("state/first.json"),
        serde_json::to_vec(&serde_json::json!({
            "tool": "example.tremolo", "state": external,
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(project.poll_files().unwrap(), 1);
    assert_eq!(project.state(&first).unwrap(), &external);
    project.render(&mut samples, 48_000.0);
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
    project.undo().unwrap();
    assert_eq!(
        project.state(&first).unwrap(),
        &TremoloState {
            gain: 0.0,
            ..Default::default()
        }
    );
    project.render(&mut samples, 48_000.0);
    assert!(samples.iter().all(|sample| *sample == 0.0));
    project.redo().unwrap();
    drop(project);

    let mut registry = Registry::default();
    let tool = tremolo::register(&mut registry);
    let mut reopened = Project::open(&directory.0, registry).unwrap();
    let first = reopened.resolve(tool, "first").unwrap();
    let second = reopened.resolve(tool, "second").unwrap();
    assert_eq!(reopened.state(&first).unwrap(), &external);
    assert_eq!(reopened.state(&second).unwrap(), &second_state);
    assert_eq!(reopened.undo_len(), 0);
    assert_eq!(reopened.connections().len(), 1);
    assert_eq!(reopened.connections()[0].instance, "first");
    assert_eq!(reopened.connections()[0].port, "audio");
    reopened.render(&mut samples, 48_000.0);
    assert!(samples.iter().all(|sample| *sample == 0.0));
    reopened.set_playing(true);
    reopened.render(&mut samples, 48_000.0);
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn parameter_replacement_preserves_both_runtime_phases() {
    let directory = Directory::new();
    let mut registry = Registry::default();
    let tool = tremolo::register(&mut registry);
    let mut project = Project::open(&directory.0, registry).unwrap();
    let initial = TremoloState::default();
    let instance = project.create(tool, "voice", initial.clone()).unwrap();
    project.connect(&instance, "audio").unwrap();
    project.set_playing(true);
    project.render(&mut [0.0; 137], 48_000.0);
    project
        .edit(&instance, "Change all controls", |state| {
            state.frequency_hz = 880.0;
            state.gain = 0.4;
            state.rate_hz = 13.0;
            state.depth = 1.0;
        })
        .unwrap();
    let mut phase = 0.0_f32;
    let mut modulation_phase = 0.0_f32;
    for _ in 0..137 {
        phase = (phase + initial.frequency_hz / 48_000.0).fract();
        modulation_phase = (modulation_phase + initial.rate_hz / 48_000.0).fract();
    }
    let expected = (phase * std::f32::consts::TAU).sin()
        * 0.4
        * (1.0 + (modulation_phase * std::f32::consts::TAU).sin())
        * 0.5;
    let mut next = [0.0];
    project.render(&mut next, 48_000.0);
    assert!(
        (next[0] - expected).abs() < 0.000001,
        "An edit must preserve oscillator and modulator phases"
    );
}
