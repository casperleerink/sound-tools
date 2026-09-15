//! Tooltip view for GPUI's `.tooltip(..)`: label text plus optional keyboard
//! shortcut chips. Ported from Hooman Studio `tooltip.tsx` (primary + outline).

use gpui::{
    AnyView, App, Context, FontWeight, Hsla, Render, SharedString, Window, div, prelude::*, px,
};

use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum TooltipVariant {
    /// Inverted surface: light background, dark text.
    #[default]
    Default,
    /// Dark background with a bright hairline border.
    Outline,
}

pub struct Tooltip {
    text: SharedString,
    keys: Vec<SharedString>,
    variant: TooltipVariant,
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            keys: Vec::new(),
            variant: TooltipVariant::default(),
        }
    }

    /// One keyboard key, drawn as a kbd chip. Call once per key.
    pub fn key(mut self, key: impl Into<SharedString>) -> Self {
        self.keys.push(key.into());
        self
    }

    pub fn variant(mut self, variant: TooltipVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Hand this to `.tooltip(move |window, cx| Tooltip::new("Play").view(cx))`.
    pub fn view(self, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }
}

fn kbd(key: SharedString, fg: Hsla, chip: Hsla) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(16.))
        .min_w(px(16.))
        .px(px(4.))
        .rounded(px(4.))
        .bg(chip)
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .child(key)
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border) = match self.variant {
            TooltipVariant::Default => (theme.gray_950, theme.gray_50, None),
            TooltipVariant::Outline => (theme.gray_50, theme.gray_950, Some(theme.alpha_at(0.70))),
        };
        // Chips read as a tint of the foreground so both variants stay legible.
        let mut chip = fg;
        chip.a = 0.12;

        // Outer padding keeps the popup off the cursor.
        div().pt(px(6.)).pl(px(6.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(28.))
                .px(px(10.))
                .rounded(px(8.))
                .bg(bg)
                .when_some(border, |d, color| d.border_1().border_color(color))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(self.text.clone())
                .children(self.keys.iter().map(|key| kbd(key.clone(), fg, chip))),
        )
    }
}
