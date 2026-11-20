//! What comes in: the MIDI keyboard and the computer keys, the audio input with the takes of the
//! recorders, the sound of other apps, and what the app has to say: a lost output device, an
//! update.

use rune::Tx;
use runtime::input::{self, Devices, Happened, Input, KeyDown, OtherApps};
use runtime::update::{Action, Ready, Update};
use sound_core::{OutputStream, Project, ProjectEvent};
use sound_typescript::window::Waker;

use super::State;

pub(crate) struct Incoming {
    pub(crate) live: Input,
    /// macOS only: elsewhere a connection from an app is a problem that says so.
    apps: Option<OtherApps>,
    /// None in a run without a window, which checks for nothing.
    update: Option<Update>,
    /// The update the notice shows: it goes with Restart.
    pub(crate) ready: Option<Ready>,
}

impl Incoming {
    /// A run without a window has no device: the keyboard joins the offline engine and the
    /// audio input never opens. `input` is the input device the composer chose in Settings.
    pub(super) fn open(
        project: &mut Project,
        output: Option<&OutputStream>,
        input: Option<String>,
        waker: Waker,
    ) -> Self {
        let Some(output) = output else {
            return Self {
                live: Input::new(project, Devices::default(), waker),
                apps: None,
                update: None,
                ready: None,
            };
        };
        let devices = Devices {
            open_input: Some(input::system_input(input)),
            timing: Some(output.timing().clone()),
            midi_ports: true,
        };
        let apps = cfg!(target_os = "macos")
            .then(|| OtherApps::new(input::AppSounds::system(), waker.clone()));
        let update = (runtime::app::support_folder().ok())
            .map(|support| Update::start(&support, waker.clone()));
        Self {
            live: Input::new(project, devices, waker),
            apps,
            update,
            ready: None,
        }
    }
}

impl State {
    /// Takes what came in since the last frame: a finished take goes to its tool's code, what
    /// went wrong to the notice. `events` are the project's since. Whether what shows changed.
    pub(super) fn take_in(&mut self, events: &[ProjectEvent]) -> bool {
        let selected = self.session.selected().cloned();
        let incoming = &mut self.incoming;
        let happened = self.session.background(|project| {
            if let Some(apps) = &mut incoming.apps {
                apps.poll(project, events);
            }
            incoming.live.poll(project, events, selected.as_ref())
        });
        let mut changed = !happened.is_empty();
        for happened in happened {
            let notice = match happened {
                Happened::Notice(notice) => Some(notice),
                Happened::Take(take) => (self.tools.as_mut()).and_then(|tools| {
                    (self.session).background(|project| tools.take(project, take))
                }),
            };
            if let Some(notice) = notice {
                self.session.report(notice);
            }
        }
        let recording = self.incoming.live.is_recording();
        changed |= recording != self.session.recording();
        self.session.set_recording(recording);
        // The switch is the session's, which code and cmd-K turn: without a keyboard the keys
        // stay off.
        let keys = self.session.transport().keys;
        if keys != self.incoming.live.computer_keys() {
            let on = self.incoming.live.set_computer_keys(keys);
            self.session.set_computer_keys(on);
        }
        if let super::Sound::Device(output) = &self.sound {
            for error in output.take_errors() {
                self.session.report(error);
            }
        }
        let ready = self.incoming.update.as_ref().and_then(Update::poll);
        if ready.is_some() && self.incoming.ready.is_none() {
            self.incoming.ready = ready;
            changed = true;
        }
        changed
    }

    /// A key down the computer keys may play: one they hold, or with `free` one of theirs. Whether
    /// they took it, and nothing else may hear it.
    pub(super) fn computer_key(
        &mut self,
        key: &str,
        modified: bool,
        repeat: bool,
        free: bool,
    ) -> bool {
        self.incoming.live.computer_key_down(KeyDown {
            key,
            modified,
            repeat,
            free,
        })
    }

    /// The button of the update's notice: Restart starts the app again, which installs it;
    /// Download opens the release page.
    pub(crate) fn take_update(&mut self, tx: &mut Tx) {
        let Some(ready) = &self.incoming.ready else {
            return;
        };
        let page = match &ready.action {
            // A run without a window starts nothing.
            Action::Restart if self.headless() => return,
            Action::Restart => return crate::shell::start_again(tx),
            Action::Download { page } | Action::Failed { page, .. } => page,
        };
        if let Err(error) = tx.open_url(page) {
            self.session
                .report(format!("The page did not open: {error}"));
        }
    }
}
