//! The record control: a take from its start to its clips, of MIDI and of audio, from the
//! window's live input.

use std::sync::Arc;

use arrangement::{TrackKind, TrackState};
use gpui::Context;
use sound_core::{Changes, Clock, Instance, InstanceId, Ticks};
use sound_media::Imported;
use sound_ui::{LiveBody, LiveSound, LiveTake, Playhead};

use super::{Shown, Take, TransportPill};
use crate::arrangement_wiring::recording;
use crate::recorder::{
    Placement, RecorderCommand, RecorderReport, StartedTake, TakeRequest, take_seconds_at,
};
use crate::window::live_input::{Hold, InputEvent, LiveInput};

/// Polls a finished recording waits for its audio files to hold what was heard up to its end,
/// about two seconds, before it closes them with what they have.
const FINISH_POLLS: u32 = 120;

pub(super) struct AudioTake {
    placement: Placement,
    started: Vec<StartedTake>,
    /// The waveforms of the takes are lined up with the timeline.
    shown: bool,
}

pub(super) struct Ended {
    end: Ticks,
    /// The raw MIDI take, already on disk, and its name, for the clip.
    midi: Option<(Instance<TrackState>, midi::Take, Option<String>)>,
    polls: u32,
}

impl TransportPill {
    /// The notes of the MIDI take so far, a held key up to the playhead, for the timeline.
    pub(super) fn show_notes(&mut self, cx: &mut Context<Self>) {
        let Some(take) = self.take.as_ref().filter(|take| take.ended.is_none()) else {
            return;
        };
        let (Some(track), clock) = (take.midi_track.clone(), take.clock.clone()) else {
            return;
        };
        let until = self.playhead.read(cx).tick;
        let notes = self.live.update(cx, |live, _| {
            let keys = live.keyboard()?.keys_so_far(until)?;
            keys.clip(&clock)
        });
        self.recording.update(cx, |recording, cx| {
            recording.show_notes(track.id(), notes, cx)
        });
    }

    /// Whether a recording runs: from the record control until it is pressed again, or a
    /// stop, a pause or a seek ends it.
    pub fn is_recording(&self) -> bool {
        self.take.as_ref().is_some_and(|take| take.ended.is_none())
    }

    /// The record control and the `r` key: start recording from the playhead, or end the take.
    ///
    /// A recording takes every armed audio track, and the MIDI input records onto the selected
    /// track when it is an instrument track, or the first one when nothing is selected, as it
    /// always did. With nothing armed and an audio track selected, that track is armed first.
    /// Starting also starts playback: the playhead has to move for a take to have any length.
    /// Ending leaves playback as it is, so a composer can go on listening.
    pub fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        let Playhead { playing, tick, .. } = *self.playhead.read(cx);
        if self.is_recording() {
            self.finish_recording(tick, cx);
            return;
        }
        // The last one is still being written: its clips come first. Without the arrangement
        // in the main area there is nothing to record onto.
        if self.take.is_some() || self.shown != Shown::Arrangement {
            return;
        }
        let session = self.session.clone();
        let selected = session.read(cx).selected().cloned();
        let midi_track = recording::target_track(session.read(cx).project(), selected.as_ref());
        let clock = Arc::new(session.read(cx).project().clock().clone());
        let has_keyboard = self.live.read(cx).midi_input().is_some();
        let shown = midi_track.iter().filter(|_| has_keyboard);
        let shown = shown.map(|track| LiveTake {
            track: track.id().clone(),
            start: tick,
            body: LiveBody::Notes(None),
        });
        let shown = shown.collect();
        self.recording
            .update(cx, |recording, cx| recording.set_takes(shown, cx));
        let audio_tracks = self.tracks_to_record(selected.as_ref(), cx);
        let audio = match audio_tracks.is_empty() {
            true => None,
            false => self.start_audio(audio_tracks, tick, clock.clone(), cx),
        };
        self.live.update(cx, |live, _| {
            if let Some(keyboard) = live.keyboard() {
                keyboard.start_recording(tick);
            }
        });
        self.take = Some(Take {
            start: tick,
            clock,
            midi_track,
            audio,
            ended: None,
        });
        self.hold_live(cx);
        if !playing {
            session.update(cx, |session, _| session.engine().play());
        }
        cx.notify();
    }

    /// The armed audio tracks, in the order of their ids. With none armed and an audio track
    /// selected, it arms that one.
    fn tracks_to_record(
        &mut self,
        selected: Option<&InstanceId>,
        cx: &mut Context<Self>,
    ) -> Vec<Instance<TrackState>> {
        let project = self.session.read(cx).project();
        let audio_track = |id: &InstanceId| {
            let track = project.resolve::<TrackState>(id)?;
            let state = project.state(&track)?;
            (state.kind == TrackKind::Audio).then_some(track)
        };
        let armed: Vec<_> = self
            .recording
            .read(cx)
            .armed()
            .filter_map(audio_track)
            .collect();
        if !armed.is_empty() {
            return armed;
        }
        let Some(track) = selected.and_then(audio_track) else {
            return Vec::new();
        };
        let id = track.id().clone();
        self.recording
            .update(cx, |recording, cx| recording.set_armed(id, true, cx));
        vec![track]
    }

    /// Starts a take on each of these audio tracks, each from the channels its record names.
    /// `None` when the input cannot be opened, which the notice says.
    fn start_audio(
        &mut self,
        tracks: Vec<Instance<TrackState>>,
        start: Ticks,
        clock: Arc<Clock>,
        cx: &mut Context<Self>,
    ) -> Option<AudioTake> {
        if !self.live.read(cx).can_open_input() {
            let error = sound_core::DeviceError::NoInputDevice;
            self.session
                .update(cx, |session, cx| session.report(error, cx));
            return None;
        }
        // The takes start as soon as the input is open: the start waits for the recorder.
        self.live.update(cx, LiveInput::open_input);
        let project = self.session.read(cx).project();
        let requests: Vec<_> = tracks
            .iter()
            .filter_map(|track| {
                let state = project.state(track)?;
                Some(TakeRequest {
                    track: track.id().clone(),
                    name: format!("{}-take", state.name),
                    channels: state.input.device_channels(),
                })
            })
            .collect();
        let live = requests.iter().map(|request| LiveTake {
            track: request.track.clone(),
            start,
            body: LiveBody::Audio(None),
        });
        let live = live.collect();
        self.recording
            .update(cx, |recording, cx| recording.set_audio_takes(live, cx));
        self.live
            .update(cx, |live, _| live.record(RecorderCommand::Start(requests)));
        Some(AudioTake {
            placement: Placement::new(start, clock),
            started: Vec::new(),
            shown: false,
        })
    }

    /// Ends the take at `until`. The raw MIDI take is written at once and never touched again;
    /// the clips, of MIDI and audio alike, are one undo step once the audio files hold what was
    /// heard up to `until`, which the input brings a little later.
    pub(super) fn finish_recording(&mut self, until: Ticks, cx: &mut Context<Self>) {
        if !self.is_recording() {
            return;
        }
        let session = self.session.clone();
        let timing = self.timing.clone();
        let input_rate = self.live.read(cx).input_rate();
        // The last block of the take is still in the ring. Nothing played: no clip and no file.
        let played = self.live.update(cx, |live, cx| {
            let keyboard = live.keyboard()?;
            let polled = session.update(cx, |session, _| {
                keyboard.poll(session.engine(), timing.as_deref())
            });
            if let Err(error) = polled {
                session.update(cx, |session, cx| session.report(error, cx));
            }
            keyboard
                .finish_recording(until)
                .filter(|take| !take.is_empty())
        });
        let Some(take) = self.take.as_mut().filter(|take| take.ended.is_none()) else {
            return;
        };
        let mut midi = None;
        if let Some(played) = played {
            // The performance first, and whatever happens to the clip. It is the only copy of
            // what the composer played, and a clip can fail to be made: its track may be gone.
            let written = recording::write_take(session.read(cx).project(), &played);
            let name = match written {
                Ok(name) => Some(name),
                Err(error) => {
                    session.update(cx, |session, cx| session.report(error, cx));
                    None
                }
            };
            midi = take.midi_track.clone().map(|track| (track, played, name));
        }
        if let Some(audio) = &take.audio {
            let frames = input_rate.and_then(|rate| {
                let placement = &audio.placement;
                placement.input_frames_until(until, timing.as_deref(), rate)
            });
            self.live.update(cx, |live, _| {
                live.record(RecorderCommand::Finish { frames })
            });
        }
        let waits = take.audio.is_some();
        take.ended = Some(Ended {
            end: until,
            midi,
            polls: 0,
        });
        cx.notify();
        if !waits {
            self.make_clips(Vec::new(), cx);
        }
    }

    /// The clips of the ended take, of MIDI and of audio, as one undo step. The files of the
    /// audio are held in memory until then, so the tracks read nothing.
    fn make_clips(&mut self, audio_takes: Vec<(InstanceId, Imported)>, cx: &mut Context<Self>) {
        let Some(Take {
            audio,
            ended,
            clock,
            ..
        }) = self.take.take()
        else {
            return;
        };
        self.recording
            .update(cx, |recording, cx| recording.set_takes(Vec::new(), cx));
        let Some(Ended { end, midi, .. }) = ended else {
            return;
        };
        let timing = self.timing.clone();
        let clips = match &audio {
            Some(audio) => {
                recording::take_clips(&audio.placement, &audio_takes, end, timing.as_deref())
            }
            None => Vec::new(),
        };
        // A track deleted while it recorded gets no clip. Its raw take is on disk already, and
        // the clips of the other tracks are made all the same.
        let project = self.session.read(cx).project();
        let (midi, gone) = match midi {
            Some(midi) if project.state(&midi.0).is_none() => (None, true),
            midi => (midi, false),
        };
        if gone {
            let notice = "The track of the MIDI take went away while it recorded. What was played is kept under assets/takes/.";
            self.session
                .update(cx, |session, cx| session.report(notice, cx));
        }

        if clips.len() < audio_takes.len() {
            let missing = audio_takes.len() - clips.len();
            let notice = format!(
                "{missing} of the takes held nothing heard while the project played, and became no clip. Their files are in assets/audio/."
            );
            self.session
                .update(cx, |session, cx| session.report(notice, cx));
        }
        if midi.is_some() || !clips.is_empty() {
            self.session.update(cx, |session, cx| {
                session.edit(cx, |project| {
                    let mut changes = Changes::new();
                    if let Some((track, played, name)) = &midi {
                        let take = (played, name.clone());
                        recording::add_take_clip(project, &mut changes, track, take, &clock)?;
                    }
                    recording::add_audio_take_clips(project, &mut changes, clips)?;
                    project.commit(recording::LABEL, changes)
                })
            });
        }
        drop(audio_takes);
        self.hold_live(cx);
    }

    /// What the take holds of the window's live input: its notes stay where it began, and the
    /// input stays open while it records audio.
    fn hold_live(&mut self, cx: &mut Context<Self>) {
        let hold = Hold {
            notes: self.take.is_some(),
            audio: (self.take.as_ref()).is_some_and(|take| take.audio.is_some()),
        };
        self.live.update(cx, |live, cx| live.hold(hold, cx));
    }

    /// The input failed to open, or went away. A take that records audio ends here and keeps
    /// what was recorded.
    pub(super) fn input_did(&mut self, event: InputEvent, cx: &mut Context<Self>) {
        match event {
            InputEvent::Failed => {
                let ended = self.take.as_mut().is_some_and(|take| {
                    take.audio = None;
                    take.ended.is_some()
                });
                self.hold_live(cx);
                // A take that ended while the input opened waited for audio that never comes.
                if ended {
                    self.make_clips(Vec::new(), cx);
                }
            }
            InputEvent::Gone => {
                let records = (self.take.as_ref()).is_some_and(|take| take.audio.is_some());
                if !records {
                    return;
                }
                let ends_a_take = self.is_recording();
                if ends_a_take {
                    let tick = self.playhead.read(cx).tick;
                    self.finish_recording(tick, cx);
                }
                let notice = match ends_a_take {
                    true => {
                        "The audio input went away. The take ends here and keeps what was recorded."
                    }
                    false => "The audio input went away.",
                };
                self.session
                    .update(cx, |session, cx| session.report(notice, cx));
            }
        }
    }

    /// One poll of a take's audio: what ties it to the timeline, and its waveform.
    pub(super) fn poll_audio(&mut self, cx: &mut Context<Self>) {
        let status = self.session.read(cx).engine_status();
        let mut gave_up = false;
        if let Some(take) = &mut self.take {
            // Only while it records: after its end the engine may play under another tempo.
            if let Some(audio) = take.audio.as_mut().filter(|_| take.ended.is_none()) {
                audio.placement.observe(status);
            }
            // The input never brought the end: the files close with what they have.
            if let Some(ended) = &mut take.ended {
                ended.polls += 1;
                gave_up = ended.polls == FINISH_POLLS;
            }
        }
        if gave_up {
            self.live.update(cx, |live, _| {
                live.record(RecorderCommand::Finish { frames: None })
            });
        }
        self.show_takes(cx);
    }

    /// Lines the waveforms of the takes up with the timeline, once the first frames came and
    /// the recording is tied to it.
    fn show_takes(&mut self, cx: &mut Context<Self>) {
        let timing = self.timing.clone();
        let Some(take) = &mut self.take else {
            return;
        };
        let Some(audio) = take.audio.as_mut().filter(|audio| !audio.shown) else {
            return;
        };
        let clock = audio.placement.clock().clone();
        let Some(head) = audio.placement.head(timing.as_deref()) else {
            return;
        };
        if audio.started.is_empty() {
            return;
        }
        audio.shown = true;
        let start_seconds = take_seconds_at(head, clock.frame_of(take.start), &clock);
        let live = audio.started.iter().map(|started| LiveTake {
            track: started.track.clone(),
            start: take.start,
            body: LiveBody::Audio(Some(LiveSound {
                overview: started.overview.clone(),
                start_seconds,
            })),
        });
        let live = live.collect();
        self.recording
            .update(cx, |recording, cx| recording.set_audio_takes(live, cx));
    }

    /// What the recorder of the input reported: about a take of the pill, when one runs.
    pub(super) fn on_reports(&mut self, reports: &[RecorderReport], cx: &mut Context<Self>) {
        if self.take.as_ref().is_none_or(|take| take.audio.is_none()) {
            return;
        }
        for report in reports {
            match report {
                RecorderReport::Started { first_nanos, takes } => {
                    if let Some(audio) = self.take.as_mut().and_then(|take| take.audio.as_mut()) {
                        audio.placement.first_nanos = Some(*first_nanos);
                        audio.started = takes.clone();
                    }
                }
                RecorderReport::Failed { track, error } => {
                    let project = self.session.read(cx).project();
                    let name = project
                        .resolve::<TrackState>(track)
                        .and_then(|track| Some(project.state(&track)?.name.clone()))
                        .unwrap_or_else(|| track.to_string());
                    let notice = format!(
                        "The take of {name} stopped: {error}. What was recorded before is kept."
                    );
                    self.session
                        .update(cx, |session, cx| session.report(notice, cx));
                }
                RecorderReport::Behind { frames } => {
                    let notice = format!(
                        "Recording fell behind the input and lost {frames} frames. The take has silence there and stays in time."
                    );
                    self.session
                        .update(cx, |session, cx| session.report(notice, cx));
                }
                RecorderReport::Finished { first_nanos, takes } => {
                    if let Some(audio) = self.take.as_mut().and_then(|take| take.audio.as_mut())
                        && first_nanos.is_some()
                    {
                        audio.placement.first_nanos = *first_nanos;
                    }
                    self.make_clips(takes.clone(), cx);
                }
            }
        }
    }
}
