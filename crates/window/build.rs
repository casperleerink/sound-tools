//! Embeds the template of a new project: `template/`, and the tools it shares with the DAW
//! template of `templates/daw`, which play the same.

use std::error::Error;
use std::path::{Path, PathBuf};

/// The DAW's tools that have no view, so a project of the new window takes them as they are.
const SHARED: [&str; 5] = ["track.ts", "chain.ts", "bus.ts", "tape.ts", "lfo.ts"];

/// What the runtime writes into a project, never part of a template.
const WRITTEN: [&str; 7] = [
    "sdk.ts",
    "sdk",
    "tsconfig.json",
    "bun.d.ts",
    "log.txt",
    ".gitignore",
    "node_modules",
];

fn main() -> Result<(), Box<dyn Error>> {
    let crate_folder = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let template = crate_folder.join("template");
    let daw = crate_folder.join("../../templates/daw/extensions");
    println!("cargo::rerun-if-changed={}", template.display());
    let mut files = Vec::new();
    files_in(&template, &template, &mut files)?;
    for name in SHARED {
        let file = daw.join(name);
        println!("cargo::rerun-if-changed={}", file.display());
        files.push((format!("extensions/{name}"), file));
    }
    files.sort();
    let mut code = String::from("&[\n");
    for (path, file) in files {
        code.push_str(&format!(
            "    ({path:?}, include_str!({:?})),\n",
            file.display().to_string()
        ));
    }
    code.push(']');
    std::fs::write(
        PathBuf::from(std::env::var("OUT_DIR")?).join("template.rs"),
        code,
    )?;
    Ok(())
}

fn files_in(
    root: &Path,
    folder: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if WRITTEN.contains(&name) {
            continue;
        }
        if path.is_dir() {
            files_in(root, &path, files)?;
        } else {
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("a template file name is not UTF-8")?;
            files.push((relative.replace('\\', "/"), path.clone()));
        }
    }
    Ok(())
}
