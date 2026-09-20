//! The transport: a floating pill. Play or pause, stop, the position as bar and beat and as
//! time, and a hairline seek strip with the duration when the project has an end.
//!
//! It follows the playhead, so it renders every frame while the project plays. It therefore
//! reads the end of the project, which walks every clip, only after a project event, and
//! once for all events of a group.

use gpui::{
    App, BorderStyle, Bounds, BoxShadow, Context, DispatchPhase, Entity, FocusHandle, Hitbox,
    HitboxBehavior, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Window, canvas, div, fill, hsla, point, prelude::*, px, quad, size,
};
use sound_core::{ProjectEvent, Ticks};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::{ActiveTheme, Playhead, Session, typography};

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

pub struct TransportPill {
    session: Entity<Session>,
    playhead: Entity<Playhead>,
    /// Derived from the project, never edited here. Read again in `refresh` after an event.
    end: Option<Ticks>,
    end_is_stale: bool,
    scrubbing: bool,
    play_focus: FocusHandle,
    stop_focus: FocusHandle,
    strip_focus: FocusHandle,
}

impl TransportPill {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let playhead = session.read(cx).playhead().clone();
        cx.observe(&playhead, |_, _, cx| cx.notify()).detach();
        // Any record may move the end, and the project file holds the tempo of the times shown.
        // Problems change neither. A notice or a finished edit sends no event at all.
        cx.subscribe(&session, |pill, _, event, cx| {
            if !matches!(event, ProjectEvent::ProblemsChanged) {
                pill.end_is_stale = true;
                cx.notify();
            }
        })
        .detach();
        Self {
            end: session.read(cx).project().end(),
            end_is_stale: false,
            session,
            playhead,
            scrubbing: false,
            play_focus: cx.focus_handle().tab_stop(true),
            stop_focus: cx.focus_handle().tab_stop(true),
            strip_focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// Reads the end again when an event made it stale. `render` calls it, so a group of ten
    /// thousand events costs one walk.
    fn refresh(&mut self, cx: &App) {
        if !self.end_is_stale {
            return;
        }
        self.end = self.session.read(cx).project().end();
        self.end_is_stale = false;
        // Without an end there is no strip, and no mouse up on it would end a drag.
        if self.end.is_none() {
            self.scrubbing = false;
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
        let fraction = fraction_at(f32::from(x - strip.left()), f32::from(strip.size.width));
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
        self.refresh(cx);
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
            // No fixed widths: tabular numbers keep the pill still, and it grows by one digit
            // at bar 100 or at ten minutes.
            .child(div().font(typography::tabular()).child(bar_beat))
            .child(
                div()
                    .font(typography::tabular())
                    .text_color(muted)
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
    use gpui::{AppContext, TestAppContext};
    use sound_core::{Changes, Engine, InstanceId, Ticks};
    use sound_notes::{Clip, Length};
    use sound_ui::Session;

    use super::{KNOB, TransportPill, clock_time, fraction_at, knob_left};
    use crate::{OFFLINE, open_or_create};

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

    #[gpui::test]
    fn a_scrub_ends_when_the_project_loses_its_end(cx: &mut TestAppContext) {
        let folder = tempfile::tempdir().unwrap();
        let (control, _engine) = Engine::new(OFFLINE);
        let mut project = open_or_create(folder.path(), control).unwrap();
        let mut changes = Changes::new();
        let clip = Clip {
            start: Ticks(0),
            length: Length::new(Ticks(3840)).unwrap(),
            notes: Vec::new(),
        };
        let id = InstanceId::new("arrangement/track-1/part").unwrap();
        changes.create(id, clip);
        project.commit("Add clip", changes).unwrap();
        let session = cx.new(|cx| Session::new(project, cx));
        let pill = cx.new(|cx| TransportPill::new(session.clone(), cx));
        pill.update(cx, |pill, _| {
            assert_eq!(pill.end, Some(Ticks(3840)));
            pill.scrubbing = true;
        });

        // A dismissed notice is no project event: the end is not read again.
        session.update(cx, |session, cx| {
            session.report("something", cx);
            session.dismiss_notice(cx);
        });
        cx.run_until_parked();
        pill.update(cx, |pill, _| assert!(!pill.end_is_stale));

        session.update(cx, |session, cx| session.undo(cx));
        cx.run_until_parked();
        pill.update(cx, |pill, cx| {
            assert!(pill.end_is_stale);
            pill.refresh(cx);
            assert_eq!(pill.end, None);
            assert!(!pill.scrubbing);
        });
    }

    #[test]
    fn time_shows_as_minutes_and_seconds() {
        assert_eq!(clock_time(0.0), "0:00");
        assert_eq!(clock_time(59.9), "0:59");
        assert_eq!(clock_time(67.2), "1:07");
        assert_eq!(clock_time(3600.0), "60:00");
    }
}
