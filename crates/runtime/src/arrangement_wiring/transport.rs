//! The transport of the arrangement: a pill in the middle of the title row while the arrangement
//! is the main view. Play or pause, stop, record, the position as bar and beat and as time, a
//! hairline seek strip with the duration when the project has an end, the tempo and the time
//! signature at the playhead, the steadiness of a fit, the click, and the window's live input:
//! the computer keys and the meter, see [`LiveInput`].
//!
//! It follows the playhead, so it renders every frame while the project plays. It therefore
//! reads the end of the project, which walks every clip, only after a project event, and
//! once for all events of a group.
//!
//! The tempo is a controlled readout: it reads the tempo map when it renders and keeps no copy,
//! so a `project.json` written from outside shows at once, also during a drag. A drag is one
//! gesture of the session and one undo step. The click is not project state at all: the session
//! holds its switch. Neither is the MIDI input, see [`midi`]: a finished recording is an edit,
//! and nothing before it is.
//!
//! Space plays and `r` records while the arrangement is the main view: its keys, which it takes
//! as any project takes its own, see [`GlobalKeys`].
//!
//! This file holds the pill itself: its fields, the tempo and the steadiness, and the render.
//! The rest is in `transport/`, as more `impl TransportPill`:
//! - `take`: the record control, a take from its start to its clips.
//! - `strip`: the seek strip.
//! - `signature`: the time signature.

mod signature;
mod steadiness;
mod strip;
mod take;
mod tempo;

use std::sync::Arc;

use arrangement::{ArrangementState, TrackState};
use fit_tempo::FitState;
use gpui::{
    App, Context, Entity, FocusHandle, MouseButton, MouseDownEvent, MouseUpEvent, Task, Window,
    div, prelude::*, px,
};
use sound_core::{Changes, Clock, Instance, ProjectEvent, StreamTiming, Tempo, TempoChange, Ticks};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::drag_number::DragNumber;
use sound_ui::components::gesture::ValueChange;
use sound_ui::{
    ActiveTheme, DragEdit, GlobalKeys, Playhead, Recording, Session, Views, every_poll, typography,
    weak_callback,
};

use self::take::{AudioTake, Ended};
use crate::window::live_input::{LiveInput, Recorded};

/// A recording of the record control, from its start to its clips.
struct Take {
    /// The playhead it began at.
    start: Ticks,
    /// The clock it began under. Its takes, of MIDI and of audio, are placed under this one: a
    /// change of the tempo map ends a take, as a seek does.
    clock: Arc<Clock>,
    /// The track the MIDI input records onto: the one it began on, whatever the composer
    /// selects while it runs.
    midi_track: Option<Instance<TrackState>>,
    /// The armed audio tracks it records, when there are any.
    audio: Option<AudioTake>,
    /// It ended, and waits for its audio files.
    ended: Option<Ended>,
}

/// The height of the pill, in the 48 pt title row.
const HEIGHT: f32 = 36.;

/// Whether the arrangement is the main view, which the pill and its keys are for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    Arrangement,
    /// Another tool is: the window shows its live input alone, and the keys of the pill are not
    /// the arrangement's to take.
    Other,
}

/// A drag of the tempo, from the press to its end.
struct TempoDrag {
    /// The tick of the tempo change this drag edits, picked at the press. By its tick and
    /// never by its place in the list: an outside edit may add or remove a tempo change while
    /// the drag goes on, and a drag must never change one that only took the place of the one
    /// the composer grabbed. A playhead that runs over a later tempo change does not move it
    /// either.
    at: Ticks,
    /// It opens with the first move that changes something, so a press without a move is no
    /// undo step.
    edit: DragEdit,
}

pub struct TransportPill {
    session: Entity<Session>,
    playhead: Entity<Playhead>,
    /// Derived from the project, never edited here. Read again in `refresh` after an event.
    end: Option<Ticks>,
    /// The fit of the project, when it has one. Finding it walks every instance, and this is
    /// read on every frame, so it is kept between events like the end of the project.
    fit: Option<Instance<FitState>>,
    /// By the main view, as of the last refresh or poll.
    shown: Shown,
    end_is_stale: bool,
    scrubbing: bool,
    /// The window's MIDI keyboard, audio input and meter, which a take records from.
    live: Entity<LiveInput>,
    /// When the sound of an engine frame reaches the device, for placing a take. `None` without
    /// a device.
    timing: Option<Arc<StreamTiming>>,
    /// The playhead the last poll saw: where a recording ends when a stop or a seek ends it,
    /// because the playhead has already moved by then.
    seen: Playhead,
    /// The recording of the record control, from its start until its clips are made.
    take: Option<Take>,
    recording: Entity<Recording>,
    tempo_drag: Option<TempoDrag>,
    signature_drag: Option<signature::SignatureDrag>,
    steadiness_drag: DragEdit,
    play_focus: FocusHandle,
    stop_focus: FocusHandle,
    record_focus: FocusHandle,
    strip_focus: FocusHandle,
    click_focus: FocusHandle,
    /// Follows a take as the session polls the engine.
    _polling: Task<()>,
}

impl TransportPill {
    pub fn new(session: Entity<Session>, live: Entity<LiveInput>, cx: &mut Context<Self>) -> Self {
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
            // A change of the tempo map moves every tick in time from where the take began, and
            // the take was heard under the old one: it ends here, as at a seek.
            if matches!(event, ProjectEvent::ProjectFileChanged) && pill.is_recording() {
                let project = pill.session.read(cx).project();
                let changed = pill
                    .take
                    .as_ref()
                    .is_some_and(|take| take.clock.tempo_map() != project.clock().tempo_map());
                if changed {
                    let tick = pill.playhead.read(cx).tick;
                    pill.finish_recording(tick, cx);
                }
            }
        })
        .detach();
        cx.subscribe(&live, |pill, _, Recorded(reports), cx| {
            pill.on_reports(reports, cx)
        })
        .detach();
        cx.subscribe(&live, |pill, _, event, cx| pill.input_did(*event, cx))
            .detach();
        let recording = session.read(cx).recording().clone();
        // The end is not read during a drag, see `refresh`. The end of a gesture sends no
        // event, and the session notifies after it.
        cx.observe(&session, |pill, _, cx| {
            if pill.end_is_stale {
                cx.notify();
            }
        })
        .detach();
        // Space plays and `r` records while the arrangement is the main view.
        let pill = cx.weak_entity();
        let keys = GlobalKeys::add(
            move |key, cx| {
                let shown = pill.read_with(cx, |pill, _| pill.shown).ok();
                if shown != Some(Shown::Arrangement) {
                    return false;
                }
                match key.name.as_str() {
                    "space" => {
                        if key.down {
                            (pill.update(cx, |pill, cx| {
                                pill.session.update(cx, Session::toggle_playback)
                            }))
                            .ok();
                        }
                        true
                    }
                    "r" => {
                        if key.down {
                            pill.update(cx, Self::toggle_recording).ok();
                        }
                        true
                    }
                    _ => false,
                }
            },
            cx,
        );
        cx.on_release(move |_, cx| GlobalKeys::remove(keys, cx))
            .detach();
        let polling = every_poll(cx, Self::poll_takes);
        let timing = live.read(cx).timing();
        Self {
            end: session.read(cx).project().end(),
            fit: fit_tempo::fit_of(session.read(cx).project()),
            shown: shown(&session, cx),
            end_is_stale: false,
            seen: *session.read(cx).playhead().read(cx),
            take: None,
            recording,
            session,
            playhead,
            scrubbing: false,
            live,
            timing,
            tempo_drag: None,
            signature_drag: None,
            steadiness_drag: DragEdit::default(),
            play_focus: cx.focus_handle().tab_stop(true),
            stop_focus: cx.focus_handle().tab_stop(true),
            record_focus: cx.focus_handle().tab_stop(true),
            strip_focus: cx.focus_handle().tab_stop(true),
            click_focus: cx.focus_handle().tab_stop(true),
            _polling: polling,
        }
    }

    /// Follows a take: the notes it records so far, and its audio. The timer calls it; tests
    /// call it to skip the wait.
    pub fn poll_takes(&mut self, cx: &mut Context<Self>) {
        // A tool of the project may become the main view in a new load of its code, which is
        // no project event.
        let shown = shown(&self.session, cx);
        if self.shown != shown {
            self.shown = shown;
            cx.notify();
        }
        self.poll_audio(cx);
        self.show_notes(cx);
    }

    /// Whether a press on the tempo is held, from the press to its end. For tests.
    pub fn holds_a_tempo_drag(&self) -> bool {
        self.tempo_drag.is_some()
    }

    /// Whether the click sounds. For tests and for the button.
    pub fn click_is_on(&self, cx: &App) -> bool {
        self.session.read(cx).click_on()
    }

    /// Turns the click on or off. Not an edit: nothing is saved and there is no undo step.
    pub fn toggle_click(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let on = !session.click_on();
            session.set_click(on, cx);
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
        project.project_file().tempo_map.change_at(tick)
    }

    /// A change of the tempo control: a drag through the gesture of the session, so playback
    /// and every other view follow each move and the whole drag is one undo step, or a key step
    /// as one commit.
    fn on_tempo(&mut self, change: ValueChange<f64>, cx: &mut Context<Self>) {
        match change {
            ValueChange::Drag(bpm) => self.drag_tempo(bpm, cx),
            ValueChange::DragEnd => {
                if let Some(mut drag) = self.tempo_drag.take() {
                    drag.edit.finish(&self.session, cx);
                }
            }
            // Escape: the tempo goes back to what it was at the press.
            ValueChange::DragCancel => {
                if let Some(mut drag) = self.tempo_drag.take() {
                    drag.edit.cancel(&self.session, cx);
                }
            }
            ValueChange::Set(bpm) => self.set_tempo(bpm, cx),
        }
    }

    /// One move of a tempo drag. The tempo map it changes is the one the project has now, so a
    /// file edit during the drag keeps what it changed.
    fn drag_tempo(&mut self, bpm: f64, cx: &mut Context<Self>) {
        let Some(drag) = self.tempo_drag.as_mut() else {
            return;
        };
        let at = drag.at;
        // It opens even when the tempo change turns out to be gone: the empty step is dropped,
        // and the drag must not leave a gesture open.
        let found = drag
            .edit
            .publish(&self.session, tempo::LABEL, cx, |project, edit| {
                let live = &project.project_file().tempo_map;
                let Some(tempo_map) = tempo::with_bpm(live, at, bpm) else {
                    return Ok(false);
                };
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.publish(edit, changes)?;
                Ok(true)
            });
        // The tempo change is gone, removed from outside. That delete was the last write, so
        // the drag finishes and does not cancel, as a clip drag does when its clip is deleted.
        // The drag is over with it: the moves that still come find no drag.
        if found == Some(false)
            && let Some(mut drag) = self.tempo_drag.take()
        {
            drag.edit.finish(&self.session, cx);
        }
    }

    /// An arrow key on the tempo: one finished change of the tempo change at the playhead.
    fn set_tempo(&mut self, bpm: f64, cx: &mut Context<Self>) {
        let tick = self.playhead.read(cx).tick;
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let live = &project.project_file().tempo_map;
                let change = live.change_at(tick);
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
            .on_change(weak_callback(cx, Self::on_tempo))
            .child(div().font(typography::tabular()).child(tempo.to_string()))
            .child(div().text_size(px(12.)).text_color(muted).child("bpm"));
        // The press picks the tempo change the drag edits, before the first move. A press that
        // moved nothing leaves nothing behind when it comes up, and the second press of a double
        // click opens no drag.
        div()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pill, event: &MouseDownEvent, _, cx| {
                    if event.click_count != 1 {
                        return;
                    }
                    let at = pill.change_at_playhead(cx).tick;
                    let edit = DragEdit::default();
                    pill.tempo_drag = Some(TempoDrag { at, edit });
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pill, _: &MouseUpEvent, _, _| {
                    let drag = pill.tempo_drag.as_ref();
                    if drag.is_some_and(|drag| !drag.edit.is_open()) {
                        pill.tempo_drag = None;
                    }
                }),
            )
            .child(number)
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
                let label = fit_tempo::STEADINESS_LABEL;
                self.steadiness_drag
                    .publish(&session, label, cx, |project, edit| {
                        let mut changes = Changes::new();
                        let steadiness = steadiness::steadiness_of(percent);
                        if fit_tempo::set_steadiness(project, &mut changes, steadiness).is_some() {
                            project.publish(edit, changes)?;
                        }
                        Ok(())
                    });
            }
            ValueChange::DragEnd => {
                self.steadiness_drag.finish(&session, cx);
            }
            // Escape: the steadiness goes back to what it was at the press.
            ValueChange::DragCancel => {
                self.steadiness_drag.cancel(&session, cx);
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
            .on_change(weak_callback(cx, Self::on_steadiness))
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
        self.shown = shown(&self.session, cx);
        self.end_is_stale = false;
        // Without an end there is no strip, and no mouse up on it would end a drag.
        if self.end.is_none() {
            self.scrubbing = false;
        }
    }
}

fn shown(session: &Entity<Session>, cx: &App) -> Shown {
    let project = session.read(cx).project();
    match Views::main_instance(session, cx) {
        Some(main) if project.resolve::<ArrangementState>(&main).is_some() => Shown::Arrangement,
        _ => Shown::Other,
    }
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
        .time_signatures()
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
        let end = self.end;
        let duration = end.map(|end| clock_time(project.clock().seconds_of(end)));
        let tempo = self.tempo(cx);
        let time_signature = self.time_signature(cx);
        let steadiness = self.steadiness(cx);
        let strip = end.map(|end| self.strip(end, cx));
        let click_on = self.click_is_on(cx);
        let has_click = self.session.read(cx).has_click();
        let recording = self.is_recording();
        let live = self.live.read(cx);
        let can_record = live.midi_input().is_some() || live.can_open_input();
        let session = self.session.clone();

        div()
            .debug_selector(|| "transport".to_string())
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
                            .debug_selector(|| "play".to_string())
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
                            .disabled(!can_record)
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
            .child(time_signature)
            // Only a project with a fit has this, so every other pill is what it always was.
            .children(steadiness)
            // The click and the computer keys are switches of the composer, not of the mix, so
            // they take no colour: a muted glyph while off, and white under a dark glyph while on.
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
            // The computer keys, and what the device plays: the master, and the click.
            .child(self.live.clone())
    }
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext, TestAppContext};
    use plugin_host::{Plugins, ScanCache, ScanCommand};
    use sound_core::{Changes, Engine, InstanceId, Ticks};
    use sound_notes::{Clip, Length};
    use sound_ui::Session;

    use super::{TransportPill, clock_time};
    use crate::arrangement_wiring::{add_track, main_arrangement};
    use crate::window::live_input::LiveInput;
    use crate::{OFFLINE, open_or_create_with};

    #[gpui::test]
    fn a_scrub_ends_when_the_project_loses_its_end(cx: &mut TestAppContext) {
        let folder = tempfile::tempdir().unwrap();
        let (control, _engine) = Engine::new(OFFLINE);
        // No folder to look in, so no plugin of this machine and the scanner never runs.
        let plugins = Plugins::new(
            Vec::new(),
            ScanCommand::this_program().unwrap(),
            ScanCache::none(),
        );
        let mut project = open_or_create_with(folder.path(), control, plugins).unwrap();
        let arrangement = main_arrangement(&project).unwrap();
        add_track(&mut project, &arrangement).unwrap();
        let mut changes = Changes::new();
        let clip = Clip::new(Ticks(0), Length::new(Ticks(3840)).unwrap(), Vec::new());
        let id = InstanceId::new("arrangement/track-1/part").unwrap();
        changes.create(id, clip);
        project.commit("Add clip", changes).unwrap();
        let session = cx.new(|cx| Session::new(project, cx));
        let live = cx.new(|cx| LiveInput::new(session.clone(), None, None, None, cx));
        let pill = cx.new(|cx| TransportPill::new(session.clone(), live, cx));
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
