//! The transport: a floating pill. Play or pause, stop, the position as bar and beat and as
//! time, a hairline seek strip with the duration when the project has an end, the tempo at the
//! playhead, and the click.
//!
//! It follows the playhead, so it renders every frame while the project plays. It therefore
//! reads the end of the project, which walks every clip, only after a project event, and
//! once for all events of a group.
//!
//! The tempo is a controlled readout: it reads the tempo map when it renders and keeps no copy,
//! so a `project.json` written from outside shows at once, also during a drag. A drag is one
//! gesture of the session and one undo step. The click is not project state at all: it is a
//! processor in the engine with a switch, see [`metronome`].

use gpui::{
    App, BorderStyle, Bounds, BoxShadow, Context, DispatchPhase, Entity, FocusHandle, Hitbox,
    HitboxBehavior, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Window, canvas, div, fill, hsla, point, prelude::*, px, quad, size,
};
use metronome::Click;
use sound_core::{Changes, ProjectEvent, Tempo, TempoMap, Ticks};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::{ActiveTheme, Playhead, Session, typography};

use super::tempo;

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

/// A drag on the tempo number, from mouse down to mouse up.
struct TempoDrag {
    /// Where the pointer went down, and the tempo there. A drag works out from these, so a
    /// drag there and back ends at the tempo it began with.
    start_y: f32,
    start_bpm: f64,
    /// The tempo change this drag edits, picked at mouse down. A playhead that runs over a
    /// later tempo change during the drag must not make it change another one halfway.
    change: usize,
    /// Whether the gesture of the session is open. It begins with the first move that changes
    /// something, so a press without a move is no undo step.
    begun: bool,
    /// The tempo that went out last. Several mouse moves may arrive between two frames.
    sent: f64,
}

pub struct TransportPill {
    session: Entity<Session>,
    playhead: Entity<Playhead>,
    /// Derived from the project, never edited here. Read again in `refresh` after an event.
    end: Option<Ticks>,
    end_is_stale: bool,
    scrubbing: bool,
    /// The click in the engine. `None` only when the engine refused it, which is reported.
    click: Option<Click>,
    tempo_drag: Option<TempoDrag>,
    play_focus: FocusHandle,
    stop_focus: FocusHandle,
    strip_focus: FocusHandle,
    tempo_focus: FocusHandle,
    click_focus: FocusHandle,
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
        // The end is not read during a drag, see `refresh`. The end of a gesture sends no
        // event, and the session notifies after it.
        cx.observe(&session, |pill, _, cx| {
            if pill.end_is_stale {
                cx.notify();
            }
        })
        .detach();
        // The click is a processor in the engine, not project state: attaching it writes
        // nothing and adds no undo step. It starts off and silent.
        let click = session.update(cx, |session, cx| match Click::attach(session.engine()) {
            Ok(click) => Some(click),
            Err(error) => {
                session.report(error, cx);
                None
            }
        });
        Self {
            end: session.read(cx).project().end(),
            end_is_stale: false,
            session,
            playhead,
            scrubbing: false,
            click,
            tempo_drag: None,
            play_focus: cx.focus_handle().tab_stop(true),
            stop_focus: cx.focus_handle().tab_stop(true),
            strip_focus: cx.focus_handle().tab_stop(true),
            tempo_focus: cx.focus_handle().tab_stop(true),
            click_focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// Whether the click sounds. For tests and for the button.
    pub fn click_is_on(&self) -> bool {
        self.click.as_ref().is_some_and(Click::is_on)
    }

    /// Turns the click on or off. Not an edit: nothing is saved and there is no undo step.
    pub fn toggle_click(&mut self, cx: &mut Context<Self>) {
        let Some(click) = &mut self.click else {
            return;
        };
        let on = !click.is_on();
        self.session.update(cx, |session, cx| {
            if let Err(error) = click.set_on(session.engine(), on) {
                session.report(error, cx);
            }
        });
        cx.notify();
    }

    /// The tempo the transport shows: the one in effect at the playhead. It is read from the
    /// project on every render, so an outside edit of `project.json` shows at once.
    pub fn shown_tempo(&self, cx: &App) -> Tempo {
        self.tempo_at_playhead(cx).0
    }

    /// The tempo in effect at the playhead, and the index of its tempo change.
    fn tempo_at_playhead(&self, cx: &App) -> (Tempo, usize) {
        let tick = self.playhead.read(cx).tick;
        let project = self.session.read(cx).project();
        let change = tempo::change_at(&project.project_file().tempo_map, tick);
        (project.clock().tempo_at(tick), change)
    }

    /// The tempo map with one of its changes set. A failure is reported, never dropped.
    fn tempo_map_with(
        &mut self,
        change: usize,
        bpm: f64,
        cx: &mut Context<Self>,
    ) -> Option<TempoMap> {
        let current = self
            .session
            .read(cx)
            .project()
            .project_file()
            .tempo_map
            .clone();
        match tempo::with_bpm(&current, change, bpm) {
            Ok(tempo_map) => Some(tempo_map),
            Err(error) => {
                self.session
                    .update(cx, |session, cx| session.report(error, cx));
                None
            }
        }
    }

    fn begin_tempo_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        let (tempo, change) = self.tempo_at_playhead(cx);
        self.tempo_drag = Some(TempoDrag {
            start_y: y,
            start_bpm: tempo.bpm(),
            change,
            begun: false,
            sent: tempo.bpm(),
        });
    }

    /// One mouse move of a tempo drag: the tempo change becomes what the pointer says, through
    /// the gesture of the session, so playback and every other view follow at once.
    fn drag_tempo(&mut self, y: f32, fine: bool, cx: &mut Context<Self>) {
        let Some(drag) = &self.tempo_drag else {
            return;
        };
        let bpm = tempo::dragged_bpm(drag.start_bpm, drag.start_y - y, fine);
        let (change, begun) = (drag.change, drag.begun);
        if bpm == drag.sent {
            return;
        }
        let Some(tempo_map) = self.tempo_map_with(change, bpm, cx) else {
            return;
        };
        self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(tempo::LABEL, cx);
            }
            session.gesture(cx, |project, edit| {
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.publish(edit, changes)
            });
        });
        if let Some(drag) = &mut self.tempo_drag {
            (drag.begun, drag.sent) = (true, bpm);
        }
    }

    /// Mouse up: the gesture becomes one undo step and `project.json` is written once.
    fn end_tempo_drag(&mut self, cx: &mut Context<Self>) {
        if self.tempo_drag.take().is_some_and(|drag| drag.begun) {
            self.session
                .update(cx, |session, cx| session.finish_gesture(cx));
        }
    }

    /// Escape: the tempo goes back to what it was at mouse down. Whether there was a drag.
    fn cancel_tempo_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.tempo_drag.take() else {
            return false;
        };
        if drag.begun {
            self.session
                .update(cx, |session, cx| session.cancel_gesture(cx));
        }
        true
    }

    /// An arrow key on the focused tempo: one finished change and one undo step.
    fn nudge_tempo(&mut self, delta: f64, cx: &mut Context<Self>) {
        // The mouse has the tempo: a key would fight the next mouse move.
        if self.tempo_drag.is_some() {
            return;
        }
        let (tempo, change) = self.tempo_at_playhead(cx);
        let Some(tempo_map) = self.tempo_map_with(change, tempo.bpm() + delta, cx) else {
            return;
        };
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.commit(tempo::LABEL, changes)
            });
        });
    }

    /// The keys of the focused tempo: the arrows step, with shift by a tenth, and escape puts
    /// a drag back.
    fn on_tempo_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        let step = match modifiers.shift {
            true => tempo::FINE_KEY_STEP,
            false => tempo::KEY_STEP,
        };
        match event.keystroke.key.as_str() {
            "escape" => {
                if self.cancel_tempo_drag(cx) {
                    cx.stop_propagation();
                }
            }
            "up" | "right" => {
                cx.stop_propagation();
                self.nudge_tempo(step, cx);
            }
            "down" | "left" => {
                cx.stop_propagation();
                self.nudge_tempo(-step, cx);
            }
            _ => {}
        }
    }

    /// The tempo at the playhead, as a number that a drag and the arrows change.
    fn tempo(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (muted, ring) = (theme.gray_700, theme.lavender);
        let (tempo, _) = self.tempo_at_playhead(cx);
        let pill = cx.entity();
        // A drag goes on wherever the pointer is, so these listeners are not hit tested.
        let listeners = canvas(
            |_, _, _| {},
            move |_, (), window, _| listen_to_tempo(pill, window),
        );
        div()
            .id("tempo")
            .debug_selector(|| "tempo".to_string())
            .track_focus(&self.tempo_focus)
            .on_key_down(cx.listener(|pill, event, _, cx| pill.on_tempo_key(event, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pill, event: &MouseDownEvent, _, cx| {
                    pill.begin_tempo_drag(f32::from(event.position.y), cx);
                }),
            )
            .flex()
            .flex_none()
            .items_baseline()
            .gap(px(4.))
            .px(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(gpui::Hsla::transparent_black())
            .focus_visible(move |style| style.border_color(ring))
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .child(
                div()
                    .font(typography::tabular())
                    .child(tempo::tempo_text(tempo)),
            )
            .child(div().text_size(px(12.)).text_color(muted).child("bpm"))
            .child(listeners.absolute().size_0())
    }

    /// Reads the end again when an event made it stale. `render` calls it, so a group of ten
    /// thousand events costs one walk. Not while a gesture is open: a drag publishes on every
    /// mouse move, and the walk over every clip was a quarter of the time of a move on a large
    /// project. The duration follows when the drag ends.
    fn refresh(&mut self, cx: &App) {
        if !self.end_is_stale || self.session.read(cx).gesture_open() {
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

    fn strip(&self, end: Ticks, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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

/// A drag on the tempo number goes on wherever the pointer is, until the button is up.
fn listen_to_tempo(pill: Entity<TransportPill>, window: &mut Window) {
    window.on_mouse_event({
        let pill = pill.clone();
        move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            pill.update(cx, |pill, cx| {
                if pill.tempo_drag.is_none() {
                    return;
                }
                if event.dragging() {
                    let fine = event.modifiers.shift;
                    pill.drag_tempo(f32::from(event.position.y), fine, cx);
                } else {
                    // The button came up somewhere that did not tell this window.
                    pill.end_tempo_drag(cx);
                }
            });
        }
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
            pill.update(cx, |pill, cx| pill.end_tempo_drag(cx));
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
        let Playhead { playing, tick, .. } = *self.playhead.read(cx);
        let project = self.session.read(cx).project();
        let (bar_beat, time) = position_texts(project, tick);
        let duration = self
            .end
            .map(|end| clock_time(project.clock().seconds_of(end)));
        let tempo = self.tempo(cx);
        let strip = self.end.map(|end| self.strip(end, cx));
        let click_on = self.click_is_on();
        let has_click = self.click.is_some();
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
            .child(tempo)
            // The click is a reference, not part of the mix, so it takes no accent: a subtle
            // fill says it sounds, as the mute button of a track does.
            .child(
                Button::icon_only("click", "metronome")
                    .variant(match click_on {
                        true => ButtonVariant::Subtle,
                        false => ButtonVariant::Ghost,
                    })
                    .size(ButtonSize::Sm)
                    .rounded(true)
                    .disabled(!has_click)
                    .debug_selector(|| "click".to_string())
                    .focus_handle(&self.click_focus)
                    .on_click(cx.listener(|pill, _, _, cx| pill.toggle_click(cx))),
            )
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
