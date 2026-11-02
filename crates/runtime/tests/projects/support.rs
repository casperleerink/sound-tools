use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use plugin_host::{PluginFormat, Plugins, ScanCache};
use runtime::OFFLINE;
use sound_core::{Engine, Project};

use crate::plugin_hosts::{open_or_create, scanner, test_plugin_folders, test_plugin_host};
use crate::rack::racks_files;

/// Frames per bar at 120 bpm in 4/4 and 48 kHz.
pub(crate) const BAR: usize = 96_000;

pub(crate) fn synth(gain: f32) -> String {
    format!(r#"{{"tool": "instrument.synth", "state": {{"gain": {gain:?}}}}}"#)
}

/// A timeline record with (start, length, pitch) notes at velocity 100, from `start` for
/// `length` ticks.
pub(crate) fn clip(start: u64, length: u64, notes: &[(u64, u64, u8)]) -> String {
    let notes: Vec<String> = notes
        .iter()
        .map(|(start, length, pitch)| {
            format!(
                r#"{{"start": {start}, "length": {length}, "pitch": {pitch}, "velocity": 100}}"#
            )
        })
        .collect();
    format!(
        r#"{{"tool": "timeline", "state": {{"start": {start}, "length": {length}, "notes": [{}]}}}}"#,
        notes.join(", ")
    )
}

/// The chords of the piano of [`Harness::piece`], a chord per bar over four bars.
pub(crate) const CHORDS: [(u64, u64, u8); 5] = [
    (0, 3800, 48),
    (0, 3800, 64),
    (3840, 3800, 53),
    (7680, 3800, 55),
    (11520, 3800, 48),
];

/// A live project on a temporary folder with the offline stereo engine.
pub(crate) struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// The plugin host of this project. A render polls it for every buffer, which is what the
    /// runtime's own render loop does: a plugin may be waiting for main-thread work.
    pub plugins: Plugins,
    /// The time of the last outside change. Each one comes a minute after the one before, so
    /// it is an undo step of its own, as for changes made by hand. Without this, outside
    /// changes that a test makes within milliseconds would join (`OUTSIDE_POINT_WINDOW`).
    now: Instant,
    pub folder: tempfile::TempDir,
}

impl Harness {
    /// The default project.
    pub(crate) fn new() -> Self {
        Self::open(tempfile::tempdir().unwrap())
    }

    pub(crate) fn open(folder: tempfile::TempDir) -> Self {
        let (control, engine) = Engine::new(OFFLINE);
        let (project, plugins) = open_or_create(folder.path(), control);
        Self {
            project,
            engine,
            plugins,
            now: Instant::now(),
            folder,
        }
    }

    /// A project whose plugin host looks only in `plugins/` inside the project folder, where
    /// the repository's own test plugin is put. No plugin of this machine is used, so these
    /// tests run the same in CI.
    pub(crate) fn with_test_plugin(folder: tempfile::TempDir) -> (Self, Plugins) {
        let (control, engine) = Engine::new(OFFLINE);
        let plugins = test_plugin_host(folder.path());
        let project =
            runtime::open_or_create_with(folder.path(), control, plugins.clone()).unwrap();
        let harness = Self {
            project,
            engine,
            plugins: plugins.clone(),
            now: Instant::now(),
            folder,
        };
        (harness, plugins)
    }

    /// The same folder again, as closing and reopening the project does.
    pub(crate) fn reopen_with_test_plugin(self) -> (Self, Plugins) {
        let Self {
            project,
            engine,
            folder,
            ..
        } = self;
        drop((project, engine));
        Self::with_test_plugin(folder)
    }

    pub(crate) fn reopen(self) -> Self {
        let Self {
            project,
            engine,
            folder,
            ..
        } = self;
        drop((project, engine));
        Self::open(folder)
    }

    /// Racks of built-in devices, see `crate::rack`, each by its name with its children.
    pub(crate) fn racks(racks: &[(&str, &[(&str, &str)])]) -> Self {
        Self::racks_in(tempfile::tempdir().unwrap(), racks)
    }

    /// The same folder with these racks, opened again: for racks that need files, such as
    /// samples, that a test writes into the folder first.
    pub(crate) fn with_racks(self, racks: &[(&str, &[(&str, &str)])]) -> Self {
        let Self {
            project,
            engine,
            folder,
            ..
        } = self;
        drop((project, engine));
        Self::racks_in(folder, racks)
    }

    fn racks_in(folder: tempfile::TempDir, racks: &[(&str, &[(&str, &str)])]) -> Self {
        assert!(
            sound_typescript::has_bun(),
            "a rack is a tool of the project, which runs in Bun: install it, see tooling/fetch-bun.sh"
        );
        for (path, contents) in racks_files(racks) {
            write(folder.path(), &path, &contents);
        }
        Self::open(folder)
    }

    /// One rack called `rack`.
    pub(crate) fn rack(children: &[(&str, &str)]) -> Self {
        Self::racks(&[("rack", children)])
    }

    /// A piano with a chord per bar over four bars and a pad with one long note, each a rack
    /// of the synth. Both sound all the time, so a disturbance would show.
    pub(crate) fn piece() -> Self {
        let (piano, chords) = (synth(0.15), clip(0, 15360, &CHORDS));
        let (pad, long) = (synth(0.1), clip(0, 15360, &[(0, 15360, 72)]));
        Self::racks(&[
            ("piano", &[("instrument", &piano), ("chords", &chords)]),
            ("pad", &[("instrument", &pad), ("long", &long)]),
        ])
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    pub(crate) fn write(&self, relative: &str, contents: &str) -> PathBuf {
        write(self.project.root(), relative, contents)
    }

    pub(crate) fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.write(relative, contents);
        self.apply(&[path])
    }

    pub(crate) fn apply(&mut self, paths: &[PathBuf]) -> usize {
        self.now += Duration::from_secs(60);
        let changed = self.project.apply_outside_changes_at(paths, self.now);
        changed.unwrap()
    }

    pub(crate) fn render(&mut self, frames: usize) -> Vec<f32> {
        let output =
            runtime::render(&mut self.project, &mut self.engine, &self.plugins, frames).unwrap();
        let status = self.project.engine().poll().unwrap();
        assert_eq!((status.event_overflows, status.port_misuses), (0, 0));
        output
    }

    pub(crate) fn play(&mut self, frames: usize) -> Vec<f32> {
        self.project.engine().play();
        self.render(frames)
    }

    /// Plays from the start of the piece, so two renders of one session can be compared
    /// frame for frame. The plugins are told to release what they hold first, which a stop
    /// and a seek both do.
    pub(crate) fn play_from_the_start(&mut self, frames: usize) -> Vec<f32> {
        self.project.engine().stop();
        self.project.engine().seek(sound_core::Ticks(0));
        self.render(64);
        self.play(frames)
    }
}

/// The folder of test plugins with the host `--render` opens: it loads a plugin and never
/// writes its state.
pub(crate) fn read_only_test_plugin_host(root: &Path) -> Plugins {
    Plugins::read_only(test_plugin_folders(root), scanner(), ScanCache::none())
}

/// The same folder of test plugins, with the host `--inspect` opens: it says which plugins are
/// there and loads none of them.
pub(crate) fn test_plugin_host_that_only_lists(root: &Path) -> Plugins {
    Plugins::listing(test_plugin_folders(root), scanner(), ScanCache::none())
}

/// The record of a CLAP plugin instrument that names the repository's test plugin.
pub(crate) fn test_plugin(state_asset: &str) -> String {
    test_plugin_of(PluginFormat::Clap, state_asset)
}

/// The same for either format. Both test plugins are the same instrument, so a project can
/// hold one of each and a test can compare what they play.
pub(crate) fn test_plugin_of(format: PluginFormat, state_asset: &str) -> String {
    let plugin_id = match format {
        PluginFormat::Clap => test_clap_plugin::PLUGIN_ID,
        PluginFormat::Vst3 => test_vst3_plugin::PLUGIN_ID,
    };
    format!(
        r#"{{"tool": "plugin", "state": {{"format": "{}", "plugin_id": "{plugin_id}", "state_asset": "{state_asset}"}}}}"#,
        format.as_str()
    )
}

/// A state asset that makes the effect half of the test plugin add `offset` hundredths to
/// every sample, as the host would have saved it.
///
/// A CLAP asset is the plugin's own bytes. A VST 3 asset is the container the host writes,
/// because VST 3 keeps two states: `SVT3`, the component's state with its length, then the
/// controller's.
pub(crate) fn plugin_state(format: PluginFormat, offset: i32) -> Vec<u8> {
    let state = test_plugin_support::SavedState {
        offset,
        ..Default::default()
    };
    plugin_state_of(format, state)
}

/// A state asset of the test plugin that holds `state`, in the form of each format.
pub(crate) fn plugin_state_of(
    format: PluginFormat,
    state: test_plugin_support::SavedState,
) -> Vec<u8> {
    let own = test_plugin_support::save_state(state);
    match format {
        PluginFormat::Clap => own,
        PluginFormat::Vst3 => {
            let mut bytes = b"SVT3".to_vec();
            bytes.extend_from_slice(&(own.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&own);
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes
        }
    }
}

/// What the effect half of the test plugin saved in an asset, in hundredths.
pub(crate) fn saved_offset(format: PluginFormat, bytes: &[u8]) -> i32 {
    let own = match format {
        PluginFormat::Clap => bytes,
        PluginFormat::Vst3 => {
            assert_eq!(&bytes[..4], b"SVT3", "not a VST 3 state asset");
            let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
            &bytes[8..8 + length]
        }
    };
    test_plugin_support::load_state(own)
        .expect("a Test Tone state")
        .offset
}

/// A new project, as the app makes one in an empty folder, for a test to add files to before
/// it opens: files without a `project.json` are not a project.
pub(crate) fn project_folder() -> tempfile::TempDir {
    Harness::new().folder
}

pub(crate) fn write(root: &Path, relative: &str, contents: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    path
}

/// A mono float WAV of `seconds` of these samples at `rate`, in `assets/audio/`.
pub(crate) fn write_samples(
    harness: &Harness,
    name: &str,
    rate: u32,
    seconds: f64,
    sample: impl Fn(f64) -> f32,
) {
    let path = harness.path(&format!("assets/audio/{name}"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..(seconds * f64::from(rate)) as usize {
        writer
            .write_sample(sample(frame as f64 / f64::from(rate)))
            .unwrap();
    }
    writer.finalize().unwrap();
}

/// The first and the last frame on which two renders differ.
pub(crate) fn difference(a: &[f32], b: &[f32]) -> Option<(usize, usize)> {
    assert_eq!(a.len(), b.len());
    let differs = |(a, b): (&[f32], &[f32])| a != b;
    let mut frames = a.chunks(2).zip(b.chunks(2));
    let first = frames.position(differs)?;
    let last = a.chunks(2).zip(b.chunks(2)).rposition(differs)?;
    Some((first, last))
}

/// The first frame from `from` on whose left channel sounds.
pub(crate) fn first_sound(samples: &[f32], from: usize) -> Option<usize> {
    let left = samples.iter().step_by(2).enumerate().skip(from);
    let mut sounding = left.filter(|(_, sample)| sample.abs() > 1e-4);
    sounding.next().map(|(frame, _)| frame)
}
