//! Chip: a removable token. Variants `primary`, `subtle` and the same two on any accent.
//! Heights 20/24/32/40 px, pill `rounded`, optional remove button.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FontWeight, Hsla, Interactivity, SharedString,
    StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ChipVariant {
    #[default]
    Primary,
    Subtle,
    /// Solid accent fill with dark text.
    Solid(Hsla),
    /// Accent at 10% with accent text.
    SubtleColor(Hsla),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ChipSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
}

impl ChipSize {
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

    fn text_size(self) -> f32 {
        match self {
            Self::Xs | Self::Sm => 12.,
            Self::Md => 14.,
            Self::Lg => 16.,
        }
    }

    /// Size of the square remove button.
    fn close_size(self) -> f32 {
        match self {
            Self::Xs => 16.,
            Self::Sm => 18.,
            Self::Md => 24.,
            Self::Lg => 32.,
        }
    }
}

type RemoveHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Chip {
    base: Div,
    id: ElementId,
    label: SharedString,
    variant: ChipVariant,
    size: ChipSize,
    rounded: bool,
    on_remove: Option<RemoveHandler>,
}

impl Chip {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            label: label.into(),
            variant: ChipVariant::default(),
            size: ChipSize::default(),
            rounded: false,
            on_remove: None,
        }
    }

    pub fn variant(mut self, variant: ChipVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ChipSize) -> Self {
        self.size = size;
        self
    }

    /// Pill shape.
    pub fn rounded(mut self, rounded: bool) -> Self {
        self.rounded = rounded;
        self
    }

    /// Show the remove button and call this when it is clicked.
    pub fn on_remove(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(Rc::new(f));
        self
    }
}

impl Styled for Chip {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Chip {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

/// Background, foreground, hover background and remove-button background.
fn look(variant: ChipVariant, cx: &App) -> (Hsla, Hsla, Hsla, Hsla) {
    let theme = cx.theme();
    match variant {
        ChipVariant::Primary => (
            theme.gray_950,
            theme.gray_200,
            theme.gray_900,
            theme.gray_200.opacity(0.10),
        ),
        ChipVariant::Subtle => (
            theme.alpha_at(0.05),
            theme.gray_950,
            theme.alpha_at(0.10),
            theme.alpha_at(0.05),
        ),
        ChipVariant::Solid(accent) => (
            accent,
            theme.gray_200,
            accent.opacity(0.85),
            theme.gray_200.opacity(0.10),
        ),
        ChipVariant::SubtleColor(accent) => (
            accent.opacity(0.10),
            accent,
            accent.opacity(0.20),
            accent.opacity(0.10),
        ),
    }
}

impl RenderOnce for Chip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (bg, fg, hover_bg, close_bg) = look(self.variant, cx);
        let size = self.size;
        let radius = if self.rounded {
            size.height() / 2.
        } else {
            size.radius()
        };
        let close_radius = if self.rounded {
            size.close_size() / 2.
        } else {
            (size.radius() - 2.).max(2.)
        };

        self.base
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(size.height()))
            .px(px(4.))
            .rounded(px(radius))
            .bg(bg)
            .text_color(fg)
            .text_size(px(size.text_size()))
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.bg(hover_bg))
            .child(div().px(px(4.)).whitespace_nowrap().child(self.label))
            .when_some(self.on_remove, |b, f| {
                b.child(
                    div()
                        .id("remove")
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(size.close_size()))
                        .rounded(px(close_radius))
                        .bg(close_bg)
                        .cursor_pointer()
                        .child(Icon::new("x").size(14.).color(fg.opacity(0.7)))
                        .on_click(move |event, window, cx| {
                            cx.stop_propagation();
                            f(event, window, cx)
                        }),
                )
            })
    }
}
