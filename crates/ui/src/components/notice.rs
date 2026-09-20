//! Notice: one quiet line for something the composer should know and need not act on, such as
//! a failed edit or files that did not load. A dot, the message, and an optional dismiss
//! button. It never blocks: float it in a corner.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, SharedString, StyleRefinement, Window, div, prelude::*, px,
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
}

impl Notice {
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            message: message.into(),
            tone: NoticeTone::default(),
            on_dismiss: None,
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

        self.base
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(32.))
            .min_w_0()
            .pl(px(12.))
            .pr(px(if self.on_dismiss.is_some() { 4. } else { 12. }))
            .rounded(px(10.))
            .bg(fill)
            .border_1()
            .border_color(border)
            .text_size(px(14.))
            .text_color(text)
            .child(
                Indicator::new("notice-dot")
                    .size(IndicatorSize::Sm)
                    .color(dot),
            )
            .child(div().min_w_0().flex_1().truncate().child(self.message))
            .when_some(self.on_dismiss, |d, on_dismiss| {
                d.child(
                    Button::icon_only("notice-dismiss", "x")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Xs)
                        .on_click(move |event, window, cx| on_dismiss(event, window, cx)),
                )
            })
    }
}
