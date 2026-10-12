//! The record control: a take from its start to its clips, of MIDI and of audio, and the
//! audio input it records from.

use std::sync::Arc;

use arrangement::{TrackKind, TrackState};
use gpui::{Context, prelude::*};
use sound_core::{Changes, Clock, Instance, InstanceId, Ticks};
use sound_media::Imported;
use sound_ui::{LiveBody, LiveSound, LiveTake, Playhead};

use super::{AudioTake, Ended, Take, TransportPill};
use crate::recorder::{Placement, RecorderCommand, RecorderReport, TakeRequest, take_seconds_at};
use crate::window::audio_input::OpenedInput;
use crate::window::recording;

/// Polls a finished recording waits for its audio files to hold what was heard up to its end,
/// about two seconds, before it closes them with what they have.
const FINISH_POLLS: u32 = 120;

impl TransportPill {
    /// The notes of the MIDI take so far, a held key up to the playhead, for the timeline.
    pub(super) fn show_notes(&mut self, cx: &mut Context<Self>) {
        let Some(take) = self.take.as_ref().filter(|take| take.ended.is_none()) else {
            return;
        };
        let (Some(track), Some(keyboard)) = (&take.midi_track, &self.keyboard) else {
            return;
        };
        let until = self.playhead.read(cx).tick;
        let notes = keyboard
            .keys_so_far(until)
            .and_then(|keys| keys.clip(&take.clock));
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
        // The last one is still being written: its clips come first.
        if self.take.is_some() {
            return;
        }
        let session = self.session.clone();
        let selected = session.read(cx).selected().cloned();
        let midi_track = recording::target_track(session.read(cx).project(), selected.as_ref());
        let clock = Arc::new(session.read(cx).project().clock().clone());
        let live = midi_track.iter().filter(|_| self.keyboard.is_some());
        let live = live.map(|track| LiveTake {
            track: track.id().clone(),
            start: tick,
            body: LiveBody::Notes(None),
        });
        let live = live.collect();
        self.recording
            .update(cx, |recording, cx| recording.set_takes(live, cx));
        let audio_tracks = self.tracks_to_record(selected.as_ref(), cx);
        let audio = match audio_tracks.is_empty() {
            true => None,
            false => self.start_audio(audio_tracks, tick, clock.clone(), cx),
        };
        if let Some(keyboard) = self.keyboard.as_mut() {
            keyboard.start_recording(tick);
        }
        self.take = Some(Take {
            start: tick,
            clock,
            midi_track,
            audio,
            ended: None,
        });
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
        if !self.audio.can_open() {
            let error = sound_core::DeviceError::NoInputDevice;
            self.session
                .update(cx, |session, cx| session.report(error, cx));
            return None;
        }
        // The takes start as soon as the input is open: the start waits for the recorder.
        self.open_input(cx);
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
        self.audio.send(RecorderCommand::Start(requests));
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
        let session = self.session.clone();
        let timing = self.timing.clone();
        let input_rate = self.audio.sample_rate();
        let Some(take) = self.take.as_mut().filter(|take| take.ended.is_none()) else {
            return;
        };
        let mut midi = None;
        if let Some(keyboard) = self.keyboard.as_mut() {
            // The last block of the take is still in the ring.
            let polled = session.update(cx, |session, _| {
                keyboard.poll(session.engine(), timing.as_deref())
            });
            if let Err(error) = polled {
                session.update(cx, |session, cx| session.report(error, cx));
            }
            // Nothing was played: no clip and no file.
            if let Some(played) = keyboard
                .finish_recording(until)
                .filter(|take| !take.is_empty())
            {
                // The performance first, and whatever happens to the clip. It is the only copy
                // of what the composer played, and a clip can fail to be made: its track may be
                // gone.
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
        }
        if let Some(audio) = &take.audio {
            let frames = input_rate.and_then(|rate| {
                let placement = &audio.placement;
                placement.input_frames_until(until, timing.as_deref(), rate)
            });
            self.audio.send(RecorderCommand::Finish { frames });
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
            Some(audio) => audio.placement.clips(&audio_takes, end, timing.as_deref()),
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
        self.follow_input(cx);
    }

    /// Opens the input on the background executor, when it is not open or on its way.
    fn open_input(&mut self, cx: &mut Context<Self>) {
        let Some((opener, generation)) = self.audio.start_opening() else {
            return;
        };
        let work = cx.background_spawn(async move { opener() });
        cx.spawn(async move |pill, cx| {
            let opened = work.await;
            match pill.update(cx, |pill, cx| pill.input_opened(generation, opened, cx)) {
                Ok(()) => {}
                // A window that went away takes the input with it.
                Err(_) => {}
            }
        })
        .detach();
    }

    /// The input opened, or why not. With no input, nothing is armed and no audio records.
    fn input_opened(
        &mut self,
        generation: u64,
        mut opened: Result<OpenedInput, sound_core::DeviceError>,
        cx: &mut Context<Self>,
    ) {
        let assets = self.session.read(cx).project().assets().clone();
        let notice = opened.as_mut().ok().and_then(|input| input.notice.take());
        match self.audio.opened(generation, opened, &assets, cx) {
            None => {}
            Some(Ok((channels, live))) => {
                if let Some(notice) = notice {
                    self.session
                        .update(cx, |session, cx| session.report(notice, cx));
                }
                let plays = live.sample_rate() == self.session.read(cx).project().sample_rate();
                // The engine reads it while the input is open, whether a connection hears it
                // or not, so it never falls behind.
                self.session.update(cx, |session, cx| {
                    session.background(cx, |project| project.set_live_input(Some(live)))
                });
                // Opened for a connection alone, an input at another rate plays nothing. The
                // problem of the connection says why, and the device closes. A track records
                // at any rate.
                let armed = self.recording.read(cx).armed().next().is_some();
                let records = self.take.as_ref().is_some_and(|take| take.audio.is_some());
                if !plays && !armed && !records {
                    self.audio.give_up(cx);
                    return;
                }
                self.recording.update(cx, |recording, cx| {
                    recording.set_input_channels(Some(channels), cx)
                });
            }
            Some(Err(error)) => {
                self.session
                    .update(cx, |session, cx| session.report(error, cx));
                // Armed with no input would show a level that never moves.
                self.recording.update(cx, |recording, cx| {
                    recording.retain_armed(|_| false, cx);
                    recording.set_audio_takes(Vec::new(), cx);
                });
                let ended = self.take.as_mut().is_some_and(|take| {
                    take.audio = None;
                    take.ended.is_some()
                });
                // A take that ended while the input opened waited for audio that never comes.
                if ended {
                    self.make_clips(Vec::new(), cx);
                }
            }
        }
    }

    /// The input is open while a track is armed, audio records or a `project.json` connection
    /// in the graph hears the device input, and closed otherwise, so the device is not held and
    /// macOS shows no microphone in use. One that failed or went away is closed and not opened
    /// again here until something new wants it: the next arming, or a connection that comes,
    /// opens the input there is then.
    pub(super) fn follow_input(&mut self, cx: &mut Context<Self>) {
        let armed: Vec<InstanceId> = self.recording.read(cx).armed().cloned().collect();
        let records = self.take.as_ref().is_some_and(|take| take.audio.is_some());
        let heard = self.session.read(cx).project().hears_device_input();
        if self.audio.wants(armed, heard) && !self.audio.is_gone() {
            self.open_input(cx);
        } else if !records && self.audio.is_open_or_opening() {
            self.audio.close(cx);
            self.recording
                .update(cx, |recording, cx| recording.set_input_channels(None, cx));
            self.session.update(cx, |session, cx| {
                session.background(cx, |project| project.set_live_input(None))
            });
        }
    }

    /// One poll of the audio input: the level, a device that went away, what ties a take to
    /// the timeline, and a run of the recorder on the background executor.
    pub(super) fn poll_audio(&mut self, cx: &mut Context<Self>) {
        if let Some((levels, silent)) = self.audio.poll_levels() {
            self.recording
                .update(cx, |recording, cx| recording.set_levels(levels, cx));
            if silent {
                let notice = "The audio input gives nothing but silence. If it is a microphone, allow Sound Tools, or the terminal it runs from, in System Settings, Privacy & Security, Microphone, then open the project again.";
                self.session
                    .update(cx, |session, cx| session.report(notice, cx));
            }
        }
        if self.audio.went_away() {
            self.input_gone(cx);
        }
        let status = self.session.read(cx).engine_status();
        if let Some(take) = &mut self.take {
            // Only while it records: after its end the engine may play under another tempo.
            if let Some(audio) = take.audio.as_mut().filter(|_| take.ended.is_none()) {
                audio.placement.observe(status);
            }
            // The input never brought the end: the files close with what they have.
            if let Some(ended) = &mut take.ended {
                ended.polls += 1;
                if ended.polls == FINISH_POLLS {
                    self.audio.send(RecorderCommand::Finish { frames: None });
                }
            }
        }
        self.show_takes(cx);
        self.run_recorder(cx);
    }

    /// The input went away, such as an interface that was unplugged. A take that records ends
    /// here and keeps what was recorded, and every track is disarmed: arming again opens the
    /// default input that macOS has then.
    fn input_gone(&mut self, cx: &mut Context<Self>) {
        let records = self.take.as_ref().is_some_and(|take| take.audio.is_some());
        let ends_a_take = self.is_recording() && records;
        if ends_a_take {
            let tick = self.playhead.read(cx).tick;
            self.finish_recording(tick, cx);
        }
        let notice = match ends_a_take {
            true => "The audio input went away. The take ends here and keeps what was recorded.",
            false => "The audio input went away.",
        };
        self.session
            .update(cx, |session, cx| session.report(notice, cx));
        self.recording
            .update(cx, |recording, cx| recording.retain_armed(|_| false, cx));
        // The recorder writes the last of the take first; the input closes once it is done.
        if !records {
            self.follow_input(cx);
        }
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

    /// Runs the recorder once on the background executor, when it is here: it takes what the
    /// input captured, writes the takes and says what came of it.
    fn run_recorder(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.audio.start_run() else {
            return;
        };
        let work = cx.background_spawn(async move { run.run() });
        cx.spawn(async move |pill, cx| {
            let (recorder, reports, generation) = work.await;
            let ended = pill.update(cx, |pill, cx| {
                pill.audio.end_run(recorder, generation);
                pill.on_reports(reports, cx);
            });
            match ended {
                Ok(()) => {}
                // A window that went away takes the recorder with it, and its files close.
                Err(_) => {}
            }
        })
        .detach();
    }

    fn on_reports(&mut self, reports: Vec<RecorderReport>, cx: &mut Context<Self>) {
        for report in reports {
            match report {
                RecorderReport::Started { first_nanos, takes } => {
                    if let Some(audio) = self.take.as_mut().and_then(|take| take.audio.as_mut()) {
                        audio.placement.first_nanos = Some(first_nanos);
                        audio.started = takes;
                    }
                }
                RecorderReport::Failed { track, error } => {
                    let project = self.session.read(cx).project();
                    let name = project
                        .resolve::<TrackState>(&track)
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
                        audio.placement.first_nanos = first_nanos;
                    }
                    self.make_clips(takes, cx);
                }
            }
        }
    }
}
