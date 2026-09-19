//! Badge: a label with an optional icon and an optional remove button (Hooman Studio's badge
//! and chip merged). Variants `primary`, `subtle`, `ghost`, `outline` and solid/subtle/ghost on
//! any accent. Heights 20/28/32/40 px, pill `rounded`.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FontWeight, Hsla, SharedString, StyleRefinement, Window, div,
    prelude::*, px,
};

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
            Self::Sm => 28.,
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

    /// Outer horizontal padding; the label adds `label_pad_x` on each side.
    fn pad_x(self) -> f32 {
        match self {
            Self::Xs => 4.,
            Self::Sm => 5.,
            Self::Md => 8.,
            Self::Lg => 12.,
        }
    }

    fn label_pad_x(self) -> f32 {
        match self {
            Self::Lg => 4.,
            _ => 2.,
        }
    }

    fn gap(self) -> f32 {
        match self {
            Self::Xs => 1.,
            Self::Sm => 2.,
            Self::Md | Self::Lg => 4.,
        }
    }

    fn text_size(self) -> f32 {
        match self {
            Self::Xs => 12.,
            Self::Sm | Self::Md => 14.,
            Self::Lg => 16.,
        }
    }

    fn icon_size(self) -> f32 {
        match self {
            Self::Xs => 12.,
            Self::Sm => 14.,
            Self::Md | Self::Lg => 16.,
        }
    }

    /// Space between the remove button and the badge edge, equal on top, bottom and right.
    fn remove_inset(self) -> f32 {
        match self {
            Self::Xs | Self::Sm => 2.,
            Self::Md | Self::Lg => 4.,
        }
    }

    /// Square remove button. The 1 px border counts towards the height.
    fn remove_size(self) -> f32 {
        self.height() - 2. * (self.remove_inset() + 1.)
    }
}

type RemoveHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Badge {
    base: Div,
    label: SharedString,
    icon: Option<SharedString>,
    variant: BadgeVariant,
    size: BadgeSize,
    rounded: bool,
    remove: Option<(ElementId, RemoveHandler)>,
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
            remove: None,
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

    /// Show a remove button after the label. The id must be unique among siblings.
    pub fn on_remove(
        mut self,
        id: impl Into<ElementId>,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.remove = Some((id.into(), Rc::new(f)));
        self
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

struct Look {
    bg: Hsla,
    fg: Hsla,
    border: Hsla,
    remove_bg: Hsla,
}

fn look(variant: BadgeVariant, cx: &App) -> Look {
    let theme = cx.theme();
    let clear = Hsla::transparent_black();
    let (bg, fg, border, remove_bg) = match variant {
        BadgeVariant::Primary => (
            theme.gray_950,
            theme.gray_50,
            theme.alpha_at(0.10),
            theme.gray_50.opacity(0.10),
        ),
        BadgeVariant::Subtle => (
            theme.alpha_at(0.05),
            theme.gray_950,
            clear,
            theme.alpha_at(0.05),
        ),
        BadgeVariant::Ghost => (clear, theme.gray_950, clear, theme.alpha_at(0.05)),
        BadgeVariant::Outline => (
            clear,
            theme.gray_950,
            theme.alpha_at(0.10),
            theme.alpha_at(0.05),
        ),
        BadgeVariant::Solid(accent) => (accent, theme.gray_50, clear, theme.gray_50.opacity(0.10)),
        BadgeVariant::SubtleColor(accent) => {
            (accent.opacity(0.10), accent, clear, accent.opacity(0.10))
        }
        BadgeVariant::GhostColor(accent) => (clear, accent, clear, accent.opacity(0.10)),
    };
    Look {
        bg,
        fg,
        border,
        remove_bg,
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Look {
            bg,
            fg,
            border,
            remove_bg,
        } = look(self.variant, cx);
        let size = self.size;
        let rounded = self.rounded;
        let has_remove = self.remove.is_some();
        let remove_radius = if rounded {
            size.remove_size() / 2.
        } else {
            (size.radius() - size.remove_inset()).max(2.)
        };

        self.base
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(size.gap()))
            .h(px(size.height()))
            .pl(px(size.pad_x()))
            .pr(px(if has_remove {
                size.remove_inset()
            } else {
                size.pad_x()
            }))
            .map(|b| {
                if rounded {
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
                b.child(Icon::new(name).size(size.icon_size()))
            })
            .child(
                div()
                    .px(px(size.label_pad_x()))
                    .whitespace_nowrap()
                    .child(self.label),
            )
            .when_some(self.remove, |b, (id, f)| {
                b.child(
                    div()
                        .id(id)
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(size.remove_size()))
                        .rounded(px(remove_radius))
                        .bg(remove_bg)
                        .cursor_pointer()
                        .child(Icon::new("x").size(size.icon_size()).color(fg.opacity(0.7)))
                        .on_click(move |event, window, cx| {
                            cx.stop_propagation();
                            f(event, window, cx)
                        }),
                )
            })
    }
}
