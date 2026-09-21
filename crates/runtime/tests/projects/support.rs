use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use plugin_host::{PluginFormat, Plugins, ScanCache, ScanCommand};
use runtime::OFFLINE;
use sound_core::{Engine, Project};

/// Frames per bar at 120 bpm in 4/4 and 48 kHz.
pub const BAR: usize = 96_000;

pub const TRACK: &str =
    r#"{"tool": "arrangement.track", "state": {"name": "NAME", "order": ORDER}}"#;

pub fn synth(gain: f32) -> String {
    format!(r#"{{"tool": "instrument.synth", "state": {{"gain": {gain:?}}}}}"#)
}

/// A clip record with (start, length, pitch) notes at velocity 100.
pub fn clip(start: u64, length: u64, notes: &[(u64, u64, u8)]) -> String {
    let notes: Vec<String> = notes
        .iter()
        .map(|(start, length, pitch)| {
            format!(
                r#"{{"start": {start}, "length": {length}, "pitch": {pitch}, "velocity": 100}}"#
            )
        })
        .collect();
    format!(
        r#"{{"tool": "arrangement.clip", "state": {{"start": {start}, "length": {length}, "notes": [{}]}}}}"#,
        notes.join(", ")
    )
}

/// A live project on a temporary folder with the offline stereo engine.
pub struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// The time of the last outside change. Each one comes a minute after the one before, so
    /// it is an undo step of its own, as for changes made by hand. Without this, outside
    /// changes that a test makes within milliseconds would join (`OUTSIDE_UNDO_WINDOW`).
    now: Instant,
    pub folder: tempfile::TempDir,
}

impl Harness {
    /// The default project.
    pub fn new() -> Self {
        Self::open(tempfile::tempdir().unwrap())
    }

    pub fn open(folder: tempfile::TempDir) -> Self {
        let (control, engine) = Engine::new(OFFLINE);
        let (project, _plugins) = runtime::open_or_create(folder.path(), control).unwrap();
        Self {
            project,
            engine,
            now: Instant::now(),
            folder,
        }
    }

    /// A project whose plugin host looks only in `plugins/` inside the project folder, where
    /// the repository's own test plugin is put. No plugin of this machine is used, so these
    /// tests run the same in CI.
    pub fn with_test_plugin(folder: tempfile::TempDir) -> (Self, Plugins) {
        let (control, engine) = Engine::new(OFFLINE);
        let plugins = test_plugin_host(folder.path(), true);
        let project =
            runtime::open_or_create_with(folder.path(), control, plugins.clone()).unwrap();
        let harness = Self {
            project,
            engine,
            now: Instant::now(),
            folder,
        };
        (harness, plugins)
    }

    /// The same folder again, as closing and reopening the project does.
    pub fn reopen_with_test_plugin(self) -> (Self, Plugins) {
        let Self {
            project,
            engine,
            folder,
            ..
        } = self;
        drop((project, engine));
        Self::with_test_plugin(folder)
    }

    pub fn reopen(self) -> Self {
        let Self {
            project,
            engine,
            folder,
            ..
        } = self;
        drop((project, engine));
        Self::open(folder)
    }

    /// The default project plus a piano with a chord per bar over four bars and a pad with
    /// one long note. Both sound all the time, so a disturbance would show.
    pub fn piece() -> Self {
        let mut harness = Self::new();
        let chords = [
            (0, 3800, 48),
            (0, 3800, 64),
            (3840, 3800, 53),
            (7680, 3800, 55),
            (11520, 3800, 48),
        ];
        harness.write_track("piano", 1, 0.15, &[("chords", clip(0, 15360, &chords))]);
        harness.write_track(
            "pad",
            2,
            0.1,
            &[("long", clip(0, 15360, &[(0, 15360, 72)]))],
        );
        assert_eq!(harness.project.problems(), []);
        harness
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        write(self.project.root(), relative, contents)
    }

    /// Writes a track folder the way an agent would and applies it as one group.
    pub fn write_track(&mut self, name: &str, order: u32, gain: f32, clips: &[(&str, String)]) {
        let folder = format!("state/arrangement/{name}");
        let track = TRACK
            .replace("NAME", name)
            .replace("ORDER", &order.to_string());
        self.write(&format!("{folder}/instance.json"), &track);
        self.write(&format!("{folder}/instrument.json"), &synth(gain));
        for (clip, contents) in clips {
            self.write(&format!("{folder}/{clip}.json"), contents);
        }
        let folder = self.path(&folder);
        assert_eq!(self.apply(&[folder]), 2 + clips.len());
    }

    pub fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.write(relative, contents);
        self.apply(&[path])
    }

    pub fn apply(&mut self, paths: &[PathBuf]) -> usize {
        self.now += Duration::from_secs(60);
        let changed = self.project.apply_outside_changes_at(paths, self.now);
        changed.unwrap()
    }

    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        let output = runtime::render(&mut self.project, &mut self.engine, frames).unwrap();
        let status = self.project.engine().poll().unwrap();
        assert_eq!((status.event_overflows, status.port_misuses), (0, 0));
        output
    }

    pub fn play(&mut self, frames: usize) -> Vec<f32> {
        self.project.engine().play();
        self.render(frames)
    }

    /// Plays from the start of the piece, so two renders of one session can be compared
    /// frame for frame. The plugins are told to release what they hold first, which a stop
    /// and a seek both do.
    pub fn play_from_the_start(&mut self, frames: usize) -> Vec<f32> {
        self.project.engine().stop();
        self.project.engine().seek(sound_core::Ticks(0));
        self.render(64);
        self.play(frames)
    }
}

/// A plugin host that scans one folder, with the test plugin of every format in it. The
/// scanner is the real `runtime` executable with its scan argument, so the child process of a
/// scan is the one the application uses. No plugin of this machine is ever listed, and the
/// cache of this machine is never read or written.
pub fn test_plugin_host(root: &Path, writes_state: bool) -> Plugins {
    let folder = root.join("plugins");
    test_clap_plugin::install_into(&folder);
    test_vst3_plugin::install_into(&folder);
    let scanner = ScanCommand::new(
        env!("CARGO_BIN_EXE_runtime"),
        [std::ffi::OsString::from(plugin_host::SCAN_ARGUMENT)],
    );
    if writes_state {
        Plugins::new(vec![folder], scanner, ScanCache::none())
    } else {
        Plugins::read_only(vec![folder], scanner, ScanCache::none())
    }
}

/// The record of a CLAP plugin instrument that names the repository's test plugin.
pub fn test_plugin(state_asset: &str) -> String {
    test_plugin_of(PluginFormat::Clap, state_asset)
}

/// The same for either format. Both test plugins are the same instrument, so a project can
/// hold one of each and a test can compare what they play.
pub fn test_plugin_of(format: PluginFormat, state_asset: &str) -> String {
    let plugin_id = match format {
        PluginFormat::Clap => test_clap_plugin::PLUGIN_ID,
        PluginFormat::Vst3 => test_vst3_plugin::PLUGIN_ID,
    };
    format!(
        r#"{{"tool": "plugin", "state": {{"format": "{}", "plugin_id": "{plugin_id}", "state_asset": "{state_asset}"}}}}"#,
        format.as_str()
    )
}

pub fn write(root: &Path, relative: &str, contents: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    path
}

/// The first and the last frame on which two renders differ.
pub fn difference(a: &[f32], b: &[f32]) -> Option<(usize, usize)> {
    assert_eq!(a.len(), b.len());
    let differs = |(a, b): (&[f32], &[f32])| a != b;
    let mut frames = a.chunks(2).zip(b.chunks(2));
    let first = frames.position(differs)?;
    let last = a.chunks(2).zip(b.chunks(2)).rposition(differs)?;
    Some((first, last))
}
