//! The transport: a pill in the middle of the title row. Play or pause, stop, record, the
//! position as bar and beat and as time, a hairline seek strip with the duration when the
//! project has an end, the tempo at the playhead, the steadiness of a fit, the computer keys,
//! the click and the master meter.
//!
//! It follows the playhead, so it renders every frame while the project plays. It therefore
//! reads the end of the project, which walks every clip, only after a project event, and
//! once for all events of a group.
//!
//! The tempo is a controlled readout: it reads the tempo map when it renders and keeps no copy,
//! so a `project.json` written from outside shows at once, also during a drag. A drag is one
//! gesture of the session and one undo step. The click is not project state at all: it is a
//! processor in the engine with a switch, see [`metronome`]. Neither is the MIDI input, see
//! [`midi`]: a finished recording is an edit, and nothing before it is. The computer keys play
//! into that input as one more keyboard.
//!
//! This file holds the pill itself: its fields, the MIDI input and the computer keys, the click,
//! the tempo and the steadiness, and the render. The rest is in `transport/`, as more
//! `impl TransportPill`:
//! - `take`: the record control, a take from its start to its clips, and the audio input.
//! - `strip`: the seek strip.
//!
//! `tempo` and `steadiness` there hold the numbers of those two controls.

mod steadiness;
mod strip;
mod take;
mod tempo;

use std::sync::Arc;

use arrangement::{ArrangementState, TrackState};
use fit_tempo::FitState;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseUpEvent, Task, Window, div, prelude::*, px,
};
use metronome::Click;
use midi::{ComputerKeys, Input, Keyboard, Latency, Lost};
use sound_core::{
    Changes, Clock, InputEndpoint, Instance, InstanceId, Peaks, ProjectEvent, StreamTiming, Tempo,
    TempoChange, Ticks,
};
use sound_typescript::Midi;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::drag_number::DragNumber;
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::meter::Meter;
use sound_ui::{
    ActiveTheme, DragEdit, Metering, Playhead, Recording, Session, Views, every_poll, typography,
    weak_action, weak_callback,
};

use self::take::{AudioTake, Ended};
use super::recording;
use crate::window::audio_input::{AudioInput, OpenInput};

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
    /// Whether the main area shows the arrangement. Without it there is no transport, see
    /// `without_arrangement`. Kept between events like the end of the project.
    arrangement_is_main: bool,
    /// Whether something takes the notes of the keyboard, as of the last poll.
    takes_notes: bool,
    end_is_stale: bool,
    scrubbing: bool,
    /// The click in the engine. `None` only when the engine refused it, which is reported.
    click: Option<Click>,
    /// The MIDI input in the engine. `None` only when the engine refused it, which is reported.
    keyboard: Option<Keyboard>,
    /// The track whose tools of the project hear the MIDI input: the one it played into when
    /// what they hear sounded. It moves with the keyboard, only after what was held is let go,
    /// so a tool hears the end of each note it heard begin.
    hearing: Option<InstanceId>,
    /// The keys the tools of `hearing` heard go down and not up yet, one bit each.
    hearing_held: u128,
    /// Where the keyboard was last asked to play, and the track of that.
    asked: (Option<InputEndpoint>, Option<InstanceId>),
    /// The computer keys while they play as a MIDI keyboard. Not project state, like the click.
    computer_keys: Option<ComputerKeys>,
    /// When the sound of an engine frame reaches the device, for the latency. `None` without a
    /// device, so an offline window measures nothing instead of guessing.
    timing: Option<Arc<StreamTiming>>,
    /// The playhead the last poll saw: where a recording ends when a stop or a seek ends it,
    /// because the playhead has already moved by then.
    seen: Playhead,
    /// The recording of the record control, from its start until its clips are made.
    take: Option<Take>,
    /// The audio input, open while a track is armed or `project.json` connects it, and its
    /// recorder.
    audio: AudioInput,
    recording: Entity<Recording>,
    tempo_drag: Option<TempoDrag>,
    steadiness_drag: DragEdit,
    play_focus: FocusHandle,
    stop_focus: FocusHandle,
    record_focus: FocusHandle,
    strip_focus: FocusHandle,
    keys_focus: FocusHandle,
    click_focus: FocusHandle,
    /// What the device plays, taken once per poll: the master meter at the right end.
    output: Peaks,
    metering: Metering,
    /// Drains what the engine reports about the MIDI input, as the session polls the engine,
    /// and reads the master meter.
    _polling: Task<()>,
}

impl TransportPill {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        Self::with_device(session, None, None, cx)
    }

    /// The pill of the real window, which has a device and can therefore say how long a key
    /// press takes to reach the speakers, and an input it opens while a track is armed.
    pub fn with_device(
        session: Entity<Session>,
        timing: Option<Arc<StreamTiming>>,
        open_input: Option<OpenInput>,
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
            // An agent may connect the device input in `project.json`, or take it away, while
            // the window runs. A connection that comes into the graph or leaves it, as its
            // instance comes or goes, changes the problems.
            if matches!(
                event,
                ProjectEvent::ProjectFileChanged | ProjectEvent::ProblemsChanged
            ) {
                pill.follow_input(cx);
            }
            // A deleted track is no longer armed.
            if matches!(event, ProjectEvent::Deleted(_)) {
                let project = pill.session.read(cx).project();
                let exists: Vec<InstanceId> = pill
                    .recording
                    .read(cx)
                    .armed()
                    .filter(|track| project.resolve::<TrackState>(track).is_some())
                    .cloned()
                    .collect();
                pill.recording.update(cx, |recording, cx| {
                    recording.retain_armed(|track| exists.contains(track), cx);
                });
            }
        })
        .detach();
        // The input is open while a track is armed: that is how a composer sees the level
        // before a take. A project that connects the device input opens it with the window.
        let recording = session.read(cx).recording().clone();
        cx.observe(&recording, |pill, _, cx| pill.follow_input(cx))
            .detach();
        let pill = cx.weak_entity();
        cx.defer(move |cx| {
            // A window that went in the meantime opens nothing.
            pill.update(cx, |pill, cx| pill.follow_input(cx)).ok();
        });
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
            print_midi_report(pill.latency(), pill.lost_messages());
        })
        .detach();
        // Its own timer, next to the one of the session: what the engine reports about the
        // MIDI input must be drained whether the project plays or not.
        let polling = every_poll(cx, |pill: &mut Self, cx| {
            pill.poll_input(cx);
            pill.read_meter(cx);
        });
        let output = session.update(cx, |session, _| session.engine().output_peaks().clone());
        Self {
            end: session.read(cx).project().end(),
            fit: fit_tempo::fit_of(session.read(cx).project()),
            arrangement_is_main: arrangement_is_main(&session, cx),
            takes_notes: false,
            end_is_stale: false,
            seen: *session.read(cx).playhead().read(cx),
            take: None,
            audio: AudioInput::new(open_input),
            recording,
            session,
            playhead,
            scrubbing: false,
            click,
            keyboard,
            hearing: None,
            hearing_held: 0,
            asked: (None, None),
            computer_keys: None,
            timing,
            tempo_drag: None,
            steadiness_drag: DragEdit::default(),
            play_focus: cx.focus_handle().tab_stop(true),
            stop_focus: cx.focus_handle().tab_stop(true),
            record_focus: cx.focus_handle().tab_stop(true),
            strip_focus: cx.focus_handle().tab_stop(true),
            keys_focus: cx.focus_handle().tab_stop(true),
            click_focus: cx.focus_handle().tab_stop(true),
            output,
            metering: Metering::default(),
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
        let track = recording::target_track(project, selected.as_ref());
        let track = track.map(|track| track.id().clone());
        let timing = self.timing.clone();
        let recording_track = self.take.as_ref().map(|take| take.midi_track.clone());
        if self.takes_notes != destination.is_some() {
            self.takes_notes = destination.is_some();
            cx.notify();
        }
        self.poll_audio(cx);
        let Some(keyboard) = self.keyboard.as_mut() else {
            return;
        };
        // The poll also makes the port of an earlier `play_into` the one the live input
        // reaches: the release of what was held went out in a block before it.
        let polled = session.update(cx, |session, _| {
            keyboard.poll(session.engine(), timing.as_deref())
        });
        // What sounded since the last poll went where the keyboard played until now, and the
        // tools of that track hear it.
        let heard = keyboard.take_heard().into_iter();
        let mut heard: Vec<_> = heard.filter_map(recording::tool_message).collect();
        for message in &heard {
            match *message {
                Midi::NoteOn { pitch, .. } => self.hearing_held |= 1 << pitch,
                Midi::NoteOff { pitch } => self.hearing_held &= !(1 << pitch),
                Midi::Cc { .. } | Midi::Bend { .. } => {}
            }
        }
        let hearing = self.hearing.clone();
        if keyboard.destination() == self.asked.0 {
            self.hearing = self.asked.1.clone();
        }
        // The keyboard lets go of nothing when it plays into the same port, such as nowhere
        // on two tracks with no instrument: the tools of the old track hear the ends here.
        if self.hearing != hearing {
            let held = std::mem::take(&mut self.hearing_held);
            let offs = (0..128_u8).filter(|pitch| held & (1 << pitch) != 0);
            heard.extend(offs.map(|pitch| Midi::NoteOff { pitch }));
        }
        // A take goes to the track it began on, so the live input stays there too while it
        // runs. Selecting another track during a take would otherwise split the two.
        let wired = match recording_track.is_some() {
            true => Ok(()),
            false => {
                self.asked = (destination, track);
                session.update(cx, |session, _| {
                    keyboard.play_into(session.engine(), destination)
                })
            }
        };
        if let Err(error) = polled.and(wired) {
            session.update(cx, |session, cx| session.report(error, cx));
        }
        sound_typescript::hear_midi(hearing.as_ref(), &heard, cx);
        self.show_notes(cx);
    }

    /// One poll of the master meter: what the device played since the last one. The timer
    /// calls it; tests call it to skip the wait.
    pub fn read_meter(&mut self, cx: &mut Context<Self>) {
        if self.metering.read(Some(&self.output)) {
            cx.notify();
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

    /// Whether a press on the tempo is held, from the press to its end. For tests.
    pub fn holds_a_tempo_drag(&self) -> bool {
        self.tempo_drag.is_some()
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

    /// Whether the computer keys play. For tests and for the button.
    pub fn computer_keys_are_on(&self) -> bool {
        self.computer_keys.is_some()
    }

    /// Turns the computer keys on or off. Not an edit, like the click. Off ends what they hold.
    pub fn toggle_computer_keys(&mut self, cx: &mut Context<Self>) {
        if self.keyboard.is_none() {
            return;
        }
        match self.computer_keys.is_some() {
            true => {
                self.let_go_of_computer_keys(cx);
                self.computer_keys = None;
            }
            false => self.computer_keys = Some(ComputerKeys::default()),
        }
        cx.notify();
    }

    /// A key went down. Gives whether the computer keys took it, and then nothing else may
    /// hear it. A key they hold stays theirs wherever the focus went since, so its repeats
    /// reach nothing else. Another key is theirs while they play, when it is one of theirs
    /// with no cmd, ctrl, alt or fn and the plain keys are `free`.
    pub fn computer_key_down(&mut self, event: &KeyDownEvent, free: bool) -> bool {
        let (Some(keys), Some(keyboard)) = (&mut self.computer_keys, &self.keyboard) else {
            return false;
        };
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        if keys.holds(key) {
            return true;
        }
        let modifiers = keystroke.modifiers;
        let plain =
            !(modifiers.platform || modifiers.control || modifiers.alt || modifiers.function);
        if !(free && plain && ComputerKeys::plays(key)) {
            return false;
        }
        // A repeat of a key that went down where they did not hear it, such as on a tool's
        // card, starts nothing. Else it goes where a MIDI keyboard's messages go, so the
        // instrument, a take and the tools hear it as they hear those.
        if !event.is_held
            && let Some(played) = keys.down(key)
        {
            keyboard.input().send(played);
        }
        true
    }

    /// A key came up. Gives whether it was the computer keys', and ends its note.
    pub fn computer_key_up(&mut self, key: &str) -> bool {
        let (Some(keys), Some(keyboard)) = (&mut self.computer_keys, &self.keyboard) else {
            return false;
        };
        let held = keys.holds(key);
        if let Some(played) = keys.up(key) {
            keyboard.input().send(played);
        }
        held
    }

    /// Ends every note the computer keys hold, for when their key ups will not come: cmd is
    /// down, or the window lost the keys.
    pub fn let_go_of_computer_keys(&mut self, _: &mut Context<Self>) {
        let (Some(keys), Some(keyboard)) = (&mut self.computer_keys, &self.keyboard) else {
            return;
        };
        let input = keyboard.input();
        for played in keys.release() {
            input.send(played);
        }
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
        self.arrangement_is_main = arrangement_is_main(&self.session, cx);
        self.end_is_stale = false;
        // Without an end there is no strip, and no mouse up on it would end a drag.
        if self.end.is_none() {
            self.scrubbing = false;
        }
    }
}

/// What a key press cost, measured over the session. It covers the wait for the next audio
/// block and the output latency the device reports, not the keyboard and its cable.
fn print_midi_report(latency: Latency, lost: Lost) {
    if latency.count() == 0 {
        return;
    }
    println!(
        "midi to sound over {} messages: mean {:?}, shortest {:?}, longest {:?}, jitter {:?}",
        latency.count(),
        latency.mean(),
        latency.shortest(),
        latency.longest(),
        latency.spread()
    );
    if lost.any() {
        println!(
            "midi lost: {} messages never played, {} missing from a take",
            lost.input, lost.reports
        );
    }
}

fn arrangement_is_main(session: &Entity<Session>, cx: &App) -> bool {
    let project = session.read(cx).project();
    Views::main_instance(session, cx)
        .is_some_and(|main| project.resolve::<ArrangementState>(&main).is_some())
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

impl TransportPill {
    fn computer_keys_button(&self, cx: &mut Context<Self>) -> Button {
        let muted = cx.theme().gray_700;
        Button::icon_only("computer-keys", "keyboard-music")
            .variant(match self.computer_keys_are_on() {
                true => ButtonVariant::Primary,
                false => ButtonVariant::GhostColor(muted),
            })
            .size(ButtonSize::Sm)
            .rounded(true)
            .disabled(self.keyboard.is_none())
            .debug_selector(|| "computer-keys".to_string())
            .focus_handle(&self.keys_focus)
            .on_click(cx.listener(|pill, _, _, cx| pill.toggle_computer_keys(cx)))
    }

    fn master_meter(&self, cx: &mut Context<Self>) -> Meter {
        Meter::new("master", self.metering.level())
            .horizontal()
            .on_clear_clip(weak_action(cx, |pill: &mut Self, cx| {
                pill.metering.clear_clip();
                cx.notify();
            }))
    }

    /// No play, tempo, record or seek: a tool that needs them draws them in its own page. The
    /// computer keys, while something takes notes, and the master meter stay.
    fn without_arrangement(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .h(px(HEIGHT))
            .occlude()
            .children(self.takes_notes.then(|| self.computer_keys_button(cx)))
            .child(self.master_meter(cx))
            .into_any_element()
    }
}

impl Render for TransportPill {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh(cx);
        if !self.arrangement_is_main {
            return self.without_arrangement(cx);
        }
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
        let can_record = self.keyboard.is_some() || self.audio.can_open();
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
            // Only a project with a fit has this, so every other pill is what it always was.
            .children(steadiness)
            // The computer keys and the click are switches of the composer, not of the mix, so
            // they take no colour: a muted glyph while off, and white under a dark glyph while on.
            .child(self.computer_keys_button(cx))
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
            // What the device plays: the master, and the click beside it.
            .child(self.master_meter(cx))
            .into_any_element()
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
    use crate::{OFFLINE, add_track, main_arrangement, open_or_create_with};

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
