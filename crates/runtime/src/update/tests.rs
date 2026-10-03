//! The check, download and install against a release served from local files, with the real
//! curl and no network.

use std::os::unix::fs::PermissionsExt;

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

fn file_url(path: &Path) -> String {
    format!("file://{}", path.display())
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

fn check(updater: &Updater, now: SystemTime) -> Result<Option<Ready>> {
    smol::block_on(updater.check(now))
}

/// The Linux tarball of 0.2.0 in `root/served`, whose `install.sh` writes `installed`.
fn linux_release(root: &Path, installed: &Path) -> PathBuf {
    let source = root.join("source");
    let folder = source.join("sound-tools-0.2.0-linux-x86_64");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("sound-tools"), "0.2.0").unwrap();
    let script = format!("#!/bin/sh\ncp sound-tools '{}'\n", installed.display());
    fs::write(folder.join("install.sh"), script).unwrap();
    let served = root.join("served");
    fs::create_dir_all(&served).unwrap();
    let archive = served.join("sound-tools-0.2.0-linux-x86_64.tar.gz");
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&source)
        .arg("sound-tools-0.2.0-linux-x86_64")
        .status()
        .unwrap();
    assert!(status.success());
    archive
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
    // No Intel Mac build, and no Windows one.
    assert_eq!(asset_name(version, "macos", "x86_64"), None);
    assert_eq!(asset_name(version, "windows", "x86_64"), None);
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
fn a_check_comes_at_most_once_a_day() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("updates/last-check");
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    assert!(due(&file, now));
    remember_check(&file, now).unwrap();
    assert!(!due(&file, now + Duration::from_secs(60 * 60)));
    assert!(!due(&file, now + CHECK_EVERY - Duration::from_secs(1)));
    assert!(due(&file, now + CHECK_EVERY));
    // A clock set back does not stop the checks.
    assert!(due(&file, now - Duration::from_secs(60)));
    fs::write(&file, "not a time").unwrap();
    assert!(due(&file, now));
}

#[test]
fn a_newer_release_is_downloaded_checked_and_installed_by_its_script() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let installed = root.join("installed");
    let archive = linux_release(root, &installed);
    publish(&root.join("served"), "v0.2.0", &archive);
    let program = root.join("home/.local/lib/sound-tools/sound-tools");
    let updater = updater(
        root,
        "linux",
        "x86_64",
        Place::Script {
            program: program.clone(),
        },
    );
    let now = SystemTime::now();

    let ready = check(&updater, now).unwrap();
    assert_eq!(
        ready,
        Some(Ready {
            version: version("0.2.0"),
            action: Action::Restart
        })
    );
    let (pending, unpacked) = updater.pending().unwrap();
    assert_eq!(pending, version("0.2.0"));
    assert!(
        unpacked
            .join("sound-tools-0.2.0-linux-x86_64/install.sh")
            .is_file()
    );
    // Not again the same day.
    assert_eq!(
        check(&updater, now + Duration::from_secs(60)).unwrap(),
        None
    );

    // The next start installs it, and is told what to start.
    assert_eq!(updater.install_pending().unwrap(), Some(program));
    assert_eq!(fs::read_to_string(&installed).unwrap(), "0.2.0");
    assert!(updater.pending().is_none());
    assert!(!updater.version_folder(version("0.2.0")).exists());
    assert_eq!(updater.install_pending().unwrap(), None);
}

#[test]
fn a_damaged_download_is_never_ready_and_is_tried_again() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let archive = linux_release(root, &root.join("installed"));
    publish(&root.join("served"), "v0.2.0", &archive);
    // One byte changes after the checksums were written.
    let mut bytes = fs::read(&archive).unwrap();
    bytes[100] ^= 1;
    fs::write(&archive, bytes).unwrap();
    let updater = updater(
        root,
        "linux",
        "x86_64",
        Place::Script {
            program: root.join("program"),
        },
    );

    let failed = check(&updater, SystemTime::now()).unwrap_err();
    assert!(failed.to_string().contains("damaged"), "{failed:#}");
    assert!(updater.pending().is_none());
    // Not remembered as a check, so the next launch tries again.
    assert!(due(&updater.last_check(), SystemTime::now()));
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
    assert_eq!(check(&updater, SystemTime::now()).unwrap(), None);
    // That was a check: the next is tomorrow.
    assert!(!due(&updater.last_check(), SystemTime::now()));

    fs::remove_file(updater.last_check()).unwrap();
    write_release(&served, "v0.3.0-beta.1", true, &[]);
    assert_eq!(check(&updater, SystemTime::now()).unwrap(), None);
}

#[test]
fn an_app_whose_folder_cannot_be_written_offers_the_release_page() {
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

    let ready = check(&updater, SystemTime::now()).unwrap();
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

    let ready = check(&updater, SystemTime::now()).unwrap().unwrap();
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
