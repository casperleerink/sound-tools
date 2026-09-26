//! The transport: a pill in the middle of the title row. Play or pause, stop, record, the
//! position as bar and beat and as time, a hairline seek strip with the duration when the
//! project has an end, the tempo at the playhead, the steadiness of a fit, the click and the
//! master meter.
//!
//! It follows the playhead, so it renders every frame while the project plays. It therefore
//! reads the end of the project, which walks every clip, only after a project event, and
//! once for all events of a group.
//!
//! The tempo is a controlled readout: it reads the tempo map when it renders and keeps no copy,
//! so a `project.json` written from outside shows at once, also during a drag. A drag is one
//! gesture of the session and one undo step. The click is not project state at all: it is a
//! processor in the engine with a switch, see [`metronome`]. Neither is the MIDI input, see
//! [`midi`]: a finished recording is an edit, and nothing before it is.

use std::sync::Arc;

use arrangement::TrackState;
use fit_tempo::FitState;
use gpui::{
    App, BorderStyle, Bounds, Context, DispatchPhase, Entity, FocusHandle, Hitbox, HitboxBehavior,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Task, Window,
    canvas, div, fill, point, prelude::*, px, quad, size,
};
use metronome::Click;
use midi::{Input, Keyboard, Latency, Lost};
use sound_core::{Changes, Instance, ProjectEvent, StreamTiming, Tempo, TempoChange, Ticks};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::drag_number::DragNumber;
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::meter::{Level, Meter};
use sound_ui::{ActiveTheme, POLL_INTERVAL, Playhead, Session, typography};

use super::{recording, steadiness, tempo};

/// The height of the pill, in the 48 pt title row.
pub const HEIGHT: f32 = 36.;
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

/// A drag of the tempo, from the press to its end.
struct TempoDrag {
    /// The tick of the tempo change this drag edits, picked at the press. By its tick and
    /// never by its place in the list: an outside edit may add or remove a tempo change while
    /// the drag goes on, and a drag must never change one that only took the place of the one
    /// the composer grabbed. A playhead that runs over a later tempo change does not move it
    /// either.
    at: Ticks,
    /// Whether the gesture of the session is open. It begins with the first move that changes
    /// something, so a press without a move is no undo step.
    begun: bool,
    /// The tempo change went away under the drag, and the gesture ended with it.
    gone: bool,
}

pub struct TransportPill {
    session: Entity<Session>,
    playhead: Entity<Playhead>,
    /// Derived from the project, never edited here. Read again in `refresh` after an event.
    end: Option<Ticks>,
    /// The fit of the project, when it has one. Finding it walks every instance, and this is
    /// read on every frame, so it is kept between events like the end of the project.
    fit: Option<Instance<FitState>>,
    end_is_stale: bool,
    scrubbing: bool,
    /// The click in the engine. `None` only when the engine refused it, which is reported.
    click: Option<Click>,
    /// The MIDI input in the engine. `None` only when the engine refused it, which is reported.
    keyboard: Option<Keyboard>,
    /// When the sound of an engine frame reaches the device, for the latency. `None` without a
    /// device, so an offline window measures nothing instead of guessing.
    timing: Option<Arc<StreamTiming>>,
    /// The playhead the last poll saw: where a recording ends when a stop or a seek ends it,
    /// because the playhead has already moved by then.
    seen: Playhead,
    /// The track the take that is running began on. A take goes there, whatever the composer
    /// selects while it runs.
    recording_track: Option<Instance<TrackState>>,
    tempo_drag: Option<TempoDrag>,
    /// Whether a drag of the steadiness has the gesture of the session open.
    steadiness_drag: bool,
    play_focus: FocusHandle,
    stop_focus: FocusHandle,
    record_focus: FocusHandle,
    strip_focus: FocusHandle,
    click_focus: FocusHandle,
    /// Drains what the engine reports about the MIDI input, as the session polls the engine.
    _polling: Task<()>,
}

impl TransportPill {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        Self::with_device(session, None, cx)
    }

    /// The pill of the real window, which has a device and can therefore say how long a key
    /// press takes to reach the speakers.
    pub fn with_device(
        session: Entity<Session>,
        timing: Option<Arc<StreamTiming>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let playhead = session.read(cx).playhead().clone();
        cx.observe(&playhead, |pill, playhead, cx| {
            let now = *playhead.read(cx);
            let stopped = pill.seen.playing && !now.playing;
            let jumped = pill.seen.jumps != now.jumps;
            let was = std::mem::replace(&mut pill.seen, now);
            // A stop, a pause or a seek ends the take. The playhead has already moved, so the
            // take ends where it was before.
            if pill.is_recording() && (stopped || jumped) {
                pill.finish_recording(was.tick, cx);
            }
            cx.notify();
        })
        .detach();
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
        // The MIDI input is a processor in the engine too. It plays nowhere until the first
        // poll wires it to the instrument of the selected track.
        let keyboard = session.update(cx, |session, cx| match Keyboard::attach(session.engine()) {
            Ok(keyboard) => Some(keyboard),
            Err(error) => {
                session.report(error, cx);
                None
            }
        });
        // What a key press cost, once, when the window goes away. macOS ends the process
        // without unwinding, and GPUI drops the views first, so this is the last moment the
        // numbers exist. A session with no MIDI message prints nothing.
        cx.on_release(|pill, _| {
            super::print_midi_report(pill.latency(), pill.lost_messages());
        })
        .detach();
        // Its own timer, next to the one of the session: what the engine reports about the
        // MIDI input must be drained whether the project plays or not.
        let polling = cx.spawn(async move |pill, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if pill.update(cx, |pill, cx| pill.poll_input(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            end: session.read(cx).project().end(),
            fit: fit_tempo::fit_of(session.read(cx).project()),
            end_is_stale: false,
            seen: *session.read(cx).playhead().read(cx),
            recording_track: None,
            session,
            playhead,
            scrubbing: false,
            click,
            keyboard,
            timing,
            tempo_drag: None,
            steadiness_drag: false,
            play_focus: cx.focus_handle().tab_stop(true),
            stop_focus: cx.focus_handle().tab_stop(true),
            record_focus: cx.focus_handle().tab_stop(true),
            strip_focus: cx.focus_handle().tab_stop(true),
            click_focus: cx.focus_handle().tab_stop(true),
            _polling: polling,
        }
    }

    /// Takes what the engine reported about the MIDI input and wires the live input to the
    /// instrument of the selected track. The timer calls it; tests call it to skip the wait.
    ///
    /// Nothing here is on the way from a key to its sound: that path is the input ring and the
    /// audio thread, and it runs whether this is called or not.
    pub fn poll_input(&mut self, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let selected = session.read(cx).selected().cloned();
        let project = session.read(cx).project();
        let destination = recording::live_notes_input(project, selected.as_ref());
        let timing = self.timing.clone();
        let recording_track = self.recording_track.clone();
        let Some(keyboard) = self.keyboard.as_mut() else {
            return;
        };
        // The poll also makes the port of an earlier `play_into` the one the live input
        // reaches: the release of what was held went out in a block before it.
        let polled = session.update(cx, |session, _| {
            keyboard.poll(session.engine(), timing.as_deref())
        });
        // A take goes to the track it began on, so the live input stays there too while it
        // runs. Selecting another track during a take would otherwise split the two.
        let wired = match recording_track.is_some() {
            true => Ok(()),
            false => session.update(cx, |session, _| {
                keyboard.play_into(session.engine(), destination)
            }),
        };
        if let Err(error) = polled.and(wired) {
            session.update(cx, |session, cx| session.report(error, cx));
        }
    }

    /// Where a device layer puts the messages it reads. `None` when the engine refused the
    /// MIDI input.
    pub fn midi_input(&self) -> Option<Input> {
        self.keyboard.as_ref().map(Keyboard::input)
    }

    /// How long a MIDI message took to reach the device, over this session.
    pub fn latency(&self) -> Latency {
        self.keyboard
            .as_ref()
            .map(Keyboard::latency)
            .unwrap_or_default()
    }

    /// Messages that were never played, and messages that are missing from a take.
    pub fn lost_messages(&self) -> Lost {
        self.keyboard
            .as_ref()
            .map(Keyboard::lost)
            .unwrap_or_default()
    }

    pub fn is_recording(&self) -> bool {
        self.keyboard.as_ref().is_some_and(Keyboard::is_recording)
    }

    /// The record control and the `r` key: start recording from the playhead, or end the take.
    ///
    /// Starting also starts playback: the playhead has to move for a take to have any length.
    /// Ending leaves playback as it is, so a composer can go on listening.
    pub fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        let Playhead { playing, tick, .. } = *self.playhead.read(cx);
        if self.is_recording() {
            self.finish_recording(tick, cx);
            return;
        }
        let session = self.session.clone();
        let selected = session.read(cx).selected().cloned();
        let track = recording::target_track(session.read(cx).project(), selected.as_ref());
        let Some(keyboard) = self.keyboard.as_mut() else {
            return;
        };
        keyboard.start_recording(tick);
        self.recording_track = track;
        if !playing {
            session.update(cx, |session, _| session.engine().play());
        }
        cx.notify();
    }

    /// Ends the take at `until`: the clip is one undo step, and the raw take is written next
    /// to it and never touched again.
    fn finish_recording(&mut self, until: Ticks, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let timing = self.timing.clone();
        let Some(keyboard) = self.keyboard.as_mut() else {
            return;
        };
        // The last block of the take is still in the ring.
        let polled = session.update(cx, |session, _| {
            keyboard.poll(session.engine(), timing.as_deref())
        });
        let take = keyboard.finish_recording(until);
        if let Err(error) = polled {
            session.update(cx, |session, cx| session.report(error, cx));
        }
        let Some(take) = take else {
            return;
        };
        cx.notify();
        // The track the take began on, not the one that is selected now.
        let track = self.recording_track.take();
        if take.is_empty() {
            // Nothing was played: no clip and no file.
            return;
        }
        // The performance first, and whatever happens to the clip. It is the only copy of what
        // the composer played, and a clip can fail to be made: its track may be gone.
        let written = recording::write_take(session.read(cx).project(), &take);
        let name = match written {
            Ok(name) => Some(name),
            Err(error) => {
                session.update(cx, |session, cx| session.report(error, cx));
                None
            }
        };
        let Some(track) = track else {
            return;
        };
        session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                recording::add_take_clip(project, &track, &take, name)
            })
        });
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
        self.change_at_playhead(cx).bpm
    }

    /// The tempo change in effect at the playhead. Read from the project, never kept.
    fn change_at_playhead(&self, cx: &App) -> TempoChange {
        let tick = self.playhead.read(cx).tick;
        let project = self.session.read(cx).project();
        tempo::change_at(&project.project_file().tempo_map, tick)
    }

    /// A change of the tempo control: a drag through the gesture of the session, so playback
    /// and every other view follow each move and the whole drag is one undo step, or a key step
    /// as one commit.
    fn on_tempo(&mut self, change: ValueChange<f64>, cx: &mut Context<Self>) {
        match change {
            ValueChange::Drag(bpm) => self.drag_tempo(bpm, cx),
            ValueChange::DragEnd => {
                if self
                    .tempo_drag
                    .take()
                    .is_some_and(|drag| drag.begun && !drag.gone)
                {
                    self.session
                        .update(cx, |session, cx| session.finish_gesture(cx));
                }
            }
            // Escape: the tempo goes back to what it was at the press.
            ValueChange::DragCancel => {
                if self
                    .tempo_drag
                    .take()
                    .is_some_and(|drag| drag.begun && !drag.gone)
                {
                    self.session
                        .update(cx, |session, cx| session.cancel_gesture(cx));
                }
            }
            ValueChange::Set(bpm) => self.set_tempo(bpm, cx),
        }
    }

    /// One move of a tempo drag. The tempo map it changes is the one the project has now, so a
    /// file edit during the drag keeps what it changed.
    fn drag_tempo(&mut self, bpm: f64, cx: &mut Context<Self>) {
        let Some(drag) = self.tempo_drag.as_mut().filter(|drag| !drag.gone) else {
            return;
        };
        let at = drag.at;
        // It opens even when the tempo change turns out to be gone: the empty step is dropped,
        // and the drag must not leave a gesture open.
        let begun = std::mem::replace(&mut drag.begun, true);
        let found = self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(tempo::LABEL, cx);
            }
            session.gesture(cx, |project, edit| {
                let live = &project.project_file().tempo_map;
                let Some(tempo_map) = tempo::with_bpm(live, at, bpm) else {
                    return Ok(false);
                };
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.publish(edit, changes)?;
                Ok(true)
            })
        });
        // The tempo change is gone, removed from outside. That delete was the last write, so
        // the drag finishes and does not cancel, as a clip drag does when its clip is deleted.
        if found == Some(false) {
            if let Some(drag) = self.tempo_drag.as_mut() {
                drag.gone = true;
            }
            self.session
                .update(cx, |session, cx| session.finish_gesture(cx));
        }
    }

    /// An arrow key on the tempo: one finished change of the tempo change at the playhead.
    fn set_tempo(&mut self, bpm: f64, cx: &mut Context<Self>) {
        let tick = self.playhead.read(cx).tick;
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let live = &project.project_file().tempo_map;
                let change = tempo::change_at(live, tick);
                let Some(tempo_map) = tempo::with_bpm(live, change.tick, bpm) else {
                    return Ok(());
                };
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.commit(tempo::LABEL, changes)
            });
        });
    }

    /// The tempo at the playhead, as a number that a drag and the arrows change: whole bpm
    /// from where a drag began, half a bpm per point, with shift tenths.
    fn tempo(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().gray_700;
        let tempo = self.shown_tempo(cx);
        let number = DragNumber::new("tempo", tempo.bpm(), Tempo::MIN_BPM, Tempo::MAX_BPM)
            .drag(tempo::DRAG_PER_POINT, tempo::DRAG_STEP)
            .keys(tempo::KEY_STEP, tempo::FINE_KEY_STEP)
            .on_change(Self::callback(cx, Self::on_tempo))
            .child(
                div()
                    .font(typography::tabular())
                    .child(tempo::tempo_text(tempo)),
            )
            .child(div().text_size(px(12.)).text_color(muted).child("bpm"));
        // The press picks the tempo change the drag edits, before the first move.
        div()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pill, _: &MouseDownEvent, _, cx| {
                    let at = pill.change_at_playhead(cx).tick;
                    let (begun, gone) = (false, false);
                    pill.tempo_drag = Some(TempoDrag { at, begun, gone });
                }),
            )
            .child(number)
    }

    /// A callback of a control of the pill. It holds the pill weakly, as `cx.listener` does.
    fn callback<E>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, E, &mut Context<Self>) + 'static,
    ) -> impl Fn(E, &mut Window, &mut App) + 'static {
        let pill = cx.weak_entity();
        move |event, _, cx| {
            pill.update(cx, |pill, cx| f(pill, event, cx)).ok();
        }
    }

    /// The steadiness the transport shows, as a percentage. The value is read from the project
    /// on every render and never kept, so an outside edit shows at once; which instance holds
    /// it is what `refresh` keeps.
    pub fn shown_steadiness(&self, cx: &App) -> Option<f64> {
        let project = self.session.read(cx).project();
        let state = project.state(self.fit.as_ref()?)?;
        Some(steadiness::percent_of(state.steadiness))
    }

    /// A change of the steadiness control, the same way as the tempo: a drag is one gesture
    /// that rewrites the tempo map through the derive of the fit on each move, a key one commit.
    fn on_steadiness(&mut self, change: ValueChange<f64>, cx: &mut Context<Self>) {
        let session = self.session.clone();
        match change {
            ValueChange::Drag(percent) => {
                let begun = std::mem::replace(&mut self.steadiness_drag, true);
                session.update(cx, |session, cx| {
                    if !begun {
                        session.begin_gesture(fit_tempo::STEADINESS_LABEL, cx);
                    }
                    session.gesture(cx, |project, edit| {
                        let mut changes = Changes::new();
                        let steadiness = steadiness::steadiness_of(percent);
                        if fit_tempo::set_steadiness(project, &mut changes, steadiness).is_some() {
                            project.publish(edit, changes)?;
                        }
                        Ok(())
                    });
                });
            }
            ValueChange::DragEnd => {
                if std::mem::take(&mut self.steadiness_drag) {
                    session.update(cx, |session, cx| session.finish_gesture(cx));
                }
            }
            // Escape: the steadiness goes back to what it was at the press.
            ValueChange::DragCancel => {
                if std::mem::take(&mut self.steadiness_drag) {
                    session.update(cx, |session, cx| session.cancel_gesture(cx));
                }
            }
            ValueChange::Set(percent) => session.update(cx, |session, cx| {
                session.edit(cx, |project| {
                    let mut changes = Changes::new();
                    let steadiness = steadiness::steadiness_of(percent);
                    match fit_tempo::set_steadiness(project, &mut changes, steadiness) {
                        Some(()) => project.commit(fit_tempo::STEADINESS_LABEL, changes),
                        None => Ok(()),
                    }
                });
            }),
        }
    }

    /// How steady the fitted tempo is, as a number that a drag and the arrows change: one
    /// percent per point, with shift tenths. `None` when the project has no fit: then the pill
    /// is the one every project has always had.
    fn steadiness(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let percent = self.shown_steadiness(cx)?;
        let muted = cx.theme().gray_700;
        let number = DragNumber::new("steadiness", percent, 0., 100.)
            .drag(steadiness::DRAG_PER_POINT, steadiness::DRAG_STEP)
            .keys(steadiness::KEY_STEP, steadiness::FINE_KEY_STEP)
            .on_change(Self::callback(cx, Self::on_steadiness))
            .child(
                div()
                    .font(typography::tabular())
                    .child(steadiness::percent_text(percent)),
            )
            .child(div().text_size(px(12.)).text_color(muted).child("steady"));
        Some(number)
    }

    /// Reads the end again when an event made it stale. `render` calls it, so a group of ten
    /// thousand events costs one walk. Not while a gesture is open: a drag publishes on every
    /// mouse move, and the walk over every clip was a quarter of the time of a move on a large
    /// project. The duration follows when the drag ends.
    fn refresh(&mut self, cx: &App) {
        if !self.end_is_stale || self.session.read(cx).gesture_open() {
            return;
        }
        let project = self.session.read(cx).project();
        self.end = project.end();
        self.fit = fit_tempo::fit_of(project);
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
        let (fill, border, muted, green, red) = (
            theme.gray_200.blend(theme.alpha_at(0.06)),
            theme.alpha_at(0.10),
            theme.gray_700,
            theme.green,
            theme.red,
        );
        let Playhead { playing, tick, .. } = *self.playhead.read(cx);
        let project = self.session.read(cx).project();
        let (bar_beat, time) = position_texts(project, tick);
        let duration = self
            .end
            .map(|end| clock_time(project.clock().seconds_of(end)));
        let tempo = self.tempo(cx);
        let steadiness = self.steadiness(cx);
        let strip = self.end.map(|end| self.strip(end, cx));
        let click_on = self.click_is_on();
        let has_click = self.click.is_some();
        let recording = self.is_recording();
        let has_keyboard = self.keyboard.is_some();
        let session = self.session.clone();

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .h(px(HEIGHT))
            .pl(px(4.))
            .pr(px(16.))
            .rounded_full()
            .bg(fill)
            .border_1()
            .border_color(border)
            // A click on the pill does not move the window, as the rest of the title row does.
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
                            .debug_selector(|| "stop".to_string())
                            .focus_handle(&self.stop_focus)
                            .on_click(move |_, _, cx: &mut App| {
                                session.update(cx, |session, _| session.engine().stop())
                            }),
                    )
                    // Red, like every record control anywhere: a red ring while it is off, and
                    // solid red while it records, which a laptop screen shows from afar.
                    .child(
                        Button::icon_only("record", "circle")
                            .variant(match recording {
                                true => ButtonVariant::Solid(red),
                                false => ButtonVariant::GhostColor(red),
                            })
                            .size(ButtonSize::Sm)
                            .rounded(true)
                            .disabled(!has_keyboard)
                            .debug_selector(|| "record".to_string())
                            .focus_handle(&self.record_focus)
                            .on_click(cx.listener(|pill, _, _, cx| pill.toggle_recording(cx))),
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
            // Only a project with a fit has this, so every other pill is what it always was.
            .children(steadiness)
            // The click is a reference, not part of the mix, so it takes no colour: a muted
            // glyph while it is off, and white under a dark glyph while it sounds.
            .child(
                Button::icon_only("click", "metronome")
                    .variant(match click_on {
                        true => ButtonVariant::Primary,
                        false => ButtonVariant::GhostColor(muted),
                    })
                    .size(ButtonSize::Sm)
                    .rounded(true)
                    .disabled(!has_click)
                    .debug_selector(|| "click".to_string())
                    .focus_handle(&self.click_focus)
                    .on_click(cx.listener(|pill, _, _, cx| pill.toggle_click(cx))),
            )
            // The level of the master. Step 2 of the third milestone feeds it; until then it
            // is at rest.
            .child(Meter::new("master", Level::SILENT).horizontal())
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
        let (mut project, _plugins) = open_or_create(folder.path(), control).unwrap();
        let mut changes = Changes::new();
        let clip = Clip::new(Ticks(0), Length::new(Ticks(3840)).unwrap(), Vec::new());
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
