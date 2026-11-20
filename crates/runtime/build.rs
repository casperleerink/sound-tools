//! Embeds every folder of `templates/` as a template of a new project.

use std::error::Error;
use std::path::{Path, PathBuf};

#[path = "src/templates/written.rs"]
mod written;
use written::written_by_the_runtime;

/// What a folder of `templates/` says of itself in its `template.json`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct About {
    name: String,
    description: String,
    /// Paths in `templates/` whose files this template has too, at the same path under the
    /// other template's folder: a folder or a file, or with `!` first, one left out.
    #[serde(default)]
    shares: Vec<String>,
}

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
/// with every file in it but `template.json`, and the files it shares.
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
        let files = files.into_iter().map(|file| (folder.clone(), file));
        let shared = shared(templates, &about.shares)?;
        let mut files: Vec<(PathBuf, PathBuf)> = files.chain(shared).collect();
        files.sort_by(|(_, one), (_, other)| one.cmp(other));
        code.push_str(&format!(
            "    Template {{ name: {:?}, description: {:?}, files: &[\n",
            about.name, about.description
        ));
        for (folder, file) in files {
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

/// The files of `shares` in `templates`, each with the folder of the template it is in.
fn shared(templates: &Path, shares: &[String]) -> Result<Vec<(PathBuf, PathBuf)>, Box<dyn Error>> {
    let (left_out, shared): (Vec<&String>, Vec<&String>) =
        shares.iter().partition(|path| path.starts_with('!'));
    let left_out: Vec<PathBuf> = (left_out.iter())
        .map(|path| templates.join(&path[1..]))
        .collect();
    let mut files = Vec::new();
    for path in shared {
        let template = path
            .split('/')
            .next()
            .ok_or("a shared path names no template")?;
        let path = templates.join(path);
        let mut found = Vec::new();
        match path.is_dir() {
            true => files_in(&path, &mut found)?,
            false if path.is_file() => found.push(path),
            false => return Err(format!("{} is no file of a template", path.display()).into()),
        }
        let kept = found
            .into_iter()
            .filter(|file| !left_out.iter().any(|out| file.starts_with(out)));
        files.extend(kept.map(|file| (templates.join(template), file)));
    }
    Ok(files)
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
