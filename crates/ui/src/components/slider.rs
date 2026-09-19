//! Horizontal slider: hairline track, small round handle, drag to change. Optional label and a
//! tabular value readout. Stateful, so create it with `cx.new(|cx| Slider::new(cx))`.
//! Left/right arrows nudge by one step, shift-arrow by ten.

use std::rc::Rc;

use gpui::{
    App, Context, DragMoveEvent, FocusHandle, Focusable, KeyDownEvent, MouseButton,
    MouseDownEvent, Render, SharedString, Window, div, prelude::*, px,
};

use crate::theme::ActiveTheme;
use crate::typography;

/// Marker for gpui's drag machinery; the preview below renders nothing.
struct SliderDrag;

struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

type ChangeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

pub struct Slider {
    focus_handle: FocusHandle,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    decimals: usize,
    width: f32,
    label: Option<SharedString>,
    unit: SharedString,
    disabled: bool,
    drag_start: Option<(f32, f32)>,
    on_change: Option<ChangeHandler>,
}

impl Slider {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            value: 0.,
            min: 0.,
            max: 1.,
            step: 0.01,
            decimals: 2,
            width: 200.,
            label: None,
            unit: SharedString::default(),
            disabled: false,
            drag_start: None,
            on_change: None,
        }
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

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

    /// Label above the track; the value readout appears with it.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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

    fn fraction(&self) -> f32 {
        ((self.value - self.min) / (self.max - self.min)).clamp(0., 1.)
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        self.drag_start = Some((f32::from(ev.position.x), self.value));
        cx.notify();
    }

    fn on_drag_move(
        &mut self,
        ev: &DragMoveEvent<SliderDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((start_x, start_value)) = self.drag_start else {
            return;
        };
        let travel = (self.width - HANDLE).max(1.);
        let delta = (f32::from(ev.event.position.x) - start_x) / travel;
        self.set_value(start_value + delta * (self.max - self.min), window, cx);
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let multiplier = if ev.keystroke.modifiers.shift { 10. } else { 1. };
        let delta = match ev.keystroke.key.as_str() {
            "left" | "down" => -self.step,
            "right" | "up" => self.step,
            "home" => return self.set_value(self.min, window, cx),
            "end" => return self.set_value(self.max, window, cx),
            _ => return,
        };
        self.set_value(self.value + delta * multiplier, window, cx);
    }
}

const HANDLE: f32 = 10.;

impl Focusable for Slider {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Slider {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (track, fill, handle_color, muted, text, ring) = (
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.gray_950,
            theme.gray_600,
            theme.gray_950,
            theme.blue,
        );
        let focused = self.focus_handle.is_focused(window);
        let fraction = self.fraction();
        let width = self.width;
        let disabled = self.disabled;
        let readout = format!("{:.*}{}", self.decimals, self.value, self.unit);

        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(6.))
            .w(px(width))
            .when(disabled, |d| d.opacity(0.4))
            .when_some(self.label.clone(), |d, label| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child(label)
                        .child(
                            div()
                                .font(typography::tabular())
                                .text_color(text)
                                .child(readout),
                        ),
                )
            })
            .child(
                div()
                    .id("slider-track")
                    .relative()
                    .h(px(HANDLE))
                    .w(px(width))
                    .when(!disabled, |d| {
                        d.cursor_pointer()
                            .track_focus(&self.focus_handle)
                            .on_key_down(cx.listener(Self::on_key_down))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_drag(SliderDrag, |_, _, _, cx| cx.new(|_| DragGhost))
                            .on_drag_move(cx.listener(Self::on_drag_move))
                    })
                    // Hairline track.
                    .child(
                        div()
                            .absolute()
                            .top(px(HANDLE / 2. - 0.5))
                            .left_0()
                            .w(px(width))
                            .h(px(1.))
                            .bg(track),
                    )
                    // Filled portion.
                    .child(
                        div()
                            .absolute()
                            .top(px(HANDLE / 2. - 0.5))
                            .left_0()
                            .w(px((width - HANDLE) * fraction + HANDLE / 2.))
                            .h(px(1.))
                            .bg(fill),
                    )
                    // Handle.
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left(px((width - HANDLE) * fraction))
                            .size(px(HANDLE))
                            .rounded_full()
                            .bg(handle_color)
                            .when(focused, |d| {
                                d.bg(ring).border_1().border_color(ring.opacity(0.7))
                            }),
                    ),
            )
    }
}
