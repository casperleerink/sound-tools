//! Toggle: a 24 pt button that is on or off, such as M and S of a track or `Freeze` of a reverb.
//! 28 pt wide for a letter and as wide as its word otherwise. Off it is `alpha/5` with muted
//! text. On it is white at 10 %, or its colour at 16 % with that colour as text: mute is peach
//! and solo yellow, the one place a control fills with colour.
//!
//! Controlled: the caller gives `on` and hears the new state. A click toggles it, and so do
//! enter and space on the focused toggle, through GPUI's keyboard click. In the window space
//! plays and pauses first, as it does on any focused button. The ring shows only when the
//! focus came from the keyboard. The focus handle is kept in element state under the id.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FocusHandle, FontWeight, Hsla, SharedString, StyleRefinement,
    Window, div, prelude::*, px,
};

use crate::focus::KeyboardFocus;
use crate::theme::ActiveTheme;

pub const HEIGHT: f32 = 24.;
const LETTER_WIDTH: f32 = 28.;

type ChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

struct ToggleState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
}

#[derive(IntoElement)]
pub struct Toggle {
    base: Div,
    id: ElementId,
    label: SharedString,
    on: bool,
    color: Option<Hsla>,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Self {
        Self {
            base: div(),
            id: id.into(),
            label: label.into(),
            on,
            color: None,
            disabled: false,
            on_change: None,
        }
    }

    /// The colour it has when on, for a toggle whose state is a colour meaning: peach for mute,
    /// yellow for solo. Without one it is white.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for Toggle {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| ToggleState {
            focus_handle: cx.focus_handle(),
            keyboard_focus: KeyboardFocus::default(),
        });
        let disabled = self.disabled;
        let focus_handle = state.read(cx).focus_handle.clone().tab_stop(!disabled);
        let ring_shows = state
            .read(cx)
            .keyboard_focus
            .shows_ring(&focus_handle, window);

        let theme = cx.theme();
        let (background, text) = match (self.on, self.color) {
            (false, _) => (theme.alpha_at(0.05), theme.gray_800),
            (true, None) => (theme.alpha_at(0.10), theme.gray_950),
            (true, Some(color)) => (color.opacity(0.16), color),
        };
        let ring = theme.lavender;
        let letter = self.label.chars().count() == 1;
        let on = self.on;
        let on_change = self.on_change.filter(|_| !disabled);
        // For tests, which find a toggle by its id: `toggle-<id>`. Nothing in a normal build.
        let selector = self.id.clone();

        self.base
            .id(self.id)
            .debug_selector(move || format!("toggle-{selector}"))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(HEIGHT))
            .map(|toggle| match letter {
                true => toggle.w(px(LETTER_WIDTH)),
                false => toggle.px(px(10.)),
            })
            .rounded(px(6.))
            .bg(background)
            .border_1()
            .border_color(match ring_shows {
                true => ring,
                false => Hsla::transparent_black(),
            })
            .text_size(px(12.))
            .line_height(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(text)
            .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
            .when_some(on_change, |d, on_change| {
                d.cursor_pointer()
                    .track_focus(&focus_handle)
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        state.update(cx, |state, cx| state.keyboard_focus.pressed(cx));
                    })
                    .on_click(move |_: &ClickEvent, window, cx| on_change(!on, window, cx))
            })
            .child(self.label)
    }
}
