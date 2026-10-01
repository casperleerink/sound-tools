//! The environment of the composer's login shell.
//!
//! An app opened from the Finder gets a bare `PATH`, without what the shell's profile adds,
//! such as `~/.cargo/bin`. The agent needs `cargo` and `git`, so it runs with the environment an
//! interactive login shell has. Zed and VS Code do the same.

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use smol::future;

/// Separates what the shell's rc files print from the environment.
const MARKER: &str = "__SOUND_TOOLS_ENVIRONMENT__";

/// How long the shell may take. An rc file can start an agent such as `ssh-agent` that keeps
/// the output open, and then the shell never seems to end.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The environment of `$SHELL -ilc`, over this process's own. Takes a moment, so run it once,
/// in the background, and keep the result.
///
/// When the shell fails, takes over 10 s or gives no `PATH`, use this process's environment
/// (`std::env::vars_os()`) and show the error: a broken shell config must not stop the agent.
pub async fn login_shell_environment() -> io::Result<HashMap<OsString, OsString>> {
    let shell = env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let output = smol::process::Command::new(shell)
        // -i and -l: both the profile and the rc files run.
        .arg("-ilc")
        // The rc files may print anything, so a marker comes first. `env -0` ends each entry
        // with a NUL, which keeps values with newlines whole.
        .arg(format!("command printf '\\0{MARKER}\\0'; command env -0"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        // Dropped when the time is up, which ends the shell.
        .kill_on_drop(true)
        .output();
    // Runs once at start, never in a gpui test, where this timer would not be deterministic.
    #[allow(clippy::disallowed_methods)]
    let timeout = async {
        smol::Timer::after(TIMEOUT).await;
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the login shell took over 10 s",
        ))
    };
    let output = future::or(output, timeout).await?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "the login shell failed ({})",
            output.status
        )));
    }
    let captured = parse(&output.stdout);
    if !captured.contains_key(&OsString::from("PATH")) {
        return Err(io::Error::other("the login shell has no PATH"));
    }
    let mut environment: HashMap<OsString, OsString> = env::vars_os().collect();
    environment.extend(captured);
    Ok(environment)
}

/// Where `name` is on the `PATH` of `environment`, as a shell would find it.
pub fn program_on_path(name: &str, environment: &HashMap<OsString, OsString>) -> Option<PathBuf> {
    let path = environment.get(&OsString::from("PATH"))?;
    env::split_paths(path)
        .map(|folder| folder.join(name))
        .find(|program| {
            program.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
}

/// The entries after the last marker.
fn parse(output: &[u8]) -> HashMap<OsString, OsString> {
    let entries: Vec<&[u8]> = output.split(|byte| *byte == 0).collect();
    let start = entries
        .iter()
        .rposition(|entry| *entry == MARKER.as_bytes())
        .map_or(0, |marker| marker + 1);
    entries
        .into_iter()
        .skip(start)
        .filter_map(|entry| {
            let equals = entry.iter().position(|byte| *byte == b'=')?;
            let (key, value) = entry.split_at(equals);
            let value = value.get(1..)?;
            (!key.is_empty()).then(|| {
                (
                    OsString::from_vec(key.to_vec()),
                    OsString::from_vec(value.to_vec()),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_entries_after_the_marker() {
        let output = format!("rc noise\0{MARKER}\0PATH=/usr/bin:/bin\0NOTE=two\nlines\0EMPTY=\0");
        let environment = parse(output.as_bytes());
        assert_eq!(environment.len(), 3);
        assert_eq!(environment[&OsString::from("PATH")], "/usr/bin:/bin");
        assert_eq!(environment[&OsString::from("NOTE")], "two\nlines");
        assert_eq!(environment[&OsString::from("EMPTY")], "");
    }
}
