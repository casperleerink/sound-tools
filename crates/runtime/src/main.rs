//! The project runtime. It opens a project folder, keeps it live and plays it on the default
//! output device, in a window or headless.
//!
//! ```text
//! runtime                                                  the app: the last project, or a folder panel
//! runtime <project-folder>                                 run live in the window
//! runtime <project-folder> --headless                      run live, commands from stdin
//! runtime <project-folder> --inspect                       print a summary, open no device
//! runtime <project-folder> --render <wav>                 render the project and its tail
//! runtime <project-folder> --render <wav> --seconds <n>    render the first n seconds
//! runtime <project-folder> --render <wav> --from <at> --to <at>
//!                                                          render a range and its tail
//! runtime <project-folder> --analyze                       measure the project and its tail
//! runtime <audio-file> --analyze                           measure a WAV, AIFF or FLAC file
//! runtime --plugins                                        list the plugins of this machine
//! runtime --plugin-params <format> <plugin_id>             list the parameters of one plugin
//! runtime --help                                           print this usage
//! ```
//!
//! `<at>` is a position in ticks (`3840`), or a time as a playhead shows it (`1:23.5`) or in
//! seconds (`83.5s`). A render and an analysis of a project take `--from` and `--to`, either
//! one alone, or `--seconds`, and `--solo <track>`, as often as there are tracks to hear, by
//! name or id. Solo changes no file. An analysis of a file takes times only.
//!
//! A render with `--progress` at the end also prints `progress: <percent>` lines, for the
//! progress bar of the window's export.
//!
//! Only the first two forms start GPUI. Tests, CI and agents use the others. In
//! `Sound Tools.app` and as the command line tool this program is called `sound-tools`.
//!
//! Headless, it reads one command per line from stdin: `play`, `pause`, `stop`,
//! `seek <ticks>`, `undo`, `redo`, `status`, `quit`. The end of stdin also quits. This is
//! provisional. It is not the protocol of the outer application.
//!
//! On Windows it is a window program, so the app opens no console window next to its own.
//! Started from a terminal, it prints there anyway: see `print_to_the_terminal`.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::io::BufRead;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use plugin_host::{PluginFormat, Plugins};
use runtime::analysis::Meter;
use runtime::analysis::report::{Timeline, report};
use runtime::{OFFLINE, open_or_create_with, open_read_only_with, problems, summary};
use sound_core::{
    Clock, Engine, EngineConfig, EngineStatus, OutputDevice, Project, ProjectEvent, Ticks,
};

fn print_summary(project: &Project) {
    println!("project: {}", project.root().display());
    println!("{}", summary(project));
}

fn print_problems(project: &Project) {
    println!("{}", problems(project));
}

/// Prints what changed, so that a person or an agent sees each live change arrive.
fn print_events(project: &mut Project) {
    let events = project.drain_events();
    let mut problems_changed = false;
    for event in &events {
        match event {
            ProjectEvent::Created(id) | ProjectEvent::Changed(id) => {
                let verb = if matches!(event, ProjectEvent::Created(_)) {
                    "created"
                } else {
                    "changed"
                };
                let state = project.state_json(id).unwrap_or_default();
                println!("{verb} {id}  {state}");
            }
            ProjectEvent::Deleted(id) => println!("deleted {id}"),
            ProjectEvent::ProjectFileChanged => {
                let project_file = project.project_file();
                let tempo = project_file.tempo_map.tempo_changes().first();
                println!(
                    "project.json changed: {} connections, {} bpm at the start",
                    project_file.connections.len(),
                    tempo.map_or(0.0, |change| change.bpm.bpm()),
                );
            }
            ProjectEvent::ProblemsChanged => problems_changed = true,
        }
    }
    if problems_changed {
        print_problems(project);
    }
}

fn print_status(project: &mut Project, status: &EngineStatus) {
    let clock = project.engine().clock();
    let position = clock
        .tempo_map()
        .time_signatures()
        .bar_beat_of(status.playhead_tick);
    println!(
        "status: {}, playhead {} (tick {}), {} edits applied, {} events dropped, undo: {}, redo: {}",
        if status.playing { "playing" } else { "stopped" },
        position,
        status.playhead_tick.0,
        status.batches_applied,
        status.event_overflows,
        project.undo_label().unwrap_or("nothing"),
        project.redo_label().unwrap_or("nothing"),
    );
}

enum Flow {
    Continue,
    Quit,
}

fn run_command(line: &str, project: &mut Project, status: &EngineStatus) -> Result<Flow> {
    let mut words = line.split_whitespace();
    match (words.next(), words.next()) {
        (Some("play"), None) => project.engine().play(),
        (Some("pause"), None) => project.engine().pause(),
        (Some("stop"), None) => project.engine().stop(),
        (Some("seek"), Some(ticks)) => {
            let ticks = ticks.parse().context("seek takes a position in ticks")?;
            project.engine().seek(Ticks(ticks));
        }
        (Some("undo"), None) => match project.undo()? {
            Some(label) => println!("undid: {label}"),
            None => println!("nothing to undo"),
        },
        (Some("redo"), None) => match project.redo()? {
            Some(label) => println!("redid: {label}"),
            None => println!("nothing to redo"),
        },
        (Some("status"), None) => print_status(project, status),
        (Some("quit"), None) => return Ok(Flow::Quit),
        (None, _) => {}
        _ => println!(
            "unknown command {line:?}: play, pause, stop, seek <ticks>, undo, redo, status, quit"
        ),
    }
    Ok(Flow::Continue)
}

/// Lines from stdin arrive through a channel, so the control loop never blocks on input.
fn stdin_lines() -> Receiver<String> {
    let (sender, lines) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    lines
}

fn run(folder: &Path) -> Result<()> {
    let device = OutputDevice::default_output()?;
    let config = EngineConfig::new(device.sample_rate(), device.channels());
    println!(
        "device: {} Hz, {} channels",
        config.sample_rate, config.channels
    );
    let (control, engine) = Engine::new(config);
    // Headless may block on the scan: it has no window to keep answering.
    let plugins = runtime::plugins(false)?;
    let mut project = open_or_create_with(folder, control, plugins.clone())?;
    for notice in plugins.take_notices() {
        println!("plugin scan: {notice}");
    }
    project.drain_events();
    print_summary(&project);
    project.watch()?;
    let stream = device.start(engine)?;
    println!("ready");

    let lines = stdin_lines();
    let status = loop {
        let status = project.engine().poll()?;
        // One bad file or one failed write must not end the session. It is reported instead.
        if let Err(error) = project.poll() {
            println!("error: {error}");
        }
        let (problems, errors) = runtime::tick(&mut project, &plugins);
        for problem in problems {
            println!("error: {problem}");
        }
        for error in errors {
            println!("error: {error}");
        }
        print_events(&mut project);
        match lines.try_recv() {
            Ok(line) => match run_command(&line, &mut project, &status) {
                Ok(Flow::Continue) => print_events(&mut project),
                Ok(Flow::Quit) => break status,
                Err(error) => println!("error: {error}"),
            },
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break status,
        }
        std::thread::sleep(Duration::from_millis(5));
    };

    for problem in plugins.close(&mut project) {
        println!("error: {problem}");
    }
    print_status(&mut project, &status);
    let device_status = stream.status();
    let errors = stream.take_errors();
    drop(stream);
    println!("callbacks: {}", status.blocks);
    println!("xruns: {}", device_status.xruns);
    println!("late callbacks: {}", device_status.late_callbacks);
    println!("slowest callback: {:?}", device_status.slowest_callback);
    println!("edits still waiting: {}", project.engine().pending_edits());
    println!(
        "command ring full: {}",
        project.engine().command_ring_full()
    );
    println!("return ring full: {}", status.return_ring_full);
    println!("event overflows: {}", status.event_overflows);
    println!("port misuses: {}", status.port_misuses);
    println!("stream errors: {}", errors.len());
    for error in &errors {
        println!("  {error}");
    }
    if device_status.xruns > 0 || !errors.is_empty() {
        bail!("playback had problems, see the report above");
    }
    Ok(())
}

/// Reads the project without its lock, so it works next to a running runtime. It loads no
/// plugin: printing a project needs none, and a plugin that is loaded runs somebody else's
/// code in this process. A plugin this machine does not have is still named in the problems.
fn inspect(folder: &Path) -> Result<()> {
    let project = runtime::open_for_inspect(folder)?;
    print_summary(&project);
    Ok(())
}

/// What a render plays.
enum Span {
    /// From the start, this long, with no tail. Whatever the clips say.
    Seconds(f64),
    /// This range and the tail, as the window exports selected clips.
    Range(Ticks, Ticks),
}

/// Where a render starts or ends: ticks, or a time as the composer reads it off the playhead,
/// which is how they say where they heard something.
#[derive(Clone, Copy)]
enum Position {
    Ticks(Ticks),
    Seconds(f64),
}

impl Position {
    /// `3840` is ticks, `1:23.5` minutes and seconds, `83.5s` seconds.
    fn parse(text: &str) -> Result<Self> {
        if let Ok(ticks) = text.parse() {
            return Ok(Self::Ticks(Ticks(ticks)));
        }
        let seconds = match (text.strip_suffix('s'), text.split_once(':')) {
            (Some(seconds), _) => seconds.parse().ok(),
            (None, Some((minutes, seconds))) => {
                match (minutes.parse::<u32>(), seconds.parse::<f64>()) {
                    (Ok(minutes), Ok(seconds)) if (0.0..60.0).contains(&seconds) => {
                        Some(f64::from(minutes) * 60.0 + seconds)
                    }
                    _ => None,
                }
            }
            (None, None) => None,
        };
        match seconds {
            Some(seconds) if seconds.is_finite() && seconds >= 0.0 => Ok(Self::Seconds(seconds)),
            _ => Err(anyhow!(
                "{text:?} is not a position: give ticks (3840), a time (1:23.5) or seconds (83.5s)"
            )),
        }
    }

    fn tick(self, clock: &Clock) -> Ticks {
        match self {
            Self::Ticks(ticks) => ticks,
            Self::Seconds(seconds) => clock.tick_at_seconds(seconds),
        }
    }
}

/// What a render and an analysis play: the flags after `--render <wav>` or `--analyze`.
#[derive(Default)]
struct Options {
    seconds: Option<f64>,
    from: Option<Position>,
    to: Option<Position>,
    solo: Vec<String>,
}

impl Options {
    fn parse(mut arguments: &[&str]) -> Result<Self> {
        let mut options = Self::default();
        while let [flag, value, rest @ ..] = arguments {
            match *flag {
                "--seconds" => {
                    options.seconds = Some(value.parse().context("--seconds takes a number")?);
                }
                "--from" => options.from = Some(Position::parse(value)?),
                "--to" => options.to = Some(Position::parse(value)?),
                "--solo" => options.solo.push(value.to_string()),
                _ => bail!(USAGE),
            }
            arguments = rest;
        }
        if !arguments.is_empty() {
            bail!(USAGE);
        }
        Ok(options)
    }

    /// What a render of `project` plays. With neither `--from` nor `--to`, the project from the
    /// start to the end of the last clip, and the tail.
    fn span(&self, project: &Project) -> Result<Span> {
        let clock = project.clock();
        match (self.seconds, self.from, self.to) {
            (Some(seconds), None, None) => Ok(Span::Seconds(seconds)),
            (Some(_), _, _) => {
                bail!("--seconds renders from the start: give it alone, or --from and --to")
            }
            (None, from, to) => {
                let from = from.map_or(Ticks(0), |from| from.tick(clock));
                let to = match to {
                    Some(to) => to.tick(clock),
                    None => runtime::project_end(project)
                        .context("the project has no clips, so there is nothing to render")?,
                };
                if to <= from {
                    bail!("--to must come after --from");
                }
                Ok(Span::Range(from, to))
            }
        }
    }

    /// The span of an audio file of `length` seconds, in seconds. A file has no ticks.
    fn file_span(&self, length: f64) -> Result<(f64, f64)> {
        if !self.solo.is_empty() {
            bail!("--solo is for a project: a file has no tracks");
        }
        let seconds = |position: Option<Position>, otherwise: f64| match position {
            None => Ok(otherwise),
            Some(Position::Seconds(seconds)) => Ok(seconds.min(length)),
            Some(Position::Ticks(_)) => {
                bail!("a file has no ticks: give a time, such as 1:23.5 or 83.5s")
            }
        };
        let (from, to) = match (self.seconds, self.from, self.to) {
            (Some(seconds), None, None) => (0.0, seconds.min(length)),
            (Some(_), _, _) => {
                bail!("--seconds measures from the start: give it alone, or --from and --to")
            }
            (None, from, to) => (seconds(from, 0.0)?, seconds(to, length)?),
        };
        if to <= from {
            bail!("--to must come after --from, and --from before the end of the file");
        }
        Ok((from, to))
    }
}

/// Opens the project read-only, with the tracks of `--solo` soloed in memory.
fn open_for_render(folder: &Path, options: &Options) -> Result<(Project, Engine, Plugins)> {
    let plugins = runtime::plugins(true)?;
    let (mut project, mut engine) = open_read_only_with(folder, plugins.clone())?;
    print_problems(&project);
    for problem in runtime::solo(&mut project, &mut engine, &plugins, &options.solo)? {
        println!("error: {problem}");
    }
    Ok((project, engine, plugins))
}

/// Plays `span` into `write`, and prints what the plugin host reported on the way.
///
/// The plugin host is polled for every buffer, as the live loop does: a render answers a
/// plugin's main-thread requests or it renders what a plugin that is waiting for one sounds
/// like, which can be nothing at all.
fn play(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    span: &Span,
    write: impl FnMut(&[f32]) -> Result<()>,
) -> Result<()> {
    let problems = match *span {
        Span::Seconds(seconds) => {
            project.engine().play();
            let frames = (seconds * f64::from(OFFLINE.sample_rate)) as usize;
            runtime::render_into(project, engine, plugins, frames, write)?
        }
        Span::Range(from, to) => runtime::render_range(project, engine, plugins, from, to, write)?,
    };
    for problem in problems {
        println!("error: {problem}");
    }
    Ok(())
}

/// Above zero, notes were lost: more events in one block than a port holds, or more held notes
/// than a track keeps.
fn print_event_overflows(project: &mut Project) -> Result<()> {
    let status = project.engine().poll()?;
    println!("event overflows: {}", status.event_overflows);
    Ok(())
}

/// Renders the span of `options` to `wav`. With `progress` it prints each whole percent of the
/// span it reaches.
fn render(folder: &Path, wav: &Path, options: &Options, progress: bool) -> Result<()> {
    let (mut project, mut engine, plugins) = open_for_render(folder, options)?;
    // Before the file is made, so a render that cannot happen leaves no empty file behind.
    let span = options.span(&project)?;
    let rate = f64::from(OFFLINE.sample_rate);
    let length = match span {
        Span::Seconds(seconds) => (seconds * rate) as u64,
        Span::Range(from, to) => {
            let clock = project.clock();
            clock.frame_of(to).0.saturating_sub(clock.frame_of(from).0)
        }
    };
    let mut writer = hound::WavWriter::create(
        wav,
        hound::WavSpec {
            channels: OFFLINE.channels as u16,
            sample_rate: OFFLINE.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    let mut peak = 0.0_f32;
    let (mut written, mut shown) = (0_u64, None);
    let write = |samples: &[f32]| {
        for sample in samples {
            peak = peak.max(sample.abs());
            writer.write_sample(*sample)?;
        }
        written += (samples.len() / OFFLINE.channels) as u64;
        // The tail has no known length, so it stays at 100.
        let percent = (written * 100 / length.max(1)).min(100);
        if progress && shown != Some(percent) {
            println!("progress: {percent}");
            shown = Some(percent);
        }
        Ok(())
    };
    play(&mut project, &mut engine, &plugins, &span, write)?;
    let frames = writer.len() / OFFLINE.channels as u32;
    writer.finalize()?;
    let seconds = f64::from(frames) / f64::from(OFFLINE.sample_rate);
    println!(
        "rendered {seconds:.2} s to {}, peak {peak:.4}",
        wav.display()
    );
    print_event_overflows(&mut project)
}

/// How much earlier than its start a range of an analysis plays: long enough for most tails of
/// reverbs and releases to reach into it.
const WARM_UP_SECONDS: f64 = 4.0;

/// Renders the span of `options` and prints what it measures, row by row: no file is written.
/// See [`runtime::analysis`].
fn analyze(path: &Path, options: &Options) -> Result<()> {
    if path.is_file() {
        return analyze_file(path, options);
    }
    let (mut project, mut engine, plugins) = open_for_render(path, options)?;
    let span = options.span(&project)?;
    let clock = project.clock().clone();
    let timeline = match span {
        Span::Seconds(seconds) => Timeline::Project {
            from: Ticks(0),
            to: clock.tick_at_seconds(seconds),
            tail: false,
            clock: clock.clone(),
        },
        Span::Range(from, to) => Timeline::Project {
            clock: clock.clone(),
            from,
            to,
            tail: true,
        },
    };
    // A range plays from a little earlier, so what sounds into it from before, such as the
    // tail of a reverb, is in it as it is when the whole piece plays. The meter hears that
    // part and measures none of it.
    let (span, mut warm_up) = match span {
        Span::Range(from, to) => {
            let earlier = (clock.seconds_of(from) - WARM_UP_SECONDS).max(0.0);
            let earlier = clock.tick_at_seconds(earlier).min(from);
            let frames = clock.frame_of(from).0 - clock.frame_of(earlier).0;
            (Span::Range(earlier, to), frames as usize * OFFLINE.channels)
        }
        span => (span, 0),
    };
    let mut meter = Meter::new(OFFLINE.sample_rate, timeline.row_starts());
    let measure = |samples: &[f32]| {
        let (before, after) = samples.split_at(warm_up.min(samples.len()));
        warm_up -= before.len();
        meter.warm_up(before);
        meter.push(after);
        Ok(())
    };
    play(&mut project, &mut engine, &plugins, &span, measure)?;
    println!("{}", report(&timeline, &meter.finish()));
    print_event_overflows(&mut project)
}

/// Measures an audio file, such as a sample under `assets/`, as [`analyze`] measures a render.
fn analyze_file(path: &Path, options: &Options) -> Result<()> {
    let bytes =
        std::fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let audio = sound_media::Audio::parse(bytes)
        .with_context(|| format!("{} is not a WAV, AIFF or FLAC file", path.display()))?;
    let (from, to) = options.file_span(audio.seconds())?;
    let rate = audio.sample_rate();
    // As it plays in a project: a mono file on both channels, which reads 3 dB above a meter
    // of one channel, and a file with more channels as its first two.
    let channels = match audio.channels() {
        1 => "mono, measured on both channels".to_string(),
        2 => "stereo".to_string(),
        count => format!("{count} channels, measured as its first two"),
    };
    println!(
        "{}: {rate} Hz, {channels}, {:.2} s",
        path.display(),
        audio.seconds()
    );
    let timeline = Timeline::File {
        sample_rate: rate,
        from,
        to,
    };
    let mut meter = Meter::new(rate, timeline.row_starts());
    let at = |seconds: f64| (seconds * f64::from(rate)).round() as i64;
    let (start, end) = (at(from), at(to));
    // From the file itself, as a range of a project warms up from what plays before it.
    let warm_up = at((from - WARM_UP_SECONDS).max(0.0));
    let mut buffer = vec![[0.0_f32; 2]; 4096];
    for first in (warm_up..end).step_by(buffer.len()) {
        let buffer = &mut buffer[..(end - first).min(4096) as usize];
        audio.read(first, buffer);
        let samples = buffer.as_flattened();
        let before = ((start - first).clamp(0, 4096) as usize).min(buffer.len());
        let (before, after) = samples.split_at(before * 2);
        meter.warm_up(before);
        meter.push(after);
    }
    println!("{}", report(&timeline, &meter.finish()));
    Ok(())
}

/// Prints every plugin this machine has, with how long the scan took. A composer or an agent
/// needs the plugin's own id to put it on a track, and it is in no file.
///
/// It looks at every bundle again and writes what it finds, so it is also how a plugin that
/// hung or crashed once is tried again: the cache of this machine does not try one twice.
fn list_plugins() -> Result<()> {
    let plugins = runtime::plugins_refreshing_the_cache()?;
    let started = std::time::Instant::now();
    plugins.wait_for_scan();
    let scan = plugins.scan();
    let took = started.elapsed();
    for plugin in &scan.plugins {
        // What the plugin says it is, which is what a picker offers it for. A record may name
        // any plugin in any slot: nothing can check that what a plugin says is true.
        let kind = match (plugin.is_instrument(), plugin.is_effect()) {
            (true, true) => "instrument and effect",
            (true, false) => "instrument",
            (false, true) => "effect",
            (false, false) => "neither an instrument nor an effect",
        };
        println!(
            "{:<5} {}  {} {} ({kind}, {})",
            plugin.format.as_str(),
            plugin.id,
            plugin.vendor,
            plugin.name,
            plugin.features.join(" ")
        );
    }
    for failure in &scan.failures {
        println!("{}: {}", failure.path.display(), failure.message);
    }
    if let Some(error) = &scan.cache_error {
        println!("error: {error}");
    }
    println!(
        "{} bundles, {} plugins, {} failed, in {took:?}",
        scan.bundles,
        scan.plugins.len(),
        scan.failures.len()
    );
    if scan
        .plugins
        .iter()
        .any(|plugin| plugin.format == PluginFormat::Vst3)
    {
        println!("{}", plugin_host::VST_TRADEMARK);
    }
    Ok(())
}

/// Prints every parameter of one plugin. See [`runtime::plugin_parameters`].
fn list_plugin_parameters(format: &str, plugin_id: &str) -> Result<()> {
    let Some(format) = PluginFormat::of_str(format) else {
        bail!("{format:?} is not a plugin format: clap or vst3");
    };
    let plugins = runtime::plugins(true)?;
    println!(
        "{}",
        runtime::plugin_parameters(&plugins, format, plugin_id)?
    );
    Ok(())
}

/// Prints one line of JSON per plugin in the bundle. See [`plugin_host::scan_one_bundle`].
fn scan_one_bundle(format: &str, bundle: &Path) -> Result<()> {
    let Some(format) = plugin_host::PluginFormat::of_str(format) else {
        bail!("{format:?} is not a plugin format this build hosts");
    };
    match plugin_host::scan_one_bundle(format, bundle) {
        Ok(lines) => {
            print!("{lines}");
            Ok(())
        }
        Err(message) => bail!("{message}"),
    }
}

const USAGE: &str = "usage: sound-tools [<project-folder> [--headless | --inspect | --render <wav> [<span>] [--progress] | --analyze [<span>]]]\n       sound-tools <audio-file> --analyze [--seconds <n> | --from <time> --to <time>]\n       sound-tools --plugins | --plugin-params <format> <plugin_id> | --version | --help\n<span>: [--seconds <n> | --from <at> --to <at>] [--solo <track>]...\n<at>: ticks (3840), a time (1:23.5) or seconds (83.5s)";

/// Gives a window program the terminal that started it, so `--version` and the other forms
/// print there. Only when it was handed no output: what a parent pipes, such as an agent's
/// shell or the window's own export, stays in the pipe.
#[cfg(windows)]
fn print_to_the_terminal() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: both take plain values and touch no memory of this program.
    unsafe {
        let output = GetStdHandle(STD_OUTPUT_HANDLE);
        if output.is_null() || output == INVALID_HANDLE_VALUE {
            // Fails when no terminal started this, as for a double click: nobody reads it then.
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn main() -> Result<()> {
    #[cfg(windows)]
    print_to_the_terminal();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (arguments, progress) = match arguments.as_slice() {
        [arguments @ .., "--progress"] => (arguments, true),
        arguments => (arguments, false),
    };
    runtime::use_library();
    match arguments {
        // What a double click in the Finder starts.
        [] => {
            if !runtime::update::start_pending_update() {
                runtime::window::run_app();
            }
            Ok(())
        }
        ["--plugins"] => list_plugins(),
        ["--plugin-params", format, plugin_id] => list_plugin_parameters(format, plugin_id),
        ["--version"] => {
            println!("sound-tools {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["--help" | "-h"] => {
            println!("{USAGE}");
            Ok(())
        }
        // An unknown flag is not a project folder. Opening it would show a window named after it.
        [flag] if flag.starts_with('-') => bail!(USAGE),
        [folder] if runtime::update::start_pending_update() => Ok(()),
        [folder] => runtime::window::run(Path::new(folder)),
        [folder, "--headless"] => run(Path::new(folder)),
        [folder, "--inspect"] => inspect(Path::new(folder)),
        // The child of a plugin scan. It loads one bundle, which is why it is a process of
        // its own: a plugin that crashes while it is looked at costs this child and no more.
        [plugin_host::SCAN_ARGUMENT, format, bundle] => scan_one_bundle(format, Path::new(bundle)),
        [folder, "--render", wav, options @ ..] => {
            let options = Options::parse(options)?;
            render(Path::new(folder), Path::new(wav), &options, progress)
        }
        [path, "--analyze", options @ ..] => analyze(Path::new(path), &Options::parse(options)?),
        _ => bail!(USAGE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_is_ticks_a_time_or_seconds() {
        let seconds = |text| match Position::parse(text) {
            Ok(Position::Seconds(seconds)) => Some(seconds),
            _ => None,
        };
        assert!(matches!(
            Position::parse("3840"),
            Ok(Position::Ticks(Ticks(3840)))
        ));
        assert_eq!(seconds("1:23.5"), Some(83.5));
        assert_eq!(seconds("0:05"), Some(5.0));
        assert_eq!(seconds("83.5s"), Some(83.5));
        for wrong in ["1:75", "1:-5", "-3s", "1:2:3", "bar 5", "", "inf s"] {
            assert!(Position::parse(wrong).is_err(), "{wrong}");
        }
    }
}
