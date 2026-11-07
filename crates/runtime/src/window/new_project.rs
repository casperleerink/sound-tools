//! The templates a new project starts from, in a window of their own: "New project…" in the
//! project menu, cmd-N and the start window open it. A pick closes it and asks where the
//! project goes. A template the composer saved can be removed here.

use std::rc::Rc;

use gpui::{
    App, Context, Div, PromptLevel, SharedString, Stateful, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};
use sound_ui::components::button::{Button, ButtonVariant};
use sound_ui::{ActiveTheme, typography};

use crate::templates::{self, Choice, Saved};

const WIDTH: f32 = 520.;
const HEIGHT: f32 = 400.;

/// What picking a template does: where it asks for the folder and how the project opens.
pub type OnPick = Rc<dyn Fn(Choice, &mut App)>;

/// Opens the picker, or brings the open one forward with `on_pick` as what a pick does.
pub fn choose_template(on_pick: OnPick, cx: &mut App) {
    let open = (cx.windows().into_iter()).find_map(|window| window.downcast::<TemplatePicker>());
    if let Some(open) = open {
        let shown = open.update(cx, |picker, window, cx| {
            picker.on_pick = on_pick.clone();
            picker.load(cx);
            window.activate_window();
        });
        if shown.is_ok() {
            return;
        }
    }
    let options = small_window("New project", WIDTH, HEIGHT, cx);
    let opened = cx.open_window(options, |_, cx| {
        cx.new(|cx| {
            let mut picker = TemplatePicker {
                on_pick,
                choices: Vec::new(),
                message: None,
            };
            picker.load(cx);
            picker
        })
    });
    if let Err(error) = opened {
        eprintln!("error: the window did not open: {error:#}");
    }
}

/// A small window of the app's own, centered, such as the picker.
pub fn small_window(title: &'static str, width: f32, height: f32, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::centered(
            None,
            size(px(width), px(height)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        app_id: Some(crate::app::TOOL_NAME.into()),
        ..Default::default()
    }
}

pub struct TemplatePicker {
    on_pick: OnPick,
    /// The templates of the app until the saved ones are read.
    choices: Vec<Choice>,
    message: Option<SharedString>,
}

impl TemplatePicker {
    /// Reads the saved templates, off the UI thread.
    fn load(&mut self, cx: &mut Context<Self>) {
        if self.choices.is_empty() {
            self.choices = (templates::TEMPLATES.iter()).map(Choice::Bundled).collect();
        }
        cx.spawn(async move |picker, cx| {
            let choices = cx.background_spawn(async { templates::choices() }).await;
            // The window is gone when this fails.
            (picker.update(cx, |picker, cx| {
                match choices {
                    Ok(choices) => picker.choices = choices,
                    Err(error) => picker.message = Some(format!("{error:#}").into()),
                }
                cx.notify();
            }))
            .ok();
        })
        .detach();
    }

    /// Asks, then removes `saved` and reads the list again.
    fn remove(&mut self, saved: Saved, window: &mut Window, cx: &mut Context<Self>) {
        let question = format!("Remove the template “{}”?", saved.name);
        let detail = "Projects made from it keep their tools.";
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(detail),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn(async move |picker, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let removed = cx.background_spawn(async move { saved.remove() }).await;
            (picker.update(cx, |picker, cx| match removed {
                Ok(()) => picker.load(cx),
                Err(error) => {
                    picker.message = Some(format!("{error:#}").into());
                    cx.notify();
                }
            }))
            .ok();
        })
        .detach();
    }
}

impl Render for TemplatePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, text, quiet) = (theme.gray_100, theme.gray_950, theme.gray_700);
        let rows = self.choices.iter().enumerate().map(|(index, choice)| {
            let on_pick = self.on_pick.clone();
            let picked = choice.clone();
            let row = template_row(choice.name(), choice.description(), cx).on_click(
                move |_, window, cx| {
                    window.remove_window();
                    on_pick(picked.clone(), cx);
                },
            );
            match choice {
                Choice::Bundled(_) => row,
                Choice::Saved(saved) => {
                    let saved = saved.clone();
                    let remove = Button::icon_only(("remove-template", index), "trash-2")
                        .variant(ButtonVariant::Ghost)
                        .on_click(cx.listener(move |picker, _, window, cx| {
                            picker.remove(saved.clone(), window, cx)
                        }));
                    (row.relative().pr_10()).child(div().absolute().top_2().right_2().child(remove))
                }
            }
        });
        div()
            .id("templates")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .bg(background)
            .text_color(text)
            .font(typography::ui_font())
            .text_size(px(14.))
            .child(div().text_size(px(16.)).child("Start from"))
            .children(rows)
            .children((self.message.clone()).map(|message| div().text_color(quiet).child(message)))
    }
}

/// A template by its name and description, for a list to pick from.
pub fn template_row(name: &str, description: &str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let id = SharedString::from(format!("template-{name}"));
    div()
        .id(id.clone())
        .debug_selector(move || id.to_string())
        .flex()
        .flex_col()
        .flex_none()
        .gap_1()
        .p_3()
        .text_left()
        .rounded_md()
        .border_1()
        .border_color(theme.gray_300)
        .bg(theme.gray_50)
        .hover(|row| row.bg(theme.gray_200))
        .cursor_pointer()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(name.to_string()),
        )
        .child(
            div()
                .text_color(theme.gray_700)
                .text_size(px(13.))
                .child(description.to_string()),
        )
}
