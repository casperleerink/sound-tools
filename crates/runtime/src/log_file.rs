//! The log of the app in a window: everything it prints, in `log.txt` in the support folder,
//! for a composer to send when something went wrong. There is no error tracking in the cloud.
//!
//! The process's own stdout and stderr point at the file, so every `error:` line, every notice
//! (see `Session::report`), a panic and what a plugin or a tool of the project prints land
//! there with no call of their own. A write goes straight to the file, so a crash loses
//! nothing. A dev build, and a release started from a terminal, keep their output where it
//! was: a person or an agent reads it there.

use std::fs::{self, File};
use std::io::{self, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

const FILE: &str = "log.txt";
/// The log before the last time it got too long, replaced each time.
const OLD_FILE: &str = "log.old.txt";

/// A log longer than this at a start goes to [`OLD_FILE`], so several sessions share one file
/// and two files bound the whole.
const START_NEW_ABOVE: u64 = 1 << 20;
/// A session that prints this much is cleared and goes on, the last of it being what tells
/// most. Only a plugin or a tool that prints all the time gets there.
const CLEAR_ABOVE: u64 = 20 << 20;
const CHECK_SIZE_EVERY: Duration = Duration::from_secs(10);

/// The file, and the open handle that stdout and stderr write to, for the life of the process.
static LOG: OnceLock<(PathBuf, File)> = OnceLock::new();

/// The log this process writes, for the project menu to show. `None` for a dev build or one
/// started from a terminal.
pub fn path() -> Option<&'static Path> {
    LOG.get().map(|(path, _)| path.as_path())
}

/// Points stdout and stderr at the log, for the window forms only: a command line form prints
/// for its caller, and a test never comes here.
pub fn start() {
    if cfg!(debug_assertions) || io::stderr().is_terminal() {
        return;
    }
    let started = crate::app::support_folder().and_then(|support| {
        let (path, file) = open(&support)?;
        redirect_output(&file)?;
        Ok((path, file))
    });
    let (path, file) = match started {
        Ok(log) => LOG.get_or_init(|| log),
        Err(error) => {
            // Nobody reads stderr here, but there is nowhere else to say it.
            eprintln!("error: the log did not start: {error:#}");
            return;
        }
    };
    // A panic says where it happened; the backtrace says how it got there. Nobody sets
    // `RUST_BACKTRACE` for an app opened from the Finder.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let backtrace = std::backtrace::Backtrace::force_capture();
        // A write that fails here has nowhere left to go.
        writeln!(io::stderr(), "{backtrace}").ok();
    }));
    let checked = std::thread::Builder::new()
        .name("log size".into())
        .stack_size(64 << 10)
        .spawn(move || {
            loop {
                std::thread::sleep(CHECK_SIZE_EVERY);
                if let Err(error) = clear_when_full(path, file) {
                    eprintln!("error: the log could not be cleared: {error}");
                }
            }
        });
    if let Err(error) = checked {
        eprintln!("error: the log size is not checked: {error}");
    }
}

/// Opens the log in `support` to add to it, after moving a long one aside, and writes the
/// first line of this session.
fn open(support: &Path) -> io::Result<(PathBuf, File)> {
    fs::create_dir_all(support)?;
    let path = support.join(FILE);
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > START_NEW_ABOVE) {
        fs::rename(&path, support.join(OLD_FILE))?;
    }
    let mut file = File::options().create(true).append(true).open(&path)?;
    writeln!(file, "{}", heading("started"))?;
    Ok((path, file))
}

/// Empties a log that passed [`CLEAR_ABOVE`] and says so in it. `file` adds to its end, so it
/// goes on at the start.
fn clear_when_full(path: &Path, mut file: &File) -> io::Result<()> {
    if file.metadata()?.len() > CLEAR_ABOVE {
        // A handle of its own: one that only adds to a file may not shorten it on Windows.
        File::options().write(true).open(path)?.set_len(0)?;
        let cleared = format!("cleared, as it passed {} MB", CLEAR_ABOVE >> 20);
        writeln!(file, "{}", heading(&cleared))?;
    }
    Ok(())
}

/// The line a session starts with: the version and system, for the reader of a log sent in.
fn heading(what: &str) -> String {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z");
    format!(
        "--- {now}: Sound Tools {} on {} {}, {what} ---",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

#[cfg(unix)]
fn redirect_output(to: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;
    for output in [libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        // SAFETY: both are descriptors of this process. `dup2` only points `output` at the
        // file; Rust's stdout and stderr hold no state about what they point at.
        if unsafe { libc::dup2(to.as_raw_fd(), output) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Rust's stdout and stderr ask Windows for the handle at every write, and a child that
/// inherits its output gets the file too.
#[cfg(windows)]
fn redirect_output(to: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
    for output in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: the handle stays open for the life of the process: `start` keeps the file in
        // `LOG`, and nothing closes it.
        if unsafe { SetStdHandle(output, to.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_log_moves_aside_at_a_start_and_a_short_one_goes_on() {
        let support = tempfile::tempdir().unwrap();
        let log = support.path().join(FILE);
        fs::write(&log, "error: the last session\n").unwrap();
        open(support.path()).unwrap();
        let text = fs::read_to_string(&log).unwrap();
        assert!(text.starts_with("error: the last session\n"), "{text}");
        assert!(text.contains("started ---"), "{text}");
        assert!(!support.path().join(OLD_FILE).exists());

        File::options()
            .write(true)
            .open(&log)
            .unwrap()
            .set_len(START_NEW_ABOVE + 1)
            .unwrap();
        open(support.path()).unwrap();
        let old = fs::metadata(support.path().join(OLD_FILE)).unwrap();
        assert_eq!(old.len(), START_NEW_ABOVE + 1);
        let text = fs::read_to_string(&log).unwrap();
        assert!(text.trim_start().starts_with("--- "), "{text}");
    }

    #[test]
    fn a_full_log_is_cleared_and_goes_on_at_its_start() {
        let support = tempfile::tempdir().unwrap();
        let (path, file) = open(support.path()).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(CLEAR_ABOVE + 1)
            .unwrap();
        clear_when_full(&path, &file).unwrap();
        writeln!(&file, "error: after").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].contains("cleared"), "{text}");
        assert_eq!(lines[1], "error: after");
    }
}
