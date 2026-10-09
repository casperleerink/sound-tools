//! What takes long to read, read once: an instrument of hundreds of samples, or a minute of
//! sound at the rate of the engine. What a read gave is kept with the files it read, a failure
//! too, and given again until one of those files changes. So a file that cannot be read is read
//! once, and says why every time it is asked for.
//!
//! In the window a read runs on a thread of its own ([`load_in_background`]): the instances that
//! ask for it wait, and [`Loader::take_done`] names them when it is done, to run their behaviour
//! again. A render, an inspect and the tests read at once, so they play what the records say
//! from the first block.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use sound_core::{Assets, InstanceId};

static IN_BACKGROUND: AtomicBool = AtomicBool::new(false);

/// Reads on a thread of its own from here on, for the whole process, so the window never waits
/// for a read, also not to open. The window calls it before the project opens.
pub fn load_in_background() {
    IN_BACKGROUND.store(true, Ordering::Relaxed);
}

/// A file a read depends on, with its size and time then, or none when it was not there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    path: PathBuf,
    seen: Option<(u64, Option<SystemTime>)>,
}

impl Stamp {
    pub fn of(path: &Path) -> Self {
        let seen = fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        Self {
            path: path.to_path_buf(),
            seen,
        }
    }

    fn changed(&self) -> bool {
        Self::of(&self.path) != *self
    }
}

/// What a read gave, or why it failed, and the files it depends on.
pub type Read<T> = (Result<T, String>, Vec<Stamp>);

/// How long a [`Loader`] keeps what it read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// For as long as the process runs: what only copies of it play, such as the sound of a
    /// tool.
    Always,
    /// While something else holds it, such as an instrument a processor plays.
    WhileUsed,
}

/// An instance that waits for a read: the assets of its project, and itself.
type Waiter = (Assets, InstanceId);

/// What was read, by key, for the whole process.
pub struct Loader<K, T> {
    thread: &'static str,
    keep: Keep,
    state: Mutex<State<K, T>>,
    finished: Condvar,
}

struct State<K, T> {
    known: BTreeMap<K, (Vec<Stamp>, Result<Arc<T>, String>)>,
    running: BTreeSet<K>,
    waiting: BTreeMap<K, BTreeSet<Waiter>>,
    /// The instances whose read is done, for [`Loader::take_done`].
    done: Vec<(K, Waiter)>,
}

impl<K, T> Loader<K, T> {
    /// A loader whose reads run on threads named `thread`.
    pub const fn new(thread: &'static str, keep: Keep) -> Self {
        Self {
            thread,
            keep,
            state: Mutex::new(State {
                known: BTreeMap::new(),
                running: BTreeSet::new(),
                waiting: BTreeMap::new(),
                done: Vec::new(),
            }),
            finished: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State<K, T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits until no read runs. For tests.
    pub fn wait(&self) {
        let mut state = self.lock();
        while !state.running.is_empty() {
            state = (self.finished.wait(state)).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl<K, T> Loader<K, T>
where
    K: Ord + Clone + Send + 'static,
    T: Send + Sync + 'static,
{
    /// What `key` gave when it was read before and none of its files changed since: the thing,
    /// or why it cannot be. Else `read` reads it: at once, or on a thread of its own while
    /// `waiter`, an instance of the project of the assets, waits, which gives `None`.
    pub fn get(
        &'static self,
        key: K,
        (assets, instance): (&Assets, &InstanceId),
        read: impl FnOnce() -> Read<T> + Send + 'static,
    ) -> Result<Option<Arc<T>>, String> {
        let known = (self.lock().known.get(&key)).map(|(files, got)| (files.clone(), got.clone()));
        // The files are looked at with no lock held, so a thread that finishes never waits.
        if let Some((files, got)) = known
            && !files.iter().any(Stamp::changed)
        {
            return got.map(Some);
        }
        if !IN_BACKGROUND.load(Ordering::Relaxed) {
            let (got, files) = read();
            let got = got.map(Arc::new);
            self.lock().known.insert(key, (files, got.clone()));
            return got.map(Some);
        }
        let mut state = self.lock();
        let waiter = (assets.clone(), instance.clone());
        state.waiting.entry(key.clone()).or_default().insert(waiter);
        if state.running.insert(key.clone()) {
            let spawned = std::thread::Builder::new().name(self.thread.into()).spawn({
                let key = key.clone();
                move || self.finish(key, read())
            });
            if let Err(error) = spawned {
                state.running.remove(&key);
                state.waiting.remove(&key);
                return Err(format!("a thread to read it did not start: {error}"));
            }
        }
        Ok(None)
    }

    fn finish(&self, key: K, (got, files): Read<T>) {
        let mut state = self.lock();
        state.running.remove(&key);
        let waiting = state.waiting.remove(&key).unwrap_or_default();
        let done = waiting.into_iter().map(|waiter| (key.clone(), waiter));
        state.done.extend(done);
        state.known.insert(key, (files, got.map(Arc::new)));
        self.finished.notify_all();
    }

    /// The instances of the project of `assets` whose read is done since the last call, to run
    /// their behaviour again: they get what it gave, or why it failed.
    pub fn take_done(&self, assets: &Assets) -> Vec<InstanceId> {
        let mut state = self.lock();
        let State { known, done, .. } = &mut *state;
        if self.keep == Keep::WhileUsed {
            // The instances named at the last call took what they waited for then, or let go.
            known.retain(|key, (_, got)| match got {
                Ok(value) => Arc::strong_count(value) > 1 || done.iter().any(|(of, _)| of == key),
                Err(_) => true,
            });
        }
        let (ours, others): (Vec<_>, Vec<_>) = std::mem::take(done)
            .into_iter()
            .partition(|(_, (project, _))| project == assets);
        *done = others;
        let instances: BTreeSet<InstanceId> = (ours.into_iter())
            .map(|(_, (_, instance))| instance)
            .collect();
        instances.into_iter().collect()
    }

    /// Whether `key` is read on a thread of its own now.
    pub fn is_loading(&self, key: &K) -> bool {
        self.lock().running.contains(key)
    }
}
