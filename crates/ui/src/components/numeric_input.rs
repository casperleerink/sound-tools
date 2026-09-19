//! Numeric field: drag vertically to change, double-click to type. Editing reuses `TextInput`;
//! enter commits, escape cancels. Stateful, so create it with `cx.new(|cx| NumericInput::new(cx))`.
//! Up/down arrows nudge by one step, shift-arrow by ten.

use std::rc::Rc;

use gpui::{
    App, Context, CursorStyle, DragMoveEvent, Entity, FocusHandle, Focusable, KeyDownEvent,
    MouseButton, MouseDownEvent, Render, SharedString, Window, div, prelude::*, px,
};

use crate::components::text_input::{InputSize, TextInput};
use crate::theme::ActiveTheme;
use crate::typography;

/// Marker for gpui's drag machinery; the preview below renders nothing.
struct NumberDrag;

struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

type ChangeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

pub struct NumericInput {
    focus_handle: FocusHandle,
    input: Entity<TextInput>,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    decimals: usize,
    width: f32,
    unit: SharedString,
    disabled: bool,
    editing: bool,
    drag_start: Option<(f32, f32)>,
    on_change: Option<ChangeHandler>,
}

impl NumericInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(cx).size(InputSize::Sm));
        let this = cx.weak_entity();
        input.update(cx, |input, _| {
            let commit = this.clone();
            input.set_on_submit(move |text, window, cx| {
                let parsed = text.trim().parse::<f32>().ok();
                commit
                    .update(cx, |this, cx| this.stop_editing(parsed, window, cx))
                    .ok();
            });
            input.set_on_cancel(move |_, window, cx| {
                this.update(cx, |this, cx| this.stop_editing(None, window, cx))
                    .ok();
            });
        });
        Self {
            focus_handle: cx.focus_handle(),
            input,
            value: 0.,
            min: f32::MIN,
            max: f32::MAX,
            step: 1.,
            decimals: 0,
            width: 72.,
            unit: SharedString::default(),
            disabled: false,
            editing: false,
            drag_start: None,
            on_change: None,
        }
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// Value change per pixel dragged, and per arrow press.
    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = value.clamp(self.min, self.max);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn unit(mut self, unit: impl Into<SharedString>) -> Self {
        self.unit = unit.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn current(&self) -> f32 {
        self.value
    }

    fn set_value(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        let value = value.clamp(self.min, self.max);
        if (value - self.value).abs() < f32::EPSILON {
            return;
        }
        self.value = value;
        if let Some(f) = self.on_change.clone() {
            f(value, window, cx);
        }
        cx.notify();
    }

    fn start_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = true;
        let text = format!("{:.*}", self.decimals, self.value);
        let handle = self.input.read(cx).focus_handle(cx);
        self.input.update(cx, |input, cx| {
            input.set_text(text, cx);
            input.select_all_text(cx);
        });
        window.focus(&handle, cx);
        cx.notify();
    }

    fn stop_editing(&mut self, value: Option<f32>, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = false;
        if let Some(value) = value {
            self.set_value(value, window, cx);
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.click_count >= 2 {
            self.start_editing(window, cx);
            return;
        }
        window.focus(&self.focus_handle, cx);
        self.drag_start = Some((f32::from(ev.position.y), self.value));
        cx.notify();
    }

    fn on_drag_move(
        &mut self,
        ev: &DragMoveEvent<NumberDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((start_y, start_value)) = self.drag_start else {
            return;
        };
        let delta = start_y - f32::from(ev.event.position.y);
        self.set_value(start_value + delta * self.step, window, cx);
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let multiplier = if ev.keystroke.modifiers.shift { 10. } else { 1. };
        let delta = match ev.keystroke.key.as_str() {
            "down" => -self.step,
            "up" => self.step,
            "enter" => return self.start_editing(window, cx),
            _ => return,
        };
        self.set_value(self.value + delta * multiplier, window, cx);
    }
}

impl Focusable for NumericInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for NumericInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (surface, border, hover_bg, text, ring) = (
            theme.alpha_at(0.05),
            theme.alpha_at(0.10),
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.blue,
        );
        let focused = self.focus_handle.is_focused(window);
        let disabled = self.disabled;
        let width = self.width;

        if self.editing {
            return div().w(px(width)).flex_none().child(self.input.clone());
        }

        div()
            .w(px(width))
            .flex_none()
            .child(
                div()
                    .id("numeric-input")
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(28.))
                    .w(px(width))
                    .rounded(px(6.))
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .font(typography::tabular())
                    .text_size(px(14.))
                    .text_color(text)
                    .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
                    .when(!disabled, |d| {
                        d.cursor(CursorStyle::ResizeUpDown)
                            .track_focus(&self.focus_handle)
                            .when(focused, |d| d.border_color(ring.opacity(0.7)))
                            .hover(|s| s.bg(hover_bg))
                            .on_key_down(cx.listener(Self::on_key_down))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_drag(NumberDrag, |_, _, _, cx| cx.new(|_| DragGhost))
                            .on_drag_move(cx.listener(Self::on_drag_move))
                    })
                    .child(format!("{:.*}{}", self.decimals, self.value, self.unit)),
            )
    }
}
