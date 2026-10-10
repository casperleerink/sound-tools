#![allow(clippy::unwrap_used)]
//! Reads in the background, as in the window: one that fails is read once, its instances run
//! again once, and it says why until its file changes; one nothing holds is let go of. A process
//! of its own, because reading in the background is for the whole process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sound_core::{Assets, InstanceId};
use sound_media::{Keep, Loader, Stamp};

type Got = Result<Option<Arc<String>>, String>;

/// Reads the text of `path`, which plays when it is "plays", and counts the reads.
fn get(loader: &'static Loader<PathBuf, String>, reads: &'static AtomicUsize, path: &Path) -> Got {
    let id = InstanceId::new("tool").unwrap();
    let assets = Assets::new(path.parent().unwrap());
    let path = path.to_path_buf();
    loader.get(path.clone(), (&assets, &id), move || {
        reads.fetch_add(1, Ordering::Relaxed);
        let files = vec![Stamp::of(&path)];
        let text = std::fs::read_to_string(&path).unwrap();
        let got = match text == "plays" {
            true => Ok(text),
            false => Err(format!("{text} does not play")),
        };
        (got, files)
    })
}

/// The instances that run again.
fn take_done(loader: &Loader<PathBuf, String>, path: &Path) -> Vec<InstanceId> {
    loader.wait();
    loader.take_done(&Assets::new(path.parent().unwrap()))
}

#[test]
fn a_read_that_fails_is_read_once_and_says_why_until_its_file_changes() {
    static LOADER: Loader<PathBuf, String> = Loader::new("test", Keep::Always);
    static READS: AtomicUsize = AtomicUsize::new(0);
    sound_media::load_in_background();
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("file");
    std::fs::write(&path, "silence").unwrap();
    let tool = InstanceId::new("tool").unwrap();

    // Asked twice while it is read, as a behaviour asks for a sample.
    assert_eq!(get(&LOADER, &READS, &path), Ok(None));
    assert_eq!(get(&LOADER, &READS, &path), Ok(None));
    assert_eq!(take_done(&LOADER, &path), std::slice::from_ref(&tool));
    for _ in 0..3 {
        let failed = get(&LOADER, &READS, &path);
        assert_eq!(failed, Err("silence does not play".to_string()));
    }
    assert_eq!(take_done(&LOADER, &path), []);
    assert_eq!(READS.load(Ordering::Relaxed), 1);

    std::fs::write(&path, "plays").unwrap();
    assert_eq!(get(&LOADER, &READS, &path), Ok(None));
    assert_eq!(take_done(&LOADER, &path), [tool]);
    let plays = get(&LOADER, &READS, &path).unwrap().unwrap();
    assert_eq!(*plays, "plays");
    assert_eq!(READS.load(Ordering::Relaxed), 2);
}

#[test]
fn what_nothing_holds_is_let_go_of_once_its_instances_took_it() {
    static LOADER: Loader<PathBuf, String> = Loader::new("test", Keep::WhileUsed);
    static READS: AtomicUsize = AtomicUsize::new(0);
    sound_media::load_in_background();
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("file");
    std::fs::write(&path, "plays").unwrap();

    assert_eq!(get(&LOADER, &READS, &path), Ok(None));
    take_done(&LOADER, &path);
    let held = get(&LOADER, &READS, &path).unwrap().unwrap();
    take_done(&LOADER, &path);
    assert!(Arc::ptr_eq(
        &held,
        &get(&LOADER, &READS, &path).unwrap().unwrap()
    ));
    assert_eq!(READS.load(Ordering::Relaxed), 1);

    drop(held);
    take_done(&LOADER, &path);
    assert_eq!(get(&LOADER, &READS, &path), Ok(None));
    take_done(&LOADER, &path);
    assert_eq!(READS.load(Ordering::Relaxed), 2);
}
