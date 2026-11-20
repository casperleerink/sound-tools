//! Embeds the template of a new project: `template/`, and the tools it shares with the DAW
//! template of `templates/daw`, which play the same. On Windows, also puts the app icon into
//! the program: Explorer, the Start menu and the taskbar show it.

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
    if std::env::var("CARGO_CFG_TARGET_OS")? == "windows" {
        let repository = crate_folder
            .ancestors()
            .nth(2)
            .ok_or("crates/window is not in a repository")?;
        embed_icon(&repository.join("tooling/icon/sound-tools.ico"))?;
    }
    Ok(())
}

fn embed_icon(icon: &Path) -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={}", icon.display());
    // A resource file of our own with the icon's absolute path, because the resource
    // compilers look for a relative one in different folders: MSVC's rc in the folder of the
    // build, llvm-rc in the one of the resource file. Forward slashes need no escaping in it.
    let resource = PathBuf::from(std::env::var("OUT_DIR")?).join("sound-tools.rc");
    let icon = icon.display().to_string().replace('\\', "/");
    std::fs::write(&resource, format!("1 ICON \"{icon}\"\n"))?;
    embed_resource::compile(&resource, embed_resource::NONE).manifest_required()?;
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
