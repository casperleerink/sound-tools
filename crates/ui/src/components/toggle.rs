//! Toggle: a button that is on or off, such as M and S of a track or `Freeze` of a reverb. Its
//! face is a word, a circle ([`Toggle::dot`]) or an icon ([`Toggle::icon`]). The word is its
//! name, never `On` or `Off`. Its state reads by shape, not by tone alone: off it is hollow, an
//! outline with a quiet face, and on it is filled with a tint of its colour under a bright
//! face. The colour is white, or one that means something ([`Toggle::color`]): mute is peach
//! and solo yellow, the one place a control fills with colour.
//!
//! Controlled: the caller gives `on` and hears the new state. A click toggles it, and so do
//! enter and space on the focused toggle, through GPUI's keyboard click. In the window space
//! plays and pauses first, as it does on any focused button.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FocusHandle, FontWeight, Hsla, SharedString, StyleRefinement,
    Window, div, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::components::tooltip::Tooltip;
use crate::focus::KeyboardFocus;
use crate::scale::ControlSize;
use crate::theme::ActiveTheme;
use crate::typography::{TextVariant, Typography};

/// How much wider than it is tall a toggle of one letter is.
const LETTER_EXTRA: f32 = 4.;
/// The circle of [`Toggle::dot`].
const DOT: f32 = 10.;
/// The glyph of [`Toggle::icon`].
const GLYPH: f32 = 14.;
/// How much of its colour fills a toggle that is on.
const TINT: f32 = 0.16;
/// From the sides of a toggle of a set width to its word.
const WORD_PADDING: f32 = 4.;

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
    size: ControlSize,
    width: Option<f32>,
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
            size: ControlSize::Sm,
            width: None,
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

    /// 24 pt unless told, as the controls of a card are.
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// Makes a toggle of a word this wide whatever it says, so the switches of a card line up:
    /// a word too long for it ends in an ellipsis. Else it is as wide as its word.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
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
        let (background, outline, text) = match self.on {
            false => (
                Hsla::transparent_black(),
                theme.border_strong(),
                theme.text_secondary(),
            ),
            true => {
                let color = self.color.unwrap_or(theme.text());
                (color.opacity(TINT), Hsla::transparent_black(), color)
            }
        };
        let ring = theme.lavender;
        let set_width = self.width.is_some();
        let height = self.size.height();
        let width = match &self.face {
            Face::Icon { count, .. } if *count > 0 => None,
            Face::Dot | Face::Icon { .. } => Some(height),
            Face::Label(label) if label.chars().count() == 1 => Some(height + LETTER_EXTRA),
            Face::Label(_) => self.width,
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
            .h(px(height))
            .map(|toggle| match width {
                Some(width) => toggle.w(px(width)),
                None if matches!(self.face, Face::Icon { .. }) => {
                    toggle.pl(px((height - GLYPH) / 2.)).pr(px(8.)).gap(px(4.))
                }
                None => toggle.px(px(10.)),
            })
            .rounded(self.size.radius())
            .bg(background)
            .border_1()
            .border_color(match ring_shows {
                true => ring,
                false => outline,
            })
            .text_variant(TextVariant::Label)
            .line_height(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
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
                Face::Label(label) => {
                    let word = div().min_w_0().truncate().child(label);
                    toggle.child(word.when(set_width, |word| word.px(px(WORD_PADDING))))
                }
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
