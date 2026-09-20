//! The project runtime. It opens a project folder, keeps it live and plays it on the default
//! output device, in a window or headless.
//!
//! ```text
//! runtime <project-folder>                                 run live in the window
//! runtime <project-folder> --headless                      run live, commands from stdin
//! runtime <project-folder> --inspect                       print a summary, open no device
//! runtime <project-folder> --render <wav> --seconds <n>    render offline
//! ```
//!
//! Only the first form starts GPUI. Tests, CI and agents use the others.
//!
//! Headless, it reads one command per line from stdin: `play`, `pause`, `stop`,
//! `seek <ticks>`, `undo`, `redo`, `status`, `quit`. The end of stdin also quits. This is
//! provisional. It is not the protocol of the outer application.

use std::io::BufRead;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use runtime::{OFFLINE, open_or_create, open_read_only, problems, summary};
use sound_core::{Engine, EngineConfig, EngineStatus, OutputDevice, Project, ProjectEvent, Ticks};

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
    let time_signature = project.engine().clock().tempo_map().time_signature();
    println!(
        "status: {}, playhead {} (tick {}), {} edits applied, {} events dropped, undo: {}, redo: {}",
        if status.playing { "playing" } else { "stopped" },
        time_signature.bar_beat_of(status.playhead_tick),
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
    let mut project = open_or_create(folder, control)?;
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

/// Reads the project without its lock, so it works next to a running runtime.
fn inspect(folder: &Path) -> Result<()> {
    let (project, _engine) = open_read_only(folder)?;
    print_summary(&project);
    Ok(())
}

fn render(folder: &Path, wav: &Path, seconds: f64) -> Result<()> {
    let (mut project, mut engine) = open_read_only(folder)?;
    print_problems(&project);
    project.engine().play();
    let mut writer = hound::WavWriter::create(
        wav,
        hound::WavSpec {
            channels: OFFLINE.channels as u16,
            sample_rate: OFFLINE.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    let mut buffer = [0.0_f32; 512 * OFFLINE.channels];
    let mut frames_left = (seconds * f64::from(OFFLINE.sample_rate)) as usize;
    let mut peak = 0.0_f32;
    while frames_left > 0 {
        let frames = frames_left.min(buffer.len() / OFFLINE.channels);
        let output = &mut buffer[..frames * OFFLINE.channels];
        engine.process_block(output);
        for sample in output.iter() {
            peak = peak.max(sample.abs());
            writer.write_sample(*sample)?;
        }
        frames_left -= frames;
        project.engine().poll()?;
    }
    writer.finalize()?;
    println!("rendered {seconds} s to {}, peak {peak:.4}", wav.display());
    // Above zero, notes were lost: more events in one block than a port holds, or more held
    // notes than a track keeps.
    let status = project.engine().poll()?;
    println!("event overflows: {}", status.event_overflows);
    Ok(())
}

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match arguments.as_slice() {
        [folder] => runtime::window::run(Path::new(folder)),
        [folder, "--headless"] => run(Path::new(folder)),
        [folder, "--inspect"] => inspect(Path::new(folder)),
        [folder, "--render", wav, "--seconds", seconds] => {
            let seconds = seconds.parse().context("--seconds takes a number")?;
            render(Path::new(folder), Path::new(wav), seconds)
        }
        _ => bail!(
            "usage: runtime <project-folder> [--headless | --inspect | --render <wav> --seconds <n>]"
        ),
    }
}
