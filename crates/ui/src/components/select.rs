//! Select: the list of the [dropdown menu](super::dropdown_menu) behind a 24 pt trigger that
//! says what is picked, as a controlled control on saved state. The owner gives the value on
//! every render and hears the value picked, as with a segmented control, so a list of many
//! selects, such as the routes of a matrix, keeps no menu per row in sync with the record.
//!
//! Only whether it is open, the keyboard highlight and the focus are kept, in element state
//! under the id. Tab reaches the trigger; enter or space opens the list, up and down move in
//! it, enter picks and escape or tab closes, and the focus goes back to the trigger.

use std::rc::Rc;

use gpui::{
    App, Div, ElementId, FocusHandle, Hsla, KeyDownEvent, MouseDownEvent, SharedString,
    StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::dropdown_menu::{MenuEntry, MenuList, flat, highlight_step, select_trigger};
use crate::components::icon::Icon;
use crate::components::popover::{Align, Side, anchor, surface};
use crate::theme::ActiveTheme;

type ChangeHandler = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

struct SelectState {
    trigger_focus: FocusHandle,
    menu_focus: FocusHandle,
    open: bool,
    highlighted: usize,
}

#[derive(IntoElement)]
pub struct Select {
    base: Div,
    id: ElementId,
    value: SharedString,
    /// What the trigger says while the value is none of the rows.
    placeholder: SharedString,
    entries: Vec<MenuEntry>,
    trigger_width: Option<f32>,
    menu_width: f32,
    on_change: Option<ChangeHandler>,
}

impl Select {
    pub fn new(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            value: value.into(),
            placeholder: SharedString::default(),
            entries: Vec::new(),
            trigger_width: None,
            menu_width: 200.,
            on_change: None,
        }
    }

    pub fn entries(mut self, entries: Vec<MenuEntry>) -> Self {
        self.entries = entries;
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Makes the trigger this wide, its label at the left and its chevron at the right. Else
    /// it is as wide as what it says.
    pub fn trigger_width(mut self, width: f32) -> Self {
        self.trigger_width = Some(width);
        self
    }

    pub fn menu_width(mut self, width: f32) -> Self {
        self.menu_width = width;
        self
    }

    /// Hears the value of a row that is picked, when it is not the value already.
    pub fn on_change(mut self, f: impl Fn(SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for Select {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// Closes the list and gives the focus back to the trigger.
fn close(state: &gpui::Entity<SelectState>, window: &mut Window, cx: &mut App) {
    let trigger = state.update(cx, |state, cx| {
        state.open = false;
        state.highlighted = usize::MAX;
        cx.notify();
        state.trigger_focus.clone()
    });
    window.focus(&trigger, cx);
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| SelectState {
            trigger_focus: cx.focus_handle().tab_stop(true),
            menu_focus: cx.focus_handle(),
            open: false,
            highlighted: usize::MAX,
        });
        let (trigger_focus, menu_focus, open, highlighted) = {
            let state = state.read(cx);
            let (trigger, menu) = (state.trigger_focus.clone(), state.menu_focus.clone());
            (trigger, menu, state.open, state.highlighted)
        };
        let label = flat(&self.entries)
            .into_iter()
            .find(|item| item.value == self.value)
            .map_or(self.placeholder.clone(), |item| item.label());
        let theme = cx.theme();
        let (muted, ring) = (theme.gray_700, theme.lavender);
        let trigger_width = self.trigger_width;
        let value = self.value.clone();
        let on_change = self.on_change;
        // Picks a value: closes the list and tells the owner when it is another value.
        let pick = {
            let state = state.clone();
            move |picked: SharedString, window: &mut Window, cx: &mut App| {
                close(&state, window, cx);
                if picked != value
                    && let Some(on_change) = &on_change
                {
                    on_change(picked, window, cx);
                }
            }
        };
        let entries = self.entries;
        let on_key = {
            let (state, entries, pick) = (state.clone(), entries.clone(), pick.clone());
            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                let highlighted = state.read(cx).highlighted;
                match event.keystroke.key.as_str() {
                    "escape" => close(&state, window, cx),
                    key @ ("down" | "up") => {
                        let delta = if key == "down" { 1 } else { -1 };
                        if let Some(next) = highlight_step(&entries, highlighted, delta) {
                            state.update(cx, |state, cx| {
                                state.highlighted = next;
                                cx.notify();
                            });
                        }
                    }
                    // Tab goes on to the next control, and the list closes behind it.
                    "tab" => return close(&state, window, cx),
                    "enter" => {
                        let row = flat(&entries)
                            .get(highlighted)
                            .filter(|item| !item.is_disabled())
                            .map(|item| item.value.clone());
                        match row {
                            Some(row) => pick(row, window, cx),
                            None => close(&state, window, cx),
                        }
                    }
                    _ => return,
                }
                // An open list keeps its keys: escape would also close what holds it.
                cx.stop_propagation();
            }
        };
        let toggle = {
            let (state, menu_focus) = (state.clone(), menu_focus.clone());
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                if state.read(cx).open {
                    return close(&state, window, cx);
                }
                state.update(cx, |state, cx| {
                    state.open = true;
                    cx.notify();
                });
                window.focus(&menu_focus, cx);
            }
        };
        let close_outside =
            move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| close(&state, window, cx);
        // For tests, which find a select by its id: `select-<id>`.
        let selector = self.id.clone();

        self.base
            .relative()
            .flex()
            .flex_none()
            .child(
                select_trigger(self.id, cx)
                    .debug_selector(move || format!("select-{selector}"))
                    .when_some(trigger_width, |trigger, width| trigger.w(px(width)))
                    .track_focus(&trigger_focus)
                    .border_1()
                    .border_color(Hsla::transparent_black())
                    .focus_visible(move |style| style.border_color(ring))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .when(trigger_width.is_some(), |label| label.flex_1())
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .child(Icon::new("chevron-down").size(12.).color(muted)),
                    )
                    .on_click(toggle),
            )
            .when(open, |select| {
                select.child(anchor(
                    Side::Bottom,
                    Align::Start,
                    surface(cx)
                        .track_focus(&menu_focus)
                        .w(px(self.menu_width))
                        .on_mouse_down_out(close_outside)
                        .on_key_down(on_key)
                        .child(
                            MenuList::new(entries)
                                .selected(Some(self.value))
                                .highlighted(highlighted)
                                .on_select(pick),
                        ),
                ))
            })
    }
}
