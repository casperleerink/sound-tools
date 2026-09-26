//! Notice: a quiet line for something the composer should know and need not act on, such as
//! a failed edit or files that did not load. A dot, the message, and an optional dismiss
//! button. A long message wraps to at most three lines. It never blocks: float it in a corner.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FocusHandle, SharedString, StyleRefinement, Window, div,
    prelude::*, px,
};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::indicator::{Indicator, IndicatorSize};
use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NoticeTone {
    /// Red dot.
    #[default]
    Error,
    /// Peach dot.
    Warning,
}

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Notice {
    base: Div,
    id: ElementId,
    message: SharedString,
    tone: NoticeTone,
    on_dismiss: Option<Handler>,
    dismiss_focus: Option<FocusHandle>,
}

impl Notice {
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            message: message.into(),
            tone: NoticeTone::default(),
            on_dismiss: None,
            dismiss_focus: None,
        }
    }

    pub fn tone(mut self, tone: NoticeTone) -> Self {
        self.tone = tone;
        self
    }

    /// Adds a dismiss button.
    pub fn on_dismiss(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(f));
        self
    }
}

impl Notice {
    /// Gives the dismiss button a focus handle, so Tab reaches it and it shows a focus ring.
    pub fn dismiss_focus(mut self, handle: &FocusHandle) -> Self {
        self.dismiss_focus = Some(handle.clone());
        self
    }
}

impl Styled for Notice {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Notice {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let dot = match self.tone {
            NoticeTone::Error => theme.red,
            NoticeTone::Warning => theme.peach,
        };
        let (fill, border, text) = (
            theme.gray_200.blend(theme.alpha_at(0.06)),
            theme.alpha_at(0.10),
            theme.gray_900,
        );

        // For tests, which find a notice by its id: `notice-<id>`. Nothing in a normal build.
        let selector = self.id.clone();
        self.base
            .id(self.id)
            .debug_selector(move || format!("notice-{selector}"))
            .flex()
            .items_start()
            .gap(px(8.))
            .min_w_0()
            .py(px(5.))
            .pl(px(12.))
            .pr(px(if self.on_dismiss.is_some() { 4. } else { 12. }))
            .rounded(px(10.))
            .bg(fill)
            .border_1()
            .border_color(border)
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(text)
            // The dot sits on the first line, whatever the number of lines.
            .child(
                div().h(px(20.)).flex().items_center().child(
                    Indicator::new("notice-dot")
                        .size(IndicatorSize::Sm)
                        .color(dot),
                ),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .line_clamp(3)
                    .text_ellipsis()
                    .child(self.message),
            )
            .when_some(self.on_dismiss, |d, on_dismiss| {
                let button = Button::icon_only("notice-dismiss", "x")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Xs)
                    .mt(px(-2.))
                    .on_click(move |event, window, cx| on_dismiss(event, window, cx));
                d.child(match &self.dismiss_focus {
                    Some(handle) => button.focus_handle(handle),
                    None => button,
                })
            })
    }
}
