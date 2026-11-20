//! Renders the project to a file in a process of its own: this program with `--render`, which
//! reads the project from its folder as an agent's render does. The window keeps playing, the
//! session shows how far it is, and a plugin that crashes in the render costs the render only.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;

use anyhow::{Result, anyhow};

use super::{Session, Waker};

/// What a render said on the way.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rendered {
    /// What is missing from the file, such as a plugin that did not load.
    pub problems: Vec<String>,
    /// What the composer should know about the file, such as a loudness it could not reach.
    pub notes: Vec<String>,
}

/// The render that runs: its notice, how far it is, and the process, which a session that goes
/// stops.
pub(super) struct Running {
    notice: String,
    percent: u8,
    said: mpsc::Receiver<Said>,
    child: Child,
}

enum Said {
    Percent(u8),
    /// The output ended: what stdout said, and the last line of stderr.
    Ended(std::io::Result<Rendered>, String),
}

impl Drop for Running {
    fn drop(&mut self) {
        // It may have ended already.
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Session {
    /// Renders the project to `file` with `program --render`, `arguments` after the file, such
    /// as `--from` and `--to`, while the session shows `notice` and how far it is. One at a
    /// time. A poll gives what it said once it ends.
    pub fn render(
        &mut self,
        program: &Path,
        notice: impl Into<String>,
        file: PathBuf,
        arguments: &[String],
    ) -> Result<()> {
        if self.render.is_some() {
            return Err(anyhow!(
                "Another render is running: try again once it is done."
            ));
        }
        // Starting it is quick; threads read what it says.
        #[allow(clippy::disallowed_methods)]
        let mut child = sound_core::process::background_command(program)
            .arg(self.project.root())
            .arg("--render")
            .arg(file)
            .args(arguments)
            .arg("--progress")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(anyhow!("the render has no output"));
        };
        let (tell, said) = mpsc::channel();
        read_on_threads(stdout, stderr, tell, self.waker.clone());
        self.render = Some(Running {
            notice: notice.into(),
            percent: 0,
            said,
            child,
        });
        Ok(())
    }

    /// The notice of the render that runs and how far it is, from 0 to 1.
    pub fn rendering(&self) -> Option<(&str, f32)> {
        let running = self.render.as_ref()?;
        Some((&running.notice, f32::from(running.percent) / 100.))
    }

    pub(super) fn rendering_now(&self) -> Option<u8> {
        self.render.as_ref().map(|running| running.percent)
    }

    /// What the render said since the last poll; once it ended, what it rendered.
    pub(super) fn take_rendered(&mut self) -> Option<Result<Rendered>> {
        let running = self.render.as_mut()?;
        let (rendered, stderr) = loop {
            match running.said.try_recv() {
                Ok(Said::Percent(percent)) => running.percent = percent,
                Ok(Said::Ended(rendered, stderr)) => break (rendered, stderr),
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    break (Err(std::io::ErrorKind::BrokenPipe.into()), String::new());
                }
            }
        };
        // Its output ended, so it ends now.
        let status = running.child.wait();
        self.render = None;
        let last = stderr.lines().rfind(|line| !line.trim().is_empty());
        let failed = last.unwrap_or("the render stopped");
        Some(match status {
            Ok(status) if status.success() => rendered.map_err(Into::into),
            Ok(_) => Err(anyhow!("{}", failed.trim_start_matches("Error: "))),
            Err(error) => Err(error.into()),
        })
    }
}

/// Reads stdout and stderr at once, on threads: a pipe nobody reads fills up and stops the
/// render. Lossy and never stopping on a line it does not know, as a plugin may print.
fn read_on_threads(
    stdout: impl Read + Send + 'static,
    mut stderr: impl Read + Send + 'static,
    tell: mpsc::Sender<Said>,
    waker: Waker,
) {
    let errors = std::thread::spawn(move || {
        let mut said = Vec::new();
        stderr.read_to_end(&mut said).ok();
        String::from_utf8_lossy(&said).into_owned()
    });
    std::thread::spawn(move || {
        let read = || -> std::io::Result<Rendered> {
            let mut rendered = Rendered::default();
            for line in BufReader::new(stdout).split(b'\n') {
                let line = String::from_utf8_lossy(&line?).into_owned();
                let percent = line.strip_prefix("progress: ").map(str::parse);
                if let Some(Ok(percent)) = percent {
                    tell.send(Said::Percent(percent)).ok();
                    waker();
                } else if let Some(error) = line.strip_prefix("error: ") {
                    rendered.problems.push(error.to_string());
                } else if let Some(note) = line.strip_prefix("note: ") {
                    rendered.notes.push(note.to_string());
                }
            }
            Ok(rendered)
        };
        let rendered = read();
        let stderr = errors.join().unwrap_or_default();
        tell.send(Said::Ended(rendered, stderr)).ok();
        waker();
    });
}
