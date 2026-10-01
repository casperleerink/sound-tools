//! The download of a provider's pinned program into the machine's support folder.
//!
//! `/usr/bin/curl` fetches it unmodified, so the app needs no HTTP or TLS code of its own, and
//! the file is checked against the pinned sha256 before it is used. It is written under a
//! temporary name and renamed only once it checks out, so a half file never runs. A broken
//! download resumes where it stopped.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use sha2::{Digest, Sha256};
use smol::future;
use smol::io::AsyncReadExt;

/// On macOS and on every Linux desktop. Its TLS is the system's.
const CURL: &str = "/usr/bin/curl";

/// How often the progress looks at the bytes on disk.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// curl's exit code for an HTTP error.
const HTTP_ERROR: i32 = 22;
/// curl's exit code when the server does not resume. It also comes for an error page, which
/// has no range: only a fresh start shows what the server says.
const CANNOT_RESUME: i32 = 33;

/// The longest server message shown. A longer answer is a page, not a message.
const MESSAGE_LENGTH: usize = 300;

/// A pinned program a provider downloads, with the values its driver fills in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    /// The program's file name, and the folder its versions are kept in, such as `claude`.
    pub name: &'static str,
    pub version: &'static str,
    pub url: String,
    /// Lowercase hex.
    pub sha256: &'static str,
    /// In bytes.
    pub size: u64,
}

impl Download {
    /// Where the program is once it checked out: `<agents>/<name>/<version>/<name>`.
    pub fn program(&self, agents: &Path) -> PathBuf {
        self.folder(agents).join(self.name)
    }

    fn folder(&self, agents: &Path) -> PathBuf {
        agents.join(self.name).join(self.version)
    }

    /// Where the bytes go until they check out.
    fn partial(&self, agents: &Path) -> PathBuf {
        self.folder(agents).join(format!("{}.partial", self.name))
    }
}

#[derive(Debug)]
pub enum InstallError {
    /// curl did not get it, such as with no connection. What came stays on disk, so the next
    /// try resumes.
    Network { detail: String },
    /// The server answered with an error, such as for a region it does not serve.
    /// `message` is what it said, or curl's line when it said nothing readable.
    Refused { message: String },
    /// The file does not match the pinned checksum. It was deleted.
    Damaged,
    /// Writing on this machine failed.
    Saving(io::Error),
}

impl InstallError {
    /// One sentence for the composer. `title` is the program's name, such as "Claude Code".
    pub fn sentence(&self, title: &str) -> String {
        match self {
            InstallError::Network { .. } => {
                format!("Could not download {title}. Check the internet connection.")
            }
            InstallError::Refused { message } => format!("Could not download {title}: {message}"),
            InstallError::Damaged => "The download was damaged.".to_string(),
            InstallError::Saving(error) => format!("Could not save {title}: {error}."),
        }
    }
}

/// Downloads the program into `agents`, checks it, makes it executable and removes the other
/// versions. Gives the program's path. `progress` hears the bytes on disk a few times a second.
///
/// Dropping the future cancels it: curl is killed, and what came stays for the next try.
pub async fn install(
    download: &Download,
    agents: &Path,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, InstallError> {
    let partial = download.partial(agents);
    fs::create_dir_all(download.folder(agents)).map_err(InstallError::Saving)?;
    // More than the whole file is not the start of it.
    if length(&partial) > download.size {
        fs::remove_file(&partial).map_err(InstallError::Saving)?;
    }
    if length(&partial) < download.size {
        fetch(&download.url, &partial, &mut progress).await?;
    }
    progress(length(&partial));
    let sha256 = smol::unblock({
        let partial = partial.clone();
        move || sha256(&partial)
    })
    .await
    .map_err(InstallError::Saving)?;
    if sha256 != download.sha256 {
        fs::remove_file(&partial).map_err(InstallError::Saving)?;
        return Err(InstallError::Damaged);
    }
    fs::set_permissions(&partial, fs::Permissions::from_mode(0o755))
        .map_err(InstallError::Saving)?;
    let program = download.program(agents);
    fs::rename(&partial, &program).map_err(InstallError::Saving)?;
    // The new one works without them, so a folder that stays is only space.
    if let Err(error) = remove_other_versions(download, agents) {
        eprintln!("Could not remove an older {}: {error}", download.name);
    }
    Ok(program)
}

/// Runs curl until the file is whole, from where `partial` ends.
async fn fetch(
    url: &str,
    partial: &Path,
    progress: &mut impl FnMut(u64),
) -> Result<(), InstallError> {
    loop {
        let had = length(partial);
        let mut child = smol::process::Command::new(CURL)
            .args(["--silent", "--show-error", "--location"])
            // Fails on an HTTP error, and still writes what the server said, so a region block
            // can be shown in its own words.
            .arg("--fail-with-body")
            // Resumes from the end of the file.
            .args(["--continue-at", "-"])
            .arg("--output")
            .arg(partial)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            // Cancel drops the future, and with it curl.
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| InstallError::Network {
                detail: format!("curl did not start: {error}"),
            })?;
        let status = loop {
            // Runs on the background executor, never in a gpui test, where this timer would
            // not be deterministic.
            #[allow(clippy::disallowed_methods)]
            let tick = async {
                smol::Timer::after(PROGRESS_INTERVAL).await;
                None
            };
            let exit = async { Some(child.status().await) };
            if let Some(status) = future::or(exit, tick).await {
                break status;
            }
            progress(length(partial));
        };
        let status = status.map_err(|error| InstallError::Network {
            detail: error.to_string(),
        })?;
        if status.success() {
            return Ok(());
        }
        let mut said = String::new();
        if let Some(stderr) = &mut child.stderr {
            // What curl said is only the detail of the error; without it the error stays.
            stderr.read_to_string(&mut said).await.ok();
        }
        let said = curl_line(&said);
        match status.code() {
            Some(CANNOT_RESUME) if had > 0 => {
                fs::remove_file(partial).map_err(InstallError::Saving)?;
            }
            Some(HTTP_ERROR) => {
                let answer = take_answer(partial, had).map_err(InstallError::Saving)?;
                let message = server_message(&answer).unwrap_or(said);
                return Err(InstallError::Refused { message });
            }
            _ => return Err(InstallError::Network { detail: said }),
        }
    }
}

/// The bytes of the file, or 0 when there is none.
fn length(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |metadata| metadata.len())
}

/// The server's answer, which curl wrote after the first `had` bytes, and the file as it was.
fn take_answer(partial: &Path, had: u64) -> io::Result<Vec<u8>> {
    let mut file = File::options().read(true).write(true).open(partial)?;
    file.seek(SeekFrom::Start(had))?;
    let mut answer = Vec::new();
    file.read_to_end(&mut answer)?;
    file.set_len(had)?;
    if had == 0 {
        fs::remove_file(partial)?;
    }
    Ok(answer)
}

/// One readable line of the server's answer: the message of a JSON error, or the first line of
/// plain text. `None` for a page or nothing.
fn server_message(answer: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(answer).ok()?.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        let message = [
            value.pointer("/error/message"),
            value.get("message"),
            value.get("error"),
        ]
        .into_iter()
        .flatten()
        .find_map(serde_json::Value::as_str)?;
        return Some(message.trim().to_string()).filter(|message| !message.is_empty());
    }
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let page = line.starts_with('<');
    (!page && line.chars().count() <= MESSAGE_LENGTH).then(|| line.to_string())
}

/// curl's first line without its `curl: (22) ` prefix.
fn curl_line(said: &str) -> String {
    let line = said.lines().map(str::trim).find(|line| !line.is_empty());
    let line = line.unwrap_or("curl failed");
    let line = line.strip_prefix("curl: ").unwrap_or(line);
    match line.split_once(") ") {
        Some((code, rest)) if code.starts_with('(') => rest.to_string(),
        _ => line.to_string(),
    }
}

fn sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        let Some(bytes) = buffer.get(..read).filter(|bytes| !bytes.is_empty()) else {
            break;
        };
        hasher.update(bytes);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn remove_other_versions(download: &Download, agents: &Path) -> io::Result<()> {
    for entry in fs::read_dir(agents.join(download.name))? {
        let entry = entry?;
        if entry.file_name() == download.version {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
