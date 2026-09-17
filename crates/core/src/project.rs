use crate::{
    Error, Result,
    clock::Clock,
    registry::{Registry, State, Tool},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub tool: String,
    pub state: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Endpoint {
    pub instance: String,
    pub port: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub source: Endpoint,
    pub target: Endpoint,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub format: u32,
    pub name: String,
    pub clock: Clock,
    pub instances: Vec<String>,
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub owners: BTreeMap<String, String>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            format: 1,
            name: "Untitled".into(),
            clock: Clock::default(),
            instances: Vec::new(),
            connections: Vec::new(),
            owners: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    manifest: Manifest,
    records: BTreeMap<String, Record>,
}

type Validator = dyn Fn(&BTreeMap<String, Record>) -> Result<()> + Send + Sync;

pub struct Project {
    validator: Option<Box<Validator>>,
    root: PathBuf,
    registry: Arc<Registry>,
    current: Snapshot,
    disk: Option<Snapshot>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    gesture: Option<Snapshot>,
    revision: u64,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let temporary = path.with_extension("json.tmp");
    let result = (|| {
        use std::io::Write;
        let mut file = fs::File::create(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

impl Project {
    pub fn create(root: impl Into<PathBuf>, registry: Arc<Registry>, name: &str) -> Result<Self> {
        let root = root.into();
        if root.join("project.json").exists() {
            return Err(Error("Project already exists".into()));
        }
        fs::create_dir_all(root.join("state"))?;
        fs::create_dir_all(root.join("assets"))?;
        let project = Self {
            validator: None,
            root,
            registry,
            current: Snapshot {
                manifest: Manifest {
                    name: name.into(),
                    ..Manifest::default()
                },
                records: BTreeMap::new(),
            },
            disk: None,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            revision: 0,
        };
        project.persist(&project.current)?;
        Ok(project)
    }
    pub fn open(root: impl Into<PathBuf>, registry: Arc<Registry>) -> Result<Self> {
        let root = root.into();
        let current = Self::load(&root, &registry)?;
        Ok(Self {
            validator: None,
            root,
            registry,
            disk: Some(current.clone()),
            current,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            revision: 0,
        })
    }
    fn load(root: &Path, registry: &Registry) -> Result<Snapshot> {
        let manifest: Manifest = serde_json::from_slice(&fs::read(root.join("project.json"))?)?;
        let mut records = BTreeMap::new();
        for id in &manifest.instances {
            if !valid_id(id) {
                return Err(Error(format!("Invalid instance ID: {id}")));
            }
            let record: Record =
                serde_json::from_slice(&fs::read(root.join("state").join(format!("{id}.json")))?)?;
            if records.insert(id.clone(), record).is_some() {
                return Err(Error(format!("Duplicate instance: {id}")));
            }
        }
        let snapshot = Snapshot { manifest, records };
        Self::validate(&snapshot, registry)?;
        Ok(snapshot)
    }
    fn validate(snapshot: &Snapshot, registry: &Registry) -> Result<()> {
        if snapshot.manifest.format != 1 {
            return Err(Error("Unsupported project format".into()));
        }
        snapshot.manifest.clock.validate()?;
        for (id, record) in &snapshot.records {
            if !valid_id(id) {
                return Err(Error(format!("Invalid instance ID: {id}")));
            }
            registry.validate(&record.tool, record.state.clone())?;
        }
        for connection in &snapshot.manifest.connections {
            for endpoint in [&connection.source, &connection.target] {
                if endpoint.instance != "device"
                    && !snapshot.records.contains_key(&endpoint.instance)
                {
                    return Err(Error("Connection references a missing instance".into()));
                }
                if endpoint.port.is_empty() {
                    return Err(Error("Empty port name".into()));
                }
            }
        }
        for (child, parent) in &snapshot.manifest.owners {
            if !snapshot.records.contains_key(child) || !snapshot.records.contains_key(parent) {
                return Err(Error("Ownership references a missing instance".into()));
            }
            let mut cursor = parent;
            for _ in 0..=snapshot.records.len() {
                if cursor == child {
                    return Err(Error("Cyclic tool ownership".into()));
                }
                match snapshot.manifest.owners.get(cursor) {
                    Some(next) => cursor = next,
                    None => break,
                }
            }
        }
        Ok(())
    }
    pub fn set_validator(
        &mut self,
        validator: impl Fn(&BTreeMap<String, Record>) -> Result<()> + Send + Sync + 'static,
    ) -> Result<()> {
        validator(&self.current.records)?;
        self.validator = Some(Box::new(validator));
        Ok(())
    }
    fn validate_candidate(&self, snapshot: &Snapshot) -> Result<()> {
        Self::validate(snapshot, &self.registry)?;
        if let Some(validator) = &self.validator {
            validator(&snapshot.records)?;
        }
        Ok(())
    }
    fn persist(&self, snapshot: &Snapshot) -> Result<()> {
        for (id, record) in &snapshot.records {
            atomic_write(
                &self.root.join("state").join(format!("{id}.json")),
                &serde_json::to_vec_pretty(record)?,
            )?;
        }
        atomic_write(
            &self.root.join("project.json"),
            &serde_json::to_vec_pretty(&snapshot.manifest)?,
        )
    }
    fn apply(&mut self, next: Snapshot, persist: bool) -> Result<bool> {
        if next == self.current {
            return Ok(false);
        }
        self.validate_candidate(&next)?;
        if persist && self.gesture.is_none() {
            self.persist(&next)?;
            self.disk = Some(next.clone());
        }
        let old = std::mem::replace(&mut self.current, next);
        if self.gesture.is_none() {
            self.undo.push(old);
        }
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn manifest(&self) -> &Manifest {
        &self.current.manifest
    }
    pub fn records(&self) -> &BTreeMap<String, Record> {
        &self.current.records
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn read<S: State>(&self, id: &str, tool: Tool<S>) -> Result<S> {
        let record = self
            .current
            .records
            .get(id)
            .ok_or_else(|| Error(format!("Missing instance: {id}")))?;
        if record.tool != tool.name() {
            return Err(Error("Instance has a different tool type".into()));
        }
        self.registry.decode(tool, record.state.clone())
    }
    pub fn insert<S: State>(
        &mut self,
        id: &str,
        tool: Tool<S>,
        state: &S,
        owner: Option<&str>,
    ) -> Result<()> {
        if !valid_id(id) || id == "device" || self.current.records.contains_key(id) {
            return Err(Error("Invalid or duplicate instance ID".into()));
        }
        let mut next = self.current.clone();
        next.records.insert(
            id.into(),
            Record {
                tool: tool.name().into(),
                state: serde_json::to_value(state)?,
            },
        );
        next.manifest.instances.push(id.into());
        if let Some(parent) = owner {
            next.manifest.owners.insert(id.into(), parent.into());
        }
        self.apply(next, true)?;
        Ok(())
    }
    pub fn replace<S: State>(&mut self, id: &str, tool: Tool<S>, state: &S) -> Result<()> {
        self.read(id, tool)?;
        let mut next = self.current.clone();
        next.records.get_mut(id).unwrap().state = serde_json::to_value(state)?;
        self.apply(next, true)?;
        Ok(())
    }
    pub fn edit_manifest(&mut self, edit: impl FnOnce(&mut Manifest)) -> Result<()> {
        let mut next = self.current.clone();
        edit(&mut next.manifest);
        if next.manifest.instances != self.current.manifest.instances {
            return Err(Error("Use instance operations to change the index".into()));
        }
        self.apply(next, true)?;
        Ok(())
    }
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if !self.current.records.contains_key(id) {
            return Err(Error("Missing instance".into()));
        }
        let mut next = self.current.clone();
        let mut removed = vec![id.to_string()];
        let mut index = 0;
        while index < removed.len() {
            let children: Vec<_> = next
                .manifest
                .owners
                .iter()
                .filter(|(_, parent)| **parent == removed[index])
                .map(|(child, _)| child.clone())
                .collect();
            removed.extend(children);
            index += 1;
        }
        for id in &removed {
            next.records.remove(id);
            next.manifest.owners.remove(id);
        }
        next.manifest.instances.retain(|id| !removed.contains(id));
        next.manifest.connections.retain(|connection| {
            !removed.contains(&connection.source.instance)
                && !removed.contains(&connection.target.instance)
        });
        self.apply(next, true)?;
        Ok(())
    }
    pub fn poll(&mut self) -> Result<bool> {
        let disk = Self::load(&self.root, &self.registry)?;
        if self.gesture.is_none() {
            let changed = self.apply(disk.clone(), false)?;
            self.disk = Some(disk);
            return Ok(changed);
        }
        if self.disk.as_ref() == Some(&disk) {
            return Ok(false);
        }
        let baseline = self.gesture.as_ref().unwrap();
        let mut next = self.current.clone();
        if disk.manifest != baseline.manifest {
            next.manifest = disk.manifest.clone();
        }
        for (id, record) in &disk.records {
            if baseline.records.get(id) != Some(record) {
                next.records.insert(id.clone(), record.clone());
            }
        }
        next.records
            .retain(|id, _| next.manifest.instances.contains(id));
        let changed = self.apply(next, false)?;
        self.disk = Some(disk);
        Ok(changed)
    }
    pub fn begin_edit(&mut self) -> Result<()> {
        if self.gesture.is_some() {
            return Err(Error("An edit is already active".into()));
        }
        self.gesture = Some(self.current.clone());
        Ok(())
    }
    pub fn finish_edit(&mut self) -> Result<()> {
        if self.gesture.is_none() {
            return Err(Error("No active edit".into()));
        }
        self.validate_candidate(&self.current)?;
        self.persist(&self.current)?;
        let before = self.gesture.take().unwrap();
        if before != self.current {
            self.undo.push(before);
        }
        Ok(())
    }
    pub fn cancel_edit(&mut self) -> Result<()> {
        let before = self
            .gesture
            .as_ref()
            .ok_or_else(|| Error("No active edit".into()))?
            .clone();
        self.validate_candidate(&before)?;
        self.persist(&before)?;
        self.current = before;
        self.gesture = None;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn undo(&mut self) -> Result<bool> {
        if self.gesture.is_some() {
            return Err(Error("Finish the active edit before undo".into()));
        }
        let Some(next) = self.undo.last() else {
            return Ok(false);
        };
        self.validate_candidate(next)?;
        self.persist(next)?;
        let next = self.undo.pop().unwrap();
        self.redo.push(std::mem::replace(&mut self.current, next));
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }
    pub fn redo(&mut self) -> Result<bool> {
        if self.gesture.is_some() {
            return Err(Error("Finish the active edit before redo".into()));
        }
        let Some(next) = self.redo.last() else {
            return Ok(false);
        };
        self.validate_candidate(next)?;
        self.persist(next)?;
        let next = self.redo.pop().unwrap();
        self.undo.push(std::mem::replace(&mut self.current, next));
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);
    fn assert_rejected(project: &mut Project, operation: impl FnOnce(&mut Project) -> Result<()>) {
        let before = (
            project.current.clone(),
            project.disk.clone(),
            project.undo.clone(),
            project.redo.clone(),
            project.gesture.clone(),
            project.revision,
        );
        let manifest = fs::read(project.root.join("project.json")).unwrap();
        let records: BTreeMap<_, _> = fs::read_dir(project.root.join("state"))
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        assert!(operation(project).is_err());
        assert_eq!(
            (
                project.current.clone(),
                project.disk.clone(),
                project.undo.clone(),
                project.redo.clone(),
                project.gesture.clone(),
                project.revision,
            ),
            before
        );
        assert_eq!(
            fs::read(project.root.join("project.json")).unwrap(),
            manifest
        );
        for (path, bytes) in records {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn candidate_validator_guards_mutations_history_and_gesture_persistence() {
        use std::sync::atomic::AtomicBool;
        let root = std::env::temp_dir().join(format!(
            "sound-project-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut registry = Registry::default();
        let tool = registry.register::<f32>("gain", |_| Ok(())).unwrap();
        let mut project = Project::create(&root, Arc::new(registry), "Test").unwrap();
        project.insert("a", tool, &0.5, None).unwrap();
        let reject = Arc::new(AtomicBool::new(false));
        let blocked = Arc::clone(&reject);
        project
            .set_validator(move |records| {
                if blocked.load(Ordering::Relaxed)
                    || records
                        .get("a")
                        .is_some_and(|record| record.state == serde_json::json!(0.75))
                {
                    Err(Error("Candidate rejected".into()))
                } else {
                    Ok(())
                }
            })
            .unwrap();
        assert_rejected(&mut project, |project| {
            project.set_validator(|_| Err(Error("Registration rejected".into())))
        });
        project.replace("a", tool, &0.6).unwrap();
        assert_rejected(&mut project, |project| project.replace("a", tool, &0.75));
        reject.store(true, Ordering::Relaxed);
        assert_rejected(&mut project, |project| {
            project.insert("b", tool, &0.2, None)
        });
        assert!(!root.join("state/b.json").exists());
        assert_rejected(&mut project, |project| project.replace("a", tool, &0.7));
        assert_rejected(&mut project, |project| project.delete("a"));
        assert_rejected(&mut project, |project| {
            project.edit_manifest(|manifest| manifest.name = "Rejected".into())
        });
        assert_rejected(&mut project, |project| project.undo().map(|_| ()));
        reject.store(false, Ordering::Relaxed);
        project.undo().unwrap();
        reject.store(true, Ordering::Relaxed);
        assert_rejected(&mut project, |project| project.redo().map(|_| ()));
        reject.store(false, Ordering::Relaxed);
        project.redo().unwrap();
        project.begin_edit().unwrap();
        project.replace("a", tool, &0.7).unwrap();
        reject.store(true, Ordering::Relaxed);
        assert_rejected(&mut project, Project::finish_edit);
        assert_rejected(&mut project, Project::cancel_edit);
        let path = root.join("state/a.json");
        atomic_write(&path, br#"{"tool":"gain","state":0.8}"#).unwrap();
        for _ in 0..2 {
            assert_rejected(&mut project, |project| project.poll().map(|_| ()));
        }
        reject.store(false, Ordering::Relaxed);
        assert!(project.poll().unwrap());
        assert_eq!(project.read("a", tool).unwrap(), 0.8);
        assert!(!project.poll().unwrap());
        project.finish_edit().unwrap();
        project.undo().unwrap();
        assert_eq!(project.read("a", tool).unwrap(), 0.6);
        reject.store(true, Ordering::Relaxed);
        atomic_write(&path, br#"{"tool":"gain","state":0.9}"#).unwrap();
        for _ in 0..2 {
            assert_rejected(&mut project, |project| project.poll().map(|_| ()));
        }
        reject.store(false, Ordering::Relaxed);
        assert!(project.poll().unwrap());
        assert_eq!(project.read("a", tool).unwrap(), 0.9);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_edits_share_history_and_invalid_files_preserve_live_state() {
        let root = std::env::temp_dir().join(format!(
            "sound-project-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut registry = Registry::default();
        let tool = registry
            .register::<f32>("gain", |value| {
                if (0.0..=1.0).contains(value) {
                    Ok(())
                } else {
                    Err(Error("Invalid gain".into()))
                }
            })
            .unwrap();
        let registry = Arc::new(registry);
        let mut project = Project::create(&root, registry.clone(), "Test").unwrap();
        project.insert("a", tool, &0.5, None).unwrap();
        let path = root.join("state/a.json");
        atomic_write(&path, br#"{"tool":"gain","state":0.8}"#).unwrap();
        assert!(project.poll().unwrap());
        assert_eq!(project.read("a", tool).unwrap(), 0.8);
        assert!(!project.poll().unwrap());
        project.undo().unwrap();
        assert_eq!(project.read("a", tool).unwrap(), 0.5);
        project.redo().unwrap();
        atomic_write(&path, br#"{"tool":"gain","state":5}"#).unwrap();
        assert!(project.poll().is_err());
        assert_eq!(project.read("a", tool).unwrap(), 0.8);
        project.replace("a", tool, &0.6).unwrap();
        project.begin_edit().unwrap();
        project.replace("a", tool, &0.4).unwrap();
        assert!(!project.poll().unwrap());
        project.finish_edit().unwrap();
        project.undo().unwrap();
        assert_eq!(project.read("a", tool).unwrap(), 0.6);
        let restored = Project::open(&root, registry).unwrap();
        assert_eq!(restored.read("a", tool).unwrap(), 0.6);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn repeated_polls_do_not_resurrect_stale_file_edits_over_newer_gesture_updates() {
        let root = std::env::temp_dir().join(format!(
            "sound-project-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut registry = Registry::default();
        let tool = registry
            .register::<f32>("gain", |value| {
                if (0.0..=1.0).contains(value) {
                    Ok(())
                } else {
                    Err(Error("Invalid gain".into()))
                }
            })
            .unwrap();
        let registry = Arc::new(registry);
        let mut project = Project::create(&root, registry.clone(), "Test").unwrap();
        project.insert("a", tool, &0.5, None).unwrap();
        let path = root.join("state/a.json");
        atomic_write(&path, br#"{"tool":"gain","state":0.8}"#).unwrap();
        project.poll().unwrap();
        assert_eq!(project.read("a", tool).unwrap(), 0.8);
        project.begin_edit().unwrap();
        project.replace("a", tool, &0.4).unwrap();
        atomic_write(&path, br#"{"tool":"gain","state":0.9}"#).unwrap();
        project.poll().unwrap();
        assert_eq!(project.read("a", tool).unwrap(), 0.9);
        project.replace("a", tool, &0.6).unwrap();
        assert!(!project.poll().unwrap());
        assert_eq!(project.read("a", tool).unwrap(), 0.6);
        project.finish_edit().unwrap();
        atomic_write(&path, br#"{"tool":"gain","state":0.2}"#).unwrap();
        project.poll().unwrap();
        assert_eq!(project.read("a", tool).unwrap(), 0.2);
        fs::remove_dir_all(root).unwrap();
    }
}
