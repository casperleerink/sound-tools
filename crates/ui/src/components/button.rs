//! Button: `primary`, `subtle`, `ghost` plus colour variants (solid, subtle, ghost) taking any
//! accent, with an optional icon, icon-only squares and a pill shape.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FocusHandle, FontWeight, Hsla, Interactivity, SharedString,
    StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::focus::KeyboardFocus;
use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Subtle,
    Ghost,
    /// Solid accent fill with dark text.
    Solid(Hsla),
    /// Accent at 10% with accent text.
    SubtleColor(Hsla),
    /// Transparent with accent text.
    GhostColor(Hsla),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonSize {
    Xs,
    Sm,
    #[default]
    Md,
}

impl ButtonSize {
    pub fn height(self) -> f32 {
        match self {
            Self::Xs => 24.,
            Self::Sm => 28.,
            Self::Md => 32.,
        }
    }

    fn radius(self) -> f32 {
        match self {
            Self::Xs => 6.,
            Self::Sm | Self::Md => 8.,
        }
    }

    /// Outer horizontal padding; the label adds `label_pad_x` on each side.
    fn pad_x(self) -> f32 {
        match self {
            Self::Xs => 2.,
            Self::Sm => 6.,
            Self::Md => 8.,
        }
    }

    fn label_pad_x(self) -> f32 {
        match self {
            Self::Xs | Self::Sm => 2.,
            Self::Md => 4.,
        }
    }

    fn gap(self) -> f32 {
        match self {
            Self::Xs | Self::Sm => 2.,
            Self::Md => 4.,
        }
    }

    /// Icons next to a label are always 16; icon-only buttons shrink to 14 at xs and sm.
    fn icon_size(self, icon_only: bool) -> f32 {
        match (self, icon_only) {
            (Self::Xs | Self::Sm, true) => 14.,
            _ => 16.,
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
    tab_stop: bool,
    on_click: Option<ClickHandler>,
}

/// Kept under the button's id: its own focus handle, for [`Button::tab_stop`], and whether a
/// focus came from the keyboard.
struct ButtonState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
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
            tab_stop: false,
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

    /// Icon name, drawn before the label.
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

    /// A tab stop with a focus handle of its own, for a button whose caller keeps none.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let own = self.tab_stop && !self.disabled;
        let focus = (self.focus_handle.is_some() || own).then(|| {
            let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| ButtonState {
                focus_handle: cx.focus_handle().tab_stop(true),
                keyboard_focus: KeyboardFocus::default(),
            });
            let handle =
                (self.focus_handle.clone()).unwrap_or_else(|| state.read(cx).focus_handle.clone());
            (handle, state)
        });
        let ring_shows = (focus.as_ref()).is_some_and(|(handle, state)| {
            state.read(cx).keyboard_focus.shows_ring(handle, window)
        });
        let Look {
            bg,
            fg,
            border,
            hover_bg,
            hover_fg,
        } = look(self.variant, cx);
        // A solid lavender button has the ring's colour already, so its focus is bright.
        let ring = if border == cx.theme().lavender {
            cx.theme().gray_950
        } else {
            cx.theme().lavender
        };
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
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .when(disabled, |b| b.opacity(0.4).cursor_not_allowed())
            .when(!disabled, |b| {
                b.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
                    .active(move |s| s.bg(hover_bg.opacity(0.7)).text_color(hover_fg))
            })
            .when_some(focus, |b, (handle, state)| {
                b.track_focus(&handle)
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        state.update(cx, |state, cx| state.keyboard_focus.pressed(cx));
                    })
            })
            .when(ring_shows, |b| b.border_color(ring))
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
