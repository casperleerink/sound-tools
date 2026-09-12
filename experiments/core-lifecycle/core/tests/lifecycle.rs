use serde::{Deserialize, Serialize};
use sound_core::{Processor, Project, Record, Registry};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Serialize, Deserialize)]
struct Level {
    value: f32,
}
struct Ramp {
    level: f32,
    position: f32,
}
impl Processor<Level> for Ramp {
    fn apply(&mut self, state: &Level) {
        self.level = state.value;
    }
    fn render(&mut self, output: &mut [f32], _: f32) {
        for sample in output {
            *sample = self.position * self.level;
            self.position += 1.0;
        }
    }
}
fn registry() -> (Registry, sound_core::Tool<Level>) {
    let mut registry = Registry::default();
    let tool = registry.register("test.level", |_: &Level| Ok(()));
    registry.processor(tool, "audio", |s| {
        Box::new(Ramp {
            level: s.value,
            position: 1.0,
        })
    });
    (registry, tool)
}
struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "sound-core-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn replace(&self, id: &str, value: f32) {
        fs::write(
            self.0.join(format!("state/{id}.json")),
            serde_json::to_vec(&Record {
                tool: "test.level".into(),
                state: serde_json::json!({"value": value}),
            })
            .unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn edits_update_existing_processor_and_restore_from_disk() {
    let folder = Folder::new();
    let (registry, tool) = registry();
    let mut project = Project::open(&folder.0, registry).unwrap();
    let a = project.create(tool, "a", Level { value: 1.0 }).unwrap();
    let b = project.create(tool, "b", Level { value: 9.0 }).unwrap();
    project.connect(&a, "audio").unwrap();
    assert!(project.connect(&a, "wrong").is_err());
    project.set_playing(true);
    let mut samples = [0.0; 2];
    project.render(&mut samples, 48_000.0);
    assert_eq!(samples, [1.0, 2.0]);
    project.edit(&a, "Set level", |s| s.value = 2.0).unwrap();
    project.render(&mut samples, 48_000.0);
    assert_eq!(
        samples,
        [6.0, 8.0],
        "parameter edit must preserve processor position"
    );
    assert_eq!(project.state(&b).unwrap().value, 9.0);
    assert_eq!(
        project.poll_files().unwrap(),
        0,
        "own writes do not add undo entries"
    );
    folder.replace("a", 3.0);
    assert_eq!(project.poll_files().unwrap(), 1);
    project.render(&mut samples, 48_000.0);
    assert_eq!(samples, [15.0, 18.0]);
    project.undo().unwrap();
    assert_eq!(project.state(&a).unwrap().value, 2.0);
    project.redo().unwrap();
    drop(project);
    let (registry, tool) = self::registry();
    let mut reopened = Project::open(&folder.0, registry).unwrap();
    assert!(!reopened.playing());
    assert_eq!(reopened.undo_len(), 0);
    let restored = reopened.resolve(tool, "a").unwrap();
    assert_eq!(reopened.state(&restored).unwrap().value, 3.0);
    assert_eq!(reopened.connections().len(), 1);
    reopened.render(&mut samples, 48_000.0);
    assert_eq!(samples, [0.0; 2]);
    reopened.set_playing(true);
    reopened.render(&mut samples, 48_000.0);
    assert_eq!(samples, [3.0, 6.0], "runtime position resets on reopen");
    reopened.delete(&restored).unwrap();
    assert!(reopened.connections().is_empty());
    assert!(!folder.0.join("state/a.json").exists());
    assert!(reopened.resolve(tool, "b").is_ok());
    reopened.set_playing(true);
    reopened.render(&mut samples, 48_000.0);
    assert_eq!(samples, [0.0; 2]);
}

#[test]
fn drag_file_undo_and_cancel_are_all_last_write_wins() {
    let folder = Folder::new();
    let (registry, tool) = registry();
    let mut project = Project::open(&folder.0, registry).unwrap();
    let a = project.create(tool, "a", Level { value: 1.0 }).unwrap();
    let before_file = fs::read(folder.0.join("state/a.json")).unwrap();
    let drag = project.begin(&a, "Drag level").unwrap();
    project.publish(&drag, |s| s.value = 2.0).unwrap();
    assert_eq!(
        fs::read(folder.0.join("state/a.json")).unwrap(),
        before_file
    );
    folder.replace("a", 3.0);
    project.poll_files().unwrap();
    project.publish(&drag, |s| s.value = 4.0).unwrap();
    project.finish(drag).unwrap();
    assert_eq!(project.state(&a).unwrap().value, 4.0);
    assert_eq!(
        project.undo_len(),
        2,
        "one entry for the file and one for the drag"
    );
    project.undo().unwrap();
    assert_eq!(project.state(&a).unwrap().value, 1.0);
    project.redo().unwrap();
    assert_eq!(project.state(&a).unwrap().value, 4.0);
    let drag = project.begin(&a, "Cancel me").unwrap();
    project.publish(&drag, |s| s.value = 5.0).unwrap();
    folder.replace("a", 6.0);
    project.poll_files().unwrap();
    project.cancel(drag).unwrap();
    assert_eq!(project.state(&a).unwrap().value, 4.0);
    assert_eq!(project.poll_files().unwrap(), 0);
}

#[test]
fn invalid_file_keeps_live_state_and_remains_available_for_correction() {
    let folder = Folder::new();
    let (registry, tool) = registry();
    let mut project = Project::open(&folder.0, registry).unwrap();
    let a = project.create(tool, "a", Level { value: 1.0 }).unwrap();
    fs::write(folder.0.join("state/a.json"), "{").unwrap();
    assert!(project.poll_files().is_err());
    assert_eq!(project.state(&a).unwrap().value, 1.0);
    assert_eq!(
        fs::read_to_string(folder.0.join("state/a.json")).unwrap(),
        "{"
    );
    folder.replace("a", 2.0);
    assert_eq!(project.poll_files().unwrap(), 1);
    assert_eq!(project.state(&a).unwrap().value, 2.0);
}
