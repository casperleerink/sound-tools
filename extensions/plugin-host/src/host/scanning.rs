//! What this machine has: the scan, looking again while the app runs, and the records that
//! wait for a plugin the scan had not found.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use sound_core::Project;

use super::Plugins;
use crate::scan::{Scan, ScanFailure, ScannedPlugin, scan_folders};
use crate::{PluginFormat, PluginRecord};

/// How often the host looks again while a record names a missing plugin, so installing it is
/// all the composer has to do. A look at folders that did not change costs a few milliseconds.
const LOOK_INTERVAL: Duration = Duration::from_secs(2);

/// Shared with the scan thread: the one lock in the host, which the audio thread never touches.
#[derive(Default)]
pub(super) struct Scanning {
    scan: Scan,
    notices: Vec<String>,
    generation: u64,
    looking_again: bool,
    /// [`Plugins::scan_again`] runs. It can take as long as a scan, so the picker says one runs.
    retrying: bool,
}

impl Plugins {
    /// Starts the scan on a thread of its own, so no plugin is looked at on the thread that
    /// draws. Without it, the first record that needs the scan waits for it.
    pub fn start_scanning(&self) {
        self.0.in_background.set(true);
        if self.0.started.replace(true) {
            return;
        }
        let scanned = self.0.scanned.clone();
        let stop = self.0.stop.clone();
        let (paths, scanner, cache) = (
            self.0.search_paths.clone(),
            self.0.scanner.clone(),
            self.0.cache.clone(),
        );
        // Detached. A host that goes sets `stop`, and the thread ends after the bundle it is on.
        std::thread::Builder::new()
            .name("plugin-scan".to_string())
            .spawn(move || {
                let _over = Over(scanned.clone());
                scan_folders(&paths, &scanner, &cache, &stop, |scan| {
                    publish(&scanned, scan);
                });
            })
            .map_or_else(
                |error| {
                    // A machine that cannot start a thread scans where it stands.
                    eprintln!("error: the plugin scan needs a thread: {error}");
                    self.0.started.set(false);
                },
                |_handle| (),
            );
    }

    /// The first call pays for the scan, unless one runs in the background: then it is what is
    /// known so far.
    pub fn scan(&self) -> Scan {
        self.ensure_scan();
        self.known()
    }

    pub(super) fn known(&self) -> Scan {
        scanning(&self.0.scanned).scan.clone()
    }

    /// Goes up whenever the scan learns something, and once more when it ends, so a picker
    /// knows to fill its menu again. Cheap enough for every frame.
    pub fn scan_generation(&self) -> u64 {
        scanning(&self.0.scanned).generation
    }

    /// The first scan or [`Self::scan_again`]. Cheap enough for every poll.
    pub fn scan_is_running(&self) -> bool {
        let scanned = scanning(&self.0.scanned);
        self.0.started.get() && (!scanned.scan.finished || scanned.retrying)
    }

    /// Looks again, the bundles that crashed or hung included, which the cache otherwise never
    /// tries twice. Only once the first scan is over.
    pub fn scan_again(&self) {
        self.look(true);
    }

    pub fn failures(&self) -> Vec<ScanFailure> {
        scanning(&self.0.scanned).scan.failures.clone()
    }

    /// Looks again on a thread of its own, for a plugin installed, updated or removed while the
    /// app runs. A bundle the cache already has costs no child process, so this is cheap when
    /// nothing changed. Only in a host that scans in the background, once its first scan is
    /// over, and one at a time.
    fn look_again(&self) {
        self.look(false);
    }

    pub(super) fn look_again_while_waiting(&self, now: Instant) {
        let looked = self.0.last_look.get();
        if !self.0.waiting.borrow().is_empty()
            && looked.is_none_or(|looked| now.saturating_duration_since(looked) >= LOOK_INTERVAL)
        {
            self.look_again();
        }
    }

    fn look(&self, retrying: bool) {
        if !self.0.in_background.get() {
            return;
        }
        {
            let mut scanned = scanning(&self.0.scanned);
            if !scanned.scan.finished {
                return;
            }
            if retrying && !scanned.retrying {
                scanned.retrying = true;
                scanned.generation += 1;
            }
            if scanned.looking_again {
                return;
            }
            scanned.looking_again = true;
        }
        self.0.last_look.set(Some(Instant::now()));
        let scanned = self.0.scanned.clone();
        let stop = self.0.stop.clone();
        let (paths, scanner, cache) = (
            self.0.search_paths.clone(),
            self.0.scanner.clone(),
            self.0.cache.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("plugin-rescan".to_string())
            .spawn(move || {
                let mut done = DoneLooking(Some(scanned.clone()));
                loop {
                    let retrying = scanning(&scanned).retrying;
                    let cache = match retrying {
                        true => cache.clone().retrying_failures(),
                        false => cache.clone(),
                    };
                    let scan = scan_folders(&paths, &scanner, &cache, &stop, |_| {});
                    // A stopped scan is half a list.
                    if !scan.finished {
                        return;
                    }
                    publish_again(&scanned, scan);
                    if done.end_unless_asked(retrying) {
                        return;
                    }
                }
            });
        if let Err(error) = spawned {
            let notice = format!("looking for plugins again needs a thread: {error}");
            scanning(&self.0.scanned).notices.push(notice);
            drop(DoneLooking(Some(self.0.scanned.clone())));
        }
    }

    /// Scans if this session has not, and waits for it.
    pub(super) fn ensure_scan(&self) {
        if self.0.started.replace(true) {
            return;
        }
        let scanned = self.0.scanned.clone();
        scan_folders(
            &self.0.search_paths,
            &self.0.scanner,
            &self.0.cache,
            &self.0.stop,
            |scan| publish(&scanned, scan),
        );
    }

    /// Waits for the scan to finish, whoever started it.
    pub fn wait_for_scan(&self) {
        self.ensure_scan();
        while self.scan_is_running() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Lines a person should see once, such as a bundle that crashed.
    pub fn take_notices(&self) -> Vec<String> {
        std::mem::take(&mut scanning(&self.0.scanned).notices)
    }

    /// For a picker, which asks as it is filled, so this also looks again for a plugin installed
    /// while the app runs.
    pub fn instruments(&self) -> Vec<ScannedPlugin> {
        let mut instruments = self.scan().plugins;
        self.look_again();
        instruments.retain(ScannedPlugin::is_instrument);
        instruments
    }

    /// As [`Self::instruments`].
    pub fn effects(&self) -> Vec<ScannedPlugin> {
        let mut effects = self.scan().plugins;
        self.look_again();
        effects.retain(ScannedPlugin::is_effect);
        effects
    }

    /// `None` when missing. A card asks on every frame, so this scans nothing and copies one
    /// name.
    pub fn installed_name(&self, format: PluginFormat, plugin_id: &str) -> Option<String> {
        let scanned = scanning(&self.0.scanned);
        Some(scanned.scan.find(format, plugin_id)?.name.clone())
    }

    /// The key of the row a picker lists this plugin under, see [`crate::one_per_plugin`]: its own,
    /// or that of its twin in the format a picker prefers. As cheap as
    /// [`Self::installed_name`].
    pub fn offer_key(&self, format: PluginFormat, plugin_id: &str) -> String {
        let scan = &scanning(&self.0.scanned).scan;
        match scan.find(format, plugin_id) {
            Some(found) => found.listed(&scan.plugins).offer_key(),
            None => PluginRecord::offer_key(format, plugin_id),
        }
    }

    pub fn installed(&self, format: PluginFormat, plugin_id: &str) -> Option<ScannedPlugin> {
        scanning(&self.0.scanned)
            .scan
            .find(format, plugin_id)
            .cloned()
    }

    /// Retries the waiting records when the scan learned something since the last poll.
    pub(super) fn note_what_the_scan_found(&self, project: &Project) {
        if self.0.waiting.borrow().is_empty() {
            return;
        }
        // A record that is gone, or no longer a plugin, waits for nothing.
        self.0
            .waiting
            .borrow_mut()
            .retain(|id| project.resolve::<PluginRecord>(id).is_some());
        let generation = scanning(&self.0.scanned).generation;
        if self.0.seen.replace(generation) == generation {
            return;
        }
        let known = self.known();
        let mut waiting = self.0.waiting.borrow_mut();
        waiting.retain(|id| {
            let Some(instance) = project.resolve::<PluginRecord>(id) else {
                return false;
            };
            let Some(record) = project.state(&instance) else {
                return false;
            };
            let found = known.find(record.format, &record.plugin_id).is_some();
            if found || known.finished {
                self.retry(id);
                return false;
            }
            true
        });
    }
}

/// A scan thread that panicked leaves what it had. Nothing of ours panics while it holds this
/// lock, so going on with it is only so that a project still opens.
fn scanning(scanned: &Mutex<Scanning>) -> MutexGuard<'_, Scanning> {
    scanned
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Says the scan is over, however its thread ended, so nothing waits for a scan that is not
/// running.
struct Over(Arc<Mutex<Scanning>>);

impl Drop for Over {
    fn drop(&mut self) {
        let mut held = scanning(&self.0);
        held.scan.finished = true;
        held.generation += 1;
    }
}

/// Ends a look again, however its thread ended. `None` once it has.
struct DoneLooking(Option<Arc<Mutex<Scanning>>>);

impl DoneLooking {
    /// Ends the look, unless [`Plugins::scan_again`] was asked while a look that did not retry
    /// ran: then it goes on. One lock for both, so no ask falls in between.
    fn end_unless_asked(&mut self, retried: bool) -> bool {
        let Some(scanned) = self.0.take() else {
            return true;
        };
        let mut held = scanning(&scanned);
        if held.retrying && !retried {
            drop(held);
            self.0 = Some(scanned);
            return false;
        }
        end_look(&mut held);
        true
    }
}

impl Drop for DoneLooking {
    fn drop(&mut self) {
        if let Some(scanned) = self.0.take() {
            end_look(&mut scanning(&scanned));
        }
    }
}

fn end_look(held: &mut Scanning) {
    held.looking_again = false;
    if std::mem::take(&mut held.retrying) {
        held.generation += 1;
    }
}

/// Counts a change only when the look found something new, so a look that found nothing fills
/// no picker and retries no record.
fn publish_again(scanned: &Mutex<Scanning>, scan: Scan) {
    let mut guard = scanning(scanned);
    // Reborrowed, so its fields can be borrowed apart.
    let held = &mut *guard;
    if held.scan.plugins == scan.plugins && held.scan.failures == scan.failures {
        return;
    }
    let new = scan.failures.iter();
    let new = new.filter(|failure| !held.scan.failures.contains(failure));
    held.notices.extend(new.map(notice));
    held.notices.extend(scan.cache_error.clone());
    held.scan = scan;
    held.generation += 1;
}

fn publish(scanned: &Mutex<Scanning>, scan: &Scan) {
    let mut held = scanning(scanned);
    let said = held.scan.failures.len().min(scan.failures.len());
    held.notices
        .extend(scan.failures[said..].iter().map(notice));
    // Said once, when the scan is over: only the last call carries it.
    held.notices.extend(scan.cache_error.clone());
    held.scan = scan.clone();
    held.generation += 1;
}

fn notice(failure: &ScanFailure) -> String {
    format!(
        "{} could not be scanned: {}",
        failure.path.display(),
        failure.message
    )
}
