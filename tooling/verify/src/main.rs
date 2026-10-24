//! `cargo verify`: the checks of CI for what this branch changed, not the whole workspace.
//!
//! Formatting; clippy, warnings denied, on the changed crates and every crate that uses them;
//! the tests of the changed crates. Changed means since `origin/main`, or since the branch
//! given with `--base <branch>` first, for a branch stacked on another. The other arguments go
//! to nextest: `-E 'test(name)'` picks tests, `-p runtime` adds the tests of a crate that uses
//! a change.

use std::collections::BTreeSet;
use std::fs::{File, TryLockError};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{Metadata, MetadataCommand, Package};

/// Files outside the crates that change how every crate builds.
const WORKSPACE_FILES: [&str; 5] = [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "clippy.toml",
    ".cargo/",
];

/// Folders outside the crates that a crate builds in.
const FOLDERS: [(&str, &str); 1] = [("templates/", "runtime")];

/// Checks the docs and the dependency rules, which a change anywhere can break. It is cheap.
const ALWAYS_TESTED: &str = "workspace-rules";

fn main() -> Result<ExitCode> {
    let metadata = MetadataCommand::new().no_deps().exec()?;
    let root = &metadata.workspace_root;
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    let base = match arguments.first().map(String::as_str) {
        Some("--base") if arguments.len() > 1 => arguments.drain(..2).nth(1),
        Some("--base") => bail!("--base needs a branch"),
        _ => None,
    };
    let changed = changed_files(root, base.as_deref().unwrap_or("origin/main"))?;

    let mut tested = BTreeSet::from([ALWAYS_TESTED.to_string()]);
    // The crates whose code other crates use changed: a change only in a crate's `tests/` does
    // not reach the crates that use it.
    let mut used = BTreeSet::new();
    let mut whole_workspace = false;
    for file in &changed {
        if WORKSPACE_FILES.iter().any(|name| file.starts_with(name)) {
            whole_workspace = true;
        } else if let Some((_, package)) =
            FOLDERS.iter().find(|(folder, _)| file.starts_with(folder))
        {
            tested.insert(package.to_string());
            used.insert(package.to_string());
        } else if let Some(package) = package_of(&metadata, &root.join(file)) {
            tested.insert(package.name.to_string());
            let tests = package.manifest_path.with_file_name("tests");
            if !root.join(file).starts_with(tests) {
                used.insert(package.name.to_string());
            }
        }
    }
    let checked = if whole_workspace {
        vec!["--workspace".to_string()]
    } else {
        package_arguments(
            &with_dependents(&metadata, &used)
                .union(&tested)
                .cloned()
                .collect(),
        )
    };

    println!(
        "verify: testing {}",
        Vec::from_iter(tested.clone()).join(", ")
    );
    let _lock = lock_machine()?;
    let steps: [(&str, Vec<String>); 3] = [
        ("fmt", strings(["--all", "--check"])),
        (
            "clippy",
            [
                strings(["--locked", "--all-targets"]),
                checked,
                strings(["--", "-D", "warnings"]),
            ]
            .concat(),
        ),
        (
            "nextest",
            [
                strings(["run", "--locked"]),
                package_arguments(&tested),
                arguments,
            ]
            .concat(),
        ),
    ];
    for (subcommand, arguments) in steps {
        println!("verify: cargo {subcommand} {}", arguments.join(" "));
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .arg(subcommand)
            .args(&arguments)
            .current_dir(root)
            .status()
            .with_context(|| format!("cargo {subcommand} did not start"))?;
        if !status.success() {
            return Ok(ExitCode::FAILURE);
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Every file this branch changed since it left `base`, committed or not, relative to the root
/// of the repository.
fn changed_files(root: &Utf8Path, base: &str) -> Result<Vec<Utf8PathBuf>> {
    let base = git(root, &["merge-base", "HEAD", base])?;
    let changed = git(
        root,
        &["diff", "-z", "--no-renames", "--name-only", base.trim()],
    )?;
    let untracked = git(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    Ok(changed
        .split('\0')
        .chain(untracked.split('\0'))
        .filter(|file| !file.is_empty())
        .map(Utf8PathBuf::from)
        .collect())
}

fn git(root: &Utf8Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .context("git did not start")?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// The crate whose folder holds `file`.
fn package_of<'a>(metadata: &'a Metadata, file: &Utf8Path) -> Option<&'a Package> {
    metadata
        .workspace_packages()
        .into_iter()
        .filter(|package| {
            package
                .manifest_path
                .parent()
                .is_some_and(|folder| file.starts_with(folder))
        })
        .max_by_key(|package| package.manifest_path.as_str().len())
}

/// `packages` and every crate of the workspace that depends on one of them, through any number
/// of crates and in any kind of dependency.
fn with_dependents(metadata: &Metadata, packages: &BTreeSet<String>) -> BTreeSet<String> {
    let mut all = packages.clone();
    loop {
        let more: Vec<String> = metadata
            .workspace_packages()
            .into_iter()
            .filter(|package| !all.contains(package.name.as_str()))
            .filter(|package| {
                package
                    .dependencies
                    .iter()
                    .any(|dependency| dependency.path.is_some() && all.contains(&dependency.name))
            })
            .map(|package| package.name.to_string())
            .collect();
        if more.is_empty() {
            return all;
        }
        all.extend(more);
    }
}

fn package_arguments(packages: &BTreeSet<String>) -> Vec<String> {
    packages
        .iter()
        .flat_map(|package| ["-p".to_string(), package.clone()])
        .collect()
}

fn strings<const N: usize>(arguments: [&str; N]) -> Vec<String> {
    arguments.map(String::from).to_vec()
}

/// One verify at a time on this machine: two builds side by side run out of memory on a small
/// one, and on a large one each finishes no sooner than when they take turns.
fn lock_machine() -> Result<File> {
    let path = std::env::temp_dir().join("sound-tools-verify.lock");
    let file = File::create(&path).with_context(|| format!("{} did not open", path.display()))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            println!("verify: waiting for another verify on this machine");
            file.lock()?;
        }
        Err(TryLockError::Error(error)) => return Err(error.into()),
    }
    Ok(file)
}
