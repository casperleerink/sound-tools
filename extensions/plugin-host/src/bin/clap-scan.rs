//! Scans one CLAP bundle and prints one line of JSON per plugin in it.
//!
//! Loading a plugin runs its code, so the scan never happens in the application's own process.
//! The runtime does this with its own executable and `--scan-clap`; this program is the same
//! thing for the tests of this crate, which have no runtime.

use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let Some(bundle) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: clap-scan <plugin.clap>");
        return std::process::ExitCode::FAILURE;
    };
    match plugin_host::scan_one_bundle(&bundle) {
        Ok(lines) => {
            print!("{lines}");
            std::process::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::FAILURE
        }
    }
}
