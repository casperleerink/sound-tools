//! The few parts of git's format the history needs: loose objects (blobs, trees, commits) and
//! refs, in a bare repository that `git --git-dir` reads. No packs, no index, no merge: the
//! history only adds objects and moves refs.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

/// The SHA-1 of an object, as git names it.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId([u8; 20]);

impl ObjectId {
    pub fn parse(hex: &str) -> Option<Self> {
        let hex = hex.trim();
        if hex.len() != 40 {
            return None;
        }
        let mut bytes = [0; 20];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", &self.to_string()[..8])
    }
}

/// What a tree holds at one name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    Blob(ObjectId),
    Tree(ObjectId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Commit {
    pub(crate) tree: ObjectId,
    pub(crate) parents: Vec<ObjectId>,
    /// Seconds since 1970.
    pub(crate) time: u64,
    pub(crate) message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("the history object {0} is damaged")]
    Damaged(ObjectId),
}

/// A bare repository on disk.
pub(crate) struct Repository {
    folder: PathBuf,
}

const AUTHOR: &str = "Sound Tools <history@sound-tools>";

impl Repository {
    /// Opens the repository in `folder`, making it when it is not there.
    pub(crate) fn open(folder: &Path) -> Result<Self, GitError> {
        let repository = Self {
            folder: folder.to_path_buf(),
        };
        let head = folder.join("HEAD");
        if !head.exists() {
            for sub in ["objects", "refs/heads", "refs/tips"] {
                let path = folder.join(sub);
                fs::create_dir_all(&path).map_err(|source| GitError::Io { path, source })?;
            }
            repository.write_file(
                Path::new("config"),
                b"[core]\n\trepositoryformatversion = 0\n\tbare = true\n",
            )?;
            repository.write_file(Path::new("HEAD"), b"ref: refs/heads/main\n")?;
        }
        Ok(repository)
    }

    pub(crate) fn write_blob(&self, bytes: &[u8]) -> Result<ObjectId, GitError> {
        self.write_object("blob", bytes)
    }

    /// Entries in any order: git's order is made here.
    pub(crate) fn write_tree(
        &self,
        entries: &BTreeMap<String, Entry>,
    ) -> Result<ObjectId, GitError> {
        let mut sorted: Vec<(&String, &Entry)> = entries.iter().collect();
        // Git compares a tree's name as if it ended in a slash.
        let key = |(name, entry): &(&String, &Entry)| match entry {
            Entry::Tree(_) => format!("{name}/"),
            Entry::Blob(_) => (*name).clone(),
        };
        sorted.sort_by_key(key);
        let mut bytes = Vec::new();
        for (name, entry) in sorted {
            let (mode, id) = match entry {
                Entry::Blob(id) => ("100644", id),
                Entry::Tree(id) => ("40000", id),
            };
            bytes.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            bytes.extend_from_slice(&id.0);
        }
        self.write_object("tree", &bytes)
    }

    pub(crate) fn write_commit(&self, commit: &Commit) -> Result<ObjectId, GitError> {
        let mut text = format!("tree {}\n", commit.tree);
        for parent in &commit.parents {
            text.push_str(&format!("parent {parent}\n"));
        }
        let signature = format!("{AUTHOR} {} +0000", commit.time);
        text.push_str(&format!(
            "author {signature}\ncommitter {signature}\n\n{}\n",
            commit.message
        ));
        self.write_object("commit", text.as_bytes())
    }

    pub(crate) fn read_blob(&self, id: ObjectId) -> Result<Vec<u8>, GitError> {
        self.read_object(id, "blob")
    }

    pub(crate) fn read_tree(&self, id: ObjectId) -> Result<BTreeMap<String, Entry>, GitError> {
        let bytes = self.read_object(id, "tree")?;
        let damaged = || GitError::Damaged(id);
        let mut entries = BTreeMap::new();
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            let space = rest
                .iter()
                .position(|byte| *byte == b' ')
                .ok_or_else(damaged)?;
            let nul = rest
                .iter()
                .position(|byte| *byte == 0)
                .ok_or_else(damaged)?;
            let mode = std::str::from_utf8(&rest[..space]).map_err(|_| damaged())?;
            let name = std::str::from_utf8(rest.get(space + 1..nul).ok_or_else(damaged)?)
                .map_err(|_| damaged())?;
            let raw: [u8; 20] = (rest.get(nul + 1..nul + 21).ok_or_else(damaged)?)
                .try_into()
                .map_err(|_| damaged())?;
            let entry = match mode {
                "40000" => Entry::Tree(ObjectId(raw)),
                _ => Entry::Blob(ObjectId(raw)),
            };
            entries.insert(name.to_string(), entry);
            rest = &rest[nul + 21..];
        }
        Ok(entries)
    }

    pub(crate) fn read_commit(&self, id: ObjectId) -> Result<Commit, GitError> {
        let bytes = self.read_object(id, "commit")?;
        let text = String::from_utf8(bytes).map_err(|_| GitError::Damaged(id))?;
        let (header, message) = text.split_once("\n\n").unwrap_or((text.as_str(), ""));
        let mut tree = None;
        let mut parents = Vec::new();
        let mut time = 0;
        for line in header.lines() {
            if let Some(hex) = line.strip_prefix("tree ") {
                tree = ObjectId::parse(hex);
            } else if let Some(hex) = line.strip_prefix("parent ") {
                parents.extend(ObjectId::parse(hex));
            } else if let Some(signature) = line.strip_prefix("committer ") {
                let mut words = signature.rsplit(' ');
                words.next();
                time = words.next().and_then(|word| word.parse().ok()).unwrap_or(0);
            }
        }
        Ok(Commit {
            tree: tree.ok_or(GitError::Damaged(id))?,
            parents,
            time,
            message: message.trim_end_matches('\n').to_string(),
        })
    }

    /// The ref at `name`, such as `refs/heads/main`.
    pub(crate) fn read_ref(&self, name: &str) -> Result<Option<ObjectId>, GitError> {
        let path = self.folder.join(name);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(ObjectId::parse(&text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(GitError::Io { path, source }),
        }
    }

    pub(crate) fn write_ref(&self, name: &str, id: ObjectId) -> Result<(), GitError> {
        self.write_file(Path::new(name), format!("{id}\n").as_bytes())
    }

    pub(crate) fn remove_ref(&self, name: &str) -> Result<(), GitError> {
        let path = self.folder.join(name);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(GitError::Io { path, source }),
        }
    }

    /// The names of the refs in the folder `prefix`, such as `refs/tips`.
    pub(crate) fn refs_in(&self, prefix: &str) -> Result<Vec<String>, GitError> {
        let path = self.folder.join(prefix);
        let read = match fs::read_dir(&path) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(GitError::Io { path, source }),
        };
        let mut names = Vec::new();
        for entry in read {
            let entry = entry.map_err(|source| GitError::Io {
                path: path.clone(),
                source,
            })?;
            if let Some(name) = entry.file_name().to_str() {
                names.push(format!("{prefix}/{name}"));
            }
        }
        Ok(names)
    }

    /// A file of the repository's own, written whole or not at all.
    pub(crate) fn write_file(&self, relative: &Path, bytes: &[u8]) -> Result<(), GitError> {
        let path = self.folder.join(relative);
        write_atomically(&path, bytes).map_err(|source| GitError::Io { path, source })
    }

    pub(crate) fn read_file(&self, relative: &Path) -> Result<Option<Vec<u8>>, GitError> {
        let path = self.folder.join(relative);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(GitError::Io { path, source }),
        }
    }

    fn object_path(&self, id: ObjectId) -> PathBuf {
        let hex = id.to_string();
        self.folder.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    fn write_object(&self, kind: &str, bytes: &[u8]) -> Result<ObjectId, GitError> {
        let mut whole = format!("{kind} {}\0", bytes.len()).into_bytes();
        whole.extend_from_slice(bytes);
        let id = ObjectId(sha1_smol::Sha1::from(&whole).digest().bytes());
        let path = self.object_path(id);
        if path.exists() {
            return Ok(id);
        }
        let io_error = |source| GitError::Io {
            path: path.clone(),
            source,
        };
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&whole).map_err(io_error)?;
        let compressed = encoder.finish().map_err(io_error)?;
        write_atomically(&path, &compressed).map_err(io_error)?;
        Ok(id)
    }

    fn read_object(&self, id: ObjectId, kind: &str) -> Result<Vec<u8>, GitError> {
        let path = self.object_path(id);
        let compressed = fs::read(&path).map_err(|source| GitError::Io {
            path: path.clone(),
            source,
        })?;
        let mut whole = Vec::new();
        ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut whole)
            .map_err(|_| GitError::Damaged(id))?;
        let nul = whole.iter().position(|byte| *byte == 0);
        let nul = nul.ok_or(GitError::Damaged(id))?;
        if !whole.starts_with(format!("{kind} ").as_bytes()) {
            return Err(GitError::Damaged(id));
        }
        Ok(whole.split_off(nul + 1))
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let folder = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(folder)?;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids are git's, so `git --git-dir` reads what the history wrote.
    #[test]
    fn ids_are_those_git_gives() {
        let folder = tempfile::tempdir().unwrap();
        let repository = Repository::open(folder.path()).unwrap();
        // `printf 'hello\n' | git hash-object --stdin`
        let blob = repository.write_blob(b"hello\n").unwrap();
        assert_eq!(blob.to_string(), "ce013625030ba8dba906f756967f9e9ca394464a");
        // `git mktree` of that blob as `a.txt` and an empty tree as `b`.
        let empty = repository.write_tree(&BTreeMap::new()).unwrap();
        assert_eq!(
            empty.to_string(),
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        );
        let entries = BTreeMap::from([
            ("a.txt".to_string(), Entry::Blob(blob)),
            ("b".to_string(), Entry::Tree(empty)),
        ]);
        let tree = repository.write_tree(&entries).unwrap();
        assert_eq!(repository.read_tree(tree).unwrap(), entries);
    }

    #[test]
    fn commits_read_back() {
        let folder = tempfile::tempdir().unwrap();
        let repository = Repository::open(folder.path()).unwrap();
        let tree = repository.write_tree(&BTreeMap::new()).unwrap();
        let first = Commit {
            tree,
            parents: Vec::new(),
            time: 1_700_000_000,
            message: "Started".to_string(),
        };
        let first_id = repository.write_commit(&first).unwrap();
        let second = Commit {
            parents: vec![first_id],
            message: "Louder drums\n\nwith more lines".to_string(),
            ..first
        };
        let second_id = repository.write_commit(&second).unwrap();
        assert_eq!(repository.read_commit(first_id).unwrap(), first);
        assert_eq!(repository.read_commit(second_id).unwrap(), second);
    }
}
