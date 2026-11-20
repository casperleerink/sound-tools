//! `sound-tools`: the app. Every form without a window is the runtime's (`runtime::cli`).
//!
//! `sound-tools <project-folder>` opens the window; a missing or empty folder becomes a new
//! project of the DAW. With Rune's `--snap`, `--tree` or `--script` it runs without a window,
//! on a copy of the folder, and adds `--play <seconds>` (plays from the start first),
//! `--act <steps.json>` (runs actions on controls by label or key first, in the folder itself)
//! and `--main <instance>` (shows that instance in place of the project's main one).
//!
//! On Windows it is a window program, so the app opens no console window next to its own.
//! Started from a terminal, it prints there anyway: see `print_to_the_terminal`.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod controls;
mod fields;
mod lives;
mod shell;
mod theme;
mod tree;

use std::path::PathBuf;
use std::sync::PoisonError;

use anyhow::{Context as _, Result, bail};
use runtime::cli;

/// The second words of the forms without a window.
const COMMANDS: [&str; 5] = [
    "--headless",
    "--inspect",
    "--render",
    "--analyze",
    "--save-template",
];

/// Rune's flags that run without a window.
const HEADLESS: [&str; 7] = [
    "--snap", "--tree", "--script", "--video", "--out", "--strip", "--html",
];

const FONTS: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/InterDisplay-Regular.ttf"),
    include_bytes!("../assets/fonts/InterDisplay-Medium.ttf"),
    include_bytes!("../assets/fonts/InterDisplay-SemiBold.ttf"),
    include_bytes!("../assets/fonts/InterDisplay-Italic.ttf"),
];

/// Gives a window program the terminal that started it, so `--version` and the other forms
/// print there. Only when it was handed no output: what a parent pipes, such as an agent's
/// shell, stays in the pipe.
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
    if let Some(served) = cli::plugin_process() {
        return served;
    }
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => return no_folder(),
        [first, ..] if HEADLESS.contains(first) => {
            return shell::start::run(arguments.clone(), None);
        }
        [first, ..] if first.starts_with('-') => {
            return cli::run(&words).unwrap_or_else(|| bail!(cli::USAGE));
        }
        [_, second, ..] if COMMANDS.contains(second) => {
            return cli::run(&words).unwrap_or_else(|| bail!(cli::USAGE));
        }
        _ => {}
    }
    window(&arguments, false)
}

/// No folder, as a double click starts the app: the last project, else the start state.
fn no_folder() -> Result<()> {
    match runtime::app::last_project() {
        Some(last) => window(&[last.to_string_lossy().into_owned()], true),
        None if installs_an_update() => Ok(()),
        None => shell::start::run(Vec::new(), None),
    }
}

/// Starts the log, then a downloaded update, which starts the app again: whether it did.
fn installs_an_update() -> bool {
    runtime::log_file::start();
    runtime::update::start_pending_update()
}

/// The window, or a run of it without one. `last`: the folder is the last project, and the
/// start state says why when it does not open.
fn window(arguments: &[String], last: bool) -> Result<()> {
    let mut rest = arguments.iter();
    let folder = PathBuf::from(rest.next().context("give a project folder")?);
    let (mut play, mut act, mut main, mut rune_arguments) = (None, None, None, vec![]);
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--play" => {
                let seconds = rest
                    .next()
                    .and_then(|seconds| seconds.parse::<f64>().ok())
                    .filter(|seconds| *seconds > 0.0);
                play = Some(seconds.context("--play takes seconds, such as 2")?);
            }
            "--main" => {
                main = Some(
                    rest.next()
                        .context("--main takes an instance, such as gallery")?
                        .clone(),
                )
            }
            "--act" => {
                act = Some(PathBuf::from(
                    rest.next().context("--act takes a file of steps")?,
                ))
            }
            _ => rune_arguments.push(argument.clone()),
        }
    }
    let headless = rune_arguments
        .iter()
        .any(|argument| HEADLESS.contains(&argument.as_str()));
    if !headless && (play.is_some() || act.is_some() || main.is_some()) {
        bail!("--play, --act and --main come with --snap, --tree or --script");
    }
    // A flag Rune does not know is no reason to open a window.
    if !headless && !rune_arguments.is_empty() {
        bail!(cli::USAGE);
    }
    // The log first, so a failed update is in it.
    if !headless && installs_an_update() {
        return Ok(());
    }
    if let (Some(seconds), false) = (
        play,
        rune_arguments.iter().any(|argument| argument == "--at"),
    ) {
        rune_arguments.extend(["--at".to_string(), seconds.to_string()]);
    }
    let tree = rune_arguments
        .iter()
        .position(|argument| argument == "--tree")
        .and_then(|at| rune_arguments.get(at + 1))
        .map(PathBuf::from);
    runtime::use_library();
    let state = app::State::open(app::Options {
        folder: folder.clone(),
        headless,
        play,
        main: main
            .map(|main| sound_core::InstanceId::new(&main))
            .transpose()?,
        act: act.map(|path| controls::read_steps(&path)).transpose()?,
    });
    let state = match state {
        Err(error) if last => {
            let message = shell::start::did_not_open(&folder, &error);
            return shell::start::run(Vec::new(), Some(message));
        }
        state => state?,
    };
    // The window is named after its project; Rune keeps the title for as long as the app runs.
    let title: &'static str = Box::leak(state.name.clone().into_boxed_str());
    let spec = rune::AppSpec {
        min_size: Some(app::MIN_SIZE),
        poll: Some(app::poll),
        setup: Some(app::setup),
        on_key: Some(app::on_key),
        fonts: &FONTS,
        families: ("Inter Display", "ui-monospace"),
        ..rune::AppSpec::new(
            title,
            (1470.0, 920.0),
            || unreachable!("the state is made before"),
            app::view,
        )
    };
    rune::App::new(spec)
        .chrome(rune::Chrome {
            traffic_lights: Some(app::WINDOW_BUTTONS),
            drag_regions: true,
            ..rune::Chrome::DEFAULT
        })
        // Every control is a stop of Tab, whatever the system's keyboard setting.
        .tab_visits_controls(true)
        .args(rune_arguments)
        .init(move || state)
        .run();
    if shell::starts_again() {
        runtime::app::start_again()?;
    }
    if let Some(failed) = app::FAILED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        bail!("{failed}");
    }
    if let Some(tree) = tree {
        add_report(&tree)?;
    }
    Ok(())
}

/// Adds what the window knows to the tree Rune wrote: every control with its label, role,
/// value or state and actions, where the piece plays, and the meters.
fn add_report(path: &std::path::Path) -> Result<()> {
    let report = app::REPORT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    let (Some(serde_json::Value::Object(report)), Ok(text)) =
        (report, std::fs::read_to_string(path))
    else {
        return Ok(());
    };
    let mut tree: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("{} is no tree", path.display()))?;
    if let Some(tree) = tree.as_object_mut() {
        tree.extend(report);
    }
    std::fs::write(path, serde_json::to_string_pretty(&tree)?)?;
    Ok(())
}
