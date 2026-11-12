//! Segmented control: one choice of a few. It sits on the knob line of its cell, above the drawing of a
//! display, or in the header of a card as its pages. A marked option has a dot after its label:
//! a page with something on it worth a look. Controlled: the caller owns the selected value and
//! gets an `on_change(value)`.
//!
//! Tab reaches the group as one stop, and left and right select the option before or after.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FocusHandle, FontWeight, Hsla, Interactivity, KeyDownEvent,
    MouseButton, SharedString, StyleRefinement, Window, div, prelude::*, px,
};

use crate::focus::KeyboardFocus;
use crate::scale::ControlSize;
use crate::theme::ActiveTheme;
use crate::typography::{TextVariant, Typography};

/// The track's padding and its border, above and below a segment.
const INSET: f32 = 2.;

type ChangeHandler = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

struct SegmentedControlState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
}

#[derive(IntoElement)]
pub struct SegmentedControl {
    base: Div,
    id: ElementId,
    options: Vec<(SharedString, SharedString)>,
    value: SharedString,
    marked: Vec<SharedString>,
    size: ControlSize,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl SegmentedControl {
    pub fn new(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            options: Vec::new(),
            value: value.into(),
            marked: Vec::new(),
            size: ControlSize::Sm,
            disabled: false,
            on_change: None,
        }
    }

    /// Each option is `(value, label)`.
    pub fn options(
        mut self,
        options: impl IntoIterator<Item = (impl Into<SharedString>, impl Into<SharedString>)>,
    ) -> Self {
        self.options = options
            .into_iter()
            .map(|(value, label)| (value.into(), label.into()))
            .collect();
        self
    }

    /// The values whose option carries a dot.
    pub fn marked(mut self, values: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.marked = values.into_iter().map(Into::into).collect();
        self
    }

    /// 24 pt unless told, as the controls of a card are.
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for SegmentedControl {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for SegmentedControl {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| SegmentedControlState {
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
        let (track, selected_bg, text, muted, ring) = (
            theme.fill(),
            theme.fill_strong(),
            theme.text(),
            theme.text_secondary(),
            theme.lavender,
        );
        let size = self.size;
        let value = self.value.clone();
        let on_change = self.on_change.filter(|_| !disabled);
        let on_key_down = on_change.clone().map(|on_change| {
            let values: Vec<SharedString> = self
                .options
                .iter()
                .map(|(value, _)| value.clone())
                .collect();
            let selected = values.iter().position(|option| *option == value);
            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                let modifiers = event.keystroke.modifiers;
                if modifiers.modified() {
                    return;
                }
                let next = match (event.keystroke.key.as_str(), selected) {
                    ("left", Some(selected)) => selected.checked_sub(1),
                    ("right", Some(selected)) => Some(selected + 1),
                    ("left" | "right", None) => Some(0),
                    _ => return,
                };
                cx.stop_propagation();
                if let Some(next) = next.and_then(|next| values.get(next)) {
                    on_change(next.clone(), window, cx);
                }
            }
        });

        let items = self
            .options
            .into_iter()
            .enumerate()
            .map(|(ix, (val, label))| {
                let selected = val == value;
                let marked = self.marked.contains(&val);
                let on_change = on_change.clone();
                // For tests, which find an option by its value. Nothing in a normal build.
                let selector = val.clone();
                div()
                    .id(("segment", ix))
                    .debug_selector(move || format!("segment-{selector}"))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(size.height() - 2. * INSET))
                    .px(px(8.))
                    .rounded(size.inner_radius())
                    .text_variant(TextVariant::Label)
                    .line_height(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .when(selected, |d| d.bg(selected_bg).text_color(text))
                    .when(!selected, |d| {
                        d.text_color(muted).hover(move |s| s.text_color(text))
                    })
                    .when(!disabled, |d| d.cursor_pointer())
                    .when_some(on_change, |d, f| {
                        d.on_click(move |_: &ClickEvent, window, cx| f(val.clone(), window, cx))
                    })
                    .child(label)
                    .when(marked, |d| {
                        let dot = if selected { text } else { muted };
                        d.gap(px(4.))
                            .child(div().size(px(4.)).rounded_full().bg(dot))
                    })
            });

        self.base
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            // The border is there for the ring: with the padding it is `INSET` on each side.
            .p(px(1.))
            .border_1()
            .border_color(if ring_shows {
                ring
            } else {
                Hsla::transparent_black()
            })
            .rounded(size.radius())
            .bg(track)
            .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
            .when_some(on_key_down, |d, on_key_down| {
                d.track_focus(&focus_handle)
                    .on_key_down(on_key_down)
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        state.update(cx, |state, cx| state.keyboard_focus.pressed(cx));
                    })
            })
            .children(items)
    }
}
