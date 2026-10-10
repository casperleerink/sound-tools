//! The app updates itself from the latest GitHub release, as an Electron app does.
//!
//! When the window opens, and every day while it stays open, it asks GitHub for the latest release. A newer
//! one is downloaded in the background with the agent's downloader (the system's curl, a lock,
//! resume), checked against the release's `SHA256SUMS` and unpacked in the support folder.
//! The window then shows a notice with **Restart**. The swap itself happens at the next start
//! of the windowed app, before any window: Restart is a quit and a start again, and a quit
//! without Restart gets the update at the next launch. GPUI gives a quit too little time to
//! copy an app.
//!
//! On macOS the `.app` the program runs from is replaced. When its folder cannot be written,
//! as for an app macOS runs from a read-only copy, the notice offers **Download**, which opens
//! the release page. On Linux the tarball's `install.sh` writes into `~/.local`, and on Windows
//! the zip's `install.ps1` into `%LOCALAPPDATA%\Programs\Sound Tools`.
//!
//! A failed check or download is quiet: one line on stderr, and the next check tries again.
//! A dev build and the command line forms never check.
//!
//! In the support folder:
//!
//! ```text
//! updates/sound-tools/<version>/sound-tools   the archive of the release, checked
//! updates/sound-tools/<version>/unpacked/     what is in it, once whole
//! ```

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use gpui::{App, AppContext as _, Global};
use serde::Deserialize;
use sound_agent::Download;
use sound_core::process::curl;
#[cfg(windows)]
use sound_core::process::{background_command, windows_program};

use crate::app::{self, TOOL_NAME};

/// The JSON of the latest release. GitHub leaves drafts and prereleases out of it.
const LATEST_RELEASE: &str =
    "https://api.github.com/repos/casperleerink/sound-tools/releases/latest";

/// The asset of every release with the sha256 of the others, as `sha256sum` writes it.
const SUMS: &str = "SHA256SUMS";

/// How often an app that stays open looks again.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

const UPDATES_FOLDER: &str = "updates";
const UNPACKED: &str = "unpacked";

/// Where the installed program is on Linux, which `tooling/linux/install.sh` writes.
#[cfg(unix)]
const LINUX_PROGRAM: &str = ".local/lib/sound-tools/sound-tools";

/// `major.minor.patch`. A version with more, such as `1.0.0-beta`, is a prerelease and never
/// offered.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    /// Takes `0.2.0` and the tag `v0.2.0`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.').map(|part| part.parse().ok());
        let version = Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
        };
        parts.next().is_none().then_some(version)
    }

    fn current() -> Option<Self> {
        Self::parse(env!("CARGO_PKG_VERSION"))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A newer release the window tells the composer about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub version: Version,
    pub action: Action,
}

impl Global for Ready {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// It is downloaded: a start again installs it.
    Restart,
    /// The app's folder cannot be written. The release page, to install it by hand.
    Download { page: String },
}

/// The file of a release for this computer, as `tooling/bundle-*.sh` names it. `None` where
/// there is no build, such as an Intel Mac.
fn asset_name(version: Version, os: &str, arch: &str) -> Option<String> {
    match (os, arch) {
        ("macos", "aarch64") => Some(format!("Sound-Tools-{version}-macos-arm64.zip")),
        ("linux", "x86_64" | "aarch64") => {
            Some(format!("sound-tools-{version}-linux-{arch}.tar.gz"))
        }
        ("windows", "x86_64") => Some(format!("sound-tools-{version}-windows-x86_64.zip")),
        _ => None,
    }
}

/// The sha256 of `name` in a `SHA256SUMS` file.
fn checksum_of(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sha256, file) = line.split_once(char::is_whitespace)?;
        // `sha256sum` marks a file it read as binary with a `*`.
        let file = file.trim_start().trim_start_matches('*');
        (file == name).then(|| sha256.to_ascii_lowercase())
    })
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

impl Release {
    fn asset(&self, name: &str) -> Result<&Asset> {
        self.assets
            .iter()
            .find(|asset| asset.name == name)
            .with_context(|| format!("release {} has no {name}", self.tag_name))
    }
}

/// How an update replaces this copy of the app.
#[derive(Clone, Debug)]
enum Place {
    /// macOS: the `.app` the program runs from, replaced as a whole.
    Bundle(PathBuf),
    /// Linux and Windows: the archive's install script, which writes `program` and the rest:
    /// `install.sh` into `~/.local`, `install.ps1` into `%LOCALAPPDATA%\Programs`.
    Script { program: PathBuf },
}

/// The update of one copy of the app.
pub struct Updater {
    /// `updates/` in the support folder.
    folder: PathBuf,
    /// The JSON of the latest release, any URL curl takes.
    latest: String,
    current: Version,
    os: &'static str,
    arch: &'static str,
    place: Place,
}

impl Updater {
    /// The updater of this program, or `None` when it does not update: a dev build, a program
    /// started with `SOUND_TOOLS_NO_UPDATES` set (the perf harness of `tooling/perf`), a
    /// release build that is not the app (`cargo build --release` names the program `runtime`),
    /// and on macOS a program outside a `.app`.
    fn of_this_app(support: &Path) -> Option<Self> {
        if cfg!(debug_assertions) || std::env::var_os("SOUND_TOOLS_NO_UPDATES").is_some() {
            return None;
        }
        let program = dunce::canonicalize(std::env::current_exe().ok()?).ok()?;
        if *program.file_name()? != *app::program_file_name() {
            return None;
        }
        let place = if cfg!(target_os = "macos") {
            // `Sound Tools.app/Contents/MacOS/sound-tools`.
            let bundle = program.ancestors().nth(3)?;
            if bundle.extension()? != "app" {
                return None;
            }
            Place::Bundle(bundle.to_path_buf())
        } else {
            Place::Script {
                program: installed_program()?,
            }
        };
        Some(Self {
            folder: support.join(UPDATES_FOLDER),
            latest: LATEST_RELEASE.to_string(),
            current: Version::current()?,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            place,
        })
    }

    fn version_folder(&self, version: Version) -> PathBuf {
        self.folder.join(TOOL_NAME).join(version.to_string())
    }

    /// Looks for a newer release, and downloads it.
    async fn check(&self) -> Result<Option<Ready>> {
        let release: Release = serde_json::from_slice(&fetch(&self.latest).await?)
            .context("the latest release is not what GitHub sends")?;
        if release.draft || release.prerelease {
            return Ok(None);
        }
        let Some(version) =
            Version::parse(&release.tag_name).filter(|version| *version > self.current)
        else {
            return Ok(None);
        };
        let Some(name) = asset_name(version, self.os, self.arch) else {
            return Ok(None);
        };
        let asset = release.asset(&name)?;
        if let Place::Bundle(bundle) = &self.place
            && !can_replace(bundle)
        {
            let page = release.html_url;
            return Ok(Some(Ready {
                version,
                action: Action::Download { page },
            }));
        }
        let sums = fetch(&release.asset(SUMS)?.browser_download_url).await?;
        let sha256 = checksum_of(&String::from_utf8_lossy(&sums), &name)
            .with_context(|| format!("{SUMS} has no line for {name}"))?;
        let download = Download {
            name: TOOL_NAME,
            version: version.to_string(),
            url: asset.browser_download_url.clone(),
            sha256,
            size: asset.size,
        };
        let archive = sound_agent::install(&download, &self.folder, |_| {})
            .await
            .map_err(|error| anyhow!(error.sentence(&format!("Sound Tools {version}"))))?;
        let unpacked = self.version_folder(version).join(UNPACKED);
        let tool = match self.place {
            Place::Bundle(_) => Unpack::Ditto,
            Place::Script { .. } => Unpack::Archive,
        };
        smol::unblock(move || unpack(tool, &archive, &unpacked)).await?;
        Ok(Some(Ready {
            version,
            action: Action::Restart,
        }))
    }

    /// The newest update that is unpacked and newer than this app.
    fn pending(&self) -> Option<(Version, PathBuf)> {
        fs::read_dir(self.folder.join(TOOL_NAME))
            .ok()?
            .flatten()
            .filter_map(|entry| {
                let version = Version::parse(entry.file_name().to_str()?)?;
                let unpacked = entry.path().join(UNPACKED);
                (version > self.current && unpacked.is_dir()).then_some((version, unpacked))
            })
            .max_by_key(|(version, _)| *version)
    }

    /// Puts the pending update in the place of this app, and gives the program to start. The
    /// update goes either way: one that fails is not tried at every launch, the next check
    /// downloads it again.
    fn install_pending(&self) -> Result<Option<PathBuf>> {
        let Some((version, unpacked)) = self.pending() else {
            return Ok(None);
        };
        let installed = match &self.place {
            Place::Bundle(bundle) => replace_bundle(bundle, &only_entry(&unpacked)?)
                .map(|()| bundle.join("Contents/MacOS").join(TOOL_NAME)),
            Place::Script { program } => {
                run_install_script(&only_entry(&unpacked)?).map(|()| program.clone())
            }
        };
        if let Err(error) = fs::remove_dir_all(self.version_folder(version)) {
            eprintln!("error: could not remove the update {version}: {error}");
        }
        installed
            .with_context(|| format!("Sound Tools {version} did not install"))
            .map(Some)
    }

    /// Removes the programs an update moved aside. `install.ps1` renames the running
    /// `sound-tools.exe` and `bun.exe` to `.old`, because Windows cannot overwrite a running
    /// program but can rename it. Linux and macOS leave none.
    fn remove_old_programs(&self) {
        let Place::Script { program } = &self.place else {
            return;
        };
        let bun = program.with_file_name(format!("bun{}", std::env::consts::EXE_SUFFIX));
        for old in [program, &bun].map(|program| with_suffix(program, ".old")) {
            match fs::remove_file(&old) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                // The program that started this one, or a Bun of another window, may still be
                // ending. The next start tries again.
                Err(error) => eprintln!("error: could not remove {}: {error}", old.display()),
            }
        }
    }
}

/// The program the install script of the release writes.
#[cfg(unix)]
fn installed_program() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(LINUX_PROGRAM))
}

/// What `tooling/windows/install.ps1` writes.
#[cfg(windows)]
fn installed_program() -> Option<PathBuf> {
    let folder = app::local_app_data()
        .ok()?
        .join("Programs")
        .join("Sound Tools");
    Some(folder.join(app::program_file_name()))
}

/// curl's body of `url`, which may be a `file://` one.
async fn fetch(url: &str) -> Result<Vec<u8>> {
    let output = smol::process::Command::from(curl())
        .args(["--silent", "--show-error", "--location", "--fail"])
        .args(["--max-time", "60"])
        .args(["--header", "Accept: application/vnd.github+json"])
        .arg(url)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .context("could not run curl")?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        bail!("could not fetch {url}: {}", said.trim());
    }
    Ok(output.stdout)
}

/// Whether the folder of the `.app` takes a new one. A read-only volume, or the copy macOS
/// runs a downloaded app from, does not.
fn can_replace(bundle: &Path) -> bool {
    let Some(folder) = bundle.parent() else {
        return false;
    };
    let probe = folder.join(".sound-tools-update-probe");
    fs::File::create(&probe).is_ok() && fs::remove_file(&probe).is_ok()
}

#[derive(Clone, Copy)]
enum Unpack {
    /// The zip of the macOS app, which `ditto` made.
    Ditto,
    /// The tarball of Linux, or the zip of Windows.
    Archive,
}

/// Unpacks `archive` into `unpacked`, which appears only once whole. Another window that
/// unpacked the same already is fine.
fn unpack(tool: Unpack, archive: &Path, unpacked: &Path) -> Result<()> {
    if unpacked.is_dir() {
        return Ok(());
    }
    let partial = with_suffix(unpacked, &format!(".partial-{}", std::process::id()));
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    fs::create_dir_all(&partial)?;
    let mut command = match tool {
        Unpack::Ditto => {
            let mut command = Command::new("/usr/bin/ditto");
            command.args(["-x", "-k"]).arg(archive).arg(&partial);
            command
        }
        Unpack::Archive => {
            let mut command = unpack_archive();
            command.arg(archive).arg("-C").arg(&partial);
            command
        }
    };
    run(&mut command)?;
    if let Err(error) = fs::rename(&partial, unpacked) {
        fs::remove_dir_all(&partial)?;
        if !unpacked.is_dir() {
            return Err(error.into());
        }
    }
    Ok(())
}

/// The one file or folder an archive holds: `Sound Tools.app`, or the tarball's folder.
fn only_entry(folder: &Path) -> Result<PathBuf> {
    let mut entries = fs::read_dir(folder)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| !path.file_name().is_some_and(is_hidden));
    match (entries.next(), entries.next()) {
        (Some(entry), None) => Ok(entry),
        _ => bail!("{} does not hold one app", folder.display()),
    }
}

fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

/// Replaces `bundle` with a copy of `new`, next to it, so that the last step is a rename and
/// a failure halfway leaves the old app. The running app keeps its files open, so it may be
/// the one replaced.
fn replace_bundle(bundle: &Path, new: &Path) -> Result<()> {
    let incoming = with_suffix(&hidden(bundle), ".update");
    let outgoing = with_suffix(&hidden(bundle), ".old");
    for leftover in [&incoming, &outgoing] {
        if leftover.exists() {
            fs::remove_dir_all(leftover)?;
        }
    }
    // A copy, not a rename: the support folder may be on another volume. ditto keeps the
    // signature and the links of the bundle.
    run(Command::new("/usr/bin/ditto").arg(new).arg(&incoming))?;
    fs::rename(bundle, &outgoing)?;
    if let Err(error) = fs::rename(&incoming, bundle) {
        fs::rename(&outgoing, bundle)?;
        return Err(error.into());
    }
    if let Err(error) = fs::remove_dir_all(&outgoing) {
        eprintln!("error: could not remove {}: {error}", outgoing.display());
    }
    Ok(())
}

#[cfg(unix)]
fn unpack_archive() -> Command {
    let mut command = Command::new("tar");
    command.arg("-xzf");
    command
}

/// The tar of Windows 10 and later, which unpacks a zip too. Never a tar on the `PATH`: the
/// GNU tar of Git for Windows takes the `C:` of a path for the name of another computer.
#[cfg(windows)]
fn unpack_archive() -> Command {
    let mut command = background_command(windows_program(r"System32\tar.exe"));
    command.arg("-xf");
    command
}

#[cfg(unix)]
fn run_install_script(folder: &Path) -> Result<()> {
    run(Command::new("sh")
        .arg(folder.join("install.sh"))
        .current_dir(folder)
        .stdout(Stdio::null()))
}

/// The policy of a new Windows refuses to run scripts, so this one call bypasses it.
#[cfg(windows)]
fn run_install_script(folder: &Path) -> Result<()> {
    let powershell = windows_program(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    run(background_command(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(folder.join("install.ps1"))
        .current_dir(folder)
        .stdout(Stdio::null()))
}

fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("could not run {:?}", command.get_program()))?;
    if !status.success() {
        bail!("{:?} failed: {status}", command.get_program());
    }
    Ok(())
}

/// `/Applications/.Sound Tools.app` for `/Applications/Sound Tools.app`.
fn hidden(path: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    path.with_file_name(name)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// At the start of the windowed app, before any window: installs an update that was
/// downloaded and starts it with the same arguments. True when it did, and this process ends.
/// A failure is a line on stderr, and the app that is installed starts.
pub fn start_pending_update() -> bool {
    let Some(updater) = app::support_folder()
        .ok()
        .and_then(|support| Updater::of_this_app(&support))
    else {
        return false;
    };
    updater.remove_old_programs();
    let program = match updater.install_pending() {
        Ok(Some(program)) => program,
        Ok(None) => return false,
        Err(error) => {
            eprintln!("error: {error:#}");
            return false;
        }
    };
    match app::start(&program, std::env::args_os().skip(1)) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("error: {error:#}");
            false
        }
    }
}

/// Checks for an update in the background, now and every day while the app is open, and
/// sets [`Ready`] when there is one. Quiet when a check fails: the next one tries again.
pub fn check_in_background(support: &Path, cx: &mut App) {
    let Some(updater) = Updater::of_this_app(support) else {
        return;
    };
    let updater = Arc::new(updater);
    cx.spawn(async move |cx| {
        loop {
            let checked = cx
                .background_spawn({
                    let updater = updater.clone();
                    async move { updater.check().await }
                })
                .await;
            match checked {
                Ok(Some(ready)) => {
                    cx.update(|cx| cx.set_global(ready));
                    return;
                }
                Ok(None) => {}
                Err(error) => eprintln!("update: {error:#}"),
            }
            cx.background_executor().timer(CHECK_EVERY).await;
        }
    })
    .detach();
}

#[cfg(test)]
mod tests;
