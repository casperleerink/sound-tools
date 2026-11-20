//! Embeds every folder of `templates/` as a template of a new project.

use std::error::Error;
use std::path::{Path, PathBuf};

#[path = "src/templates/about.rs"]
mod about;
#[path = "src/templates/written.rs"]
mod written;
use about::About;
use written::written_by_the_runtime;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=build.rs");
    let crate_folder = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let repository = crate_folder
        .ancestors()
        .nth(2)
        .ok_or("crates/runtime is not in a repository")?;
    embed_templates(&repository.join("templates"))?;
    Ok(())
}

/// Writes `templates.rs` into `OUT_DIR`: a `Template` per folder of `templates`, in name order,
/// with every file in it but `template.json`.
fn embed_templates(templates: &Path) -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={}", templates.display());
    let mut folders: Vec<PathBuf> = std::fs::read_dir(templates)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    folders.retain(|folder| folder.is_dir());
    folders.sort();
    let mut code = String::from("&[\n");
    for folder in folders {
        let about = folder.join("template.json");
        let about: About = serde_json::from_str(&std::fs::read_to_string(&about)?)
            .map_err(|error| format!("{}: {error}", about.display()))?;
        let mut files = Vec::new();
        files_in(&folder, &mut files)?;
        files.sort();
        code.push_str(&format!(
            "    Template {{ name: {:?}, description: {:?}, files: &[\n",
            about.name, about.description
        ));
        for file in files {
            let path = file.strip_prefix(&folder)?;
            let path = path.to_str().ok_or("a template file name is not UTF-8")?;
            let path = path.replace('\\', "/");
            if path == "template.json" || written_by_the_runtime(&path) {
                continue;
            }
            code.push_str(&format!(
                "        ({:?}, include_str!({:?})),\n",
                path,
                file.display().to_string()
            ));
        }
        code.push_str("    ] },\n");
    }
    code.push(']');
    let out = PathBuf::from(std::env::var("OUT_DIR")?).join("templates.rs");
    std::fs::write(out, code)?;
    Ok(())
}

fn files_in(folder: &Path, files: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        match path.is_dir() {
            true => files_in(&path, files)?,
            false => files.push(path),
        }
    }
    Ok(())
}
