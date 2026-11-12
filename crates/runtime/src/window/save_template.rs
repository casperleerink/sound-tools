//! "Save as template…" in the project menu, in a window of its own: the template's name, and a
//! pick of what it keeps, which saves it. A new project then starts from it, see
//! [`super::new_project`].

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, PromptLevel, SharedString, Window, div,
    prelude::*,
};
use sound_ui::Session;
use sound_ui::components::text_input::TextInput;
use sound_ui::focus::TabStop;
use sound_ui::{ActiveTheme, typography};
use sound_ui::{TextVariant, Typography};

use super::new_project::{small_window, small_window_root, template_row};
use crate::templates::{Keep, exists_in, save_in, saved_folder};

const WIDTH: f32 = 520.;
const HEIGHT: f32 = 340.;

/// Whether each pick keeps everything, by its label and what it says of a new project.
const PICKS: [(bool, &str, &str); 2] = [
    (
        false,
        "Tools only",
        "New projects start with the tools of this one, with nothing in them.",
    ),
    (
        true,
        "Tools and content",
        "New projects start as this one is now, with its sound files.",
    ),
];

/// Opens the window for the project of `session`, or brings the open one forward.
pub(super) fn save_as_template(session: Entity<Session>, cx: &mut App) {
    let open = (cx.windows().into_iter()).find_map(|window| window.downcast::<SaveTemplate>());
    if let Some(open) = open
        && open
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = small_window("Save as template", WIDTH, HEIGHT, cx);
    let opened = cx.open_window(options, |window, cx| {
        let project = session.read(cx).project().root();
        let name = project.file_name().unwrap_or_default();
        let name = name.to_string_lossy().into_owned();
        let input = cx.new(|cx| TextInput::new(cx).placeholder("Name"));
        input.update(cx, |input, cx| {
            input.set_text(name, cx);
            input.select_all_text(cx);
            input.set_on_cancel(|_, window, _| window.remove_window());
        });
        window.focus(&input.focus_handle(cx), cx);
        cx.new(|_| SaveTemplate {
            session,
            name: input,
            saving: false,
            message: None,
        })
    });
    if let Err(error) = opened {
        eprintln!("error: the window did not open: {error:#}");
    }
}

struct SaveTemplate {
    session: Entity<Session>,
    name: Entity<TextInput>,
    saving: bool,
    /// Why the last save did not happen.
    message: Option<SharedString>,
}

impl SaveTemplate {
    /// Saves it off the UI thread, after asking when it replaces one, and closes the window.
    fn save(&mut self, everything: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let session = self.session.clone();
        let project = session.read(cx).project();
        // Before asking to replace, so a refusal comes first; the save works them out again.
        if !everything && let Err(error) = Keep::tools_of(project) {
            self.message = Some(format!("{error:#}").into());
            cx.notify();
            return;
        }
        let project = project.root().to_path_buf();
        self.saving = true;
        self.message = None;
        cx.notify();
        let name = self.name.read(cx).text().trim().to_string();
        cx.spawn_in(window, async move |this, cx| {
            let exists = cx
                .background_spawn({
                    let name = name.clone();
                    async move { exists_in(&saved_folder()?, &name) }
                })
                .await;
            let replace = match exists {
                Ok(true) => {
                    let answer = cx.update(|window, cx| {
                        let question = format!("Replace the template “{name}”?");
                        let buttons = ["Replace", "Cancel"];
                        window.prompt(PromptLevel::Warning, &question, None, &buttons, cx)
                    });
                    match answer {
                        Ok(answer) => answer.await == Ok(0),
                        // The window is gone.
                        Err(_) => return,
                    }
                }
                Ok(false) => true,
                Err(error) => return failed(&this, Some(format!("{error:#}")), cx),
            };
            if !replace {
                return failed(&this, None, cx);
            }
            // After the answer, so the records are the project's as it saves.
            let keep = match everything {
                true => Ok(Keep::Everything),
                false => (cx.update(|_, cx| Keep::tools_of(session.read(cx).project())))
                    .and_then(|keep| keep),
            };
            let keep = match keep {
                Ok(keep) => keep,
                Err(error) => return failed(&this, Some(format!("{error:#}")), cx),
            };
            let saved = cx
                .background_spawn(async move { save_in(&saved_folder()?, &project, &name, &keep) })
                .await;
            match saved {
                Ok(_) => {
                    (cx.update(|window, _| window.remove_window())).ok();
                }
                Err(error) => failed(&this, Some(format!("{error:#}")), cx),
            }
        })
        .detach();
    }
}

/// Lets the composer pick again, with why when there is a `message`. A window closed while it
/// saved leaves the message to the log.
fn failed(
    this: &gpui::WeakEntity<SaveTemplate>,
    message: Option<String>,
    cx: &mut gpui::AsyncWindowContext,
) {
    let shown = this.update(cx, |this, cx| {
        this.saving = false;
        this.message = message.clone().map(SharedString::from);
        cx.notify();
    });
    if let (Err(_), Some(message)) = (shown, message) {
        eprintln!("error: {message}");
    }
}

impl Render for SaveTemplate {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, text, quiet) = (theme.background(), theme.text(), theme.text_muted());
        let saving = self.saving;
        let picks = PICKS.map(|(everything, label, description)| {
            let row = template_row(label, description, cx);
            let id = ("keep", usize::from(everything));
            match saving {
                true => TabStop::new(id, row.opacity(0.5).cursor_default()),
                false => TabStop::new(id, row).on_click(
                    cx.listener(move |this, _, window, cx| this.save(everything, window, cx)),
                ),
            }
        });
        let message = match saving {
            true => Some("Saving…".into()),
            false => self.message.clone(),
        };
        small_window_root()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .bg(background)
            .text_color(text)
            .font(typography::ui_font())
            .text_variant(TextVariant::Body)
            .child(
                div()
                    .text_variant(TextVariant::Title)
                    .child("Save as template"),
            )
            .child(self.name.clone())
            .children(picks)
            .children(message.map(|message: SharedString| div().text_color(quiet).child(message)))
    }
}
