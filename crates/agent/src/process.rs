//! The process tree of the agent's CLI.

use std::io;
use std::process::Command;

/// A started program and every program it starts in turn, such as a `cargo build` the agent
/// runs, so that they all end together.
#[derive(Debug)]
pub(crate) struct ProcessTree {
    #[cfg(unix)]
    group: libc::pid_t,
    #[cfg(windows)]
    job: job::Job,
}

impl ProcessTree {
    /// Call on the command before it starts.
    pub(crate) fn prepare(#[cfg_attr(windows, allow(unused_variables))] command: &mut Command) {
        // Its own process group: a ctrl-c in the terminal that started the app does not stop
        // the program halfway through a write, and the group can be ended as one. On Windows
        // the hidden console of `background_command` already keeps a ctrl-c away, and the job is made
        // once the program runs.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(command, 0);
    }

    /// The tree of `child`, which started from a command [`ProcessTree::prepare`] saw.
    pub(crate) fn of(child: &smol::process::Child) -> io::Result<ProcessTree> {
        #[cfg(unix)]
        {
            let group = libc::pid_t::try_from(child.id()).map_err(io::Error::other)?;
            Ok(ProcessTree { group })
        }
        #[cfg(windows)]
        {
            Ok(ProcessTree {
                job: job::Job::of(child)?,
            })
        }
    }

    /// Ends every program of the tree. On Unix only while the first program is not reaped
    /// yet, because its id names the group.
    pub(crate) fn end(&self) {
        #[cfg(unix)]
        {
            // SAFETY: `killpg` only sends a signal; it touches no memory of ours. The group
            // is the program's own (`process_group(0)` in `prepare`), and the caller has not
            // reaped it, so the id is still its. It fails only when the group is gone
            // already, which is what this wants.
            unsafe { libc::killpg(self.group, libc::SIGTERM) };
        }
        #[cfg(windows)]
        self.job.end();
    }
}

/// A Windows job object: every process started by a process in the job is in it too, also
/// after its parent ended, which a walk of the tree by parent id (`taskkill /T`) misses. The
/// job ends them all when its last handle closes, so a crash of the app ends them too.
#[cfg(windows)]
mod job {
    use std::io;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };

    #[derive(Debug)]
    pub(super) struct Job(HANDLE);

    // SAFETY: a job handle may be used and closed from any thread.
    unsafe impl Send for Job {}
    // SAFETY: `TerminateJobObject` may be called from several threads at once.
    unsafe impl Sync for Job {}

    impl Job {
        /// A job that holds `child`. The child runs a moment before it joins: a process it
        /// starts in that moment stays out. The CLI starts nothing that early.
        pub(super) fn of(child: &smol::process::Child) -> io::Result<Job> {
            // SAFETY: no attributes and no name are valid arguments.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            // Owned from here, so an error below closes it.
            let job = Job(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // A program that asks to leave the job may: without this, Windows refuses to
            // start it at all.
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                .map_err(io::Error::other)?;
            // SAFETY: the job handle is open, and `limits` is the struct the class names,
            // with its size.
            let set = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size,
                )
            };
            if set == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: both handles are open: the job's is ours, and the child's lives as long
            // as `child`, which is borrowed here.
            let assigned = unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) };
            if assigned == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        pub(super) fn end(&self) {
            // SAFETY: the handle is open until drop. It fails only when the processes are
            // gone already, which is what this wants.
            unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is ours and closed only here. Closing the last handle ends
            // whatever still runs in the job (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`).
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use smol::io::{AsyncBufReadExt, BufReader};

    use super::*;

    /// A program whose parent ended already still ends with the tree. A walk of the tree by
    /// parent id would miss it.
    #[test]
    fn ending_the_tree_ends_a_program_whose_parent_is_gone() {
        let mut start = sound_core::process::background_command("powershell");
        start.args([
            "-NoProfile",
            "-Command",
            "(Start-Process ping -ArgumentList '-n','1000','127.0.0.1' -PassThru -NoNewWindow).Id",
        ]);
        ProcessTree::prepare(&mut start);
        let mut parent = smol::process::Command::from(start)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let tree = ProcessTree::of(&parent).unwrap();
        let mut line = String::new();
        // One line only: ping keeps the pipe open.
        let stdout = parent.stdout.take().unwrap();
        smol::block_on(BufReader::new(stdout).read_line(&mut line)).unwrap();
        let ping = line.trim().to_string();
        assert!(smol::block_on(parent.status()).unwrap().success());
        assert!(running(&ping), "ping {ping:?} did not start");

        tree.end();
        let start = Instant::now();
        while running(&ping) {
            assert!(start.elapsed() < Duration::from_secs(10), "ping still runs");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn running(pid: &str) -> bool {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    }
}
