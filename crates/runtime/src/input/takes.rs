//! The takes of the `recorder`s of the project. Notes become the record of a `timeline`, with
//! the performance kept under `assets/takes/`; the audio input the record of a `region` of a
//! new file, placed by when each frame was captured. The code of the recorder's parent keeps
//! them where it wants, as it hears them ([`Happened::Take`]).
//!
//! The recorder says on the audio thread where a take starts and ends and what notes it heard,
//! to the frame of the song ([`Heard`]). This follows those and holds the input open while a
//! take records sound.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sound_core::{Clock, EngineStatus, EventTap, Frames, InstanceId, Project, Ticks};
use sound_media::TakeOverview;
use sound_notes::{NoteEvent, Pedal, RawEvent, RawTake};
use sound_typescript::next::Take;
use sound_typescript::{ARMED, HEARD, Heard, RECORDER, RecorderState};

use super::recorder::{
    Placement, RecorderCommand, RecorderReport, StartedTake, TakeRequest, take_place,
    take_seconds_at,
};
use super::{Happened, Hold, Input};

/// How long a take that ended waits for its first frames, and then for its audio files to
/// hold what was heard up to its end, before it closes them with what they have.
const FINISH_WAIT: Duration = Duration::from_secs(2);

/// The notes a take that runs heard so far.
#[derive(Clone, Debug, PartialEq)]
pub struct TakeNotes {
    pub recorder: InstanceId,
    pub start: Ticks,
    pub notes: Vec<TakeNote>,
}

/// A note of a take while it records: a held key has no end yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TakeNote {
    pub pitch: u8,
    pub start: Ticks,
    pub end: Option<Ticks>,
}

/// The sound of a take while it records: from where the recording began to the playhead.
#[derive(Clone, Debug)]
pub struct TakeSound {
    pub recorder: InstanceId,
    pub start: Ticks,
    /// What the file holds so far, which grows as it is written.
    pub overview: TakeOverview,
    /// The second of the file the composer heard at `start`: where its waveform lines up.
    pub start_seconds: f64,
}

#[derive(Default)]
pub(super) struct Recorders {
    followed: HashMap<InstanceId, Followed>,
    /// The takes of sound that run, each a recording of the input of its own, by number.
    audio: Vec<AudioTake>,
    /// The number of the next recording of the input.
    next: u64,
    notes: Vec<TakeNotes>,
    sounds: Vec<TakeSound>,
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

/// A take of sound that runs: a recording of the input with one file, for one recorder.
struct AudioTake {
    id: u64,
    recorder: InstanceId,
    placement: Placement,
    /// Where it ended, once it did.
    end: Option<Ticks>,
    started: Vec<StartedTake>,
    /// How far its end is, once it ended.
    ending: Option<Ending>,
    /// The notes of the same take, written with its sound.
    notes: Option<Finished>,
}

/// A take of sound that ended, on its way to being closed.
#[derive(Clone, Copy)]
enum Ending {
    /// Since when it waits to know how many frames of the input hold what was heard up to its
    /// end: its first frames are not there yet.
    Waiting(Instant),
    /// Since when the recorder was asked to close it once those frames came.
    Asked(Instant),
    /// It was asked to close with what it has.
    Closed,
}

/// What the takes of a recorder did, in order: a take of sound starts at a frame of the song,
/// or a take ends at a tick with its notes.
enum Step {
    Start(InstanceId, Vec<usize>, u64),
    End(InstanceId, Ticks, Finished),
}

/// What a take made: notes, ended at a frame of the song, or a file of sound, named, with the
/// record of its region.
enum Finished {
    Notes(NotesTake, u64),
    Sound(String, Value),
}

impl Recorders {
    pub(super) fn is_recording(&self) -> bool {
        !self.audio.is_empty() || self.followed.values().any(|each| each.take.is_some())
    }

    /// The input stays open while a take records sound, and the live notes stay where they
    /// went while one takes notes.
    pub(super) fn hold(&self) -> Hold {
        Hold {
            notes: self.followed.values().any(|each| each.take.is_some()),
            audio: !self.audio.is_empty(),
        }
    }

    pub(super) fn notes_of(&self, recorder: &InstanceId) -> Option<&TakeNotes> {
        self.notes.iter().find(|take| take.recorder == *recorder)
    }

    /// The newest take of `recorder`: at a loop the one before may still wait for its end.
    pub(super) fn sound_of(&self, recorder: &InstanceId) -> Option<&TakeSound> {
        self.sounds
            .iter()
            .rev()
            .find(|take| take.recorder == *recorder)
    }
}

impl Followed {
    fn new(heard: EventTap<Heard>) -> Self {
        Self {
            heard,
            next: 0,
            armed: false,
            take: None,
        }
    }

    /// Reads what its tap heard since the last poll into its take. Says whether some of it was
    /// lost.
    fn hear(
        &mut self,
        id: &InstanceId,
        channels: &[usize],
        clock: &Arc<Clock>,
        steps: &mut Vec<Step>,
    ) -> bool {
        let mut read = Vec::new();
        let tapped = self.heard.read(self.next, &mut read);
        self.next = tapped.next;
        let missed = tapped.missed > 0 && (self.take.is_some() || !read.is_empty());
        for (frame, heard) in read {
            match heard {
                Heard::Started => {
                    // A start while a take runs ends that one there first.
                    self.end(id, frame, steps);
                    self.take = Some(NotesTake {
                        start: frame,
                        clock: clock.clone(),
                        notes: Vec::new(),
                    });
                    if !channels.is_empty() {
                        steps.push(Step::Start(id.clone(), channels.to_vec(), frame));
                    }
                }
                Heard::Note(NoteEvent::AllOff) => {
                    if let Some(take) = &mut self.take {
                        let ends = all_off(&take.notes);
                        take.notes
                            .extend(ends.into_iter().map(|event| (frame, event)));
                    }
                }
                Heard::Note(event) => {
                    if let Some(take) = &mut self.take {
                        take.notes.push((frame, event));
                    }
                }
                Heard::Ended => self.end(id, frame, steps),
            }
        }
        missed
    }

    /// Ends the take that runs at `frame` of the song, under the clock it began with.
    fn end(&mut self, id: &InstanceId, frame: u64, steps: &mut Vec<Step>) {
        if let Some(take) = self.take.take() {
            let tick = take.clock.tick_at(Frames(frame));
            steps.push(Step::End(id.clone(), tick, Finished::Notes(take, frame)));
        }
    }
}

/// A recorder of the project as it is now: its tap, whether it is armed and its channels.
struct Present {
    id: InstanceId,
    heard: EventTap<Heard>,
    armed: bool,
    channels: Vec<usize>,
}

fn recorders_of(project: &Project) -> Vec<Present> {
    let ids = project.instances().filter(|(_, tool)| *tool == RECORDER);
    ids.filter_map(|(id, _)| {
        let heard = project.tap::<Heard>(id, HEARD)?;
        let armed = (project.watches(id).into_iter())
            .any(|(name, watch)| name == ARMED && watch.get() > 0.0);
        let state = project.state_json(id)?;
        let channels = serde_json::from_str::<RecorderState>(&state)
            .map(|state| state.channels)
            .unwrap_or_default();
        Some(Present {
            id: id.clone(),
            heard,
            armed,
            channels,
        })
    })
    .collect()
}

impl Input {
    /// Follows every recorder: what it heard since the last poll, and the takes of sound.
    pub(super) fn poll_recorders(&mut self, project: &mut Project) {
        let Ok(status) = project.engine().poll() else {
            return;
        };
        let present = recorders_of(project);
        let clock = Arc::new(project.clock().clone());
        let playhead = status.playhead_tick;
        let mut steps = Vec::new();
        let mut missed = Vec::new();
        let followed = &mut self.recorders.followed;
        // A recorder that is gone, or built again with a new tap, ends its take where the
        // playhead is, after what its tap still holds: what it heard is kept.
        let gone: Vec<InstanceId> = (followed.iter())
            .filter(|(id, each)| {
                let now = present.iter().find(|present| present.id == **id);
                now.is_none_or(|present| !each.heard.is(&present.heard))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            let Some(mut each) = followed.remove(&id) else {
                continue;
            };
            if each.hear(&id, &[], &clock, &mut steps) {
                missed.push(id.clone());
            }
            if let Some(take) = &each.take {
                let frame = take.clock.frame_of(playhead).0;
                each.end(&id, frame, &mut steps);
            }
            self.armed.remove(&id);
        }
        for Present {
            id,
            heard,
            armed,
            channels,
        } in present
        {
            let each = followed
                .entry(id.clone())
                .or_insert_with(|| Followed::new(heard));
            let arms = armed && !channels.is_empty();
            if each.armed != arms {
                each.armed = arms;
                match arms {
                    true => self.armed.insert(id.clone()),
                    false => self.armed.remove(&id),
                };
            }
            // A change of the tempo map ends a take where the playhead is, as it was heard
            // under the old one. It goes on in a new one from there, sound too.
            if let Some(take) = &each.take
                && take.clock.tempo_map() != clock.tempo_map()
            {
                let end = take.clock.frame_of(playhead).0;
                each.end(&id, end, &mut steps);
                let frame = clock.frame_of(playhead).0;
                each.take = Some(NotesTake {
                    start: frame,
                    clock: clock.clone(),
                    notes: Vec::new(),
                });
                if !channels.is_empty() {
                    steps.push(Step::Start(id.clone(), channels.clone(), frame));
                }
            }
            if each.hear(&id, &channels, &clock, &mut steps) {
                missed.push(id);
            }
        }
        for id in missed {
            self.happened.push(Happened::Notice(format!(
                "The recorder {id} heard more than it could keep: its take lacks some of what was played."
            )));
        }
        let mut finished = Vec::new();
        for step in steps {
            match step {
                Step::Start(id, channels, frame) => self.start_audio(id, &channels, frame, &clock),
                // The notes of a take that records sound too wait for its sound.
                Step::End(id, end, notes) => {
                    let audio = (self.recorders.audio.iter_mut())
                        .find(|audio| audio.recorder == id && audio.end.is_none());
                    match audio {
                        Some(audio) => {
                            audio.end = Some(end);
                            audio.notes = Some(notes);
                        }
                        None => finished.push((id, notes)),
                    }
                }
            }
        }
        self.finish_audio(status);
        self.write(project, finished);
        self.show_notes();
    }

    /// The notes of the takes that run, for what draws them as they grow.
    fn show_notes(&mut self) {
        let followed = self.recorders.followed.iter();
        let shown = followed.filter_map(|(id, each)| Some((id, each.take.as_ref()?)));
        self.recorders.notes = shown
            .map(|(id, take)| TakeNotes {
                recorder: id.clone(),
                start: take.clock.tick_at(Frames(take.start)),
                notes: live_notes(take),
            })
            .collect();
    }

    /// Starts a recording of the audio input for the take of sound of `recorder`, from `frame`
    /// of the song. It runs beside the others: a take that starts where another ends, at a loop
    /// or a jump, has its sound from its first frame.
    fn start_audio(
        &mut self,
        recorder: InstanceId,
        channels: &[usize],
        frame: u64,
        clock: &Arc<Clock>,
    ) {
        let Some(first) = channels.first() else {
            return;
        };
        if !self.audio.can_open() {
            let notice = "There is no audio input to record from.".to_string();
            self.happened.push(Happened::Notice(notice));
            return;
        }
        let recordings = &mut self.recorders;
        let id = recordings.next;
        recordings.next += 1;
        let request = TakeRequest {
            recorder: recorder.clone(),
            name: format!("{}-sound", recorder.name()),
            channels: *first..first + channels.len(),
        };
        recordings.audio.push(AudioTake {
            id,
            recorder,
            placement: Placement::new(clock.tick_at(Frames(frame)), clock.clone()),
            end: None,
            started: Vec::new(),
            ending: None,
            notes: None,
        });
        self.audio.open();
        self.audio.send(RecorderCommand::Start(id, vec![request]));
    }

    /// Ties each recording to the timeline while it runs, and asks for its end once its take
    /// ended.
    fn finish_audio(&mut self, status: EngineStatus) {
        let input_rate = self.audio.sample_rate();
        let timing = self.timing.as_deref();
        let mut finishes = Vec::new();
        for audio in &mut self.recorders.audio {
            let Some(end) = audio.end else {
                audio.placement.observe(status);
                continue;
            };
            let ending = audio.ending.get_or_insert(Ending::Waiting(Instant::now()));
            let frames =
                input_rate.and_then(|rate| (audio.placement).input_frames_until(end, timing, rate));
            let finish = match (*ending, frames) {
                (Ending::Waiting(_), Some(frames)) => {
                    *ending = Ending::Asked(Instant::now());
                    Some(Some(frames))
                }
                // The input never brought the end: the file closes with what it has.
                (Ending::Waiting(since) | Ending::Asked(since), _)
                    if since.elapsed() >= FINISH_WAIT =>
                {
                    *ending = Ending::Closed;
                    Some(None)
                }
                _ => None,
            };
            if let Some(frames) = finish {
                finishes.push(RecorderCommand::Finish {
                    id: audio.id,
                    frames,
                });
            }
        }
        for finish in finishes {
            self.audio.send(finish);
        }
        self.show_audio();
    }

    /// The waveforms of the takes of sound as they grow, once their first frames came and they
    /// are tied to the timeline.
    fn show_audio(&mut self) {
        let timing = self.timing.as_deref();
        let shown = self.recorders.audio.iter().flat_map(|audio| {
            let clock = audio.placement.clock();
            let head = audio.placement.head(timing);
            let start = audio.placement.start;
            let start_seconds =
                head.map(|head| take_seconds_at(head, clock.frame_of(start), clock));
            audio.started.iter().filter_map(move |started| {
                Some(TakeSound {
                    recorder: started.recorder.clone(),
                    start,
                    overview: started.overview.clone(),
                    start_seconds: start_seconds?,
                })
            })
        });
        self.recorders.sounds = shown.collect();
    }

    /// What the recorder of the input reported about the recordings.
    pub(super) fn on_reports(&mut self, project: &mut Project, reports: Vec<RecorderReport>) {
        let mut finished = Vec::new();
        let audio = &mut self.recorders.audio;
        for report in reports {
            match report {
                RecorderReport::Started {
                    id,
                    first_nanos,
                    takes,
                } => {
                    if let Some(audio) = audio.iter_mut().find(|audio| audio.id == id) {
                        audio.placement.first_nanos = Some(first_nanos);
                        audio.started = takes;
                    }
                }
                RecorderReport::Failed { recorder, error } => {
                    self.happened.push(Happened::Notice(format!(
                        "The take of {recorder} stopped: {error}. What was recorded before is kept."
                    )));
                }
                RecorderReport::Behind { frames } => self.happened.push(Happened::Notice(format!(
                    "Recording fell behind the input: the takes hold {frames} frames of silence where it could not keep up."
                ))),
                RecorderReport::Finished {
                    id,
                    first_nanos,
                    takes,
                } => {
                    let Some(index) = audio.iter().position(|audio| audio.id == id) else {
                        continue;
                    };
                    let mut take = audio.remove(index);
                    if first_nanos.is_some() {
                        take.placement.first_nanos = first_nanos;
                    }
                    finished.push((take, takes));
                }
                // No sound comes: every take ends with its notes.
                RecorderReport::NoInput => {
                    finished.extend(audio.drain(..).map(|take| (take, Vec::new())));
                }
            }
        }
        if finished.is_empty() {
            return;
        }
        let timing = self.timing.as_deref();
        let mut records = Vec::new();
        for (audio, takes) in finished {
            let head = audio.placement.head(timing);
            let clock = audio.placement.clock().clone();
            let start = audio.placement.start;
            records.extend(audio.notes.map(|notes| (audio.recorder.clone(), notes)));
            let sound = takes.into_iter().filter_map(|(id, imported)| {
                let file = imported.audio.info();
                let (start, file_start, file_end) =
                    take_place(&file, head?, (start, audio.end?), &clock)?;
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
            records.extend(sound);
        }
        self.write(project, records);
        self.show_audio();
    }

    /// Gives what the takes made to the code of the parents of their recorders, which keeps
    /// them.
    fn write(&mut self, project: &Project, finished: Vec<(InstanceId, Finished)>) {
        for (recorder, finished) in finished {
            let (name, tool, state) = match finished {
                Finished::Notes(take, end) => match notes_record(project, &recorder, take, end) {
                    Ok(Some(made)) => made,
                    Ok(None) => continue,
                    Err(error) => {
                        self.happened.push(Happened::Notice(error));
                        continue;
                    }
                },
                Finished::Sound(name, state) => (name, "region", state),
            };
            self.happened.push(Happened::Take(Take {
                recorder,
                name,
                tool,
                state,
            }));
        }
    }
}

/// The notes of `take` from its keys going down and up, in the order they went down.
fn live_notes(take: &NotesTake) -> Vec<TakeNote> {
    let mut notes = Vec::new();
    let mut down: HashMap<u8, Vec<usize>> = HashMap::new();
    for (frame, event) in &take.notes {
        let tick = take.clock.tick_at(Frames(*frame));
        match event {
            NoteEvent::On { pitch, .. } => {
                down.entry(pitch.number()).or_default().push(notes.len());
                notes.push(TakeNote {
                    pitch: pitch.number(),
                    start: tick,
                    end: None,
                });
            }
            NoteEvent::Off { pitch } => {
                let first = (down.get_mut(&pitch.number())).and_then(|held| held.pop());
                if let Some(note) = first.and_then(|index| notes.get_mut(index)) {
                    note.end = Some(tick);
                }
            }
            _ => {}
        }
    }
    notes
}

/// What ends what `notes` holds: an off for each key down, and the pedal up when it is down.
fn all_off(notes: &[(u64, NoteEvent)]) -> Vec<NoteEvent> {
    let mut down = BTreeMap::new();
    let mut pedal = false;
    for (_, event) in notes {
        match event {
            NoteEvent::On { pitch, .. } => *down.entry(pitch.number()).or_insert(0) += 1,
            NoteEvent::Off { pitch } => {
                if let Some(count) = down.get_mut(&pitch.number()) {
                    *count -= 1;
                    if *count == 0 {
                        down.remove(&pitch.number());
                    }
                }
            }
            NoteEvent::Pedal(value) => pedal = value.is_down(),
            _ => {}
        }
    }
    let offs = notes.iter().filter_map(|(_, event)| match event {
        NoteEvent::On { pitch, .. } if down.remove(&pitch.number()).is_some() => {
            Some(NoteEvent::Off { pitch: *pitch })
        }
        _ => None,
    });
    let mut ends: Vec<NoteEvent> = offs.collect();
    if pedal {
        ends.push(NoteEvent::Pedal(Pedal::UP));
    }
    ends
}

/// The `timeline` of the notes of a take that ended at `end`, a frame of the song: its name,
/// its tool and its record. The performance is written under `assets/takes/` first, whatever
/// happens to the timeline. `None` when nothing was played.
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
    let state = sound_typescript::timeline_state(&clip, "")?;
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

#[cfg(test)]
mod tests {
    use sound_notes::{Pedal, Pitch, Velocity};

    use super::*;

    #[test]
    fn all_off_ends_the_keys_still_down_and_the_pedal() {
        let pitch = |number| Pitch::new(number).unwrap();
        let on = |number| NoteEvent::On {
            pitch: pitch(number),
            velocity: Velocity::new(100).unwrap(),
        };
        let notes = [
            (0, on(60)),
            (1, on(62)),
            (2, NoteEvent::Off { pitch: pitch(60) }),
            (3, NoteEvent::Pedal(Pedal::new(127).unwrap())),
        ];
        assert_eq!(
            all_off(&notes),
            [
                NoteEvent::Off { pitch: pitch(62) },
                NoteEvent::Pedal(Pedal::UP)
            ]
        );
    }
}
