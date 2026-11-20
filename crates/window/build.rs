//! On Windows, puts the app icon into the program: Explorer, the Start menu and the taskbar show
//! it.

use std::error::Error;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS")? == "windows" {
        let crate_folder = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
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
