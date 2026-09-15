//! Label: a 12 px medium caption for a control. Variants `default` and `error`.

use gpui::{App, Div, FontWeight, SharedString, StyleRefinement, Window, div, prelude::*, px};

use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LabelVariant {
    #[default]
    Default,
    Error,
}

#[derive(IntoElement)]
pub struct Label {
    base: Div,
    text: SharedString,
    variant: LabelVariant,
    disabled: bool,
}

impl Label {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            text: text.into(),
            variant: LabelVariant::default(),
            disabled: false,
        }
    }

    pub fn variant(mut self, variant: LabelVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for Label {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Label {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let color = match self.variant {
            LabelVariant::Default => theme.gray_950,
            LabelVariant::Error => theme.red,
        };

        self.base
            .flex()
            .flex_none()
            .items_center()
            .px(px(4.))
            .py(px(2.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(color)
            .when(self.disabled, |d| d.opacity(0.4))
            .child(self.text)
    }
}
