//! The sound of other apps that `project.json` hears: a tap of each app, open while a
//! connection in the graph starts at it, so macOS asks the composer about recording other apps
//! only for a project that does. Taps open on threads of their own: the first one waits for
//! that answer. A tap at another rate than the output plays nothing: it closes, and the problem
//! of its connections says why.
//!
//! A tap of an app by name holds the processes the app had when it opened. Every two seconds
//! they are looked at again, and the tap opens again when they changed: the app started or
//! quit, or a helper came to play its sound, as a browser starts one.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sound_core::{AppSound, DeviceError, InputDevice, LiveInput, Project, ProjectEvent};

use super::{Waker, drop_off_thread};

/// How often the processes of the apps are looked at again.
const CHECK_EVERY: Duration = Duration::from_secs(2);

/// How other apps are heard: the taps of macOS, or simulated ones in a test. Both are called on
/// a thread of their own.
#[derive(Clone)]
pub struct AppSounds {
    /// Opens a tap of the app: its stream, which stops and removes the tap when it is dropped,
    /// and the live input for the engine.
    pub open: Arc<dyn Fn(&AppSound) -> Result<OpenedTap, DeviceError> + Send + Sync>,
    /// What a tap of the app would hear now, see [`sound_core::app_processes`].
    pub processes: Arc<dyn Fn(&AppSound) -> Vec<u32> + Send + Sync>,
}

impl AppSounds {
    /// The process taps of macOS. Elsewhere a tap does not open, and the problem of the
    /// connection says why.
    pub fn system() -> Self {
        Self {
            open: Arc::new(|app| {
                let (stream, live) = InputDevice::of_apps(app)?.start_live()?;
                println!("app sound: {app}, {} Hz", live.sample_rate());
                Ok((Box::new(stream), live))
            }),
            processes: Arc::new(sound_core::app_processes),
        }
    }
}

/// A tap that opened: its stream, see [`AppSounds::open`], and the live input for the engine.
pub type OpenedTap = (Box<dyn Send>, LiveInput);

struct Tapped {
    /// What it was opened with: when they change, it opens again.
    processes: Vec<u32>,
    /// `None` for a tap at another rate than the output, which closed.
    stream: Option<Box<dyn Send>>,
}

type Opened = (AppSound, u64, Vec<u32>, Result<OpenedTap, DeviceError>);

/// The taps of the apps the project hears.
pub struct OtherApps {
    sounds: AppSounds,
    /// Every app that was opened, or that did not open, with why in the project.
    taps: BTreeMap<AppSound, Tapped>,
    /// The opening on its way for an app, by its number. One that arrives after the app is no
    /// longer heard, or after a newer opening began, is dropped.
    opening: BTreeMap<AppSound, u64>,
    openings: u64,
    opened: (Sender<Opened>, Receiver<Opened>),
    /// What the thread that looks at the processes compares with: the apps tapped and their
    /// processes then.
    checked: Arc<Mutex<Vec<(AppSound, Vec<u32>)>>>,
    changed: Receiver<AppSound>,
    _stop: Sender<()>,
    waker: Waker,
    followed: bool,
}

impl OtherApps {
    pub fn new(sounds: AppSounds, waker: Waker) -> Self {
        let checked: Arc<Mutex<Vec<(AppSound, Vec<u32>)>>> = Arc::default();
        let (stop, stopped) = mpsc::channel::<()>();
        let (tell, changed) = mpsc::channel();
        let (look, processes, wake) = (checked.clone(), sounds.processes.clone(), waker.clone());
        let started = std::thread::Builder::new()
            .name("app-sounds".into())
            .spawn(move || {
                while matches!(
                    stopped.recv_timeout(CHECK_EVERY),
                    Err(RecvTimeoutError::Timeout)
                ) {
                    let taps = look.lock().unwrap_or_else(PoisonError::into_inner).clone();
                    let mut told = false;
                    for (app, had) in taps {
                        if processes(&app) != had {
                            told |= tell.send(app).is_ok();
                        }
                    }
                    if told {
                        wake();
                    }
                }
            });
        if let Err(error) = started {
            eprintln!("error: no thread to look at other apps on: {error}");
        }
        Self {
            sounds,
            taps: BTreeMap::new(),
            opening: BTreeMap::new(),
            openings: 0,
            opened: mpsc::channel(),
            checked,
            changed,
            _stop: stop,
            waker,
            followed: false,
        }
    }

    /// Opens a tap of every app `project.json` hears that has none, closes the taps of the apps
    /// it no longer hears, gives the engine the taps that opened, and opens again a tap whose
    /// app's processes changed. `events` are the project's since the last poll. Never waits.
    pub fn poll(&mut self, project: &mut Project, events: &[ProjectEvent]) {
        while let Ok(opened) = self.opened.1.try_recv() {
            self.opened(project, opened);
        }
        let changed: Vec<AppSound> = self.changed.try_iter().collect();
        for app in changed {
            if self.taps.contains_key(&app) && !self.opening.contains_key(&app) {
                self.open(app);
            }
        }
        // A connection that comes into the graph or leaves it, as its instance comes or goes,
        // changes the problems.
        let follow = events.iter().any(|event| {
            matches!(
                event,
                ProjectEvent::ProjectFileChanged | ProjectEvent::ProblemsChanged
            )
        });
        if follow || !std::mem::replace(&mut self.followed, true) {
            self.follow(project);
        }
        let checked = (self.taps.iter())
            .filter(|(app, _)| !self.opening.contains_key(*app))
            .map(|(app, tapped)| (app.clone(), tapped.processes.clone()));
        *self.checked.lock().unwrap_or_else(PoisonError::into_inner) = checked.collect();
    }

    fn follow(&mut self, project: &mut Project) {
        let heard = project.app_sounds();
        let gone: Vec<(AppSound, Tapped)> = (self.taps)
            .extract_if(.., |app, _| !heard.contains(app))
            .collect();
        for (app, tapped) in gone {
            project.set_app_sound(&app, None);
            drop_off_thread(tapped.stream);
        }
        self.opening.retain(|app, _| heard.contains(app));
        for app in heard {
            if !self.taps.contains_key(&app) && !self.opening.contains_key(&app) {
                self.open(app);
            }
        }
    }

    /// Opens a tap of `app` on a thread of its own. A tap of it that is open plays until the
    /// new one is there.
    fn open(&mut self, app: AppSound) {
        self.openings += 1;
        let opening = self.openings;
        self.opening.insert(app.clone(), opening);
        let (sounds, opened, waker) = (
            self.sounds.clone(),
            self.opened.0.clone(),
            self.waker.clone(),
        );
        let started = std::thread::Builder::new()
            .name("app-sound-open".into())
            .spawn(move || {
                let processes = (sounds.processes)(&app);
                let tap = (sounds.open)(&app);
                if let Err(mpsc::SendError((_, _, _, Ok((stream, _))))) =
                    opened.send((app, opening, processes, tap))
                {
                    drop(stream);
                }
                waker();
            });
        if let Err(error) = started {
            eprintln!("error: no thread to open a tap on: {error}");
        }
    }

    fn opened(&mut self, project: &mut Project, (app, opening, processes, tap): Opened) {
        if self.opening.get(&app) != Some(&opening) {
            if let Ok((stream, _)) = tap {
                drop_off_thread(stream);
            }
            return;
        }
        self.opening.remove(&app);
        let rate = project.sample_rate();
        let (stream, sound) = match tap {
            Ok((stream, live)) if live.sample_rate() == rate => (Some(stream), Ok(live)),
            Ok((stream, live)) => {
                drop_off_thread(stream);
                (None, Ok(live))
            }
            Err(error) => (None, Err(error.to_string())),
        };
        let tapped = Tapped { processes, stream };
        if let Some(before) = self.taps.insert(app.clone(), tapped) {
            drop_off_thread(before.stream);
        }
        project.set_app_sound(&app, Some(sound));
    }
}

impl Drop for OtherApps {
    /// Core Audio can take a while to close a tap.
    fn drop(&mut self) {
        let streams: Vec<_> = std::mem::take(&mut self.taps)
            .into_values()
            .map(|tapped| tapped.stream)
            .collect();
        drop_off_thread(streams);
    }
}
