//! The composer's instructions to the agent, edited in the sidebar in place of the thread.
//!
//! Two files of plain text. The project's is `instructions.md` in the project folder, where
//! every agent reads it, and where the agent itself adds a line when the composer asks it to
//! remember something. The one for every project is `agent/instructions.md` in the support
//! folder: the sidebar's agent gets it in its system prompt, and it stays out of the project,
//! as it is the composer's and not the piece's.
//!
//! The text saves itself while the composer types and when the editor closes. An empty text
//! removes the file, so a project without instructions has no file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
    App, BoxShadow, Context, Entity, EventEmitter, FocusHandle, Focusable, Task, Window, div, hsla,
    point, prelude::*, px,
};
use sound_ui::ActiveTheme;
use sound_ui::components::text_input::TextInput;

use crate::store::write_whole;

/// How often the text saves while the composer types.
const SAVE_EVERY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    Project,
    AllProjects,
}

impl Scope {
    /// What the editor says above the text.
    fn explanation(self) -> &'static str {
        match self {
            Scope::Project => {
                "For this project. The agent follows these, and adds to them when you ask it to \
                 remember something. Changes apply from the next thread."
            }
            Scope::AllProjects => {
                "For all your projects. The agent follows these everywhere. Changes apply from \
                 the next thread."
            }
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Scope::Project => "Keep the strings out of the intro.",
            Scope::AllProjects => "Never automate the volume of a track directly. Use a utility.",
        }
    }
}

/// A text for the file at a path, in the order they were typed.
pub(super) type Save = (PathBuf, String);

pub(super) enum InstructionsEvent {
    /// Escape: the composer is done.
    Close,
    /// The file did not read, and says why. Nothing is written over it.
    Unreadable(String),
}

impl EventEmitter<InstructionsEvent> for InstructionsEditor {}

pub(super) struct InstructionsEditor {
    scope: Scope,
    file: PathBuf,
    input: Entity<TextInput>,
    /// The text as it was read or last handed to the writer, so only a change is written.
    saved: String,
    /// Reads the file. Nothing saves before it is in, so a slow read never empties the file.
    reading: Option<Task<()>>,
    /// The writer of the sidebar, which outlives the editor and says when a write fails.
    saves: smol::channel::Sender<Save>,
    /// Waits out [`SAVE_EVERY`] after a change.
    waiting: Option<Task<()>>,
}

impl InstructionsEditor {
    /// Reads `file` in the background, then shows it with the caret at its end.
    pub(super) fn new(
        scope: Scope,
        file: PathBuf,
        saves: smol::channel::Sender<Save>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(scope.placeholder())
                .fill()
                .bare(true)
        });
        let editor = cx.weak_entity();
        input.update(cx, |input, _| {
            input.set_on_cancel(move |_, _, cx| {
                // An editor that went has nobody to close.
                editor
                    .update(cx, |_, cx| cx.emit(InstructionsEvent::Close))
                    .ok();
            });
        });
        cx.observe(&input, |editor, _, cx| editor.changed(cx))
            .detach();
        let reading = cx.spawn_in(window, {
            let file = file.clone();
            async move |editor, cx| {
                let text = cx.background_spawn(async move { read(&file) }).await;
                editor
                    .update_in(cx, |editor, window, cx| editor.read(text, window, cx))
                    .ok();
            }
        });
        Self {
            scope,
            file,
            input,
            saved: String::new(),
            reading: Some(reading),
            saves,
            waiting: None,
        }
    }

    fn read(&mut self, text: io::Result<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.reading = None;
        match text {
            Ok(text) => {
                self.saved = text.clone();
                self.input.update(cx, |input, cx| input.set_text(text, cx));
            }
            Err(error) => {
                let message = format!("{} could not be read: {error}", self.file.display());
                cx.emit(InstructionsEvent::Unreadable(message));
                return;
            }
        }
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    /// The input also notifies at every blink of its caret, so only a new text counts.
    fn changed(&mut self, cx: &mut Context<Self>) {
        if self.reading.is_some() || self.waiting.is_some() || self.text(cx) == self.saved {
            return;
        }
        let timer = cx.background_executor().timer(SAVE_EVERY);
        self.waiting = Some(cx.spawn(async move |editor, cx| {
            timer.await;
            editor
                .update(cx, |editor, cx| {
                    editor.waiting = None;
                    editor.save(cx);
                })
                .ok();
        }));
    }

    /// Hands the text to the writer now, if it changed. The sidebar calls it on close.
    pub(super) fn save(&mut self, cx: &App) {
        let text = self.text(cx);
        if self.reading.is_some() || text == self.saved {
            return;
        }
        self.saved = text.clone();
        // The writer ends only with the sidebar, which owns the receiver's task.
        self.saves.try_send((self.file.clone(), text)).ok();
    }

    fn text(&self, cx: &App) -> String {
        self.input.read(cx).text().trim_end().to_string()
    }
}

impl Focusable for InstructionsEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for InstructionsEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, fill, muted) = (theme.alpha_at(0.10), theme.gray_200, theme.gray_700);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(12.))
            .pb(px(12.))
            .child(
                div()
                    .px(px(12.))
                    .text_size(px(13.))
                    .text_color(muted)
                    .child(self.scope.explanation()),
            )
            // The box of the composer, so it reads as a place to type.
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(14.))
                    .rounded(px(20.))
                    .border_1()
                    .border_color(border)
                    .bg(fill)
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.4),
                        offset: point(px(0.), px(8.)),
                        blur_radius: px(24.),
                        spread_radius: px(-8.),
                        inset: false,
                    }])
                    .when(self.reading.is_none(), |text| {
                        text.child(self.input.clone())
                    }),
            )
    }
}

/// The text of the file, empty when there is none yet.
fn read(file: &Path) -> io::Result<String> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(text.trim_end().to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// Writes `text` as the whole file, or removes the file for an empty text.
pub(super) fn write(file: &Path, text: &str) -> Result<(), String> {
    let text = text.trim_end();
    let failed = |error: io::Error| format!("{} was not saved: {error}", file.display());
    if text.is_empty() {
        return match fs::remove_file(file) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(failed(error)),
            Ok(()) | Err(_) => Ok(()),
        };
    }
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).map_err(failed)?;
    }
    write_whole(file, &format!("{text}\n")).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_text_removes_the_file_and_a_new_one_makes_its_folder() {
        let machine = tempfile::tempdir().unwrap();
        let file = machine.path().join("agent/instructions.md");
        write(&file, "Use a utility for volume.\n\n").unwrap();
        assert_eq!(read(&file).unwrap(), "Use a utility for volume.");
        write(&file, "  \n").unwrap();
        assert!(!file.exists());
        // Already gone.
        write(&file, "").unwrap();
        assert_eq!(read(&file).unwrap(), "");
    }
}
