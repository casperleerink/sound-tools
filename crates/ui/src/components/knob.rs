//! Knob: a small round control with a dotted value arc and a pointer. Drag vertically to change.
//! Stateful, so create it with `cx.new(|cx| Knob::new(cx))`. Up/down arrows nudge by one step,
//! shift-arrow by ten. GPUI has no arc primitive, so the arc is a ring of dots.

use std::rc::Rc;

use gpui::{
    App, Context, CursorStyle, DragMoveEvent, FocusHandle, Focusable, KeyDownEvent,
    MouseButton, MouseDownEvent, Render, SharedString, Window, div, prelude::*, px,
};

use crate::theme::ActiveTheme;
use crate::typography;

/// Marker for gpui's drag machinery; the preview below renders nothing.
struct KnobDrag;

struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The arc runs from -135 to +135 degrees, like a hardware pot.
const SWEEP: f32 = 270.;
const DOTS: usize = 25;
const DOT: f32 = 2.5;
/// Pixels of vertical drag for the full range.
const DRAG_RANGE: f32 = 160.;

type ChangeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

pub struct Knob {
    focus_handle: FocusHandle,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    decimals: usize,
    size: f32,
    label: Option<SharedString>,
    unit: SharedString,
    disabled: bool,
    drag_start: Option<(f32, f32)>,
    on_change: Option<ChangeHandler>,
}

impl Knob {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            value: 0.,
            min: 0.,
            max: 1.,
            step: 0.01,
            decimals: 2,
            size: 44.,
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

    /// Diameter in pixels.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

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

    fn fraction(&self) -> f32 {
        ((self.value - self.min) / (self.max - self.min)).clamp(0., 1.)
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

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        self.drag_start = Some((f32::from(ev.position.y), self.value));
        cx.notify();
    }

    fn on_drag_move(
        &mut self,
        ev: &DragMoveEvent<KnobDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((start_y, start_value)) = self.drag_start else {
            return;
        };
        let delta = (start_y - f32::from(ev.event.position.y)) / DRAG_RANGE;
        self.set_value(start_value + delta * (self.max - self.min), window, cx);
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let multiplier = if ev.keystroke.modifiers.shift { 10. } else { 1. };
        let delta = match ev.keystroke.key.as_str() {
            "down" | "left" => -self.step,
            "up" | "right" => self.step,
            _ => return,
        };
        self.set_value(self.value + delta * multiplier, window, cx);
    }
}

impl Focusable for Knob {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Centre offset of a point on the arc at `fraction` of the sweep, at radius `r`.
fn arc_point(fraction: f32, r: f32) -> (f32, f32) {
    let angle = (-SWEEP / 2. + SWEEP * fraction).to_radians();
    (r * angle.sin(), -r * angle.cos())
}

impl Render for Knob {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (dim, lit, face, border, muted, text, ring) = (
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.alpha_at(0.05),
            theme.alpha_at(0.10),
            theme.gray_600,
            theme.gray_950,
            theme.blue,
        );
        let focused = self.focus_handle.is_focused(window);
        let size = self.size;
        let disabled = self.disabled;
        let fraction = self.fraction();
        let centre = size / 2.;
        let arc_r = centre - DOT / 2.;
        let face_size = size - DOT * 2. - 4.;
        let (pointer_x, pointer_y) = arc_point(fraction, face_size / 2. - 5.);

        let dots = (0..DOTS).map(|ix| {
            let f = ix as f32 / (DOTS - 1) as f32;
            let (dx, dy) = arc_point(f, arc_r);
            div()
                .absolute()
                .left(px(centre + dx - DOT / 2.))
                .top(px(centre + dy - DOT / 2.))
                .size(px(DOT))
                .rounded_full()
                .bg(if f <= fraction + 0.001 { lit } else { dim })
        });

        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .when(disabled, |d| d.opacity(0.4))
            .child(
                div()
                    .id("knob")
                    .relative()
                    .size(px(size))
                    .when(disabled, |d| d.cursor_not_allowed())
                    .when(!disabled, |d| {
                        d.cursor(CursorStyle::ResizeUpDown)
                            .track_focus(&self.focus_handle)
                            .on_key_down(cx.listener(Self::on_key_down))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_drag(KnobDrag, |_, _, _, cx| cx.new(|_| DragGhost))
                            .on_drag_move(cx.listener(Self::on_drag_move))
                    })
                    .children(dots)
                    // Face.
                    .child(
                        div()
                            .absolute()
                            .left(px(centre - face_size / 2.))
                            .top(px(centre - face_size / 2.))
                            .size(px(face_size))
                            .rounded_full()
                            .bg(face)
                            .border_1()
                            .border_color(if focused { ring.opacity(0.7) } else { border }),
                    )
                    // Pointer.
                    .child(
                        div()
                            .absolute()
                            .left(px(centre + pointer_x - 1.5))
                            .top(px(centre + pointer_y - 1.5))
                            .size(px(3.))
                            .rounded_full()
                            .bg(lit),
                    ),
            )
            .when_some(self.label.clone(), |d, label| {
                d.child(div().text_size(px(12.)).text_color(muted).child(label))
            })
            .child(
                div()
                    .font(typography::tabular())
                    .text_size(px(12.))
                    .text_color(text)
                    .child(format!("{:.*}{}", self.decimals, self.value, self.unit)),
            )
    }
}
