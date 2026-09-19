//! Select: a trigger showing the current value plus `chevron-down`, opening the
//! dropdown menu list. Ported from Hooman Studio `select.tsx`.

use gpui::{
    Context, FocusHandle, IntoElement, KeyDownEvent, MouseDownEvent, Render, SharedString, Window,
    div, prelude::*, px,
};

use crate::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem, MenuList};
use crate::components::icon::Icon;
use crate::components::popover::{Align, Side, anchor, surface, trigger};
use crate::theme::ActiveTheme;

pub struct Select {
    focus_handle: FocusHandle,
    placeholder: SharedString,
    items: Vec<MenuItem>,
    selected: Option<SharedString>,
    highlighted: usize,
    open: bool,
    side: Side,
    align: Align,
    width: f32,
}

impl Select {
    pub fn new(
        placeholder: impl Into<SharedString>,
        items: Vec<MenuItem>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            placeholder: placeholder.into(),
            items,
            selected: None,
            highlighted: usize::MAX,
            open: false,
            side: Side::default(),
            align: Align::default(),
            width: 224.,
        }
    }

    pub fn selected(mut self, value: impl Into<SharedString>) -> Self {
        self.selected = Some(value.into());
        self
    }

    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn value(&self) -> Option<&SharedString> {
        self.selected.as_ref()
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }

    fn pick(&mut self, value: SharedString, cx: &mut Context<Self>) {
        self.selected = Some(value);
        self.open = false;
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.items.len();
        if count == 0 {
            return;
        }
        let mut next = if self.highlighted == usize::MAX {
            if delta > 0 { 0 } else { count - 1 }
        } else {
            (self.highlighted as isize + delta).rem_euclid(count as isize) as usize
        };
        for _ in 0..count {
            if !self.items[next].is_disabled() {
                break;
            }
            next = (next as isize + delta).rem_euclid(count as isize) as usize;
        }
        self.highlighted = next;
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "escape" => self.close(cx),
            "down" => self.step(1, cx),
            "up" => self.step(-1, cx),
            "enter" => {
                let value = self
                    .items
                    .get(self.highlighted)
                    .map(|item| item.value.clone());
                if let Some(value) = value {
                    self.pick(value, cx);
                }
            }
            _ => {}
        }
    }

    /// Label of the current value, or the placeholder.
    fn label(&self) -> SharedString {
        self.selected
            .as_ref()
            .and_then(|value| self.items.iter().find(|item| &item.value == value))
            .map(|item| item.label())
            .unwrap_or_else(|| self.placeholder.clone())
    }
}

impl Render for Select {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (side, align, width) = (self.side, self.align, self.width);
        let theme = cx.theme();
        let (muted, placeholder_color, text) = (theme.gray_700, theme.gray_600, theme.gray_950);
        let is_placeholder = self.selected.is_none();
        let label = self.label();

        div()
            .relative()
            .flex()
            .flex_none()
            .child(
                trigger("select-trigger", cx)
                    .w(px(width))
                    .justify_between()
                    .child(
                        div()
                            .truncate()
                            .min_w_0()
                            .flex_1()
                            .text_color(if is_placeholder {
                                placeholder_color
                            } else {
                                text
                            })
                            .child(label),
                    )
                    .child(Icon::new("chevron-down").size(14.).color(muted))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.open {
                            this.close(cx);
                        } else {
                            this.open(window, cx);
                        }
                    })),
            )
            .when(self.open, |d| {
                d.child(anchor(
                    side,
                    align,
                    surface(cx)
                        .track_focus(&self.focus_handle)
                        .w(px(width))
                        .on_mouse_down_out(
                            cx.listener(|this, _: &MouseDownEvent, _, cx| this.close(cx)),
                        )
                        .on_key_down(
                            cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)),
                        )
                        .child(
                            MenuList::new(vec![MenuEntry::Group(
                                MenuGroup::new().items(self.items.clone()),
                            )])
                            .selected(self.selected.clone())
                            .highlighted(self.highlighted)
                            .on_select(
                                cx.processor(|this, value: SharedString, _, cx| {
                                    this.pick(value, cx)
                                }),
                            ),
                        ),
                ))
            })
    }
}
