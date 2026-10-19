//! The takes of the `recorder`s of the project. Notes become the record of a `timeline`, with
//! the performance kept under `assets/takes/`; the audio input the record of a `region` of a new
//! file, placed by when each frame was captured. The code of the recorder's parent keeps them
//! where it wants, as it hears them (`FinishedTake`).
//!
//! The recorder says on the audio thread where a take starts and ends and what notes it
//! heard, to the frame of the song (`sound_typescript::Heard`). This follows those and holds the
//! input open while a take records sound.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gpui::{Context, Entity, Task};
use serde_json::{Value, json};
use sound_core::{Clock, EventTap, Frames, InstanceId, Project, Ticks};
use sound_notes::{NoteEvent, RawEvent, RawTake};
use sound_typescript::{ARMED, HEARD, Heard, RECORDER, RecorderState};
use sound_ui::{FinishedTake, LiveSound, LiveTake, Session, every_poll};

use super::live_input::{Hold, LiveInput, Recorded};
use crate::recorder::{
    Placement, RecorderCommand, RecorderReport, StartedTake, TakeRequest, take_place,
    take_seconds_at,
};

/// Polls a finished recording waits for its audio files to hold what was heard up to its end,
/// about two seconds, before it closes them with what they have.
const FINISH_POLLS: u32 = 120;

pub struct Recorders {
    session: Entity<Session>,
    live: Entity<LiveInput>,
    followed: HashMap<InstanceId, Followed>,
    audio: Option<AudioTake>,
    /// Takes of sound that start while another records: each starts once that one is done.
    queued: Vec<(InstanceId, Vec<usize>, u64)>,
    _polling: Task<()>,
}

/// One recorder as this follows it.
struct Followed {
    heard: EventTap<Heard>,
    /// How far its tap was read.
    next: u64,
    /// Whether the audio input is open for it.
    armed: bool,
    take: Option<NotesTake>,
}

/// The notes of a take that runs: where it started, in frames of the song, the clock it began
/// under, and the notes it heard, each at its frame of the song.
struct NotesTake {
    start: u64,
    clock: Arc<Clock>,
    notes: Vec<(u64, NoteEvent)>,
}

/// A recording of the audio input that runs: one file for each recorder that started with it.
struct AudioTake {
    placement: Placement,
    /// Its recorders, each with where its take ended, once it did.
    recorders: BTreeMap<InstanceId, Option<Ticks>>,
    started: Vec<StartedTake>,
    shown: bool,
    /// Polls since a finish was asked for, which waits for the input to bring the end.
    finishing: Option<u32>,
    /// The notes of its recorders that ended, written with its sound as one undo step.
    notes: Vec<(InstanceId, Finished)>,
}

impl Recorders {
    pub fn new(session: Entity<Session>, live: Entity<LiveInput>, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&live, |recorders, _, Recorded(reports), cx| {
            recorders.on_reports(reports, cx)
        })
        .detach();
        let polling = every_poll(cx, Self::poll);
        Self {
            session,
            live,
            followed: HashMap::new(),
            audio: None,
            queued: Vec::new(),
            _polling: polling,
        }
    }

    /// Follows every recorder: what it heard since the last poll, and the take of sound. The
    /// timer calls it; tests call it to skip the wait.
    pub fn poll(&mut self, cx: &mut Context<Self>) {
        let mut records = Vec::new();
        let mut starting = Vec::new();
        let mut ended = Vec::new();
        let recorders: Vec<(InstanceId, EventTap<Heard>, bool, Vec<usize>)> = {
            let project = self.session.read(cx).project();
            let ids = project.instances().filter(|(_, tool)| *tool == RECORDER);
            ids.filter_map(|(id, _)| {
                let heard = project.tap::<Heard>(id, HEARD)?;
                let armed = (project.watches(id).into_iter())
                    .any(|(name, watch)| name == ARMED && watch.get() > 0.0);
                let state = project.state_json(id)?;
                let channels = serde_json::from_str::<RecorderState>(&state)
                    .map(|state| state.channels)
                    .unwrap_or_default();
                Some((id.clone(), heard, armed, channels))
            })
            .collect()
        };
        let clock = Arc::new(self.session.read(cx).project().clock().clone());
        let playhead = self.session.read(cx).playhead().read(cx).tick;
        // A recorder that is gone ends its take where the playhead is: what it heard is kept.
        let gone: Vec<InstanceId> = (self.followed.keys())
            .filter(|id| !recorders.iter().any(|(each, ..)| each == *id))
            .cloned()
            .collect();
        for id in gone {
            let recording = self.session.read(cx).recording().clone();
            recording.update(cx, |recording, cx| {
                recording.set_armed(id.clone(), false, cx)
            });
            if let Some(take) = self.followed.remove(&id).and_then(|followed| followed.take) {
                let frame = take.clock.frame_of(playhead).0;
                records.push((id.clone(), Finished::Notes(take, frame)));
                ended.push((id, playhead));
            }
        }
        let mut missed = Vec::new();
        for (id, heard, armed, channels) in recorders {
            let followed = self.followed.entry(id.clone()).or_insert(Followed {
                heard: heard.clone(),
                next: 0,
                armed: false,
                take: None,
            });
            // Built again with a new tap: what it heard starts over.
            if !followed.heard.is(&heard) {
                *followed = Followed {
                    heard,
                    next: 0,
                    armed: false,
                    take: None,
                };
            }
            let arms = armed && !channels.is_empty();
            if followed.armed != arms {
                followed.armed = arms;
                let recording = self.session.read(cx).recording().clone();
                recording.update(cx, |recording, cx| {
                    recording.set_armed(id.clone(), arms, cx)
                });
            }
            let mut read = Vec::new();
            let tapped = followed.heard.read(followed.next, &mut read);
            followed.next = tapped.next;
            if tapped.missed > 0 && followed.take.is_some() {
                missed.push(id.clone());
            }
            // A change of the tempo map ends a take where the playhead is, as it was heard
            // under the old one. It goes on in a new one from there, sound too.
            if let Some(take) = &followed.take
                && take.clock.tempo_map() != clock.tempo_map()
            {
                if let Some(take) = followed.take.take() {
                    let end = take.clock.frame_of(playhead).0;
                    records.push((id.clone(), Finished::Notes(take, end)));
                }
                let frame = clock.frame_of(playhead).0;
                followed.take = Some(NotesTake {
                    start: frame,
                    clock: clock.clone(),
                    notes: Vec::new(),
                });
                ended.push((id.clone(), playhead));
                if !channels.is_empty() {
                    starting.push((id.clone(), channels.clone(), frame));
                }
            }
            for (frame, heard) in read {
                match heard {
                    Heard::Started => {
                        followed.take = Some(NotesTake {
                            start: frame,
                            clock: clock.clone(),
                            notes: Vec::new(),
                        });
                        if !channels.is_empty() {
                            starting.push((id.clone(), channels.clone(), frame));
                        }
                    }
                    Heard::Note(event) => {
                        if let Some(take) = &mut followed.take {
                            take.notes.push((frame, event));
                        }
                    }
                    Heard::Ended => {
                        if let Some(take) = followed.take.take() {
                            ended.push((id.clone(), take.clock.tick_at(Frames(frame))));
                            records.push((id.clone(), Finished::Notes(take, frame)));
                        }
                    }
                }
            }
        }
        for id in missed {
            let notice = format!(
                "The recorder {id} heard more than it could keep: its take lacks some of what was played."
            );
            self.session
                .update(cx, |session, cx| session.report(notice, cx));
        }
        for (id, end) in ended {
            if let Some(audio) = &mut self.audio
                && let Some(ended) = audio.recorders.get_mut(&id)
            {
                ended.get_or_insert(end);
            }
        }
        // The notes of a recorder that records sound too wait for its sound.
        let (waiting, records): (Vec<_>, Vec<_>) = records.into_iter().partition(|(id, _)| {
            (self.audio.as_ref()).is_some_and(|audio| audio.recorders.contains_key(id))
        });
        if let Some(audio) = &mut self.audio {
            audio.notes.extend(waiting);
        }
        // After the ends: a take that starts where another of the same recorder ends waits
        // for it.
        self.queued.extend(starting);
        self.start_audio(&clock, cx);
        self.poll_audio(cx);
        self.write(records, cx);
    }

    /// Starts one recording of the audio input for the recorders whose take of sound waits.
    /// One runs at a time: those that start while one records wait for it to be done, as at a
    /// loop or a jump, where one take ends and the next starts.
    fn start_audio(&mut self, clock: &Arc<Clock>, cx: &mut Context<Self>) {
        if self.audio.is_some() {
            return;
        }
        let starting = std::mem::take(&mut self.queued);
        let Some((_, _, frame)) = starting.first() else {
            return;
        };
        if !self.live.read(cx).can_open_input() {
            self.session.update(cx, |session, cx| {
                session.report("There is no audio input to record from.", cx)
            });
            return;
        }
        let start = clock.tick_at(Frames(*frame));
        let requests: Vec<TakeRequest> = (starting.iter())
            .filter_map(|(id, channels, _)| {
                let first = *channels.first()?;
                Some(TakeRequest {
                    track: id.clone(),
                    name: format!("{}-sound", id.name()),
                    channels: first..first + channels.len(),
                })
            })
            .collect();
        let recorders = requests.iter().map(|request| (request.track.clone(), None));
        self.audio = Some(AudioTake {
            placement: Placement::new(start, clock.clone()),
            recorders: recorders.collect(),
            started: Vec::new(),
            shown: false,
            finishing: None,
            notes: Vec::new(),
        });
        self.live.update(cx, |live, cx| {
            live.open_input(cx);
            live.record(RecorderCommand::Start(requests));
            live.hold(
                Hold {
                    notes: false,
                    audio: true,
                },
                cx,
            );
        });
    }

    /// Ties the recording to the timeline while it runs, and asks for its end once every
    /// recorder of it ended.
    fn poll_audio(&mut self, cx: &mut Context<Self>) {
        let status = self.session.read(cx).engine_status();
        let timing = self.live.read(cx).timing();
        let input_rate = self.live.read(cx).input_rate();
        let Some(audio) = &mut self.audio else {
            return;
        };
        let ends: Vec<Ticks> = audio.recorders.values().flatten().copied().collect();
        if ends.len() < audio.recorders.len() {
            audio.placement.observe(status);
        }
        let finish = match &mut audio.finishing {
            None if ends.len() == audio.recorders.len() => {
                audio.finishing = Some(0);
                let end = ends.into_iter().max().unwrap_or(audio.placement.start);
                let frames = input_rate.and_then(|rate| {
                    let placement = &audio.placement;
                    placement.input_frames_until(end, timing.as_deref(), rate)
                });
                Some(RecorderCommand::Finish { frames })
            }
            // The input never brought the end: the files close with what they have.
            Some(polls) => {
                *polls += 1;
                (*polls == FINISH_POLLS).then_some(RecorderCommand::Finish { frames: None })
            }
            None => None,
        };
        if let Some(finish) = finish {
            self.live.update(cx, |live, _| live.record(finish));
        }
        self.show_audio(cx);
    }

    /// The waveforms of the takes of sound as they grow, for `now.take`, once the first frames
    /// came and the recording is tied to the timeline.
    fn show_audio(&mut self, cx: &mut Context<Self>) {
        let timing = self.live.read(cx).timing();
        let Some(audio) = self.audio.as_mut().filter(|audio| !audio.shown) else {
            return;
        };
        let clock = audio.placement.clock().clone();
        let (Some(head), false) = (
            audio.placement.head(timing.as_deref()),
            audio.started.is_empty(),
        ) else {
            return;
        };
        audio.shown = true;
        let start = audio.placement.start;
        let start_seconds = take_seconds_at(head, clock.frame_of(start), &clock);
        let shown = audio.started.iter().map(|started| LiveTake {
            track: started.track.clone(),
            start,
            sound: LiveSound {
                overview: started.overview.clone(),
                start_seconds,
            },
        });
        let shown = shown.collect();
        let recording = self.session.read(cx).recording().clone();
        recording.update(cx, |recording, cx| recording.set_takes(shown, cx));
    }

    /// What the recorder of the input reported: about the recording of the recorders, when one
    /// runs.
    fn on_reports(&mut self, reports: &[RecorderReport], cx: &mut Context<Self>) {
        let Some(audio) = &mut self.audio else {
            return;
        };
        let ours = |track: &InstanceId| audio.recorders.contains_key(track);
        let mut finished = None;
        let mut notices = Vec::new();
        for report in reports {
            match report {
                RecorderReport::Started { first_nanos, takes } => {
                    if takes.iter().any(|take| ours(&take.track)) {
                        audio.placement.first_nanos = Some(*first_nanos);
                        audio.started = takes.clone();
                    }
                }
                RecorderReport::Failed { track, error } if ours(track) => {
                    notices.push(format!(
                        "The take of {track} stopped: {error}. What was recorded before is kept."
                    ));
                }
                RecorderReport::Finished { first_nanos, takes }
                    if takes.iter().any(|(track, _)| ours(track)) || takes.is_empty() =>
                {
                    if first_nanos.is_some() {
                        audio.placement.first_nanos = *first_nanos;
                    }
                    finished = Some(takes.clone());
                }
                _ => {}
            }
        }
        for notice in notices {
            self.session
                .update(cx, |session, cx| session.report(notice, cx));
        }
        let Some(takes) = finished else {
            return;
        };
        let Some(audio) = self.audio.take() else {
            return;
        };
        let timing = self.live.read(cx).timing();
        let head = audio.placement.head(timing.as_deref());
        let clock = audio.placement.clock().clone();
        let start = audio.placement.start;
        let records = takes.into_iter().filter_map(|(id, imported)| {
            let end = audio.recorders.get(&id).copied().flatten()?;
            let file = imported.audio.info();
            let (start, file_start, file_end) = take_place(&file, head?, (start, end), &clock)?;
            let mut state = json!({
                "asset": imported.asset.to_string(),
                "start": start.0,
                "file_start_seconds": file_start,
            });
            if let (Some(end), Some(fields)) = (file_end, state.as_object_mut()) {
                fields.insert("file_end_seconds".to_string(), json!(end));
            }
            let name = imported.asset.asset_name().name().to_string();
            Some((id, Finished::Sound(name, state)))
        });
        let records: Vec<_> = audio.notes.into_iter().chain(records).collect();
        self.write(records, cx);
        self.live
            .update(cx, |live, cx| live.hold(Hold::default(), cx));
        let recording = self.session.read(cx).recording().clone();
        recording.update(cx, |recording, cx| recording.set_takes(Vec::new(), cx));
        let clock = Arc::new(self.session.read(cx).project().clock().clone());
        self.start_audio(&clock, cx);
    }

    /// Gives what the takes made to the code of the parents of their recorders, which keeps
    /// them.
    fn write(&mut self, finished: Vec<(InstanceId, Finished)>, cx: &mut Context<Self>) {
        let mut takes = Vec::new();
        for (recorder, finished) in finished {
            let (name, tool, state) = match finished {
                Finished::Notes(take, end) => {
                    match notes_record(self.session.read(cx).project(), &recorder, take, end) {
                        Ok(Some(made)) => made,
                        Ok(None) => continue,
                        Err(error) => {
                            self.session
                                .update(cx, |session, cx| session.report(error, cx));
                            continue;
                        }
                    }
                }
                Finished::Sound(name, state) => (name, "region", state),
            };
            takes.push(FinishedTake {
                recorder,
                name,
                tool,
                state,
            });
        }
        let recording = self.session.read(cx).recording().clone();
        recording.update(cx, |recording, cx| {
            for take in takes {
                recording.finished(take, cx);
            }
        });
    }
}

/// What a take made: notes, ended at a frame of the song, or a file of sound, named, with the
/// record of its region.
enum Finished {
    Notes(NotesTake, u64),
    Sound(String, Value),
}

/// The `timeline` of the notes of a take that ended at `end`, a frame of the song: its name, its
/// tool and its record. The performance is written under `assets/takes/` first, whatever happens
/// to the timeline. `None` when nothing was played.
fn notes_record(
    project: &Project,
    recorder: &InstanceId,
    take: NotesTake,
    end: u64,
) -> Result<Option<(String, &'static str, Value)>, String> {
    let rate = u64::from(take.clock.sample_rate().max(1));
    let micros = |frame: u64| frame * 1_000_000 / rate;
    let start_us = micros(take.start);
    let events = take.notes.iter().filter_map(|(frame, event)| {
        let at = micros(*frame).saturating_sub(start_us);
        raw(*event, at)
    });
    let raw = RawTake {
        start_us,
        end_us: micros(end).max(start_us),
        start_tick: take.clock.tick_at(Frames(take.start)).0,
        end_tick: take.clock.tick_at(Frames(end)).0,
        pedal_at_start: 0,
        events: events.collect(),
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let name = raw
        .write(project.assets())
        .map_err(|error| format!("The notes {recorder} recorded were not kept: {error}"))?;
    let Some(clip) = raw.clip(|micros| take.clock.tick_at_micros(micros)) else {
        return Ok(None);
    };
    let state = json!({
        "start": clip.start.0,
        "length": clip.length,
        "notes": clip.notes,
        "pedal": clip.pedal,
        "bend": clip.bend,
        "mod_wheel": clip.mod_wheel,
        "pressure": clip.pressure,
    });
    Ok(Some((
        format!("{}-{name}", recorder.name()),
        "timeline",
        state,
    )))
}

/// A note event as a raw take keeps it, `at` microseconds into the take. Heard notes have no
/// arrival of their own: they arrived when they sounded.
fn raw(event: NoteEvent, at: u64) -> Option<RawEvent> {
    let (time_us, sounded_us) = (at, at);
    Some(match event {
        NoteEvent::On { pitch, velocity } => RawEvent::On {
            time_us,
            sounded_us,
            pitch: pitch.number(),
            velocity: velocity.value(),
        },
        NoteEvent::Off { pitch } => RawEvent::Off {
            time_us,
            sounded_us,
            pitch: pitch.number(),
            velocity: 0,
        },
        NoteEvent::Pedal(pedal) => RawEvent::Pedal {
            time_us,
            sounded_us,
            value: pedal.value(),
        },
        NoteEvent::Bend(bend) => RawEvent::Bend {
            time_us,
            sounded_us,
            value: bend.value(),
        },
        NoteEvent::ModWheel(amount) => RawEvent::ModWheel {
            time_us,
            sounded_us,
            value: amount.value(),
        },
        NoteEvent::Pressure(amount) => RawEvent::Pressure {
            time_us,
            sounded_us,
            value: amount.value(),
        },
        NoteEvent::Control { .. } | NoteEvent::AllOff => return None,
    })
}
