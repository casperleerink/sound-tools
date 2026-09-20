//! Segmented control: a single-select pill group on an `alpha/5` track. Controlled — the caller
//! owns the selected value and gets an `on_change(value)`. Unselected items sit at 40% opacity and
//! come to 100% on hover.
//!
//! Tab reaches the group as one stop, and left and right select the option before or after.
//! The ring shows only when the focus came from the keyboard. The focus handle is kept in
//! element state under the id of the control.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FocusHandle, FontWeight, Hsla, Interactivity, KeyDownEvent,
    MouseButton, SharedString, StyleRefinement, Window, div, prelude::*, px,
};

use crate::focus::KeyboardFocus;
use crate::theme::ActiveTheme;

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
        let (track, selected_bg, border, hover_bg, text, ring) = (
            theme.alpha_at(0.05),
            theme.gray_50,
            theme.alpha_at(0.10),
            theme.alpha_at(0.05),
            theme.gray_950,
            theme.lavender,
        );
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
                let on_change = on_change.clone();
                // For tests, which find an option by its value. Nothing in a normal build.
                let selector = val.clone();
                div()
                    .id(("segment", ix))
                    .debug_selector(move || format!("segment-{selector}"))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(28.))
                    .px(px(10.))
                    .rounded_full()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text)
                    .when(selected, |d| {
                        d.bg(selected_bg).border_1().border_color(border)
                    })
                    .when(!selected, |d| {
                        d.border_1()
                            .border_color(gpui::Hsla::transparent_black())
                            .opacity(0.4)
                            .hover(|s| s.bg(hover_bg).opacity(1.))
                    })
                    .when(!disabled, |d| d.cursor_pointer())
                    .when_some(on_change, |d, f| {
                        d.on_click(move |_: &ClickEvent, window, cx| f(val.clone(), window, cx))
                    })
                    .child(label)
            });

        self.base
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            // The border is there for the ring. With it the padding is 4 px, as designed.
            .p(px(3.))
            .border_1()
            .border_color(if ring_shows {
                ring
            } else {
                Hsla::transparent_black()
            })
            .rounded_full()
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
