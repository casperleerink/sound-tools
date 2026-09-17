#[cfg(unix)]
use serde_json::Value;
#[cfg(unix)]
use std::{
    env,
    ffi::OsStr,
    io::{self, Read},
    os::{
        fd::OwnedFd,
        unix::{fs::PermissionsExt, net::UnixStream, process::CommandExt},
    },
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

#[cfg(unix)]
const MAX_LINE: usize = 256 * 1024;
#[cfg(unix)]
const MAX_OUTPUT: usize = 8 * 1024 * 1024;
#[cfg(unix)]
const MAX_TEXT: usize = 32 * 1024;
#[cfg(unix)]
pub const MAX_PROMPT: usize = 16 * 1024;
#[cfg(unix)]
const POLL: Duration = Duration::from_millis(20);
#[cfg(unix)]
const PREFIX: &str = "Work on the Sound Tools project in the current directory. Edit its plain JSON instance records, preserving their schema and unrelated values. Existing record edits are watched and applied live. Do not build, compile, install packages, or change application source. Time is measured at 48000 frames/sec. Tracks, clips and notes belong to the DAW extension; the core never knows about notes. Inspect existing records before editing. Do not read or reveal credentials. Summarize what you actually changed. User task:\n";

#[cfg(not(unix))]
pub fn binary() -> Result<PathBuf, &'static str> {
    Err("Pi is unsupported on this platform: descendant cancellation is unavailable.")
}

#[cfg(unix)]
pub fn binary() -> Result<PathBuf, &'static str> {
    resolve_binary(
        env::var_os("SOUND_TOOLS_AGENT_BIN").as_deref(),
        env::var_os("PATH").as_deref(),
    )
}

#[cfg(unix)]
fn resolve_binary(
    configured: Option<&OsStr>,
    path: Option<&OsStr>,
) -> Result<PathBuf, &'static str> {
    if let Some(path) = configured {
        let path = PathBuf::from(path);
        if !path.is_absolute() || !executable(&path) {
            return Err("SOUND_TOOLS_AGENT_BIN must be an absolute path to an executable Pi CLI.");
        }
        return Ok(path);
    }
    path.into_iter()
        .flat_map(env::split_paths)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join("pi"))
        .find(|path| executable(path))
        .ok_or("Pi not found. Install and configure Pi separately, or set SOUND_TOOLS_AGENT_BIN to its absolute executable path, then retry Send.")
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[derive(Clone)]
pub struct Update {
    pub text: String,
    pub status: String,
    pub finished: bool,
    pub truncated: bool,
}

impl Default for Update {
    fn default() -> Self {
        Self {
            text: String::new(),
            status: "Starting Pi...".into(),
            finished: false,
            truncated: false,
        }
    }
}

pub struct Run {
    cancel: Arc<AtomicBool>,
    update: Arc<Mutex<Update>>,
    worker: Option<JoinHandle<()>>,
}

impl Run {
    #[cfg(not(unix))]
    pub fn start(_: &Path, _: &str) -> Result<Self, &'static str> {
        Err("Pi is unsupported on this platform: descendant cancellation is unavailable.")
    }

    #[cfg(unix)]
    pub fn start(root: &Path, prompt: &str) -> Result<Self, &'static str> {
        if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT {
            return Err("Enter a task of at most 16 KiB.");
        }
        let mut command = Command::new(binary()?);
        command.current_dir(root).args([
            "--print",
            "--mode",
            "json",
            "--no-session",
            "--no-approve",
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-context-files",
            "--",
            &format!("{PREFIX}{prompt}"),
        ]);
        command.env("PI_OFFLINE", "1").env("PI_TELEMETRY", "0");
        Self::launch(command)
    }

    #[cfg(unix)]
    fn launch(command: Command) -> Result<Self, &'static str> {
        let cancel = Arc::new(AtomicBool::new(false));
        let update = Arc::new(Mutex::new(Update::default()));
        let worker_cancel = cancel.clone();
        let worker_update = update.clone();
        let worker = thread::Builder::new()
            .name("pi-agent".into())
            .spawn(move || {
                let result = supervise(command, &worker_cancel, &worker_update);
                let mut update = worker_update
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                update.status = result;
                update.finished = true;
            })
            .map_err(|_| "Cannot start the Pi worker.")?;
        Ok(Self {
            cancel,
            update,
            worker: Some(worker),
        })
    }

    pub fn snapshot(&self) -> Update {
        self.update
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(unix)]
struct OwnedChild(Child);

#[cfg(unix)]
impl OwnedChild {
    fn exited(&self) -> io::Result<Option<bool>> {
        loop {
            let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.0.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                return Ok((unsafe { info.si_pid() } != 0).then(|| {
                    info.si_code == libc::CLD_EXITED && unsafe { info.si_status() } == 0
                }));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

#[cfg(unix)]
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Ok(pid) = libc::pid_t::try_from(self.0.id())
            && pid > 1
        {
            if matches!(self.0.try_wait(), Ok(None)) {
                unsafe { libc::kill(-pid, libc::SIGTERM) };
                thread::sleep(Duration::from_millis(150));
            }
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(unix)]
fn output_pipe() -> io::Result<(UnixStream, Stdio)> {
    let (reader, writer) = UnixStream::pair()?;
    reader.set_nonblocking(true)?;
    Ok((reader, Stdio::from(OwnedFd::from(writer))))
}

#[cfg(unix)]
fn supervise(mut command: Command, cancel: &AtomicBool, update: &Mutex<Update>) -> String {
    let Ok((mut stdout, out)) = output_pipe() else {
        return "Cannot open Pi stdout.".into();
    };
    let Ok((mut stderr, err)) = output_pipe() else {
        return "Cannot open Pi stderr.".into();
    };
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    if cancel.load(Ordering::Acquire) {
        return "Cancelled. Any edits already made remain on disk.".into();
    }
    let child = command.spawn();
    drop(command);
    let Ok(child) = child else {
        return "Cannot start Pi. Check executable permissions and project directory.".into();
    };
    let child = OwnedChild(child);
    let mut protocol = Protocol::default();
    let mut total = 0usize;
    let mut out_eof = false;
    let mut err_eof = false;
    let mut exited = None;
    loop {
        if cancel.load(Ordering::Acquire) {
            return "Cancelled. Any edits already made remain on disk.".into();
        }
        for (stream, eof, parse) in [
            (&mut stdout, &mut out_eof, true),
            (&mut stderr, &mut err_eof, false),
        ] {
            for _ in 0..16 {
                let mut bytes = [0; 8192];
                match stream.read(&mut bytes) {
                    Ok(0) => {
                        *eof = true;
                        break;
                    }
                    Ok(count) => {
                        total += count;
                        if total > MAX_OUTPUT {
                            return "Stopped: Pi output exceeded 8 MiB. Any edits already made remain on disk.".into();
                        }
                        if parse && protocol.push(&bytes[..count]).is_err() {
                            return "Stopped: invalid or oversized Pi JSON event.".into();
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => return "Cannot read Pi output.".into(),
                }
            }
        }
        {
            let mut state = update.lock().unwrap_or_else(|error| error.into_inner());
            state.text.clone_from(&protocol.text);
            state.truncated = protocol.truncated;
            state.status = "Pi running. Final messages appear below.".into();
        }
        if exited.is_none() {
            match child.exited() {
                Ok(Some(status)) => exited = Some((status, Instant::now())),
                Ok(None) => {}
                Err(_) => return "Cannot read Pi exit status.".into(),
            }
        }
        if let Some((status, time)) = exited {
            if out_eof && err_eof {
                if !status {
                    return "Pi exited unsuccessfully. Check Pi version, provider and authentication in your terminal. Raw diagnostics are hidden to avoid exposing credentials.".into();
                }
                if !protocol.line.is_empty() || !protocol.ended {
                    return "Pi exited without a complete agent_end event.".into();
                }
                return match protocol.stop.as_deref() {
                    Some("stop") => "Completed. Review the project changes.".into(),
                    Some("length") => {
                        "Pi reached its response limit; task may be incomplete.".into()
                    }
                    Some("error" | "aborted") => {
                        "Pi failed or aborted. Check provider and authentication in your terminal."
                            .into()
                    }
                    _ => "Pi exited without a final assistant response.".into(),
                };
            }
            if time.elapsed() > Duration::from_millis(250) {
                return "Pi exited but output streams remained open; task completion is unconfirmed.".into();
            }
        }
        thread::sleep(POLL);
    }
}

#[cfg(unix)]
#[derive(Default)]
struct Protocol {
    line: Vec<u8>,
    text: String,
    stop: Option<String>,
    ended: bool,
    truncated: bool,
}

#[cfg(unix)]
impl Protocol {
    fn push(&mut self, bytes: &[u8]) -> Result<(), ()> {
        for &byte in bytes {
            if byte == b'\n' {
                if self.line.last() == Some(&b'\r') {
                    self.line.pop();
                }
                if self.line.is_empty() {
                    continue;
                }
                let event: Value = serde_json::from_slice(&self.line).map_err(|_| ())?;
                self.line.clear();
                self.event(&event)?;
            } else {
                if self.line.len() == MAX_LINE {
                    return Err(());
                }
                self.line.push(byte);
            }
        }
        Ok(())
    }

    fn event(&mut self, event: &Value) -> Result<(), ()> {
        match event["type"].as_str().ok_or(())? {
            "agent_start" => self.ended = false,
            "agent_end" => self.ended = true,
            "message_end" if event["message"]["role"] == "assistant" => {
                let message = &event["message"];
                self.stop = Some(message["stopReason"].as_str().ok_or(())?.to_owned());
                if matches!(self.stop.as_deref(), Some("error" | "aborted")) {
                    return Ok(());
                }
                let content = message["content"].as_array().ok_or(())?;
                for block in content {
                    if block["type"] == "text" {
                        let text = block["text"].as_str().ok_or(())?;
                        append_capped(&mut self.text, text, &mut self.truncated);
                        append_capped(&mut self.text, "\n", &mut self.truncated);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(unix)]
fn append_capped(target: &mut String, text: &str, truncated: &mut bool) {
    let mut end = text.len().min(MAX_TEXT.saturating_sub(target.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&text[..end]);
    *truncated |= end < text.len();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(text: &str, stop: &str) -> Value {
        json!({"type":"message_end", "message": {
            "role":"assistant", "stopReason":stop,
            "content":[{"type":"thinking","thinking":"hidden"},
                {"type":"text","text":text},
                {"type":"toolCall","name":"read","arguments":{"path":"hidden"}}]
        }})
    }

    #[test]
    fn extracts_authoritative_text_without_deltas_tools_or_thinking() {
        let mut protocol = Protocol::default();
        let events = [
            json!({"type":"session","version":3}),
            json!({"type":"agent_start"}),
            json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"duplicate"}}),
            json!({"type":"message_end","message":{"role":"toolResult","content":[{"type":"text","text":"hidden"}]}}),
            message("Hello\u{2028}世界", "stop"),
            json!({"type":"agent_end","messages":[]}),
        ];
        for event in events {
            for byte in format!("{event}\r\n").as_bytes() {
                protocol.push(&[*byte]).unwrap();
            }
        }
        assert_eq!(protocol.text, "Hello\u{2028}世界\n");
        assert_eq!(protocol.stop.as_deref(), Some("stop"));
        assert!(protocol.ended);
        protocol.event(&json!({"type":"agent_start"})).unwrap();
        assert!(!protocol.ended);
    }

    #[test]
    fn caps_utf8_transcript_and_event_buffer() {
        let mut protocol = Protocol::default();
        protocol
            .event(&message(&"界".repeat(MAX_TEXT), "stop"))
            .unwrap();
        assert!(protocol.truncated);
        assert!(protocol.text.len() <= MAX_TEXT);
        assert!(protocol.text.is_char_boundary(protocol.text.len()));
        protocol.push(&vec![b'x'; MAX_LINE]).unwrap();
        assert!(protocol.push(b"x").is_err());
        assert_eq!(protocol.line.len(), MAX_LINE);
        assert!(Protocol::default().push(b"not json\n").is_err());
        assert!(Protocol::default().push(b"{}\n").is_err());
    }

    #[test]
    fn tolerates_crlf_and_blank_lines() {
        let mut protocol = Protocol::default();
        for chunk in [
            &b"{\"type\":\"agent_start\"}\r\n"[..],
            b"\r\n",
            b"\n",
            b"{\"type\":\"agent_end\"}\n",
        ] {
            protocol.push(chunk).unwrap();
        }
        assert!(protocol.ended);
    }

    #[test]
    fn does_not_expose_provider_error_content() {
        let mut protocol = Protocol::default();
        protocol
            .event(&message("private diagnostic", "error"))
            .unwrap();
        assert!(protocol.text.is_empty());
        assert_eq!(protocol.stop.as_deref(), Some("error"));
    }

    #[test]
    fn configured_binary_must_be_absolute_and_executable() {
        assert!(resolve_binary(Some(OsStr::new("pi")), None).is_err());
        assert!(resolve_binary(Some(OsStr::new("/nonexistent/sound-tools-pi")), None).is_err());
        assert!(resolve_binary(Some(OsStr::new(env!("CARGO_MANIFEST_DIR"))), None).is_err());
        assert!(resolve_binary(None, Some(OsStr::new(".:relative"))).is_err());
        assert!(resolve_binary(None, None).is_err());
        assert_eq!(
            resolve_binary(Some(OsStr::new("/usr/bin/python3")), None).unwrap(),
            PathBuf::from("/usr/bin/python3")
        );
    }

    fn mock(script: &str) -> Run {
        let mut command = Command::new("/usr/bin/python3");
        command.args(["-u", "-c", script]);
        Run::launch(command).unwrap()
    }

    fn wait(run: &Run, condition: impl Fn(&Update) -> bool) -> Update {
        let start = Instant::now();
        loop {
            let state = run.snapshot();
            if condition(&state) {
                return state;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out: {}",
                state.status
            );
            thread::sleep(POLL);
        }
    }

    #[test]
    fn completion_requires_exit_and_agent_end() {
        let run = mock(&format!(
            "import time\nprint({:?})\nprint('{{\"type\":\"agent_end\"}}')\ntime.sleep(0.3)",
            message("done", "stop").to_string()
        ));
        let state = wait(&run, |state| !state.text.is_empty());
        assert!(!state.finished);
        assert!(!state.status.starts_with("Completed"));
        assert!(
            wait(&run, |state| state.finished)
                .status
                .starts_with("Completed")
        );
        let run = mock(&format!("print({:?})", message("done", "stop").to_string()));
        assert!(
            wait(&run, |state| state.finished)
                .status
                .contains("without a complete agent_end")
        );
    }

    #[test]
    fn handles_spawn_failure_exit_failure_and_provider_error() {
        let run = Run::launch(Command::new("/nonexistent/sound-tools-pi")).unwrap();
        assert!(
            wait(&run, |state| state.finished)
                .status
                .starts_with("Cannot start Pi")
        );
        let run = mock("import sys\nsys.stderr.write('private diagnostic')\nsys.exit(7)");
        let state = wait(&run, |state| state.finished);
        assert!(state.status.contains("unsuccessfully"));
        assert!(!state.status.contains("private diagnostic"));
        let run = mock(&format!(
            "print({:?})\nprint('{{\"type\":\"agent_end\"}}')",
            message("private diagnostic", "error").to_string()
        ));
        assert!(
            wait(&run, |state| state.finished)
                .status
                .contains("failed or aborted")
        );
    }

    #[test]
    fn drains_both_streams_and_stops_at_capture_limit() {
        let run = mock(&format!(
            "import sys\nfor i in range(256):\n sys.stderr.write('x'*8192)\n print('{{\"type\":\"turn_start\"}}')\nprint({:?})\nprint('{{\"type\":\"agent_end\"}}')",
            message("done", "stop").to_string()
        ));
        assert!(
            wait(&run, |state| state.finished)
                .status
                .starts_with("Completed")
        );
        let run = mock("import sys\nsys.stderr.write('x'*(9*1024*1024))");
        assert!(
            wait(&run, |state| state.finished)
                .status
                .contains("exceeded 8 MiB")
        );
        let run = mock("print('x'*(256*1024+1))");
        assert!(
            wait(&run, |state| state.finished)
                .status
                .contains("oversized")
        );
    }

    fn sleeping_mock() -> (Run, u32) {
        let run = mock(
            "import os,json,time\nprint(json.dumps({'type':'message_end','message':{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':str(os.getpid())}]}}))\ntime.sleep(60)",
        );
        let state = wait(&run, |state| !state.text.is_empty());
        let pid = state.text.trim().parse().unwrap();
        (run, pid)
    }

    fn assert_reaped(pid: u32) {
        let status = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "child still exists after cleanup");
    }

    #[test]
    fn cancel_and_drop_kill_and_reap_child() {
        let (run, pid) = sleeping_mock();
        run.cancel();
        assert!(
            wait(&run, |state| state.finished)
                .status
                .starts_with("Cancelled")
        );
        assert_reaped(pid);
        let (run, pid) = sleeping_mock();
        let start = Instant::now();
        drop(run);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_reaped(pid);
    }

    #[test]
    fn descendants_cannot_write_after_cleanup() {
        const ISOLATED: &str = "SOUND_TOOLS_AGENT_CLEANUP_TEST";
        if env::var_os(ISOLATED).is_none() {
            let status = Command::new(env::current_exe().unwrap())
                .args([
                    "--exact",
                    "agent::tests::descendants_cannot_write_after_cleanup",
                    "--nocapture",
                ])
                .env(ISOLATED, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        #[cfg(target_os = "linux")]
        assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);

        let (unrelated, unrelated_pid) = sleeping_mock();
        for mode in ["cancel", "drop", "exit", "streams", "term"] {
            let directory =
                env::temp_dir().join(format!("sound-tools-agent-{}-{mode}", std::process::id()));
            std::fs::create_dir(&directory).unwrap();
            let output = directory.join("edited");
            let run = mock(&format!(
                r#"import os,json,time,signal
signal.signal(signal.SIGTERM, signal.SIG_DFL if {mode:?} == 'term' else signal.SIG_IGN)
reader,writer=os.pipe()
pid=os.fork()
if pid == 0:
 os.close(reader)
 if {mode:?} == 'exit':
  os.close(1)
  os.close(2)
 os.write(writer,b'1')
 os.close(writer)
 time.sleep(1)
 open({:?}, 'w').write('escaped')
 time.sleep(60)
 os._exit(0)
os.close(writer)
os.read(reader,1)
os.close(reader)
print(json.dumps({{'type':'message_end','message':{{'role':'assistant','stopReason':'stop','content':[{{'type':'text','text':json.dumps([os.getpid(),pid,os.getpgrp()])}}]}}}}))
print('{{"type":"agent_end"}}')
if {mode:?} in ['exit','streams']:
 os._exit(0)
time.sleep(60)"#,
                output.to_str().unwrap(),
            ));
            let state = wait(&run, |state| !state.text.is_empty() || state.finished);
            let start = Instant::now();
            if matches!(mode, "cancel" | "term") {
                run.cancel();
            }
            if mode != "drop" {
                let state = wait(&run, |state| state.finished);
                assert!(
                    match mode {
                        "cancel" | "term" => state.status.starts_with("Cancelled"),
                        "exit" => state.status.starts_with("Completed"),
                        "streams" => state.status.contains("streams remained open"),
                        _ => unreachable!(),
                    },
                    "{}",
                    state.status
                );
            }
            drop(run);
            assert!(start.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(1200));
            let edited = output.exists();
            std::fs::remove_dir_all(directory).unwrap();
            assert!(!edited, "descendant edited files after {mode}");
            {
                let [leader, descendant, group]: [u32; 3] =
                    serde_json::from_str(state.text.trim()).unwrap();
                assert_eq!(leader, group);
                assert_ne!(group, unsafe { libc::getpgrp() } as u32);
                assert_reaped(leader);
                #[cfg(target_os = "linux")]
                {
                    let mut status = 0;
                    assert_eq!(
                        unsafe {
                            libc::waitpid(descendant as libc::pid_t, &mut status, libc::WNOHANG)
                        },
                        descendant as libc::pid_t
                    );
                    assert!(libc::WIFSIGNALED(status));
                    assert_eq!(
                        libc::WTERMSIG(status),
                        if mode == "term" {
                            libc::SIGTERM
                        } else {
                            libc::SIGKILL
                        }
                    );
                    assert_reaped(descendant);
                    assert_eq!(unsafe { libc::kill(-(group as libc::pid_t), 0) }, -1);
                    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
                }
                #[cfg(not(target_os = "linux"))]
                assert_reaped(descendant);
            }
            assert_eq!(unsafe { libc::kill(unrelated_pid as libc::pid_t, 0) }, 0);
            assert!(!unrelated.snapshot().finished);
        }
        drop(unrelated);
    }

    #[test]
    fn inherited_streams_do_not_block_cleanup() {
        let run =
            mock("import os,time\nif os.fork() == 0:\n time.sleep(1)\n os._exit(0)\nos._exit(0)");
        assert!(
            wait(&run, |state| state.finished)
                .status
                .contains("streams remained open")
        );
    }
}
