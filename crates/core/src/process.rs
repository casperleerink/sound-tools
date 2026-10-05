//! The system programs the app starts in the background, such as curl, tar and the plugin
//! scanner.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

/// A command for `program` that opens no console window. The app has no console on Windows,
/// so a console program it starts would otherwise show an empty black window next to it.
/// The programs that one starts share its hidden console, so they open none either.
pub fn background_command(program: impl AsRef<OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        &mut command,
        windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
    );
    command
}

/// The system's curl, never one on the `PATH`. It comes with macOS, with every Linux desktop
/// and with Windows since 10 version 1803, so the app needs no HTTP or TLS code of its own.
pub fn curl() -> Command {
    #[cfg(not(windows))]
    let program = PathBuf::from("/usr/bin/curl");
    #[cfg(windows)]
    let program = windows_program(r"System32\curl.exe");
    background_command(program)
}

/// A program in the Windows folder, such as `System32\tar.exe`, so that one of the same name
/// on the `PATH` never runs instead.
#[cfg(windows)]
pub fn windows_program(path: &str) -> PathBuf {
    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(windows).join(path)
}
