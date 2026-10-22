//! Renders the project to a WAV in a process of its own: this program with `--render`, which
//! reads the project from its folder as an agent's render does. The window keeps playing
//! meanwhile, the session shows how far it is, and a plugin that crashes in the render costs
//! the render only.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use gpui::{App, AppContext as _, Entity, Global, SharedString, Task};
use smol::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use smol::stream::StreamExt;

use crate::Session;

/// The program a render runs: this one, unless a test, which is no such program, sets it.
pub struct RenderProgram(pub PathBuf);

impl Global for RenderProgram {}

/// Renders the project of `session` to `wav`, with `arguments` after `--render <wav>`, such as
/// `--from` and `--to`, while the session shows `notice` and how far it is. One at a time: it
/// fails while another runs. Gives what the render reported missing from the file, such as a
/// plugin that did not load.
pub fn render(
    session: &Entity<Session>,
    notice: SharedString,
    wav: PathBuf,
    arguments: Vec<String>,
    cx: &mut App,
) -> Task<anyhow::Result<Vec<String>>> {
    if session.read(cx).rendering().is_some() {
        let busy = anyhow::anyhow!("Another render is running: try again once it is done.");
        return Task::ready(Err(busy));
    }
    let program = match cx.try_global::<RenderProgram>() {
        Some(program) => program.0.clone(),
        None => match std::env::current_exe() {
            Ok(program) => program,
            Err(error) => return Task::ready(Err(error.into())),
        },
    };
    let folder = session.read(cx).project().root().to_path_buf();
    session.update(cx, |session, cx| {
        session.set_rendering(Some((notice.clone(), 0)), cx)
    });
    let session = session.clone();
    cx.spawn(async move |cx| {
        let (sender, percents) = smol::channel::unbounded();
        let rendered = cx.background_spawn(async move {
            render_in_child(&program, &folder, &wav, &arguments, |percent| {
                // Fails only once the window stopped listening.
                sender.try_send(percent).ok();
            })
            .await
        });
        // Ends when the render does, which drops the sender.
        while let Ok(percent) = percents.recv().await {
            session.update(cx, |session, cx| {
                session.set_rendering(Some((notice.clone(), percent)), cx)
            });
        }
        let rendered = rendered.await;
        session.update(cx, |session, cx| session.set_rendering(None, cx));
        rendered
    })
}

/// Runs `--render` and waits for it, telling `progress` each percent it reaches. Gives the
/// errors it printed on the way, such as a plugin that did not load. When it failed, what it
/// printed last on stderr is the error.
async fn render_in_child(
    program: &Path,
    folder: &Path,
    wav: &Path,
    arguments: &[String],
    mut progress: impl FnMut(u8),
) -> anyhow::Result<Vec<String>> {
    let mut child = smol::process::Command::new(program)
        .arg(folder)
        .arg("--render")
        .arg(wav)
        .args(arguments)
        .arg("--progress")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Quitting the app stops the render.
        .kill_on_drop(true)
        .spawn()?;
    let (Some(stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        anyhow::bail!("the render has no output");
    };
    // Both at once: a pipe that nobody reads fills up and stops the render.
    let read_stdout = async {
        let mut errors = Vec::new();
        // Lossy and never stopping on a line it does not know: a plugin may print to stdout,
        // and a pipe that stops being read stops the render.
        let mut lines = BufReader::new(stdout).split(b'\n');
        while let Some(line) = lines.next().await {
            let line = String::from_utf8_lossy(&line?).into_owned();
            let percent = line.strip_prefix("progress: ").map(str::parse);
            if let Some(Ok(percent)) = percent {
                progress(percent);
            } else if let Some(error) = line.strip_prefix("error: ") {
                errors.push(error.to_string());
            }
        }
        anyhow::Ok(errors)
    };
    let read_stderr = async {
        let mut said = String::new();
        stderr.read_to_string(&mut said).await?;
        anyhow::Ok(said)
    };
    let (errors, stderr) = smol::future::zip(read_stdout, read_stderr).await;
    if child.status().await?.success() {
        return errors;
    }
    let stderr = stderr?;
    let last = stderr.lines().rfind(|line| !line.trim().is_empty());
    anyhow::bail!(
        "{}",
        last.unwrap_or("the render stopped")
            .trim_start_matches("Error: ")
    )
}
