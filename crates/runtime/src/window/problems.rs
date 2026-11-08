//! The files that are not live: a warning sign with their count beside the project menu, for
//! as long as there are any. A click lists them, with what is wrong in each.

use std::collections::BTreeSet;

use gpui::{Context, MouseDownEvent, div, prelude::*, px};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::popover::{Align, Side, anchor, surface};
use sound_ui::names::file_name;

use super::Shell;

const LIST_WIDTH: f32 = 400.;
const LIST_HEIGHT: f32 = 360.;

impl Shell {
    pub(super) fn problems_sign(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let problems = self.session.read(cx).project().problems();
        if problems.is_empty() {
            self.problems_open = false;
            return None;
        }
        let theme = cx.theme();
        let (peach, muted) = (theme.peach, theme.gray_700);
        let agent = (self.left_panel.as_ref()).is_some_and(|panel| panel.ask.is_some());
        let files = (problems.iter())
            .map(|problem| problem.path.as_str())
            .collect::<BTreeSet<_>>()
            .len();
        let one = files == 1;
        let title = if one {
            "1 file is not live".to_string()
        } else {
            format!("{files} files are not live")
        };
        let fix = if one {
            "Ask the agent to fix it"
        } else {
            "Ask the agent to fix them"
        };
        let sign = Button::new("problems", files.to_string())
            .debug_selector(|| "problems".to_string())
            .icon("triangle-alert")
            .variant(ButtonVariant::GhostColor(peach))
            .size(ButtonSize::Sm)
            .on_click(cx.listener(|shell, _, _, cx| {
                shell.problems_open = !shell.problems_open;
                cx.notify();
            }));
        let rows = problems.into_iter().map(|problem| {
            div()
                .py(px(6.))
                .child(
                    div()
                        .text_color(muted)
                        .child(file_name(&self.session, &problem.path, cx)),
                )
                .child(problem.message)
        });
        let list = surface(cx)
            .id("problems-list")
            .w(px(LIST_WIDTH))
            .max_h(px(LIST_HEIGHT))
            .overflow_y_scroll()
            .on_mouse_down_out(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.problems_open = false;
                cx.notify();
            }))
            .p(px(12.))
            .text_size(px(14.))
            .line_height(px(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .pb(px(6.))
                    .child(title)
                    .child(if agent {
                        Button::new("problems-ask", fix)
                            .debug_selector(|| "problems-ask".to_string())
                            .variant(ButtonVariant::Subtle)
                            .size(ButtonSize::Xs)
                            .on_click(cx.listener(|shell, _, window, cx| {
                                shell.problems_open = false;
                                shell.ask_to_fix(window, cx);
                            }))
                            .into_any_element()
                    } else {
                        div().text_color(muted).child(fix).into_any_element()
                    }),
            )
            .children(rows);
        Some(
            div()
                .relative()
                .flex_none()
                .child(sign)
                .when(self.problems_open, |sign| {
                    sign.child(anchor(Side::Bottom, Align::Start, list))
                }),
        )
    }
}
