//! Button: `primary`, `subtle`, `outline`, `ghost` plus colour variants (solid, subtle, outline,
//! ghost) taking any accent. Heights 24/28/32/40 px, optional icon, icon-only squares, pill
//! `rounded`, disabled at 40% opacity.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FocusHandle, FontWeight, Hsla, Interactivity, SharedString,
    StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Subtle,
    Outline,
    Ghost,
    /// Solid accent fill with dark text.
    Solid(Hsla),
    /// Accent at 10% with accent text.
    SubtleColor(Hsla),
    /// Accent border with accent text.
    OutlineColor(Hsla),
    /// Transparent with accent text.
    GhostColor(Hsla),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
}

impl ButtonSize {
    fn height(self) -> f32 {
        match self {
            Self::Xs => 24.,
            Self::Sm => 28.,
            Self::Md => 32.,
            Self::Lg => 40.,
        }
    }

    fn radius(self) -> f32 {
        match self {
            Self::Xs => 6.,
            Self::Sm | Self::Md => 8.,
            Self::Lg => 10.,
        }
    }

    /// Outer horizontal padding; the label adds `label_pad_x` on each side.
    fn pad_x(self) -> f32 {
        match self {
            Self::Xs => 2.,
            Self::Sm => 6.,
            Self::Md => 8.,
            Self::Lg => 12.,
        }
    }

    fn label_pad_x(self) -> f32 {
        match self {
            Self::Xs | Self::Sm => 2.,
            Self::Md | Self::Lg => 4.,
        }
    }

    fn gap(self) -> f32 {
        match self {
            Self::Xs | Self::Sm => 2.,
            Self::Md => 4.,
            Self::Lg => 8.,
        }
    }

    fn text_size(self) -> f32 {
        match self {
            Self::Lg => 16.,
            _ => 14.,
        }
    }

    /// Icons next to a label are always 16; icon-only buttons shrink to 14 at xs and sm.
    fn icon_size(self, icon_only: bool) -> f32 {
        match (self, icon_only) {
            (Self::Xs | Self::Sm, true) => 14.,
            _ => 16.,
        }
    }

    fn weight(self) -> FontWeight {
        match self {
            Self::Lg => FontWeight::SEMIBOLD,
            _ => FontWeight::MEDIUM,
        }
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    base: Div,
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<SharedString>,
    variant: ButtonVariant,
    size: ButtonSize,
    rounded: bool,
    disabled: bool,
    focus_handle: Option<FocusHandle>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            label: Some(label.into()),
            icon: None,
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            rounded: false,
            disabled: false,
            focus_handle: None,
            on_click: None,
        }
    }

    /// Square button with an icon and no label.
    pub fn icon_only(id: impl Into<ElementId>, icon: impl Into<SharedString>) -> Self {
        let mut button = Self::new(id, "");
        button.label = None;
        button.icon = Some(icon.into());
        button
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
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

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Give the button a focus handle so it can be reached by keyboard and show a focus ring.
    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

struct Look {
    bg: Hsla,
    fg: Hsla,
    border: Hsla,
    hover_bg: Hsla,
    hover_fg: Hsla,
}

/// Colours per variant, following the source design system's button variants.
fn look(variant: ButtonVariant, cx: &App) -> Look {
    let theme = cx.theme();
    let clear = Hsla::transparent_black();
    let (bg, fg, border, hover_bg, hover_fg) = match variant {
        ButtonVariant::Primary => (
            theme.gray_950,
            theme.gray_50,
            theme.gray_950,
            theme.gray_900,
            theme.gray_50,
        ),
        ButtonVariant::Subtle => (
            theme.alpha_at(0.05),
            theme.gray_950,
            clear,
            theme.alpha_at(0.10),
            theme.gray_950,
        ),
        ButtonVariant::Outline => (
            clear,
            theme.gray_950,
            theme.alpha_at(0.10),
            theme.alpha_at(0.05),
            theme.gray_950,
        ),
        ButtonVariant::Ghost => (
            clear,
            theme.gray_950,
            clear,
            theme.alpha_at(0.05),
            theme.gray_950,
        ),
        ButtonVariant::Solid(accent) => (
            accent,
            theme.gray_50,
            accent,
            accent.opacity(0.85),
            theme.gray_50,
        ),
        ButtonVariant::SubtleColor(accent) => (
            accent.opacity(0.10),
            accent,
            clear,
            accent.opacity(0.20),
            accent,
        ),
        ButtonVariant::OutlineColor(accent) => (clear, accent, accent, accent, theme.gray_50),
        ButtonVariant::GhostColor(accent) => (clear, accent, clear, accent, theme.gray_50),
    };
    Look {
        bg,
        fg,
        border,
        hover_bg,
        hover_fg,
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Look {
            bg,
            fg,
            border,
            hover_bg,
            hover_fg,
        } = look(self.variant, cx);
        let ring = cx.theme().lavender;
        let size = self.size;
        let disabled = self.disabled;
        let icon_only = self.label.is_none();
        let icon_size = size.icon_size(icon_only);

        self.base
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(size.gap()))
            .h(px(size.height()))
            .map(|b| {
                if icon_only {
                    b.w(px(size.height()))
                } else {
                    b.px(px(size.pad_x()))
                }
            })
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
            .font_weight(size.weight())
            .when(disabled, |b| b.opacity(0.4).cursor_not_allowed())
            .when(!disabled, |b| {
                b.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
                    .active(move |s| s.bg(hover_bg.opacity(0.7)).text_color(hover_fg))
            })
            .when_some(self.focus_handle.as_ref(), |b, handle| {
                b.track_focus(handle).focus(|s| s.border_color(ring))
            })
            .when_some(self.icon, |b, name| {
                b.child(Icon::new(name).size(icon_size))
            })
            .when_some(self.label, |b, label| {
                b.child(div().px(px(size.label_pad_x())).child(label))
            })
            .when_some(self.on_click.filter(|_| !disabled), |b, f| {
                b.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    f(event, window, cx)
                })
            })
    }
}
