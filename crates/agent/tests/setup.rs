#![allow(clippy::unwrap_used)]
//! The real download and sign-in of the pinned `claude`, in a temporary support folder and a
//! temporary `CLAUDE_CONFIG_DIR`, so the login of this machine is never touched. Ignored by
//! default: the download is over 200 MB, and the sign-in opens the browser.
//!
//! ```sh
//! cargo test -p sound-agent --test setup -- --ignored --nocapture
//! ```
//!
//! Run it after moving the pin: it checks the new checksum and size of this platform.

use std::path::Path;
use std::time::{Duration, Instant};

use smol::future;
use sound_agent::{Installed, Provider, install, login_shell_environment};

/// The pinned program in `support`, with the login shell's environment and its own config.
fn installed(support: &Path) -> Installed {
    let download = Provider::Claude.download().unwrap();
    let started = Instant::now();
    let program = smol::block_on(install(&download, &support.join("agents"), |_| {})).unwrap();
    println!("downloaded and checked in {:?}", started.elapsed());
    let mut environment = smol::block_on(login_shell_environment()).unwrap();
    environment.insert(
        "CLAUDE_CONFIG_DIR".into(),
        support.join("claude-config").into(),
    );
    Installed {
        program,
        environment,
    }
}

#[test]
#[ignore = "downloads 215 MB"]
fn the_pinned_claude_downloads_checks_out_and_starts_signed_out() {
    let support = tempfile::tempdir().unwrap();
    let installed = installed(support.path());
    let status = smol::block_on(Provider::Claude.status(&installed)).unwrap();
    assert_eq!(status.account, None);
}

#[test]
#[ignore = "downloads 215 MB and opens the browser"]
fn the_sign_in_opens_the_browser_and_cancel_ends_it() {
    let support = tempfile::tempdir().unwrap();
    let installed = installed(support.path());
    let status = smol::block_on(Provider::Claude.status(&installed)).unwrap();
    let (steps, _) = smol::channel::unbounded();
    let (_, answers) = smol::channel::unbounded();
    let signing_in = smol::block_on(future::or(
        async { Some(status.choices[0].run(&installed, steps, answers).await) },
        async {
            #[allow(clippy::disallowed_methods)]
            smol::Timer::after(Duration::from_secs(10)).await;
            None
        },
    ));
    // Still waiting for the browser after 10 s; the race dropped it, which is Cancel.
    assert!(signing_in.is_none(), "{signing_in:?}");
    let status = smol::block_on(Provider::Claude.status(&installed)).unwrap();
    assert_eq!(status.account, None);
}
