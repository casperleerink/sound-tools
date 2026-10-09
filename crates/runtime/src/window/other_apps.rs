//! The sound of other apps that `project.json` hears: a tap of each app, open while a
//! connection in the graph starts at it, so macOS asks the composer about recording other apps
//! only for a project that does. Taps open on the background executor: the first one waits for
//! that answer. A tap at another rate than the output plays nothing: it closes, and the problem
//! of its connections says why.
//!
//! A tap of an app by name holds the processes the app had when it opened. Every two seconds
//! they are looked at again, and the tap opens again when they changed: the app started or
//! quit, or a helper came to play its sound, as a browser starts one.
//!
//! A tap closes on the background executor too: Core Audio can take a while to stop its stream
//! and remove it.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, Context, Entity, Task};
use sound_core::{AppSound, DeviceError, InputDevice, LiveInput, ProjectEvent};
use sound_ui::Session;

/// How often the processes of the apps are looked at again.
const CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// How the window hears other apps: the taps of macOS, or simulated ones in a test. Both are
/// called on a background thread.
#[derive(Clone)]
pub struct AppSounds {
    /// Opens a tap of the app: its stream, which stops and removes the tap when it is dropped,
    /// and the live input for the engine.
    pub open: Arc<dyn Fn(&AppSound) -> Result<OpenedTap, DeviceError> + Send + Sync>,
    /// What a tap of the app would hear now, see [`sound_core::app_processes`].
    pub processes: Arc<dyn Fn(&AppSound) -> Vec<u32> + Send + Sync>,
}

impl AppSounds {
    /// The process taps of macOS.
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
    _stream: Option<Box<dyn Send>>,
}

/// Drops a tap or its stream on the background executor, see the module doc.
fn close(tap: impl Send + 'static, cx: &mut Context<OtherApps>) {
    cx.background_spawn(async move { drop(tap) }).detach();
}

pub struct OtherApps {
    session: Entity<Session>,
    sounds: AppSounds,
    /// Every app that was opened, or that did not open, with why in the project.
    taps: BTreeMap<AppSound, Tapped>,
    /// The opening on its way for an app, by its number. One that arrives after the app is no
    /// longer heard, or after a newer opening began, is dropped.
    opening: BTreeMap<AppSound, u64>,
    openings: u64,
    _checking: Task<()>,
}

impl OtherApps {
    pub fn new(session: Entity<Session>, sounds: AppSounds, cx: &mut Context<Self>) -> Self {
        // A connection that comes into the graph or leaves it, as its instance comes or goes,
        // changes the problems.
        cx.subscribe(&session, |apps, _, event, cx| {
            if matches!(
                event,
                ProjectEvent::ProjectFileChanged | ProjectEvent::ProblemsChanged
            ) {
                apps.follow(cx);
            }
        })
        .detach();
        let apps = cx.weak_entity();
        cx.defer(move |cx| {
            // A window that went in the meantime opens nothing.
            apps.update(cx, |apps, cx| apps.follow(cx)).ok();
        });
        let checking = cx.spawn(async move |apps, cx| {
            loop {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                let Ok(changed) = apps.update(cx, |apps, cx| apps.changed(cx)) else {
                    break;
                };
                let changed = changed.await;
                let reopened = apps.update(cx, |apps, cx| {
                    for app in changed {
                        if apps.taps.contains_key(&app) && !apps.opening.contains_key(&app) {
                            apps.open(app, cx);
                        }
                    }
                });
                if reopened.is_err() {
                    break;
                }
            }
        });
        Self {
            session,
            sounds,
            taps: BTreeMap::new(),
            opening: BTreeMap::new(),
            openings: 0,
            _checking: checking,
        }
    }

    /// Opens a tap of every app `project.json` hears that has none, and closes the taps of the
    /// apps it no longer hears.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let heard = self.session.read(cx).project().app_sounds();
        let gone: Vec<(AppSound, Tapped)> = (self.taps)
            .extract_if(.., |app, _| !heard.contains(app))
            .collect();
        for (app, _) in &gone {
            self.session.update(cx, |session, cx| {
                session.background(cx, |project| project.set_app_sound(app, None))
            });
        }
        close(gone, cx);
        self.opening.retain(|app, _| heard.contains(app));
        for app in heard {
            if !self.taps.contains_key(&app) && !self.opening.contains_key(&app) {
                self.open(app, cx);
            }
        }
    }

    /// Opens a tap of `app` on the background executor. A tap of it that is open plays until
    /// the new one is there.
    fn open(&mut self, app: AppSound, cx: &mut Context<Self>) {
        self.openings += 1;
        let opening = self.openings;
        self.opening.insert(app.clone(), opening);
        let sounds = self.sounds.clone();
        let work = cx.background_spawn(async move {
            let processes = (sounds.processes)(&app);
            let opened = (sounds.open)(&app);
            (app, processes, opened)
        });
        cx.spawn(async move |apps, cx| {
            let (app, processes, opened) = work.await;
            // A window that went away takes its taps with it.
            apps.update(cx, |apps, cx| {
                apps.opened(app, opening, processes, opened, cx)
            })
            .ok();
        })
        .detach();
    }

    fn opened(
        &mut self,
        app: AppSound,
        opening: u64,
        processes: Vec<u32>,
        opened: Result<OpenedTap, DeviceError>,
        cx: &mut Context<Self>,
    ) {
        if self.opening.get(&app) != Some(&opening) {
            if let Ok((stream, _)) = opened {
                close(stream, cx);
            }
            return;
        }
        self.opening.remove(&app);
        let rate = self.session.read(cx).project().sample_rate();
        let (stream, sound) = match opened {
            Ok((stream, live)) if live.sample_rate() == rate => (Some(stream), Ok(live)),
            Ok((stream, live)) => {
                close(stream, cx);
                (None, Ok(live))
            }
            Err(error) => (None, Err(error.to_string())),
        };
        let tapped = Tapped {
            processes,
            _stream: stream,
        };
        if let Some(before) = self.taps.insert(app.clone(), tapped) {
            close(before, cx);
        }
        self.session.update(cx, |session, cx| {
            session.background(cx, |project| project.set_app_sound(&app, Some(sound)))
        });
    }

    /// The apps whose processes changed since their tap opened, worked out on the background
    /// executor.
    fn changed(&self, cx: &mut Context<Self>) -> Task<Vec<AppSound>> {
        let taps = self
            .taps
            .iter()
            .filter(|(app, _)| !self.opening.contains_key(*app));
        let taps: Vec<(AppSound, Vec<u32>)> = taps
            .map(|(app, tapped)| (app.clone(), tapped.processes.clone()))
            .collect();
        let processes = self.sounds.processes.clone();
        cx.background_spawn(async move {
            let changed = taps.into_iter().filter(|(app, had)| processes(app) != *had);
            changed.map(|(app, _)| app).collect()
        })
    }
}
