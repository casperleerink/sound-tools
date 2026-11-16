//! The project runtime: the app, and the command line forms of `runtime::cli`. Only the forms
//! with a folder and no flag, or nothing at all, open a window. In `Sound Tools.app` and as the
//! command line tool this program is called `sound-tools`.
//!
//! On Windows it is a window program, so the app opens no console window next to its own.
//! Started from a terminal, it prints there anyway: see `print_to_the_terminal`. The window
//! forms started any other way print into the log: see `runtime::log_file`.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::Path;

use anyhow::{Result, bail};
use runtime::cli;

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

/// What a window form does before its window: the log first, so a failed update is in it.
/// Whether an update started instead, which ends this process.
fn start_window_process() -> bool {
    runtime::log_file::start();
    runtime::update::start_pending_update()
}

fn main() -> Result<()> {
    #[cfg(windows)]
    print_to_the_terminal();
    if let Some(served) = cli::plugin_process() {
        return served;
    }
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (arguments, progress) = match arguments.as_slice() {
        [arguments @ .., "--progress"] => (arguments, true),
        arguments => (arguments, false),
    };
    if let Some(ran) = cli::run(arguments, progress) {
        return ran;
    }
    match arguments {
        // What a double click on the app starts.
        [] => {
            if !start_window_process() {
                runtime::window::run_app();
            }
            Ok(())
        }
        [_] if start_window_process() => Ok(()),
        [folder] => runtime::window::run(Path::new(folder)),
        #[cfg(target_os = "macos")]
        [folder, "--screenshot", png, flags @ ..] => {
            let before = runtime::screenshot::Before::parse(flags)?;
            runtime::screenshot::screenshot(Path::new(folder), Path::new(png), &before)
        }
        #[cfg(not(target_os = "macos"))]
        [_, "--screenshot", ..] => bail!("--screenshot works on macOS only"),
        _ => bail!(cli::USAGE),
    }
}
