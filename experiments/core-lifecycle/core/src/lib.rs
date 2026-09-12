mod registry;
mod storage;
use registry::Live;
pub use registry::{Instance, Processor, Registry, State, Tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self(e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    pub tool: String,
    pub state: Value,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Connection {
    pub instance: String,
    pub port: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Index {
    instances: Vec<String>,
    connections: Vec<Connection>,
}
struct Entry {
    tool: String,
    live: Box<dyn Live>,
    observed: Vec<u8>,
}
struct Change {
    id: String,
    before: Value,
    after: Value,
    label: String,
}
/// A gesture's before-state is retained until finish or cancel. No conflict logic.
pub struct Edit<S> {
    instance: Instance<S>,
    before: Value,
    label: String,
}

pub struct Project {
    root: PathBuf,
    registry: Registry,
    entries: BTreeMap<String, Entry>,
    connections: Vec<Connection>,
    undo: Vec<Change>,
    redo: Vec<Change>,
    playing: bool,
}
impl Project {
    pub fn open(root: impl AsRef<Path>, registry: Registry) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("state"))?;
        let index = if root.join("project.json").exists() {
            serde_json::from_slice::<Index>(&fs::read(root.join("project.json"))?)?
        } else {
            Index::default()
        };
        let mut project = Self {
            root,
            registry,
            entries: BTreeMap::new(),
            connections: vec![],
            undo: vec![],
            redo: vec![],
            playing: false,
        };
        for id in index.instances {
            Self::check_id(&id)?;
            let observed = fs::read(project.path(&id))?;
            let record: Record = serde_json::from_slice(&observed)?;
            let live = (project.registry.definition(&record.tool)?.factory)(record.state)?;
            project.entries.insert(
                id,
                Entry {
                    tool: record.tool,
                    live,
                    observed,
                },
            );
        }
        for connection in index.connections {
            project.check_connection(&connection)?;
            project.connections.push(connection);
        }
        Ok(project)
    }
    fn check_id(id: &str) -> Result<()> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Error(
                "Instance IDs use letters, numbers, hyphens or underscores".into(),
            ));
        }
        Ok(())
    }
    fn path(&self, id: &str) -> PathBuf {
        self.root.join("state").join(format!("{id}.json"))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
    pub fn tool_name(&self, id: &str) -> Option<&str> {
        self.entries.get(id).map(|e| e.tool.as_str())
    }
    pub fn record(&self, id: &str) -> Result<Record> {
        let entry = self
            .entries
            .get(id)
            .ok_or_else(|| Error(format!("Missing instance: {id}")))?;
        Ok(Record {
            tool: entry.tool.clone(),
            state: entry.live.json()?,
        })
    }
    pub fn resolve<S: State>(&self, tool: Tool<S>, id: &str) -> Result<Instance<S>> {
        let entry = self
            .entries
            .get(id)
            .ok_or_else(|| Error(format!("Missing instance: {id}")))?;
        if entry.tool != tool.name || !entry.live.state().is::<S>() {
            return Err(Error("Tool type mismatch".into()));
        }
        Ok(Instance::new(id.into()))
    }
    pub fn state<S: State>(&self, instance: &Instance<S>) -> Result<&S> {
        self.entries
            .get(instance.id())
            .and_then(|e| e.live.state().downcast_ref())
            .ok_or_else(|| Error(format!("Missing or wrong instance type: {}", instance.id())))
    }
    pub fn create<S: State>(&mut self, tool: Tool<S>, id: &str, state: S) -> Result<Instance<S>> {
        Self::check_id(id)?;
        if self.entries.contains_key(id) || self.path(id).exists() {
            return Err(Error(format!("Instance already exists: {id}")));
        }
        let value = serde_json::to_value(state)?;
        let live = (self.registry.definition(tool.name)?.factory)(value.clone())?;
        let observed = storage::write(
            &self.path(id),
            &Record {
                tool: tool.name.into(),
                state: value,
            },
        )?;
        self.entries.insert(
            id.into(),
            Entry {
                tool: tool.name.into(),
                live,
                observed,
            },
        );
        self.write_index()?;
        Ok(Instance::new(id.into()))
    }
    fn write_index(&self) -> Result<()> {
        storage::write(
            &self.root.join("project.json"),
            &Index {
                instances: self.entries.keys().cloned().collect(),
                connections: self.connections.clone(),
            },
        )?;
        Ok(())
    }
    fn persist(&mut self, id: &str) -> Result<()> {
        let bytes = storage::write(&self.path(id), &self.record(id)?)?;
        self.entries.get_mut(id).unwrap().observed = bytes;
        Ok(())
    }
    fn replace(&mut self, id: &str, value: Value) -> Result<()> {
        self.entries
            .get_mut(id)
            .ok_or_else(|| Error(format!("Missing instance: {id}")))?
            .live
            .replace(value)
    }
    fn remember(&mut self, change: Change) {
        self.undo.push(change);
        self.redo.clear();
    }
    pub fn begin<S: State>(
        &self,
        instance: &Instance<S>,
        label: impl Into<String>,
    ) -> Result<Edit<S>> {
        Ok(Edit {
            instance: instance.clone(),
            before: serde_json::to_value(self.state(instance)?)?,
            label: label.into(),
        })
    }
    pub fn publish<S: State>(&mut self, edit: &Edit<S>, update: impl FnOnce(&mut S)) -> Result<()> {
        let mut next = self.state(&edit.instance)?.clone();
        update(&mut next);
        self.replace(edit.instance.id(), serde_json::to_value(next)?)
    }
    pub fn finish<S: State>(&mut self, edit: Edit<S>) -> Result<()> {
        let after = self.record(edit.instance.id())?.state;
        self.persist(edit.instance.id())?;
        self.remember(Change {
            id: edit.instance.id,
            before: edit.before,
            after,
            label: edit.label,
        });
        Ok(())
    }
    pub fn cancel<S: State>(&mut self, edit: Edit<S>) -> Result<()> {
        self.replace(edit.instance.id(), edit.before)?;
        self.persist(edit.instance.id())?;
        self.redo.clear();
        Ok(())
    }
    pub fn edit<S: State>(
        &mut self,
        instance: &Instance<S>,
        label: impl Into<String>,
        update: impl FnOnce(&mut S),
    ) -> Result<()> {
        let edit = self.begin(instance, label)?;
        self.publish(&edit, update)?;
        self.finish(edit)
    }
    /// Poll known records. Own writes are remembered, so they do not reapply.
    pub fn poll_files(&mut self) -> Result<usize> {
        let ids: Vec<_> = self.entries.keys().cloned().collect();
        let mut changed = 0;
        for id in ids {
            let bytes = fs::read(self.path(&id))?;
            if bytes == self.entries[&id].observed {
                continue;
            }
            let record: Record = serde_json::from_slice(&bytes)?;
            if record.tool != self.entries[&id].tool {
                return Err(Error("Changing a record's tool requires reload".into()));
            }
            let before = self.record(&id)?.state;
            self.replace(&id, record.state.clone())?;
            self.entries.get_mut(&id).unwrap().observed = bytes;
            self.remember(Change {
                id,
                before,
                after: record.state,
                label: "File edit".into(),
            });
            changed += 1;
        }
        Ok(changed)
    }
    pub fn undo(&mut self) -> Result<()> {
        if let Some(change) = self.undo.last() {
            let id = change.id.clone();
            let before = change.before.clone();
            self.replace(&id, before)?;
            self.persist(&id)?;
            self.redo.push(self.undo.pop().unwrap());
        }
        Ok(())
    }
    pub fn redo(&mut self) -> Result<()> {
        if let Some(change) = self.redo.last() {
            let id = change.id.clone();
            let after = change.after.clone();
            self.replace(&id, after)?;
            self.persist(&id)?;
            self.undo.push(self.redo.pop().unwrap());
        }
        Ok(())
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|c| c.label.as_str())
    }
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
    fn check_connection(&self, connection: &Connection) -> Result<()> {
        let record = self.record(&connection.instance)?;
        if self.registry.definition(&record.tool)?.output != Some(connection.port.as_str()) {
            return Err(Error("No compatible mono audio output".into()));
        }
        Ok(())
    }
    pub fn connect<S: State>(&mut self, instance: &Instance<S>, port: &str) -> Result<()> {
        self.state(instance)?;
        let connection = Connection {
            instance: instance.id.clone(),
            port: port.into(),
        };
        self.check_connection(&connection)?;
        if !self.connections.contains(&connection) {
            self.connections.push(connection);
        }
        self.write_index()
    }
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }
    pub fn delete<S: State>(&mut self, instance: &Instance<S>) -> Result<()> {
        self.state(instance)?;
        self.playing = false;
        self.entries.remove(instance.id());
        self.connections.retain(|c| c.instance != instance.id);
        self.undo.retain(|c| c.id != instance.id);
        self.redo.retain(|c| c.id != instance.id);
        self.write_index()?;
        fs::remove_file(self.path(instance.id()))?;
        Ok(())
    }
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }
    pub fn playing(&self) -> bool {
        self.playing
    }
    /// Offline mono bus. A caller owns the output buffer and sample rate.
    pub fn render(&mut self, output: &mut [f32], sample_rate: f32) {
        output.fill(0.0);
        if !self.playing {
            return;
        }
        let mut scratch = [0.0; 256];
        for connection in &self.connections {
            if let Some(entry) = self.entries.get_mut(&connection.instance) {
                for chunk in output.chunks_mut(scratch.len()) {
                    let buffer = &mut scratch[..chunk.len()];
                    buffer.fill(0.0);
                    entry.live.render(buffer, sample_rate);
                    for (out, sample) in chunk.iter_mut().zip(buffer.iter()) {
                        *out += sample;
                    }
                }
            }
        }
    }
}
