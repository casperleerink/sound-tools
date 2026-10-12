//! The seek strip: a hairline with a knob at the playhead, which a press, a drag and the
//! arrows move.

use gpui::{
    BorderStyle, Bounds, Context, DispatchPhase, Entity, Hitbox, HitboxBehavior, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Window, canvas, div, fill,
    point, prelude::*, px, quad, size,
};
use sound_core::Ticks;
use sound_ui::ActiveTheme;

use super::TransportPill;

const STRIP_WIDTH: f32 = 200.;
const STRIP_HEIGHT: f32 = 16.;
const KNOB: f32 = 8.;

/// Where the knob starts on a strip of this width, for a position from 0 to 1. Painting and
/// scrubbing both use the width the strip really has, so the knob stays under the pointer.
fn knob_left(fraction: f32, strip_width: f32) -> f32 {
    (strip_width - KNOB) * fraction.clamp(0., 1.)
}

/// The position from 0 to 1 that puts the middle of the knob at `x` from the left of the strip.
fn fraction_at(x: f32, strip_width: f32) -> f32 {
    ((x - KNOB / 2.) / (strip_width - KNOB).max(1.)).clamp(0., 1.)
}

impl TransportPill {
    fn seek(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, _| session.engine().seek(tick));
    }

    /// Seeks to where the pointer is on the strip.
    fn scrub(&mut self, x: Pixels, strip: Bounds<Pixels>, cx: &mut Context<Self>) {
        let Some(end) = self.end else {
            return;
        };
        let fraction = fraction_at(f32::from(x - strip.left()), f32::from(strip.size.width));
        self.seek(
            Ticks((end.0 as f64 * f64::from(fraction)).round() as u64),
            cx,
        );
    }

    /// Left and right move by a bar when the strip has the focus.
    fn on_strip_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let time_signatures = project.project_file().tempo_map.time_signatures();
        let now = self.playhead.read(cx).tick;
        let end = self.end.unwrap_or(Ticks(u64::MAX));
        // Back by the bar before the playhead and on by the bar it is in, so from a bar line
        // each press lands on the next bar line, whatever the time signature of each bar.
        let back = time_signatures
            .bar_at(now.saturating_sub(Ticks(1)))
            .length();
        let on = time_signatures.bar_at(now).length();
        match event.keystroke.key.as_str() {
            "left" => self.seek(now.saturating_sub(back), cx),
            "right" => self.seek((now + on).min(end), cx),
            _ => {}
        }
    }

    pub(super) fn strip(&self, end: Ticks, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (track, played, ring) = (theme.alpha_at(0.10), theme.gray_950, theme.lavender);
        let tick = self.playhead.read(cx).tick.min(end);
        let fraction = if end.0 == 0 {
            0.
        } else {
            tick.0 as f32 / end.0 as f32
        };
        let pill = cx.entity();

        let surface = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                let middle = bounds.top() + bounds.size.height / 2.;
                let strip_width = f32::from(bounds.size.width);
                let knob_left = bounds.left() + px(knob_left(fraction, strip_width));
                let line = |left: Pixels, right: Pixels| {
                    Bounds::new(point(left, middle - px(0.5)), size(right - left, px(1.)))
                };
                let knob = Bounds::new(
                    point(knob_left, middle - px(KNOB / 2.)),
                    size(px(KNOB), px(KNOB)),
                );
                window.paint_quad(fill(line(bounds.left(), bounds.right()), track));
                window.paint_quad(fill(line(bounds.left(), knob_left), played));
                window.paint_quad(quad(
                    knob,
                    px(KNOB / 2.),
                    played,
                    px(0.),
                    played,
                    BorderStyle::Solid,
                ));
                listen(pill, bounds, hitbox, window);
            },
        );
        div()
            .id("seek")
            .track_focus(&self.strip_focus)
            .on_key_down(cx.listener(|pill, event, _, cx| pill.on_strip_key(event, cx)))
            .flex_none()
            .w(px(STRIP_WIDTH))
            .h(px(STRIP_HEIGHT))
            .rounded(px(4.))
            .border_1()
            .focus_visible(move |style| style.border_color(ring))
            .cursor_pointer()
            .child(surface.size_full())
    }
}

/// A drag that starts on the strip goes on wherever the pointer is, until the button is up.
fn listen(pill: Entity<TransportPill>, strip: Bounds<Pixels>, hitbox: Hitbox, window: &mut Window) {
    window.on_mouse_event({
        let pill = pill.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            let hit = phase == DispatchPhase::Bubble && hitbox.is_hovered(window);
            if hit && event.button == MouseButton::Left {
                pill.update(cx, |pill, cx| {
                    pill.scrubbing = true;
                    pill.scrub(event.position.x, strip, cx);
                });
            }
        }
    });
    window.on_mouse_event({
        let pill = pill.clone();
        move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.dragging() {
                pill.update(cx, |pill, cx| {
                    if pill.scrubbing {
                        pill.scrub(event.position.x, strip, cx);
                    }
                });
            }
        }
    });
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            pill.update(cx, |pill, _| pill.scrubbing = false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{KNOB, fraction_at, knob_left};

    #[test]
    fn the_knob_is_under_the_pointer_for_any_strip_width() {
        // The strip is 198 px inside its border, not the 200 px it asks for.
        for width in [198., 200., 64.] {
            for fraction in [0., 0.25, 0.5, 1.] {
                let middle = knob_left(fraction, width) + KNOB / 2.;
                assert!((fraction_at(middle, width) - fraction).abs() < 1e-6);
            }
            assert_eq!(knob_left(1., width) + KNOB, width);
            assert_eq!(fraction_at(-50., width), 0.);
            assert_eq!(fraction_at(width + 50., width), 1.);
        }
    }
}
