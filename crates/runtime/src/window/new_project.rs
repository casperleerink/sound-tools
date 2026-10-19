//! The templates a new project starts from, in a window of their own: "New project…" in the
//! project menu, cmd-N and the start window open it. A pick closes it and asks where the
//! project goes.

use std::rc::Rc;

use gpui::{
    App, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    size,
};
use sound_ui::{ActiveTheme, typography};

use crate::{TEMPLATES, Template};

const WIDTH: f32 = 520.;
const HEIGHT: f32 = 320.;

/// What picking a template does: where it asks for the folder and how the project opens.
pub type OnPick = Rc<dyn Fn(&'static Template, &mut App)>;

/// Opens the picker, or brings the open one forward with `on_pick` as what a pick does.
pub fn choose_template(on_pick: OnPick, cx: &mut App) {
    let open = (cx.windows().into_iter()).find_map(|window| window.downcast::<TemplatePicker>());
    if let Some(open) = open {
        let shown = open.update(cx, |picker, window, _| {
            picker.on_pick = on_pick.clone();
            window.activate_window();
        });
        if shown.is_ok() {
            return;
        }
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::centered(
            None,
            size(px(WIDTH), px(HEIGHT)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("New project".into()),
            ..Default::default()
        }),
        app_id: Some(crate::app::TOOL_NAME.into()),
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |_, cx| cx.new(|_| TemplatePicker { on_pick })) {
        eprintln!("error: the window did not open: {error:#}");
    }
}

pub struct TemplatePicker {
    on_pick: OnPick,
}

impl Render for TemplatePicker {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = TEMPLATES.iter().map(|template| {
            let on_pick = self.on_pick.clone();
            let id = SharedString::from(format!("template-{}", template.name));
            div()
                .id(id.clone())
                .debug_selector(move || id.to_string())
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(theme.gray_300)
                .bg(theme.gray_50)
                .hover(|row| row.bg(theme.gray_200))
                .cursor_pointer()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(template.name),
                )
                .child(
                    div()
                        .text_color(theme.gray_700)
                        .text_size(px(13.))
                        .child(template.description),
                )
                .on_click(move |_, window, cx| {
                    window.remove_window();
                    on_pick(template, cx);
                })
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .bg(theme.gray_100)
            .text_color(theme.gray_950)
            .font(typography::ui_font())
            .text_size(px(14.))
            .child(div().text_size(px(16.)).child("Start from"))
            .children(rows)
    }
}
