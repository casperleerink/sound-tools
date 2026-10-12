//! What the live sources of `project.json` connections play: the input of a device and the
//! sound of other apps. The window opens them; the project plays them and says why a
//! connection from one is silent.

use std::collections::{BTreeMap, BTreeSet};

use super::file::{SavedConnection, SavedSource};
use super::{Project, ProjectEvent};
use crate::apps::AppSound;
use crate::device::DeviceError;
use crate::input::{InputId, LiveInput};

/// What was given of each live source, as far as it decides whether a connection is heard.
#[derive(Default)]
pub(super) struct LiveSources {
    /// The sample rate and channels of the device input, while one is given.
    device: Option<(u32, usize)>,
    /// The sample rate of the sound of each app that was given, or why it is not heard.
    apps: BTreeMap<AppSound, Result<u32, String>>,
}

impl LiveSources {
    fn set_device(&mut self, input: Option<&LiveInput>) {
        self.device = input.map(|input| (input.sample_rate(), input.channels()));
    }

    /// Notes the sound of `app` and returns the input to play, if any.
    fn set_app(
        &mut self,
        app: &AppSound,
        sound: Option<Result<LiveInput, String>>,
    ) -> Option<LiveInput> {
        match sound {
            None => {
                self.apps.remove(app);
                None
            }
            Some(Ok(live)) => {
                self.apps.insert(app.clone(), Ok(live.sample_rate()));
                Some(live)
            }
            Some(Err(why)) => {
                self.apps.insert(app.clone(), Err(why));
                None
            }
        }
    }

    /// Why each `device_input` or `app` connection is silent with what was given, when the
    /// output runs at `output_rate`.
    pub(super) fn problems(
        &self,
        connections: &[SavedConnection],
        output_rate: u32,
    ) -> Vec<String> {
        let connections = connections.iter().enumerate();
        connections
            .filter_map(|(index, connection)| {
                let why = match &connection.from {
                    SavedSource::DeviceInput(channel) => {
                        self.device_problem(*channel, output_rate)?
                    }
                    SavedSource::App(app) => self.app_problem(app, output_rate)?,
                    SavedSource::Output(_) => return None,
                };
                Some(format!("connections[{index}]: not heard, because {why}"))
            })
            .collect()
    }

    fn app_problem(&self, app: &AppSound, output_rate: u32) -> Option<String> {
        if !cfg!(target_os = "macos") {
            return Some(DeviceError::AppsOnlyOnMacos.to_string());
        }
        match self.apps.get(app)? {
            Err(why) => Some(why.clone()),
            Ok(rate) if *rate != output_rate => Some(format!(
                "the sound of other apps comes at {rate} Hz and the output runs at {output_rate} Hz. Set the output device to {rate} Hz in Audio MIDI Setup"
            )),
            Ok(_) => None,
        }
    }

    fn device_problem(&self, channel: usize, output_rate: u32) -> Option<String> {
        let (input_rate, channels) = self.device?;
        if input_rate != output_rate {
            return Some(format!(
                "the audio input runs at {input_rate} Hz and the output at {output_rate} Hz. Live input needs both at one rate: set them to the same rate in the sound settings of the system (Audio MIDI Setup on macOS)"
            ));
        }
        let has = match channels {
            1 => "one channel".to_string(),
            channels => format!("{channels} channels"),
        };
        (channel >= channels)
            .then(|| format!("the audio input has {has}, counted from 0, and no channel {channel}"))
    }
}

impl Project {
    /// Whether a `project.json` connection from the device input is in the graph, so the input
    /// has to be open. One whose instance or port is missing needs no input.
    pub fn hears_device_input(&self) -> bool {
        let connections = self.project_file.connections.iter();
        connections
            .filter(|connection| matches!(connection.from, SavedSource::DeviceInput(_)))
            .any(|connection| self.bindings.is_bound(connection))
    }

    /// The apps whose sound a `project.json` connection in the graph starts at, so each has to
    /// be tapped.
    pub fn app_sounds(&self) -> BTreeSet<AppSound> {
        let connections = self.project_file.connections.iter();
        let bound = connections.filter(|connection| self.bindings.is_bound(connection));
        (bound.filter_map(|connection| match &connection.from {
            SavedSource::App(app) => Some(app.clone()),
            _ => None,
        }))
        .collect()
    }

    /// Plays the input of a device into the `device_input` connections of `project.json`, or
    /// silence with `None`, which is what they hear until an input is given. An input at another
    /// sample rate than the engine stays silent, and each of those connections is a problem that
    /// says why, as is one from a channel the input does not have. That stays so until the next
    /// call, also when the device itself was closed, as it is when it plays nothing.
    pub fn set_live_input(&mut self, input: Option<LiveInput>) {
        let before = self.live_input_problems();
        self.live_sources.set_device(input.as_ref());
        self.engine.set_live_input(InputId::DEVICE, input);
        if before != self.live_input_problems() {
            self.push_event(ProjectEvent::ProblemsChanged);
        }
    }

    /// Plays the sound of `app` into the `app` connections of `project.json` that name it, or
    /// silence with `None`. `Err` says why it is not heard, and each of those connections then
    /// says so as a problem, as does one when the sound comes at another rate than the output.
    pub fn set_app_sound(&mut self, app: &AppSound, sound: Option<Result<LiveInput, String>>) {
        let before = self.live_input_problems();
        let live = self.live_sources.set_app(app, sound);
        self.engine.set_live_input(app.input(), live);
        if before != self.live_input_problems() {
            self.push_event(ProjectEvent::ProblemsChanged);
        }
    }

    /// Why a `device_input` or `app` connection is silent with what was given.
    pub(super) fn live_input_problems(&self) -> Vec<String> {
        let output_rate = self.engine.config().sample_rate;
        (self.live_sources).problems(&self.project_file.connections, output_rate)
    }
}
