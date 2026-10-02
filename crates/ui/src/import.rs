//! A file of the composer brought into the project, as a view that takes a sample does: the
//! file panel of the system for one file, then the copy into `assets/audio/` on a background
//! thread. An error of either goes to the notice line of the window.

use std::path::PathBuf;

use gpui::{AppContext, Context, Entity, PathPromptOptions};
use sound_media::Imported;

use crate::session::Session;

/// Opens the file panel of the system for one file and hands its path to `chosen`. A cancelled
/// panel does nothing.
pub fn choose_file<V: 'static>(
    session: &Entity<Session>,
    prompt: &'static str,
    cx: &mut Context<V>,
    chosen: impl FnOnce(&mut V, PathBuf, &mut Context<V>) + 'static,
) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt.into()),
    });
    let session = session.clone();
    cx.spawn(async move |view, cx| {
        let answer = match picked.await {
            Ok(answer) => answer,
            // The panel went away without an answer.
            Err(_) => return,
        };
        // Released: there is nothing left to tell.
        view.update(cx, |view, cx| match answer {
            Ok(paths) => {
                if let Some(path) = paths.and_then(|paths| paths.into_iter().next()) {
                    chosen(view, path, cx);
                }
            }
            Err(error) => session.update(cx, |session, cx| session.report(error, cx)),
        })
        .ok();
    })
    .detach();
}

/// Copies the file at `path` into `assets/audio/` on a background thread and hands it to
/// `imported`. A file that does not play is not copied, and the notice line says why.
pub fn import_file<V: 'static>(
    session: &Entity<Session>,
    path: PathBuf,
    cx: &mut Context<V>,
    imported: impl FnOnce(&mut V, Imported, &mut Context<V>) + 'static,
) {
    let assets = session.read(cx).project().assets().clone();
    let importing = cx.background_spawn(async move { sound_media::import(&assets, &path) });
    let session = session.clone();
    cx.spawn(async move |view, cx| {
        let result = importing.await;
        // Released: there is nothing left to tell.
        view.update(cx, |view, cx| match result {
            Ok(file) => imported(view, file, cx),
            Err(error) => session.update(cx, |session, cx| session.report(error, cx)),
        })
        .ok();
    })
    .detach();
}
