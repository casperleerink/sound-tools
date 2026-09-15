//! Badge: a static label with an optional icon. Variants `primary`, `subtle`, `ghost`, `outline`
//! and the same solid/subtle/ghost styles on any accent. Heights 20/24/32/40 px, pill `rounded`.

use gpui::{App, Div, FontWeight, Hsla, SharedString, StyleRefinement, Window, div, prelude::*, px};

use crate::components::icon::Icon;
use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum BadgeVariant {
    #[default]
    Primary,
    Subtle,
    Ghost,
    Outline,
    /// Solid accent fill with dark text.
    Solid(Hsla),
    /// Accent at 10% with accent text.
    SubtleColor(Hsla),
    /// Transparent with accent text.
    GhostColor(Hsla),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BadgeSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
}

impl BadgeSize {
    fn height(self) -> f32 {
        match self {
            Self::Xs => 20.,
            Self::Sm => 24.,
            Self::Md => 32.,
            Self::Lg => 40.,
        }
    }

    fn radius(self) -> f32 {
        match self {
            Self::Xs => 4.,
            Self::Sm => 6.,
            Self::Md => 8.,
            Self::Lg => 10.,
        }
    }

    fn pad_x(self) -> f32 {
        match self {
            Self::Xs => 6.,
            Self::Sm => 8.,
            Self::Md => 10.,
            Self::Lg => 12.,
        }
    }

    fn text_size(self) -> f32 {
        match self {
            Self::Xs | Self::Sm => 12.,
            Self::Md => 14.,
            Self::Lg => 16.,
        }
    }

    fn icon_size(self) -> f32 {
        match self {
            Self::Xs => 12.,
            Self::Sm => 14.,
            _ => 16.,
        }
    }
}

#[derive(IntoElement)]
pub struct Badge {
    base: Div,
    label: SharedString,
    icon: Option<SharedString>,
    variant: BadgeVariant,
    size: BadgeSize,
    rounded: bool,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            label: label.into(),
            icon: None,
            variant: BadgeVariant::default(),
            size: BadgeSize::default(),
            rounded: false,
        }
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: BadgeSize) -> Self {
        self.size = size;
        self
    }

    /// Lucide icon name, drawn before the label.
    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Pill shape.
    pub fn rounded(mut self, rounded: bool) -> Self {
        self.rounded = rounded;
        self
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// Background, foreground and border for a variant.
fn look(variant: BadgeVariant, cx: &App) -> (Hsla, Hsla, Hsla) {
    let theme = cx.theme();
    let clear = Hsla::transparent_black();
    match variant {
        BadgeVariant::Primary => (theme.gray_950, theme.gray_200, theme.alpha_at(0.10)),
        BadgeVariant::Subtle => (theme.alpha_at(0.05), theme.gray_950, clear),
        BadgeVariant::Ghost => (clear, theme.gray_950, clear),
        BadgeVariant::Outline => (theme.gray_50, theme.gray_950, theme.alpha_at(0.10)),
        BadgeVariant::Solid(accent) => (accent, theme.gray_200, clear),
        BadgeVariant::SubtleColor(accent) => (accent.opacity(0.10), accent, clear),
        BadgeVariant::GhostColor(accent) => (clear, accent, clear),
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (bg, fg, border) = look(self.variant, cx);
        let size = self.size;

        self.base
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .h(px(size.height()))
            .px(px(size.pad_x()))
            .map(|b| {
                if self.rounded {
                    b.rounded_full()
                } else {
                    b.rounded(px(size.radius()))
                }
            })
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .text_size(px(size.text_size()))
            .font_weight(FontWeight::MEDIUM)
            .when_some(self.icon, |b, name| {
                b.child(Icon::new(name).size(size.icon_size()).color(fg))
            })
            .child(div().whitespace_nowrap().child(self.label))
    }
}
