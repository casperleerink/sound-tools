//! After the composer went back in the history: a line above the composer that says where to,
//! and a note the agent reads with the next message, so it knows the files changed under it.

use gpui::{AnyElement, Context, div, prelude::*, px};
use sound_core::{Line, PointId};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};

use super::Sidebar;

pub(super) struct WentBack {
    title: String,
    note: String,
}

impl Sidebar {
    /// A newer one replaces the last.
    pub(super) fn went_back(&mut self, point: PointId, cx: &mut Context<Self>) {
        let Some(history) = self.session.read(cx).project().history() else {
            return;
        };
        let title = match history.line_of(point) {
            Ok(line) => line.map(Line::title),
            Err(error) => {
                eprintln!("the version of point {point:?} is unreadable: {error}");
                None
            }
        };
        let Some(title) = title else {
            return;
        };
        let folder = history.folder().display();
        let note = format!(
            "Before this message the composer went back in the project history to point \
             {short} of the version \"{title}\". The project files hold that state now, not \
             what you last saw. The history is a git repository at {folder}; \
             `git --git-dir \"{folder}\" log --all --graph` shows every point.",
            short = point.short(),
        );
        self.went_back = Some(WentBack { title, note });
        cx.notify();
    }

    /// The note for the message that goes now. The line goes with it.
    pub(super) fn take_went_back_note(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let went_back = self.went_back.take()?;
        cx.notify();
        Some(went_back.note)
    }

    pub(super) fn went_back_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let went_back = self.went_back.as_ref()?;
        let muted = cx.theme().gray_700;
        let line = div()
            .debug_selector(|| "agent-went-back".to_string())
            .flex()
            .items_center()
            .gap(px(4.))
            .pl(px(14.))
            .pr(px(6.))
            .text_size(px(12.))
            .text_color(muted)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(format!("Went back to “{}”", went_back.title)),
            )
            .child(
                Button::icon_only("went-back-dismiss", "x")
                    .debug_selector(|| "agent-went-back-dismiss".to_string())
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Xs)
                    .focus_handle(&self.went_back_dismiss)
                    .on_click(cx.listener(|sidebar, _, _, cx| {
                        sidebar.went_back = None;
                        cx.notify();
                    })),
            );
        Some(line.into_any_element())
    }
}
