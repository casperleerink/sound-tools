//! Toggle: a 24 pt button that is on or off, such as M and S of a track or `Freeze` of a reverb.
//! 28 pt wide for a letter or a glyph and as wide as its word otherwise. The arm toggle of an
//! audio track is a 10 pt circle in a 24 pt square, the size of an icon of a card header, hollow
//! while off and filled while on ([`Toggle::dot`]). The automation toggle of a track is an icon
//! in the same square ([`Toggle::icon`]), wider when it holds a count. Off it is `alpha/5` with
//! muted text. On it is white at 10 %, or its colour at 16 % with that colour as text: mute is
//! peach and solo yellow, the one place a control fills with colour.
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

use crate::components::icon::Icon;
use crate::components::tooltip::Tooltip;
use crate::focus::KeyboardFocus;
use crate::theme::ActiveTheme;

pub const HEIGHT: f32 = 24.;
pub const LETTER_WIDTH: f32 = 28.;
/// The circle of [`Toggle::dot`].
const DOT: f32 = 10.;
/// The glyph of [`Toggle::icon`].
const GLYPH: f32 = 14.;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

struct ToggleState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
}

/// What a toggle shows.
enum Face {
    Label(SharedString),
    Dot,
    /// An icon of `crates/ui/assets/icons`, by name. It shows no words, so give it a
    /// [`Toggle::tooltip`].
    Icon {
        icon: SharedString,
        /// A number after the icon, such as how many lanes a track has. None at 0.
        count: usize,
    },
}

#[derive(IntoElement)]
pub struct Toggle {
    base: Div,
    id: ElementId,
    face: Face,
    on: bool,
    color: Option<Hsla>,
    disabled: bool,
    tab_stop: bool,
    tooltip: Option<Tooltip>,
    on_click: Option<ClickHandler>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Self {
        Self {
            base: div(),
            id: id.into(),
            face: Face::Label(label.into()),
            on,
            color: None,
            disabled: false,
            tab_stop: true,
            tooltip: None,
            on_click: None,
        }
    }

    /// A toggle whose face is a circle, hollow while off and filled while on: the arm toggle
    /// of an audio track, red when on.
    pub fn dot(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            face: Face::Dot,
            ..Self::new(id, "", on)
        }
    }

    /// A square toggle whose face is an icon, such as the one that shows the automation lanes
    /// of a track.
    pub fn icon(id: impl Into<ElementId>, icon: impl Into<SharedString>, on: bool) -> Self {
        Self {
            face: Face::Icon {
                icon: icon.into(),
                count: 0,
            },
            ..Self::new(id, "", on)
        }
    }

    /// A number after the icon of [`Toggle::icon`], inside the button. 0 shows none.
    pub fn count(mut self, count: usize) -> Self {
        if let Face::Icon { count: shown, .. } = &mut self.face {
            *shown = count;
        }
        self
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

    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    /// Leaves it out of the tab order, for a toggle that a key of its view already reaches.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn on_change(self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        let on = self.on;
        self.on_click(move |_, window, cx| f(!on, window, cx))
    }

    /// As [`Toggle::on_change`], for a caller that needs the click, such as its modifiers.
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
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
        let focus_handle = state
            .read(cx)
            .focus_handle
            .clone()
            .tab_stop(self.tab_stop && !disabled);
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
        let width = match &self.face {
            Face::Icon { count, .. } if *count > 0 => None,
            Face::Dot | Face::Icon { .. } => Some(HEIGHT),
            Face::Label(label) if label.chars().count() == 1 => Some(LETTER_WIDTH),
            Face::Label(_) => None,
        };
        let on = self.on;
        let on_click = self.on_click.filter(|_| !disabled);
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
            .map(|toggle| match width {
                Some(width) => toggle.w(px(width)),
                None if matches!(self.face, Face::Icon { .. }) => {
                    toggle.pl(px((HEIGHT - GLYPH) / 2.)).pr(px(8.)).gap(px(4.))
                }
                None => toggle.px(px(10.)),
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
            .when_some(on_click, |d, on_click| {
                d.cursor_pointer()
                    .track_focus(&focus_handle)
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        state.update(cx, |state, cx| state.keyboard_focus.pressed(cx));
                    })
                    .on_click(move |event, window, cx| on_click(event, window, cx))
            })
            .when_some(self.tooltip, |toggle, tooltip| {
                toggle.tooltip(move |_, cx| tooltip.clone().view(cx))
            })
            .map(|toggle| match self.face {
                Face::Label(label) => toggle.child(label),
                Face::Icon { icon, count } => toggle
                    .child(Icon::new(icon).size(GLYPH).color(text))
                    .when(count > 0, |toggle| toggle.child(count.to_string())),
                Face::Dot => {
                    let dot = div().size(px(DOT)).rounded_full();
                    toggle.child(match on {
                        true => dot.bg(text),
                        false => dot.border_1().border_color(text),
                    })
                }
            })
    }
}
