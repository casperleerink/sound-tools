//! A git repository written by hand, with what history needs: loose blobs, trees and commits,
//! refs and a detached `HEAD`. Plain git reads it.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use super::super::storage::write_atomically;

/// The name of a git object: the SHA-1 of its bytes.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointId(pub(super) [u8; 20]);

impl PointId {
    /// The first seven hex digits, as git shows them.
    pub fn short(&self) -> String {
        self.to_string()[..7].to_string()
    }
}

impl fmt::Display for PointId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for PointId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.short())
    }
}

impl FromStr for PointId {
    type Err = io::Error;

    fn from_str(hex: &str) -> io::Result<Self> {
        let invalid = || invalid_data(format!("not an object name: {hex:?}"));
        if hex.len() != 40 {
            return Err(invalid());
        }
        let mut bytes = [0; 20];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let pair = hex.get(index * 2..index * 2 + 2).ok_or_else(invalid)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
        }
        Ok(Self(bytes))
    }
}

impl serde::Serialize for PointId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for PointId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        hex.parse().map_err(serde::de::Error::custom)
    }
}

/// A commit as history writes it.
#[derive(Clone, Debug)]
pub(crate) struct Commit {
    pub tree: PointId,
    pub parents: Vec<PointId>,
    /// Milliseconds since the epoch. Git keeps seconds, so the message keeps these.
    pub time: u64,
    pub label: String,
}

/// Where the milliseconds of a commit go in its message.
const TIME_TRAILER: &str = "Time: ";
const IDENTITY: &str = "Sound Tools <history@sound-tools>";
const FILE_MODE: &str = "100644";
const FOLDER_MODE: &str = "40000";

/// Files by path below a tree, `/` between names, with the blob of each.
pub(crate) type Files = BTreeMap<String, PointId>;

pub(crate) struct Repository {
    folder: PathBuf,
}

impl Repository {
    /// Opens the repository in `folder`, making it when there is none.
    pub(crate) fn open(folder: &Path) -> io::Result<Self> {
        for inside in ["objects", "refs"] {
            fs::create_dir_all(folder.join(inside))?;
        }
        let config = folder.join("config");
        if !config.exists() {
            let text = "[core]\n\trepositoryformatversion = 0\n\tfilemode = false\n\tbare = true\n";
            write_atomically(&config, text.as_bytes())?;
        }
        Ok(Self {
            folder: folder.to_path_buf(),
        })
    }

    pub(crate) fn folder(&self) -> &Path {
        &self.folder
    }

    fn object_path(&self, id: PointId) -> PathBuf {
        let hex = id.to_string();
        self.folder.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    /// Writes an object unless it is there already, and gives its name.
    fn write_object(&self, kind: &str, body: &[u8]) -> io::Result<PointId> {
        let mut bytes = format!("{kind} {}\0", body.len()).into_bytes();
        bytes.extend_from_slice(body);
        let id = PointId(sha1_smol::Sha1::from(&bytes).digest().bytes());
        let path = self.object_path(id);
        if !path.exists() {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(&bytes)?;
            if let Some(folder) = path.parent() {
                fs::create_dir_all(folder)?;
            }
            write_atomically(&path, &encoder.finish()?)?;
        }
        Ok(id)
    }

    fn read_object(&self, id: PointId, kind: &str) -> io::Result<Vec<u8>> {
        let path = self.object_path(id);
        let compressed = fs::read(&path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => invalid_data(format!(
                "the history has no object {id}, or keeps it packed, which is not read"
            )),
            _ => error,
        })?;
        let mut bytes = Vec::new();
        ZlibDecoder::new(&compressed[..]).read_to_end(&mut bytes)?;
        let header_end = (bytes.iter().position(|byte| *byte == 0))
            .ok_or_else(|| invalid_data(format!("object {id} has no header")))?;
        let header = std::str::from_utf8(&bytes[..header_end]).unwrap_or_default();
        if header.split(' ').next() != Some(kind) {
            return Err(invalid_data(format!("object {id} is not a {kind}")));
        }
        bytes.drain(..=header_end);
        Ok(bytes)
    }

    pub(crate) fn write_blob(&self, bytes: &[u8]) -> io::Result<PointId> {
        self.write_object("blob", bytes)
    }

    pub(crate) fn read_blob(&self, id: PointId) -> io::Result<Vec<u8>> {
        self.read_object(id, "blob")
    }

    /// Writes the trees that hold `files` and gives the top one.
    pub(crate) fn write_tree(&self, files: &Files) -> io::Result<PointId> {
        let mut entries: Vec<(String, &str, PointId)> = Vec::new();
        let mut folders: BTreeMap<&str, Files> = BTreeMap::new();
        for (path, blob) in files {
            match path.split_once('/') {
                None => entries.push((path.clone(), FILE_MODE, *blob)),
                Some((folder, rest)) => {
                    folders
                        .entry(folder)
                        .or_default()
                        .insert(rest.to_string(), *blob);
                }
            }
        }
        // Git sorts a folder as if its name ended in `/`.
        for (name, inside) in folders {
            entries.push((format!("{name}/"), FOLDER_MODE, self.write_tree(&inside)?));
        }
        entries.sort();
        let mut body = Vec::new();
        for (name, mode, id) in entries {
            let name = name.strip_suffix('/').unwrap_or(&name);
            body.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            body.extend_from_slice(&id.0);
        }
        self.write_object("tree", &body)
    }

    /// Every file below the tree `id`.
    pub(crate) fn read_tree(&self, id: PointId) -> io::Result<Files> {
        let mut files = Files::new();
        self.read_tree_into(id, "", &mut files)?;
        Ok(files)
    }

    fn read_tree_into(&self, id: PointId, prefix: &str, files: &mut Files) -> io::Result<()> {
        let body = self.read_object(id, "tree")?;
        let mut rest = &body[..];
        while !rest.is_empty() {
            let broken = || invalid_data(format!("tree {id} is broken"));
            let space = rest
                .iter()
                .position(|byte| *byte == b' ')
                .ok_or_else(broken)?;
            let nul = rest.iter().position(|byte| *byte == 0).ok_or_else(broken)?;
            let mode = std::str::from_utf8(&rest[..space]).map_err(|_| broken())?;
            let name = std::str::from_utf8(&rest[space + 1..nul]).map_err(|_| broken())?;
            let object = rest.get(nul + 1..nul + 21).ok_or_else(broken)?;
            let mut bytes = [0; 20];
            bytes.copy_from_slice(object);
            let path = format!("{prefix}{name}");
            match mode {
                FOLDER_MODE => self.read_tree_into(PointId(bytes), &format!("{path}/"), files)?,
                _ => _ = files.insert(path, PointId(bytes)),
            }
            rest = &rest[nul + 21..];
        }
        Ok(())
    }

    pub(crate) fn write_commit(&self, commit: &Commit) -> io::Result<PointId> {
        let mut body = format!("tree {}\n", commit.tree);
        for parent in &commit.parents {
            body.push_str(&format!("parent {parent}\n"));
        }
        let seconds = commit.time / 1000;
        body.push_str(&format!("author {IDENTITY} {seconds} +0000\n"));
        body.push_str(&format!("committer {IDENTITY} {seconds} +0000\n"));
        let label = commit.label.trim();
        body.push_str(&format!("\n{label}\n\n{TIME_TRAILER}{}\n", commit.time));
        self.write_object("commit", body.as_bytes())
    }

    pub(crate) fn read_commit(&self, id: PointId) -> io::Result<Commit> {
        let body = self.read_object(id, "commit")?;
        let text = String::from_utf8_lossy(&body);
        let (headers, message) = text.split_once("\n\n").unwrap_or((&text, ""));
        let mut tree = None;
        let mut parents = Vec::new();
        let mut seconds = 0;
        for line in headers.lines() {
            if let Some(hex) = line.strip_prefix("tree ") {
                tree = Some(hex.parse()?);
            } else if let Some(hex) = line.strip_prefix("parent ") {
                parents.push(hex.parse()?);
            } else if let Some(who) = line.strip_prefix("committer ") {
                let mut words = who.rsplit(' ');
                seconds = words.nth(1).and_then(|word| word.parse().ok()).unwrap_or(0);
            }
        }
        let tree = tree.ok_or_else(|| invalid_data(format!("commit {id} has no tree")))?;
        let mut label = Vec::new();
        let mut time = seconds * 1000;
        for line in message.lines() {
            match line.strip_prefix(TIME_TRAILER).map(str::parse) {
                Some(Ok(milliseconds)) => time = milliseconds,
                _ => label.push(line),
            }
        }
        Ok(Commit {
            tree,
            parents,
            time,
            label: label.join("\n").trim().to_string(),
        })
    }

    /// Points `HEAD` at `id`, detached.
    pub(crate) fn set_head(&self, id: PointId) -> io::Result<()> {
        write_atomically(&self.folder.join("HEAD"), format!("{id}\n").as_bytes())
    }

    pub(crate) fn head(&self) -> io::Result<Option<PointId>> {
        match fs::read_to_string(self.folder.join("HEAD")) {
            Ok(text) => text.trim().parse().map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Makes the refs under `refs/<folder>/` exactly `refs`, by name.
    pub(crate) fn set_refs(
        &self,
        folder: &str,
        refs: &BTreeMap<String, PointId>,
    ) -> io::Result<()> {
        let folder = self.folder.join("refs").join(folder);
        fs::create_dir_all(&folder)?;
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !refs.contains_key(&name) {
                fs::remove_file(entry.path())?;
            }
        }
        for (name, id) in refs {
            let path = folder.join(name);
            let line = format!("{id}\n");
            if fs::read(&path).ok().as_deref() != Some(line.as_bytes()) {
                write_atomically(&path, line.as_bytes())?;
            }
        }
        Ok(())
    }
}

fn invalid_data(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
