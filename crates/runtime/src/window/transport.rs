//! The transport: a floating pill. Play or pause, stop, the position as bar and beat and as
//! time, and a hairline seek strip with the duration when the project has an end.
//!
//! It follows the playhead, so it renders every frame while the project plays. It therefore
//! reads the end of the project, which walks every clip, only when the project changes.

use gpui::{
    App, BorderStyle, Bounds, BoxShadow, Context, DispatchPhase, Entity, FocusHandle, Hitbox,
    HitboxBehavior, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Window, canvas, div, fill, hsla, point, prelude::*, px, quad, size,
};
use sound_core::Ticks;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::{ActiveTheme, Playhead, Session, typography};

const STRIP_WIDTH: f32 = 200.;
const STRIP_HEIGHT: f32 = 16.;
const KNOB: f32 = 8.;

pub struct TransportPill {
    session: Entity<Session>,
    playhead: Entity<Playhead>,
    /// Derived from the project on every change, never edited here.
    end: Option<Ticks>,
    scrubbing: bool,
    play_focus: FocusHandle,
    stop_focus: FocusHandle,
    strip_focus: FocusHandle,
}

impl TransportPill {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let playhead = session.read(cx).playhead().clone();
        cx.observe(&playhead, |_, _, cx| cx.notify()).detach();
        cx.observe(&session, |pill, session, cx| {
            pill.end = session.read(cx).project().end();
            cx.notify();
        })
        .detach();
        Self {
            end: session.read(cx).project().end(),
            session,
            playhead,
            scrubbing: false,
            play_focus: cx.focus_handle().tab_stop(true),
            stop_focus: cx.focus_handle().tab_stop(true),
            strip_focus: cx.focus_handle().tab_stop(true),
        }
    }

    fn seek(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, _| session.engine().seek(tick));
    }

    /// Seeks to where the pointer is on the strip.
    fn scrub(&mut self, x: Pixels, strip: Bounds<Pixels>, cx: &mut Context<Self>) {
        let Some(end) = self.end else {
            return;
        };
        let travel = f32::from(strip.size.width) - KNOB;
        let fraction = ((f32::from(x - strip.left()) - KNOB / 2.) / travel).clamp(0., 1.);
        self.seek(
            Ticks((end.0 as f64 * f64::from(fraction)).round() as u64),
            cx,
        );
    }

    /// Left and right move by a bar when the strip has the focus.
    fn on_strip_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let bar = project
            .project_file()
            .tempo_map
            .time_signature()
            .ticks_per_bar();
        let now = self.playhead.read(cx).tick;
        let end = self.end.unwrap_or(Ticks(u64::MAX));
        match event.keystroke.key.as_str() {
            "left" => self.seek(Ticks(now.0.saturating_sub(bar)), cx),
            "right" => self.seek((now + Ticks(bar)).min(end), cx),
            _ => {}
        }
    }

    fn strip(&self, end: Ticks, cx: &mut Context<Self>) -> impl IntoElement {
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
                let middle = bounds.top() + px(STRIP_HEIGHT / 2.);
                let knob_left = bounds.left() + px((STRIP_WIDTH - KNOB) * fraction);
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

/// Minutes and seconds, as `1:07`.
fn clock_time(seconds: f64) -> String {
    let seconds = seconds.max(0.) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn position_texts(project: &sound_core::Project, tick: Ticks) -> (String, String) {
    let position = project
        .project_file()
        .tempo_map
        .time_signature()
        .bar_beat_of(tick);
    (
        format!("{}.{}", position.bar, position.beat),
        clock_time(project.clock().seconds_of(tick)),
    )
}

impl Render for TransportPill {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fill, border, muted, green) = (
            theme.gray_200.blend(theme.alpha_at(0.06)),
            theme.alpha_at(0.10),
            theme.gray_700,
            theme.green,
        );
        let Playhead { playing, tick } = *self.playhead.read(cx);
        let project = self.session.read(cx).project();
        let (bar_beat, time) = position_texts(project, tick);
        let duration = self
            .end
            .map(|end| clock_time(project.clock().seconds_of(end)));
        let strip = self.end.map(|end| self.strip(end, cx));
        let session = self.session.clone();

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .h(px(48.))
            .pl(px(12.))
            .pr(px(20.))
            .rounded_full()
            .bg(fill)
            .border_1()
            .border_color(border)
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.25),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(-8.),
                inset: false,
            }])
            // A click on the pill is not a click on what lies under it.
            .occlude()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        Button::icon_only("play", if playing { "pause" } else { "play" })
                            .variant(if playing {
                                ButtonVariant::SubtleColor(green)
                            } else {
                                ButtonVariant::GhostColor(green)
                            })
                            .size(ButtonSize::Sm)
                            .rounded(true)
                            .focus_handle(&self.play_focus)
                            .on_click({
                                let session = session.clone();
                                move |_, _, cx: &mut App| {
                                    session.update(cx, |session, cx| session.toggle_playback(cx))
                                }
                            }),
                    )
                    .child(
                        Button::icon_only("stop", "square")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .rounded(true)
                            .focus_handle(&self.stop_focus)
                            .on_click(move |_, _, cx: &mut App| {
                                session.update(cx, |session, _| session.engine().stop())
                            }),
                    ),
            )
            .child(
                div()
                    .font(typography::tabular())
                    .min_w(px(40.))
                    .child(bar_beat),
            )
            .child(
                div()
                    .font(typography::tabular())
                    .text_color(muted)
                    .min_w(px(32.))
                    .child(time),
            )
            .children(strip)
            .children(duration.map(|duration| {
                div()
                    .font(typography::tabular())
                    .text_color(muted)
                    .child(duration)
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::clock_time;

    #[test]
    fn time_shows_as_minutes_and_seconds() {
        assert_eq!(clock_time(0.0), "0:00");
        assert_eq!(clock_time(59.9), "0:59");
        assert_eq!(clock_time(67.2), "1:07");
        assert_eq!(clock_time(3600.0), "60:00");
    }
}
