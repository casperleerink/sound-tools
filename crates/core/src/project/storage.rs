//! The project folder on disk: where records live, reading, atomic writing and the JSON layout.
//!
//! The file naming rule, see ARCHITECTURE.md "Project storage": the instance `a/b` of a tool
//! that owns no children is the file `state/a/b.json`. The instance of a tool that owns
//! children is the folder `state/a/b/` with its record in `instance.json` and its children next
//! to it. The tool decides the form. The runtime never moves a record between the two.

use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::file::ProjectFile;
use super::instance::{FOLDER_RECORD, InstanceId, Record};
use super::registry::Registry;

pub(crate) const PROJECT_FILE: &str = "project.json";
pub(crate) const STATE_FOLDER: &str = "state";
const LOCK_FILE: &str = ".sound-tools.lock";
const RECORD_EXTENSION: &str = "json";

/// The two forms of an instance on disk. The tool decides which one its instances have.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// `name.json`. For a tool that owns no children.
    File,
    /// `name/instance.json`. For a tool that owns children. They live next to the record.
    Folder,
}

impl Form {
    pub fn of(record: &Record) -> Self {
        if record.owns_children {
            Self::Folder
        } else {
            Self::File
        }
    }
}

/// What is on disk for one instance id.
pub(crate) enum RecordOnDisk {
    Missing,
    One(Form, Vec<u8>),
    /// `name.json` and `name/instance.json` both exist. Neither is loaded.
    Both,
}

/// The problem text for a folder that has no record of its own.
pub(crate) fn folder_without_record(folder: &str) -> String {
    format!(
        "not loaded: no {FOLDER_RECORD}.json in {folder}, so the folder is no instance and nothing in it is loaded"
    )
}

/// What the runtime last read from or wrote to a record file. A file with the same
/// fingerprint holds nothing new, which is how the runtime knows its own writes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct OnDisk {
    pub form: Form,
    pub fingerprint: u64,
}

pub(crate) fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// What a changed path means for the project.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PathTarget {
    ProjectFile,
    Record(InstanceId),
    /// A folder: the instance with this id, when it is one, and everything inside it.
    /// `None` is the `state/` folder itself.
    Folder(Option<InstanceId>),
    /// Not part of the project state, such as a temporary file of an editor.
    Ignored,
}

/// Why a record file is not loaded. The file stays as it is.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unloadable {
    /// No enabled extension offers the tool. Expected after removing an extension.
    UnknownTool(String),
    Invalid(String),
}

impl Unloadable {
    pub fn message(&self) -> String {
        match self {
            Self::UnknownTool(tool) => format!(
                "unknown tool {tool:?}: no enabled extension offers it, so the record is left untouched"
            ),
            Self::Invalid(message) => message.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordFile {
    tool: String,
    state: serde_json::Value,
}

/// Decodes and validates one record file. The error names the field, such as `state.gain`.
pub(crate) fn decode_record(
    bytes: &[u8],
    registry: &Registry,
    project_file: &ProjectFile,
) -> Result<Record, Unloadable> {
    let file: RecordFile = decode_json(bytes).map_err(Unloadable::Invalid)?;
    let definition = registry
        .definition(&file.tool)
        .filter(|definition| {
            let enabled = |extension: &String| extension == definition.extension;
            project_file.extensions.iter().any(enabled)
        })
        .ok_or_else(|| Unloadable::UnknownTool(file.tool.clone()))?;
    (definition.decode)(file.state).map_err(Unloadable::Invalid)
}

pub(crate) fn decode_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let path = error.path().to_string();
        if path == "." {
            error.inner().to_string()
        } else {
            format!("{path}: {}", error.inner())
        }
    })?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(value)
}

fn encode_record(record: &Record) -> Result<Vec<u8>, serde_json::Error> {
    let compact = format!(
        "{{\"tool\":{},\"state\":{}}}",
        serde_json::to_string(record.tool)?,
        record.state.to_json()?
    );
    Ok(layout(&compact).into_bytes())
}

/// A file could not be read or written. `path` is relative to the project folder.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("{path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("{path}: {source}")]
    Encode {
        path: String,
        source: serde_json::Error,
    },
}

pub(crate) struct Storage {
    /// Canonical, so that paths from the watcher can be compared with it.
    root: PathBuf,
    /// Held for as long as the project is open. `None` for a read-only project.
    _lock: Option<fs::File>,
    records: BTreeMap<InstanceId, OnDisk>,
    project_file: Option<u64>,
}

/// The result of trying to take the project lock.
pub(crate) enum Locked {
    Yes(Storage),
    /// Another runtime has the project open.
    No,
}

impl Storage {
    /// Creates the folder when it is missing, and takes the project lock.
    pub fn open_exclusive(folder: &Path) -> io::Result<Locked> {
        fs::create_dir_all(folder.join(STATE_FOLDER))?;
        let root = folder.canonicalize()?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join(LOCK_FILE))?;
        match lock.try_lock() {
            Ok(()) => Ok(Locked::Yes(Self::new(root, Some(lock)))),
            Err(fs::TryLockError::WouldBlock) => Ok(Locked::No),
            Err(fs::TryLockError::Error(error)) => Err(error),
        }
    }

    /// Takes no lock and must never write, so it is safe next to a running runtime.
    pub fn open_read_only(folder: &Path) -> io::Result<Self> {
        Ok(Self::new(folder.canonicalize()?, None))
    }

    fn new(root: PathBuf, lock: Option<fs::File>) -> Self {
        Self {
            root,
            _lock: lock,
            records: BTreeMap::new(),
            project_file: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn state_folder(&self) -> PathBuf {
        self.root.join(STATE_FOLDER)
    }

    fn instance_folder(&self, id: &InstanceId) -> PathBuf {
        self.state_folder().join(id.as_str())
    }

    pub fn record_path(&self, id: &InstanceId, form: Form) -> PathBuf {
        match form {
            Form::File => self.state_folder().join(format!("{id}.{RECORD_EXTENSION}")),
            Form::Folder => self
                .instance_folder(id)
                .join(format!("{FOLDER_RECORD}.{RECORD_EXTENSION}")),
        }
    }

    /// The path as problems and messages show it: relative to the project folder, with `/`.
    pub fn display_path(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        let names: Vec<_> = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect();
        names.join("/")
    }

    pub fn observed(&self, id: &InstanceId) -> Option<OnDisk> {
        self.records.get(id).copied()
    }

    pub fn observe(&mut self, id: InstanceId, on_disk: OnDisk) {
        self.records.insert(id, on_disk);
    }

    pub fn forget(&mut self, id: &InstanceId) {
        self.records.remove(id);
    }

    pub fn project_file_fingerprint(&self) -> Option<u64> {
        self.project_file
    }

    pub fn observe_project_file(&mut self, fingerprint: Option<u64>) {
        self.project_file = fingerprint;
    }

    /// Maps a path the watcher reported, or a test named, to what it means. The path may no
    /// longer exist, so a missing path counts as a record when it ends in `.json`.
    pub fn target_of(&self, path: &Path) -> PathTarget {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return PathTarget::Ignored;
        };
        if relative == Path::new(PROJECT_FILE) {
            return PathTarget::ProjectFile;
        }
        let Ok(inside_state) = relative.strip_prefix(STATE_FOLDER) else {
            return PathTarget::Ignored;
        };
        let mut names: Vec<&str> = Vec::new();
        for component in inside_state.components() {
            match component.as_os_str().to_str() {
                Some(name) => names.push(name),
                None => return PathTarget::Ignored,
            }
        }
        let Some(last) = names.pop() else {
            return PathTarget::Folder(None);
        };
        let record_name = last.strip_suffix(&format!(".{RECORD_EXTENSION}"));
        let is_record = record_name.is_some() && !path.is_dir();
        match record_name {
            // `a/instance.json` is the record of `a`.
            Some(FOLDER_RECORD) if is_record => {}
            Some(name) if is_record => names.push(name),
            _ => names.push(last),
        }
        match InstanceId::new(&names.join("/")) {
            Ok(id) if is_record => PathTarget::Record(id),
            Ok(id) => PathTarget::Folder(Some(id)),
            Err(_) => PathTarget::Ignored,
        }
    }

    /// Reads the record of `id` in whichever form it is on disk. `seen` is the one form a
    /// folder scan just saw, which saves looking for the other: at 10,000 records that is a
    /// quarter of the time to open.
    pub fn read_record(
        &self,
        id: &InstanceId,
        seen: Option<Form>,
    ) -> Result<RecordOnDisk, StorageError> {
        let mut found = RecordOnDisk::Missing;
        let forms = match seen {
            Some(form) => vec![form],
            None => vec![Form::Folder, Form::File],
        };
        for form in forms {
            let path = self.record_path(id, form);
            match fs::read(&path) {
                Ok(_) if matches!(found, RecordOnDisk::One(..)) => return Ok(RecordOnDisk::Both),
                Ok(bytes) => found = RecordOnDisk::One(form, bytes),
                // `NotADirectory`: a file stands where a parent folder would be.
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                    ) => {}
                Err(source) => return Err(self.io_error(&path, source)),
            }
        }
        Ok(found)
    }

    /// Whether the folder of `id` exists. Without a record in it, it holds orphaned files.
    pub fn has_folder(&self, id: &InstanceId) -> bool {
        self.instance_folder(id).is_dir()
    }

    pub fn folder_display_path(&self, id: &InstanceId) -> String {
        self.display_path(&self.instance_folder(id))
    }

    pub fn read_project_file(&self) -> Result<Option<Vec<u8>>, StorageError> {
        let path = self.root.join(PROJECT_FILE);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(self.io_error(&path, source)),
        }
    }

    /// Every instance id that has a record on disk inside `parent`, parents before children.
    /// `None` scans all of `state/`. Reports what it skips as (path, message).
    pub fn scan(
        &self,
        parent: Option<&InstanceId>,
        found: &mut Vec<(InstanceId, Option<Form>)>,
        problems: &mut Vec<(String, String)>,
    ) {
        let folder = match parent {
            Some(parent) => self.instance_folder(parent),
            None => self.state_folder(),
        };
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            // An instance in file form has no folder.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                return;
            }
            Err(error) => {
                problems.push((self.display_path(&folder), error.to_string()));
                return;
            }
        };
        // Name to (has a record file, has a folder), sorted for a repeatable order.
        let mut names: BTreeMap<String, (bool, bool)> = BTreeMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_folder = path.is_dir();
            let name = if is_folder {
                path.file_name()
            } else if path.extension().is_some_and(|it| it == RECORD_EXTENSION) {
                path.file_stem()
            } else {
                None
            };
            let Some(name) = name.and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_folder && name == FOLDER_RECORD {
                continue;
            }
            let forms = names.entry(name.to_string()).or_default();
            if is_folder {
                forms.1 = true;
            } else {
                forms.0 = true;
            }
        }
        for (name, (has_file, has_folder)) in names {
            let id = match parent {
                Some(parent) => parent.child(&name),
                None => InstanceId::new(&name),
            };
            let Ok(id) = id else {
                let path = self.display_path(&folder.join(&name));
                problems.push((
                    path,
                    "not loaded: names use lowercase letters, digits, `-` and `_`".to_string(),
                ));
                continue;
            };
            let has_folder_record = has_folder && self.record_path(&id, Form::Folder).is_file();
            match (has_file, has_folder_record) {
                (true, true) => found.push((id.clone(), None)),
                (true, false) => found.push((id.clone(), Some(Form::File))),
                (false, true) => found.push((id.clone(), Some(Form::Folder))),
                (false, false) => {}
            }
            if has_folder_record {
                self.scan(Some(&id), found, problems);
            } else if has_folder {
                let folder = self.folder_display_path(&id);
                problems.push((folder.clone(), folder_without_record(&folder)));
            }
        }
    }

    /// Writes the record, in the form of its tool, unless the file already holds these bytes.
    pub fn write_record(&mut self, id: &InstanceId, record: &Record) -> Result<(), StorageError> {
        let observed = self.observed(id);
        let form = Form::of(record);
        let path = self.record_path(id, form);
        let bytes = encode_record(record).map_err(|source| StorageError::Encode {
            path: self.display_path(&path),
            source,
        })?;
        let on_disk = OnDisk {
            form,
            fingerprint: fingerprint(&bytes),
        };
        if observed == Some(on_disk) {
            return Ok(());
        }
        self.write_atomically(&path, &bytes)?;
        self.observe(id.clone(), on_disk);
        // Only when the id now belongs to a tool of the other form, for example after undo.
        if let Some(previous) = observed
            && previous.form != form
        {
            self.remove_file(&self.record_path(id, previous.form))?;
        }
        Ok(())
    }

    /// Removes the record file, and the folder when nothing else is in it. Files the runtime
    /// does not know, such as records of unknown tools, are never removed.
    pub fn delete_record(&mut self, id: &InstanceId) -> Result<(), StorageError> {
        let Some(on_disk) = self.records.remove(id) else {
            return Ok(());
        };
        self.remove_file(&self.record_path(id, on_disk.form))?;
        if on_disk.form == Form::Folder {
            match fs::remove_dir(self.instance_folder(id)) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(source) => return Err(self.io_error(&self.instance_folder(id), source)),
            }
        }
        Ok(())
    }

    pub fn write_project_file(&mut self, project_file: &ProjectFile) -> Result<(), StorageError> {
        let path = self.root.join(PROJECT_FILE);
        let compact =
            serde_json::to_string(project_file).map_err(|source| StorageError::Encode {
                path: self.display_path(&path),
                source,
            })?;
        let bytes = layout(&compact).into_bytes();
        let fingerprint = fingerprint(&bytes);
        if self.project_file == Some(fingerprint) {
            return Ok(());
        }
        self.write_atomically(&path, &bytes)?;
        self.project_file = Some(fingerprint);
        Ok(())
    }

    /// Makes a generated file in the project folder hold `contents`, or removes it for `None`.
    /// A file that already holds the same bytes is left alone, so its modification time only
    /// moves when the text does.
    pub fn write_generated(&self, name: &str, contents: Option<&str>) -> Result<(), StorageError> {
        let path = self.root.join(name);
        let Some(contents) = contents else {
            return self.remove_file(&path);
        };
        if fs::read(&path).is_ok_and(|bytes| bytes == contents.as_bytes()) {
            return Ok(());
        }
        self.write_atomically(&path, contents.as_bytes())
    }

    /// Makes the generated folder `folder` hold exactly `files`, by name and text. Files in it
    /// that `files` does not name are removed, so a doc of an extension that is no longer
    /// enabled cannot mislead an agent. Subfolders are left alone: the runtime made none.
    pub fn write_generated_folder(
        &self,
        folder: &str,
        files: BTreeMap<String, String>,
    ) -> Result<(), StorageError> {
        let path = self.root.join(folder);
        for (name, contents) in &files {
            self.write_generated(&format!("{folder}/{name}"), Some(contents))?;
        }
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(self.io_error(&path, source)),
        };
        for entry in entries {
            let entry = entry.map_err(|source| self.io_error(&path, source))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !files.contains_key(&name) && entry.path().is_file() {
                self.remove_file(&entry.path())?;
            }
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<(), StorageError> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(self.io_error(path, source)),
        }
    }

    /// Writes a temporary file next to the target and renames it into place. A reader sees
    /// the old file or the new one, never a part. A failure leaves the old file complete.
    ///
    /// No `sync_all`: it costs about 6 ms per file on macOS, which made undo of a deleted
    /// folder of 100 records block for 0.7 s. The rename still protects against a crash of the
    /// process. Surviving a power loss is left to git and snapshots.
    fn write_atomically(&self, path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
        let write = || -> io::Result<()> {
            let folder = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
            fs::create_dir_all(folder)?;
            // The name does not end in `.json` and is not a valid instance name, so neither
            // the scan nor the watcher takes it for a record.
            let mut temporary = path.as_os_str().to_owned();
            temporary.push(".tmp");
            let temporary = PathBuf::from(temporary);
            let result = fs::File::create(&temporary).and_then(|mut file| {
                file.write_all(bytes)?;
                drop(file);
                fs::rename(&temporary, path)
            });
            if result.is_err() {
                // Best effort: the write already failed, and that error is the one to report.
                drop(fs::remove_file(&temporary));
            }
            result
        };
        write().map_err(|source| self.io_error(path, source))
    }

    fn io_error(&self, path: &Path, source: io::Error) -> StorageError {
        StorageError::Io {
            path: self.display_path(path),
            source,
        }
    }
}

/// A list or object longer than this goes on several lines.
const LINE_WIDTH: usize = 100;

/// Lays out compact JSON for people and agents. A list or object that fits in [`LINE_WIDTH`]
/// stays on one line, a longer one gets one line per item, and the top level always does. So
/// a record with 200 small items has about 200 lines, and changing one item is a one line diff.
pub(crate) fn layout(compact: &str) -> String {
    let mut output = Vec::with_capacity(compact.len() * 2);
    let mut parser = Layout {
        input: compact.as_bytes(),
        position: 0,
    };
    let laid_out = parser
        .value(None, &mut output)
        .filter(|()| parser.position == compact.len())
        .and_then(|()| String::from_utf8(output).ok());
    match laid_out {
        Some(laid_out) => laid_out + "\n",
        // Not reached for JSON from serde_json. Valid and compact beats a panic.
        None => format!("{compact}\n"),
    }
}

struct Layout<'a> {
    input: &'a [u8],
    position: usize,
}

impl Layout<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    /// Copies the value at the current position onto one line, with a space after each `:`
    /// and `,` outside strings.
    fn inline(&mut self, output: &mut Vec<u8>) -> Option<()> {
        let mut depth = 0_usize;
        let mut in_string = false;
        loop {
            let byte = self.peek()?;
            output.push(byte);
            self.position += 1;
            match byte {
                b'\\' if in_string => {
                    output.push(self.peek()?);
                    self.position += 1;
                }
                b'"' => in_string = !in_string,
                _ if in_string => {}
                b'[' | b'{' => depth += 1,
                b']' | b'}' => depth = depth.checked_sub(1)?,
                b':' | b',' => output.push(b' '),
                _ => {}
            }
            let ended = matches!(self.peek(), None | Some(b',' | b':' | b']' | b'}'));
            if depth == 0 && !in_string && ended {
                return Some(());
            }
        }
    }

    /// `indent` is `None` for the top level, which never goes on one line.
    fn value(&mut self, indent: Option<usize>, output: &mut Vec<u8>) -> Option<()> {
        let open = self.peek()?;
        let start = (self.position, output.len());
        self.inline(output)?;
        let width = output.len() - start.1 + indent.unwrap_or_default() * 2;
        let is_empty = self.position - start.0 == 2;
        let is_container = matches!(open, b'[' | b'{') && !is_empty;
        if !is_container || (indent.is_some() && width <= LINE_WIDTH) {
            return Some(());
        }

        // Too long for one line: start again with one item per line.
        self.position = start.0 + 1;
        output.truncate(start.1);
        output.push(open);
        let close = if open == b'[' { b']' } else { b'}' };
        let indent = indent.map_or(0, |indent| indent + 1);
        loop {
            output.push(b'\n');
            output.extend(std::iter::repeat_n(b' ', (indent + 1) * 2));
            if open == b'{' {
                self.inline(output)?;
                // Skips the `:` after the key.
                self.position += 1;
                output.extend(b": ");
            }
            self.value(Some(indent), output)?;
            let separator = self.peek()?;
            self.position += 1;
            if separator == close {
                break;
            }
            output.push(b',');
        }
        output.push(b'\n');
        output.extend(std::iter::repeat_n(b' ', indent * 2));
        output.push(close);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_puts_long_containers_on_several_lines() {
        let item =
            |start: u32| format!(r#"{{"start":{start},"length":480,"pitch":60,"velocity":100}}"#);
        let items: Vec<String> = [0, 480, 960].map(item).into();
        let compact = format!(
            r#"{{"tool":"x","state":{{"name":"a, \"b\": {{c}} é","items":[{}],"empty":[],"flag":true}}}}"#,
            items.join(",")
        );
        let expected = r#"{
  "tool": "x",
  "state": {
    "name": "a, \"b\": {c} é",
    "items": [
      {"start": 0, "length": 480, "pitch": 60, "velocity": 100},
      {"start": 480, "length": 480, "pitch": 60, "velocity": 100},
      {"start": 960, "length": 480, "pitch": 60, "velocity": 100}
    ],
    "empty": [],
    "flag": true
  }
}
"#;
        let laid_out = layout(&compact);
        assert_eq!(laid_out, expected);
        let reparsed: serde_json::Value = serde_json::from_str(&laid_out).unwrap();
        let original: serde_json::Value = serde_json::from_str(&compact).unwrap();
        assert_eq!(reparsed, original);
    }

    #[test]
    fn layout_keeps_short_values_on_one_line_below_the_top_level() {
        assert_eq!(layout("1.5"), "1.5\n");
        assert_eq!(layout("[]"), "[]\n");
        assert_eq!(
            layout(r#"{"a":{"b":[1,2]}}"#),
            "{\n  \"a\": {\"b\": [1, 2]}\n}\n"
        );
        assert_eq!(layout(r#"["\\\"",[3]]"#), "[\n  \"\\\\\\\"\",\n  [3]\n]\n");
    }
}
