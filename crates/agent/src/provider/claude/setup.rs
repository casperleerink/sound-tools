//! Setting up Claude Code: the pinned download, and its own sign-in with `claude auth`.
//!
//! The app never sees a credential. `auth login` runs Anthropic's flow in the browser and
//! keeps what it gets in Claude Code's own store; the app only waits for it to end and asks
//! `auth status` again.

use std::io;
use std::process::{Output, Stdio};

use serde::Deserialize;

use super::command;
use crate::install::Download;
use crate::provider::{Account, Installed};

/// The version the app downloads. The fixtures in `tests/fixtures/claude/` were recorded on
/// it: moving the pin means new values below and recording the fixtures again, in one commit.
const VERSION: &str = "2.1.286";

/// One build of the pinned version.
struct Build {
    /// Rust's names for the computer it runs on, as in `std::env::consts`.
    os: &'static str,
    arch: &'static str,
    /// Anthropic's name for the same.
    platform: &'static str,
    sha256: &'static str,
    size: u64,
}

/// From `https://downloads.claude.ai/claude-code-releases/<VERSION>/manifest.json`.
const BUILDS: [Build; 4] = [
    Build {
        os: "macos",
        arch: "aarch64",
        platform: "darwin-arm64",
        sha256: "75e3016e9d2570767b08e43a7467d4817a4f149232c169ca295f2c95fef21433",
        size: 225_167_728,
    },
    Build {
        os: "macos",
        arch: "x86_64",
        platform: "darwin-x64",
        sha256: "53e6a936e89519d695230f9cc97943991286b72766674fba11bee845f0a7c047",
        size: 233_607_424,
    },
    Build {
        os: "linux",
        arch: "x86_64",
        platform: "linux-x64",
        sha256: "fe503f65c6289d59c23e5b21ae44f03583f997dd33a2cbfc75ab4f96fb8fc73f",
        size: 241_667_256,
    },
    Build {
        os: "linux",
        arch: "aarch64",
        platform: "linux-arm64",
        sha256: "0292fa22ac2fd43e16be9d0e511ddd8347280d6e0ebaca744ef5b27e05d8d0f8",
        size: 241_033_208,
    },
];

/// The pinned `claude` for this computer, unmodified, as Anthropic's own installer fetches
/// it. `None` where Anthropic has no build.
pub fn download() -> Option<Download> {
    let build = BUILDS
        .iter()
        .find(|build| build.os == std::env::consts::OS && build.arch == std::env::consts::ARCH)?;
    let platform = build.platform;
    Some(Download {
        name: "claude",
        version: VERSION,
        url: format!(
            "https://downloads.claude.ai/claude-code-releases/{VERSION}/{platform}/claude"
        ),
        sha256: build.sha256,
        size: build.size,
    })
}

/// The two ways in that Claude Code offers, and the arguments of each. Both run in the
/// browser: a Claude subscription, or an Anthropic Console account billed per use.
pub const SIGN_IN_CHOICES: [(&str, &[&str]); 2] = [
    (
        "Sign in with your Claude plan",
        &["auth", "login", "--claudeai"],
    ),
    (
        "Use an Anthropic Console account (API)",
        &["auth", "login", "--console"],
    ),
];

/// What `claude auth status --json` says. Only what the sidebar shows.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    logged_in: bool,
    email: Option<String>,
    /// Such as `max` or `pro`, for a Claude subscription only.
    subscription_type: Option<String>,
}

impl Status {
    fn account(self) -> Account {
        let plan = self.subscription_type.map(|kind| {
            let mut letters = kind.chars();
            let first = letters
                .next()
                .map(|letter| letter.to_uppercase().to_string());
            let kind = first.unwrap_or_default() + letters.as_str();
            format!("Claude {}", kind.replace('_', " "))
        });
        Account {
            email: self.email,
            plan,
        }
    }
}

/// The account `claude` is signed in to, or `None` when it is signed out.
pub async fn account(installed: &Installed) -> io::Result<Option<Account>> {
    let output = run(installed, &["auth", "status", "--json"]).await?;
    // Signed out is `loggedIn: false` and exit code 1. Anything unreadable is the CLI failing.
    match serde_json::from_slice::<Status>(&output.stdout) {
        Ok(status) if status.logged_in => Ok(Some(status.account())),
        Ok(_) => Ok(None),
        Err(_) => Err(failed(
            &output,
            "its answer about the account was unreadable",
        )),
    }
}

/// Runs `claude auth login` with one of [`SIGN_IN_CHOICES`] and waits for it to end:
/// Anthropic's page opens in the browser, and its callback to a port on this computer finishes
/// the sign-in.
///
/// Dropping the future cancels it: the CLI is killed.
pub async fn sign_in(installed: &Installed, arguments: &[&str]) -> io::Result<()> {
    run_to_success(installed, arguments).await
}

pub async fn sign_out(installed: &Installed) -> io::Result<()> {
    run_to_success(installed, &["auth", "logout"]).await
}

/// Runs the CLI until it ends, and fails when it ends with an error.
async fn run_to_success(installed: &Installed, arguments: &[&str]) -> io::Result<()> {
    let output = run(installed, arguments).await?;
    if output.status.success() {
        return Ok(());
    }
    Err(failed(&output, &format!("it ended with {}", output.status)))
}

async fn run(installed: &Installed, arguments: &[&str]) -> io::Result<Output> {
    smol::process::Command::from(command(&installed.program, &installed.environment))
        .args(arguments)
        // `auth login` also takes a code pasted on stdin. The app has no field for one, so
        // stdin is empty and the browser's callback is the only way in.
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
}

/// The first line the CLI wrote on stderr, or `otherwise`.
fn failed(output: &Output, otherwise: &str) -> io::Error {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.lines().map(str::trim).find(|line| !line.is_empty());
    io::Error::other(line.unwrap_or(otherwise).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_platform_has_a_build() {
        for platform in ["darwin-arm64", "darwin-x64", "linux-x64", "linux-arm64"] {
            assert!(BUILDS.iter().any(|build| build.platform == platform));
        }
        let download = download().unwrap();
        assert!(download.url.ends_with("/claude"));
        assert!(download.url.contains(VERSION));
    }

    #[test]
    fn reads_the_account_of_auth_status() {
        let status: Status = serde_json::from_str(
            r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "composer@example.com",
                "orgName": "Studio", "subscriptionType": "max"}"#,
        )
        .unwrap();
        let account = status.account();
        assert_eq!(account.email.as_deref(), Some("composer@example.com"));
        assert_eq!(account.plan.as_deref(), Some("Claude Max"));
    }

    /// A Console login has no plan, and may have no email.
    #[test]
    fn an_account_with_no_plan_and_no_email_is_still_signed_in() {
        let status: Status = serde_json::from_str(
            r#"{"loggedIn": true, "authMethod": "api_key", "subscriptionType": null}"#,
        )
        .unwrap();
        assert!(status.logged_in);
        assert_eq!(status.account(), Account::default());
    }
}
