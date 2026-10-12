//! The check, download and install against a release served from local files, with the real
//! curl and no network. The release with an install script is the one of the system the tests
//! run on: the Linux tarball, or on Windows the zip.

use serde_json::json;
use sha2::{Digest, Sha256};

use super::*;

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `file:///C:/...` on Windows, whose curl reads `file://C:\...` as a computer named `C`.
fn file_url(path: &Path) -> String {
    let path = path.display().to_string().replace('\\', "/");
    format!("file:///{}", path.trim_start_matches('/'))
}

/// The JSON GitHub sends for the latest release, in `served/latest.json`, with assets of
/// these names and sizes in `served`.
fn write_release(served: &Path, tag: &str, prerelease: bool, assets: &[(&str, usize)]) {
    let assets: Vec<_> = assets
        .iter()
        .map(|(name, size)| {
            json!({
                "name": name,
                "browser_download_url": file_url(&served.join(name)),
                "size": size,
            })
        })
        .collect();
    let release = json!({
        "tag_name": tag,
        "html_url": format!("https://github.com/casperleerink/sound-tools/releases/tag/{tag}"),
        "draft": false,
        "prerelease": prerelease,
        "assets": assets,
    });
    fs::write(served.join("latest.json"), release.to_string()).unwrap();
}

/// Publishes `archive`, which is in `served`, as release `tag` with its `SHA256SUMS`.
fn publish(served: &Path, tag: &str, archive: &Path) {
    let name = archive.file_name().unwrap().to_str().unwrap();
    let bytes = fs::read(archive).unwrap();
    let sums = format!("{}  {name}\n", sha256_hex(&bytes));
    fs::write(served.join(SUMS), &sums).unwrap();
    write_release(
        served,
        tag,
        false,
        &[(name, bytes.len()), (SUMS, sums.len())],
    );
}

fn updater(root: &Path, os: &'static str, arch: &'static str, place: Place) -> Updater {
    Updater {
        folder: root.join("support/updates"),
        latest: file_url(&root.join("served/latest.json")),
        current: version("0.1.1"),
        os,
        arch,
        place,
    }
}

fn check(updater: &Updater) -> Result<Option<Ready>> {
    smol::block_on(updater.check())
}

#[cfg(unix)]
const SCRIPT_OS: &str = "linux";
#[cfg(windows)]
const SCRIPT_OS: &str = "windows";

/// The release of 0.2.0 with the install `script` in `root/served`.
fn script_release(root: &Path, script: &str) -> PathBuf {
    let source = root.join("source");
    let name = format!("sound-tools-0.2.0-{SCRIPT_OS}-x86_64");
    let folder = source.join(&name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(app::program_file_name()), "0.2.0").unwrap();
    fs::write(folder.join(INSTALL_SCRIPT), script).unwrap();
    let served = root.join("served");
    fs::create_dir_all(&served).unwrap();
    let archive = served.join(asset_name(version("0.2.0"), SCRIPT_OS, "x86_64").unwrap());
    let status = pack(&archive)
        .arg("-C")
        .arg(&source)
        .arg(&name)
        .status()
        .unwrap();
    assert!(status.success());
    archive
}

#[cfg(unix)]
const INSTALL_SCRIPT: &str = "install.sh";
#[cfg(windows)]
const INSTALL_SCRIPT: &str = "install.ps1";

/// An install script that writes `installed`.
#[cfg(unix)]
fn copying_script(installed: &Path) -> String {
    format!("#!/bin/sh\ncp sound-tools '{}'\n", installed.display())
}

#[cfg(windows)]
fn copying_script(installed: &Path) -> String {
    format!(
        "Copy-Item -LiteralPath sound-tools.exe -Destination '{}'\n",
        installed.display()
    )
}

/// An install script that stops, as `install.ps1` does when it cannot remove an old program.
#[cfg(unix)]
const FAILING_SCRIPT: &str = "#!/bin/sh\necho 'cannot remove sound-tools.old' >&2\nexit 1\n";
#[cfg(windows)]
const FAILING_SCRIPT: &str = "throw 'cannot remove sound-tools.old'\n";

#[cfg(unix)]
fn pack(archive: &Path) -> Command {
    let mut command = Command::new("tar");
    command.arg("-czf").arg(archive);
    command
}

/// `-a` picks the zip format from the name.
#[cfg(windows)]
fn pack(archive: &Path) -> Command {
    let mut command = Command::new(windows_program(r"System32\tar.exe"));
    command.arg("-a").arg("-cf").arg(archive);
    command
}

#[test]
fn versions_compare_by_number() {
    assert!(version("0.10.0") > version("0.9.9"));
    assert!(version("1.0.0") > version("0.99.99"));
    assert!(version("0.1.2") > version("0.1.1"));
    assert_eq!(version("v0.2.0"), version("0.2.0"));
    assert_eq!(version("0.2.0").to_string(), "0.2.0");
    for not_a_release in ["1.0.0-beta.1", "1.0", "1.0.0.0", "", "v", "one.two.three"] {
        assert_eq!(Version::parse(not_a_release), None, "{not_a_release}");
    }
}

#[test]
fn each_platform_gets_its_own_file() {
    let version = version("0.2.0");
    assert_eq!(
        asset_name(version, "macos", "aarch64").as_deref(),
        Some("Sound-Tools-0.2.0-macos-arm64.zip")
    );
    assert_eq!(
        asset_name(version, "linux", "x86_64").as_deref(),
        Some("sound-tools-0.2.0-linux-x86_64.tar.gz")
    );
    assert_eq!(
        asset_name(version, "linux", "aarch64").as_deref(),
        Some("sound-tools-0.2.0-linux-aarch64.tar.gz")
    );
    assert_eq!(
        asset_name(version, "windows", "x86_64").as_deref(),
        Some("sound-tools-0.2.0-windows-x86_64.zip")
    );
    // No Intel Mac build, and no Windows on Arm one.
    assert_eq!(asset_name(version, "macos", "x86_64"), None);
    assert_eq!(asset_name(version, "windows", "aarch64"), None);
}

#[test]
fn the_checksum_is_found_by_file_name() {
    let sums =
        "AAAA  Sound-Tools-0.2.0-macos-arm64.zip\nbbbb *sound-tools-0.2.0-linux-x86_64.tar.gz\n";
    assert_eq!(
        checksum_of(sums, "Sound-Tools-0.2.0-macos-arm64.zip").as_deref(),
        Some("aaaa")
    );
    assert_eq!(
        checksum_of(sums, "sound-tools-0.2.0-linux-x86_64.tar.gz").as_deref(),
        Some("bbbb")
    );
    assert_eq!(
        checksum_of(sums, "sound-tools-0.2.0-linux-aarch64.tar.gz"),
        None
    );
}

#[test]
fn a_newer_release_is_downloaded_checked_and_installed_by_its_script() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let installed = root.join("installed");
    let archive = script_release(root, &copying_script(&installed));
    publish(&root.join("served"), "v0.2.0", &archive);
    let program = root.join("home/.local/lib/sound-tools/sound-tools");
    let updater = updater(
        root,
        SCRIPT_OS,
        "x86_64",
        Place::Script {
            program: program.clone(),
        },
    );
    let ready = check(&updater).unwrap();
    assert_eq!(
        ready,
        Some(Ready {
            version: version("0.2.0"),
            action: Action::Restart
        })
    );
    let (pending, unpacked) = updater.pending().unwrap();
    assert_eq!(pending, version("0.2.0"));
    let name = format!("sound-tools-0.2.0-{SCRIPT_OS}-x86_64");
    assert!(unpacked.join(name).join(app::program_file_name()).is_file());

    // The next start installs it, and is told what to start.
    assert_eq!(updater.install_pending().unwrap(), Some(program));
    assert_eq!(fs::read_to_string(&installed).unwrap(), "0.2.0");
    assert!(updater.pending().is_none());
    assert!(!updater.version_folder(version("0.2.0")).exists());
    assert_eq!(updater.install_pending().unwrap(), None);
}

/// It would fail the same way at every start, with Restart offered again after each.
#[test]
fn an_update_that_does_not_install_says_why_once_and_is_not_downloaded_again() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let archive = script_release(root, FAILING_SCRIPT);
    publish(&root.join("served"), "v0.2.0", &archive);
    let updater = updater(
        root,
        SCRIPT_OS,
        "x86_64",
        Place::Script {
            program: root.join("program"),
        },
    );
    assert_eq!(check(&updater).unwrap().unwrap().action, Action::Restart);

    let failed = updater.install_pending().unwrap_err();
    assert_eq!(failed.version, version("0.2.0"));
    let Action::Failed { reason, .. } = failed.action else {
        panic!("{failed:?}");
    };
    // PowerShell words a thrown error its own way.
    #[cfg(unix)]
    assert!(reason.contains("cannot remove sound-tools.old"), "{reason}");
    assert!(updater.pending().is_none());
    assert_eq!(check(&updater).unwrap(), None);
    assert!(!updater.version_folder(version("0.2.0")).exists());
}

#[test]
fn a_damaged_download_is_never_ready() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let archive = script_release(root, &copying_script(&root.join("installed")));
    publish(&root.join("served"), "v0.2.0", &archive);
    // One byte changes after the checksums were written.
    let mut bytes = fs::read(&archive).unwrap();
    bytes[100] ^= 1;
    fs::write(&archive, bytes).unwrap();
    let updater = updater(
        root,
        SCRIPT_OS,
        "x86_64",
        Place::Script {
            program: root.join("program"),
        },
    );

    let failed = check(&updater).unwrap_err();
    assert!(failed.to_string().contains("damaged"), "{failed:#}");
    assert!(updater.pending().is_none());
}

#[test]
fn an_older_release_or_a_prerelease_is_not_offered() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let served = root.join("served");
    fs::create_dir_all(&served).unwrap();
    let updater = updater(
        root,
        "linux",
        "x86_64",
        Place::Script {
            program: root.join("program"),
        },
    );

    write_release(&served, "v0.1.1", false, &[]);
    assert_eq!(check(&updater).unwrap(), None);
    write_release(&served, "v0.3.0-beta.1", true, &[]);
    assert_eq!(check(&updater).unwrap(), None);
}

/// Unix only: a read-only folder on Windows still takes new files.
#[cfg(unix)]
#[test]
fn an_app_whose_folder_cannot_be_written_offers_the_release_page() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let served = root.join("served");
    fs::create_dir_all(&served).unwrap();
    let name = "Sound-Tools-0.2.0-macos-arm64.zip";
    write_release(&served, "v0.2.0", false, &[(name, 1000), (SUMS, 100)]);
    let locked = root.join("locked");
    fs::create_dir_all(locked.join("Sound Tools.app")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let updater = updater(
        root,
        "macos",
        "aarch64",
        Place::Bundle(locked.join("Sound Tools.app")),
    );

    let ready = check(&updater).unwrap();
    let page = "https://github.com/casperleerink/sound-tools/releases/tag/v0.2.0".to_string();
    assert_eq!(
        ready,
        Some(Ready {
            version: version("0.2.0"),
            action: Action::Download { page }
        })
    );
    // Nothing was downloaded.
    assert!(!updater.version_folder(version("0.2.0")).exists());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn the_app_is_replaced_where_it_runs_from() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let app = |folder: &Path, content: &str| {
        let bundle = folder.join("Sound Tools.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("Contents/MacOS/sound-tools"), content).unwrap();
        bundle
    };
    let new = app(&root.join("source"), "0.2.0");
    let served = root.join("served");
    fs::create_dir_all(&served).unwrap();
    let archive = served.join("Sound-Tools-0.2.0-macos-arm64.zip");
    run(Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&new)
        .arg(&archive))
    .unwrap();
    publish(&served, "v0.2.0", &archive);
    let applications = root.join("Applications");
    let installed = app(&applications, "0.1.1");
    let updater = updater(root, "macos", "aarch64", Place::Bundle(installed.clone()));

    let ready = check(&updater).unwrap().unwrap();
    assert_eq!(ready.action, Action::Restart);
    // Downloading changes nothing of the app.
    let program = installed.join("Contents/MacOS/sound-tools");
    assert_eq!(fs::read_to_string(&program).unwrap(), "0.1.1");

    assert_eq!(updater.install_pending().unwrap(), Some(program.clone()));
    assert_eq!(fs::read_to_string(&program).unwrap(), "0.2.0");
    let left: Vec<_> = fs::read_dir(&applications)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["Sound Tools.app"]);
}
