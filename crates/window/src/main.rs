//! `sound-tools-next`: the window on Rune, beside the GPUI app until it takes its place. Every
//! form without a window is the runtime's (`runtime::cli`), so agents may call either program.
//!
//! `sound-tools-next <project-folder>` opens the window; a missing or empty folder becomes a new
//! project of the DAW. With Rune's `--snap`, `--tree` or `--script` it runs without a window,
//! on a copy of the folder, and adds `--play <seconds>` (plays from the start first) and
//! `--act <steps.json>` (runs actions on controls by label or key first, in the folder itself).

mod app;
mod card;
mod controls;
mod lives;
mod theme;
mod tree;

use std::path::PathBuf;
use std::sync::PoisonError;

use anyhow::{Context as _, Result, bail};
use runtime::cli;

/// The second words of the forms without a window.
const COMMANDS: [&str; 6] = [
    "--headless",
    "--inspect",
    "--render",
    "--analyze",
    "--save-template",
    "--screenshot",
];

/// Rune's flags that run without a window.
const HEADLESS: [&str; 7] = [
    "--snap", "--tree", "--script", "--video", "--out", "--strip", "--html",
];

const FONTS: [&[u8]; 3] = [
    include_bytes!("../../ui/assets/fonts/InterDisplay-Regular.ttf"),
    include_bytes!("../../ui/assets/fonts/InterDisplay-Medium.ttf"),
    include_bytes!("../../ui/assets/fonts/InterDisplay-SemiBold.ttf"),
];

fn main() -> Result<()> {
    if let Some(served) = cli::plugin_process() {
        return served;
    }
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => bail!(
            "sound-tools-next opens a project: give its folder\n{}",
            cli::USAGE
        ),
        [first, ..] if first.starts_with('-') => {
            return cli::run(&words, false).unwrap_or_else(|| bail!(cli::USAGE));
        }
        [_, second, ..] if COMMANDS.contains(second) => {
            let (words, progress) = match words.as_slice() {
                [words @ .., "--progress"] => (words, true),
                words => (words, false),
            };
            if *second == "--screenshot" {
                bail!(
                    "sound-tools-next draws with --snap <png> [--play <seconds>] [--act <steps.json>] [--tree <json>]"
                );
            }
            return cli::run(words, progress).unwrap_or_else(|| bail!(cli::USAGE));
        }
        _ => {}
    }
    window(&arguments)
}

/// The window, or a run of it without one.
fn window(arguments: &[String]) -> Result<()> {
    let mut rest = arguments.iter();
    let folder = PathBuf::from(rest.next().context("give a project folder")?);
    let (mut play, mut act, mut rune_arguments) = (None, None, vec![]);
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--play" => {
                let seconds = rest
                    .next()
                    .and_then(|seconds| seconds.parse::<f64>().ok())
                    .filter(|seconds| *seconds > 0.0);
                play = Some(seconds.context("--play takes seconds, such as 2")?);
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
    if !headless && (play.is_some() || act.is_some()) {
        bail!("--play and --act come with --snap, --tree or --script");
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
        folder,
        headless,
        play,
        act: act.map(|path| controls::read_steps(&path)).transpose()?,
    })?;
    let spec = rune::AppSpec {
        poll: Some(app::poll),
        setup: Some(app::setup),
        on_key: Some(app::on_key),
        fonts: &FONTS,
        families: ("Inter Display", "ui-monospace"),
        ..rune::AppSpec::new(
            "Sound Tools",
            (1470.0, 920.0),
            || unreachable!("the state is made before"),
            app::view,
        )
    };
    rune::App::new(spec)
        .chrome(rune::Chrome {
            traffic_lights: Some(app::WINDOW_BUTTONS),
            ..rune::Chrome::DEFAULT
        })
        .args(rune_arguments)
        .init(move || state)
        .run();
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
