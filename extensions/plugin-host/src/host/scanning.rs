//! What this machine has: the scan, looking again while the app runs, and the records that
//! wait for a plugin the scan had not found.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use sound_core::Project;

use super::Plugins;
use crate::scan::{Scan, ScanFailure, ScannedPlugin, scan_folders};
use crate::{PluginFormat, PluginRecord};

/// How often the host looks at the plugin folders again while a record names a plugin this
/// machine does not have, so that installing it is all the composer has to do. A look at
/// folders that did not change costs a few milliseconds on a thread of its own.
const LOOK_INTERVAL: Duration = Duration::from_secs(2);

/// What this machine has, filled in by whoever scans. Shared with the scan thread, so this is
/// the one place in the host with a lock, and the audio thread never touches it.
#[derive(Default)]
pub(super) struct Scanning {
    scan: Scan,
    /// Bundles that failed, as one line each. The runtime shows them once.
    notices: Vec<String>,
    /// Goes up whenever the scan learns something, so a poll can tell that a record that was
    /// waiting for a plugin is worth trying again.
    generation: u64,
    /// Whether the host is looking again, see [`Plugins::look_again`].
    looking_again: bool,
    /// Whether the composer asked to try the bundles that failed again, see
    /// [`Plugins::scan_again`], until the look that does ends. It can take as long as a scan,
    /// so the picker says a scan runs meanwhile.
    retrying: bool,
}

impl Plugins {
    /// Starts the scan on a thread of its own and comes back at once.
    ///
    /// The window calls this before it opens a project, so that no plugin of this machine is
    /// ever looked at on the thread that draws. A record whose plugin the scan has not reached
    /// yet is reported and played as soon as it turns up, see [`Self::take_retries`].
    /// `--render`, `--inspect` and `--headless` do not call it and wait for the scan the first
    /// time a record needs one.
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
        // Detached: nothing waits for it. A host that goes sets `stop`, and the thread ends
        // after the bundle it is on, which is bounded by the deadline of one child.
        std::thread::Builder::new()
            .name("plugin-scan".to_string())
            .spawn(move || {
                // Whatever ends this thread, the scan is over: it was stopped, or it panicked
                // inside a bundle. Nothing may wait for a scan that is not running.
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

    /// Every plugin this machine has. The first call pays for the scan unless one is already
    /// running in the background, and then it is what is known so far.
    pub fn scan(&self) -> Scan {
        self.ensure_scan();
        self.known()
    }

    pub(super) fn known(&self) -> Scan {
        scanning(&self.0.scanned).scan.clone()
    }

    /// A number that goes up whenever the scan learns something, and once more when it ends.
    ///
    /// Whoever draws a picker keeps it and fills the menu again when it changes, because a
    /// picker built while a scan ran holds a part of the list and a line that says so. It
    /// copies nothing, so a poll may ask on every frame.
    pub fn scan_generation(&self) -> u64 {
        scanning(&self.0.scanned).generation
    }

    /// Whether a scan is still running, the first one or [`Self::scan_again`]. The picker says
    /// so quietly while it is, and whoever polls asks on every poll, so this copies nothing.
    pub fn scan_is_running(&self) -> bool {
        let scanned = scanning(&self.0.scanned);
        self.0.started.get() && (!scanned.scan.finished || scanned.retrying)
    }

    /// Looks at the plugin folders again, the bundles that crashed or hung included, which a
    /// scan otherwise remembers and does not try again. A plugin that was too slow once, or
    /// that the composer fixed, comes back this way. A look that runs already takes it up when
    /// it ends. Only once the first scan is over.
    pub fn scan_again(&self) {
        self.look(true);
    }

    /// Every bundle the scan could not read so far, and why.
    pub fn failures(&self) -> Vec<ScanFailure> {
        scanning(&self.0.scanned).scan.failures.clone()
    }

    /// Looks at the plugin folders of this machine again, on a thread of its own, for a plugin
    /// that was installed, updated or removed while the app runs. A bundle whose stamp is the
    /// one the cache has costs no child process, so this is cheap when nothing changed, and
    /// then it changes nothing either: what the host knows, and [`Self::scan_generation`], move
    /// only when the plugins or the failures do. A record waiting for its plugin is tried again
    /// when they move.
    ///
    /// It runs when a picker asks what there is ([`Self::instruments`], [`Self::effects`]), and
    /// at a poll every [`LOOK_INTERVAL`] while a record names a plugin this machine did not
    /// have. Only in a host that scans in the background, once its first scan is over, and one
    /// at a time.
    fn look_again(&self) {
        self.look(false);
    }

    /// A record waits for a plugin this machine did not have. Once the first scan is over,
    /// that is a plugin the composer may be installing right now.
    pub(super) fn look_again_while_waiting(&self, now: Instant) {
        let looked = self.0.last_look.get();
        if !self.0.waiting.borrow().is_empty()
            && looked.is_none_or(|looked| now.saturating_duration_since(looked) >= LOOK_INTERVAL)
        {
            self.look_again();
        }
    }

    /// Looks again, see [`Self::look_again`]. `retrying` asks for the look that tries the
    /// bundles that failed again, see [`Self::scan_again`].
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
                // The picker says a scan runs from now on.
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
                // Whatever ends this thread, it is not looking any more.
                let mut done = DoneLooking(Some(scanned.clone()));
                loop {
                    let retrying = scanning(&scanned).retrying;
                    let cache = match retrying {
                        true => cache.clone().retrying_failures(),
                        false => cache.clone(),
                    };
                    let scan = scan_folders(&paths, &scanner, &cache, &stop, |_| {});
                    // A scan the host stopped as it went is half a list, not what this machine
                    // has.
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

    /// Scans if this session has not, and waits for it. Does nothing once a scan has been
    /// started in the background.
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

    /// Waits for the scan to finish, whoever started it. `--render`, `--inspect` and
    /// `--headless` may block, and this is where they do.
    pub fn wait_for_scan(&self) {
        self.ensure_scan();
        while self.scan_is_running() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Lines about the scan that a person should see once, such as a bundle that crashed.
    pub fn take_notices(&self) -> Vec<String> {
        std::mem::take(&mut scanning(&self.0.scanned).notices)
    }

    /// Every instrument this machine has, of every format, in one line each, for a picker. It
    /// scans on the first call of the session, as loading a plugin does.
    ///
    /// A picker asks when it is filled, which is when a track panel opens, so this is also
    /// where the host looks again for a plugin installed while the app runs. What that finds
    /// fills the picker again through [`Self::scan_generation`].
    pub fn instruments(&self) -> Vec<ScannedPlugin> {
        let mut instruments = self.scan().plugins;
        self.look_again();
        instruments.retain(ScannedPlugin::is_instrument);
        instruments
    }

    /// Every effect this machine has, for the picker that adds one to a rack. A plugin decides
    /// which list it is in by what it declares; nothing checks that it is true, and a record
    /// written by hand may name any plugin in any slot. Looks again, as [`Self::instruments`].
    pub fn effects(&self) -> Vec<ScannedPlugin> {
        let mut effects = self.scan().plugins;
        self.look_again();
        effects.retain(ScannedPlugin::is_effect);
        effects
    }

    /// The name the maker gave the plugin with this id, when this machine has it. `None` says
    /// the plugin is missing, which is what the card of a record shows.
    ///
    /// A card asks on every frame it draws, so this scans nothing and copies one name.
    pub fn installed_name(&self, format: PluginFormat, plugin_id: &str) -> Option<String> {
        let scanned = scanning(&self.0.scanned);
        Some(scanned.scan.find(format, plugin_id)?.name.clone())
    }

    /// What this machine knows of the plugin with this id, for the line on its card. `None`
    /// when it is missing.
    pub fn installed(&self, format: PluginFormat, plugin_id: &str) -> Option<ScannedPlugin> {
        scanning(&self.0.scanned)
            .scan
            .find(format, plugin_id)
            .cloned()
    }

    /// Records that were waiting for a plugin the scan had not reached. When it has learned
    /// something since the last poll, their behaviours are worth running again.
    pub(super) fn note_what_the_scan_found(&self, project: &Project) {
        if self.0.waiting.borrow().is_empty() {
            return;
        }
        // A record that is gone, or no longer a plugin, waits for nothing, and the host does
        // not look again for it.
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
            // A record that is gone, or no longer a plugin, waits for nothing.
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

/// What the scan has found so far.
///
/// A scan thread that panicked leaves what it had. Nothing of ours can panic while it holds
/// this lock, so going on with it is only so that a project still opens.
fn scanning(scanned: &Mutex<Scanning>) -> MutexGuard<'_, Scanning> {
    scanned
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Says the scan is over, however its thread ended.
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
    /// Ends the look, unless the composer asked to scan again while a look that did not ran:
    /// then it goes on, and this says so. One lock for both, so no ask falls in between.
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

/// A picker that said a scan runs is filled again.
fn end_look(held: &mut Scanning) {
    held.looking_again = false;
    if std::mem::take(&mut held.retrying) {
        held.generation += 1;
    }
}

/// Puts what a look again found where the host can read it, when it is not what the host
/// already knew. Only then is the change counted, so a look that found nothing new fills no
/// picker again and tries no waiting record again, and nothing looks again because of it.
fn publish_again(scanned: &Mutex<Scanning>, scan: Scan) {
    let mut guard = scanning(scanned);
    // One borrow of the guard, so its notices and its scan can be borrowed apart.
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

/// Puts what the scan has found where the host can read it, and counts the change so that a
/// record that is waiting for a plugin is tried again.
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

/// The line a person sees once about a bundle that failed.
fn notice(failure: &ScanFailure) -> String {
    format!(
        "{} could not be scanned: {}",
        failure.path.display(),
        failure.message
    )
}
