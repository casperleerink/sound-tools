//! Checkbox: unchecked, checked and indeterminate, sizes 20/24/28 px. Controlled — the caller owns
//! `checked` and gets an `on_change(bool)`. Give it a `FocusHandle` to make it keyboard reachable;
//! space and enter then toggle it.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FocusHandle, Hsla, Interactivity, KeyDownEvent,
    StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CheckboxSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl CheckboxSize {
    fn box_size(self) -> f32 {
        match self {
            Self::Sm => 20.,
            Self::Md => 24.,
            Self::Lg => 28.,
        }
    }

    fn radius(self) -> f32 {
        match self {
            Self::Sm => 6.,
            Self::Md | Self::Lg => 8.,
        }
    }

    fn icon_size(self) -> f32 {
        match self {
            Self::Sm => 14.,
            Self::Md => 16.,
            Self::Lg => 18.,
        }
    }
}

type ChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Checkbox {
    base: Div,
    id: ElementId,
    checked: bool,
    indeterminate: bool,
    size: CheckboxSize,
    disabled: bool,
    focus_handle: Option<FocusHandle>,
    on_change: Option<ChangeHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            base: div(),
            id: id.into(),
            checked,
            indeterminate: false,
            size: CheckboxSize::default(),
            disabled: false,
            focus_handle: None,
            on_change: None,
        }
    }

    /// Dash instead of a tick; reads as "some of the children are checked".
    pub fn indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = indeterminate;
        self
    }

    pub fn size(mut self, size: CheckboxSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Checkbox {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (surface, hover_bg, border, accent, on_accent, glyph) = (
            theme.alpha_at(0.05),
            theme.alpha_at(0.10),
            theme.alpha_at(0.10),
            theme.blue,
            theme.gray_200,
            theme.gray_950,
        );
        let ring = theme.blue;
        let size = self.size;
        let disabled = self.disabled;
        let filled = self.checked && !self.indeterminate;
        let next = !(self.checked || self.indeterminate);
        let on_change = self.on_change.filter(|_| !disabled);
        let (bg, fg, border): (Hsla, Hsla, Hsla) = if filled {
            (accent, on_accent, accent)
        } else {
            (surface, glyph, border)
        };

        self.base
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(size.box_size()))
            .rounded(px(size.radius()))
            .bg(bg)
            .border_1()
            .border_color(border)
            .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
            .when(!disabled, |d| {
                d.cursor_pointer()
                    .when(!filled, |d| d.hover(|s| s.bg(hover_bg)))
                    .when(filled, |d| d.hover(|s| s.bg(accent.opacity(0.85))))
            })
            .when_some(self.focus_handle.as_ref(), |d, handle| {
                d.track_focus(handle)
                    .focus(|s| s.border_color(ring.opacity(0.7)))
            })
            .when(self.checked || self.indeterminate, |d| {
                d.child(
                    Icon::new(if self.indeterminate { "minus" } else { "check" })
                        .size(size.icon_size())
                        .color(fg),
                )
            })
            .when_some(on_change, |d, f| {
                let key_f = f.clone();
                d.on_click(move |_: &ClickEvent, window, cx| f(next, window, cx))
                    .on_key_down(move |ev: &KeyDownEvent, window, cx| {
                        if matches!(ev.keystroke.key.as_str(), "space" | "enter") {
                            key_f(next, window, cx);
                        }
                    })
            })
    }
}
