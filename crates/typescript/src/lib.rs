//! Cards written in TypeScript, in the `ui/` folder of the project, run by Bun while the app
//! runs. A spike: it shows whether the direction holds, see `sdk.ts` for what a card can do.
//!
//! The runtime writes `ui/sdk.ts` and `ui/tsconfig.json`, and starts `host.ts` in Bun, which
//! loads every other `.ts` and `.tsx` file there and loads them again when one is saved. A card
//! draws a tree from the record; the window draws the tree. Nothing of it runs on the audio
//! thread: a knob changes the record, as a knob of a built-in card does.

mod card;
mod host;
mod tree;

use std::path::{Path, PathBuf};

use gpui::{App, AppContext, Entity};
use sound_ui::{Session, Views};

use crate::card::TypeScriptCard;
use crate::host::Host;

const SDK: &str = include_str!("sdk.ts");
const HOST: &str = include_str!("host.ts");
/// JSX makes the nodes of `sdk.ts`.
const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "target": "esnext",
    "module": "esnext",
    "moduleResolution": "bundler",
    "jsx": "react",
    "jsxFactory": "h",
    "noEmit": true
  }
}
"#;

/// Starts the cards of the project in `ui/`, when Bun is installed. A card host stands in for
/// every card of `views`.
pub fn start(session: &Entity<Session>, views: &mut Views, cx: &mut App) {
    let Some(bun) = bun() else {
        eprintln!("TypeScript cards are off: bun is not installed");
        return;
    };
    let ui = session.read(cx).project().root().join("ui");
    let host = match write_files(&ui)
        .and_then(|host_script| Host::start(session, &bun, &host_script, &ui, cx))
    {
        Ok(host) => host,
        Err(error) => return eprintln!("error: TypeScript cards are off: {error}"),
    };
    views.set_card_host(move |session, id, frame, built_in, _, cx| {
        let (host, session, id) = (host.clone(), session.clone(), id.clone());
        let card = cx.new(|cx| TypeScriptCard::new(host, session, id, frame, built_in, cx));
        Some(card.into())
    });
}

/// Writes what the runtime owns in `ui/`, only where the text changed, and the host script.
fn write_files(ui: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(ui)?;
    for (name, text) in [("sdk.ts", SDK), ("tsconfig.json", TSCONFIG)] {
        let path = ui.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            std::fs::write(&path, text)?;
        }
    }
    let host_script = std::env::temp_dir().join("sound-tools-typescript-host.ts");
    std::fs::write(&host_script, HOST)?;
    Ok(host_script)
}

/// Bun on the `PATH`, or where its installer puts it. An app opened from the Finder has a
/// short `PATH`.
fn bun() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|folder| folder.join("bun"));
    let installed = std::env::var_os("HOME").map(|home| Path::new(&home).join(".bun/bin/bun"));
    on_path.chain(installed).find(|path| path.is_file())
}
