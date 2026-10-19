//! Recording audio: from the input to a take file per armed track, and from a take file to a
//! clip where the composer heard it.
//!
//! Three threads and no waiting, as for MIDI. The device thread puts what it captured into a
//! ring, stamped with when it was captured ([`sound_core::capture`]). A [`Recorder`] takes it
//! from there away from the thread that draws and writes one WAV file per armed track. The
//! thread that draws then places each take as a clip: the moment a frame was captured is the
//! moment the composer played it, the timing of the output device says which engine frame was
//! sounding then, and the engine status says which project frame that engine frame was.
//! Nothing of this is on the audio thread, and the recorder plays nothing.

use std::ops::Range;
use std::sync::Arc;

use sound_core::{
    Assets, CaptureReader, Clock, EngineStatus, Frames, InstanceId, StreamTiming, Ticks,
};
use sound_media::{AudioAsset, Imported, Info, MediaError, TakeFile, TakeOverview};

/// One armed track of a recording: which channels of the input it records, and the name its
/// file is made from, such as `voice-take`, which becomes `voice-take-1.wav`.
#[derive(Clone, Debug)]
pub struct TakeRequest {
    pub track: InstanceId,
    pub name: String,
    /// Channels of the input, counted from 0: one for a mono take, two for a stereo one.
    pub channels: Range<usize>,
}

/// What the thread that draws asks of a [`Recorder`]. Each recording has a number of its own:
/// several run at once over the one input, as when a second recorder starts while one records,
/// or a take starts again at a loop while the one before waits for its last frames.
#[derive(Debug)]
pub enum RecorderCommand {
    /// Begin recording `.0`: a take on each of these tracks, all from the next frame of the
    /// input.
    Start(u64, Vec<TakeRequest>),
    /// End recording `id` and close every file of it: once the takes hold `frames` frames of
    /// the input, the last one heard before the end, or at once when `None`. What the input
    /// brings past those frames is not written. The input may be late by its latency and the
    /// window's poll, so this waits for what was played up to the end. A recording whose input
    /// is gone ends at once with what it has.
    Finish { id: u64, frames: Option<u64> },
}

/// A take file of a running recording, for its waveform while it grows.
#[derive(Clone, Debug)]
pub struct StartedTake {
    pub track: InstanceId,
    pub asset: AudioAsset,
    pub overview: TakeOverview,
}

/// What a [`Recorder`] tells the thread that draws, of the recording `id`.
#[derive(Debug)]
pub enum RecorderReport {
    /// The first frames of a recording arrived. `first_nanos` is when the first frame of every
    /// file of it was captured, on the clock of [`sound_core::monotonic_nanos`].
    Started {
        id: u64,
        first_nanos: u64,
        takes: Vec<StartedTake>,
    },
    /// A recording ended: each take that has a file, in memory as an import gives it. The
    /// first frame of each was captured at `first_nanos`, `None` when no frame ever came.
    Finished {
        id: u64,
        first_nanos: Option<u64>,
        takes: Vec<(InstanceId, Imported)>,
    },
    /// The take of a track has no file, or stopped being written: the error says why. What was
    /// written before it is kept and becomes its clip, and the rest of the recording goes on.
    Failed {
        id: u64,
        track: InstanceId,
        error: TakeError,
    },
    /// The recorder fell behind the input and frames were lost: the takes have silence
    /// there, and every frame after it is still where it was heard.
    Behind { frames: u64 },
    /// The input could not open: no recording gets sound.
    NoInput,
}

struct Take {
    track: InstanceId,
    channels: Range<usize>,
    /// `None` once it failed, which was reported: the first error ends the writing.
    file: Option<TakeFile>,
    /// What a take whose writing failed holds up to the failure, closed at once.
    kept: Option<Imported>,
}

/// Why the take of a track has no file, or stopped being written.
#[derive(Debug, thiserror::Error)]
pub enum TakeError {
    #[error("the input has {channels} channels and no channel {wanted}")]
    NoSuchChannel { channels: usize, wanted: usize },
    #[error(transparent)]
    Media(#[from] MediaError),
}

struct Recording {
    id: u64,
    takes: Vec<Take>,
    first_nanos: Option<u64>,
    /// Frames of the input the takes hold.
    frames: u64,
    /// Where a finish waits for the takes to be written up to, see [`RecorderCommand::Finish`].
    until: Option<u64>,
}

/// Takes what the input captured and writes the takes of the running recordings. It reads and
/// writes files, so it runs on a thread of its own or a background task, never on the thread
/// that draws. While nothing records it still takes everything, so the ring never fills.
pub struct Recorder {
    input: CaptureReader,
    assets: Assets,
    recordings: Vec<Recording>,
    /// Frames the ring had lost when this recorder last looked.
    lost: u64,
    read: Vec<f32>,
    picked: Vec<f32>,
}

impl Recorder {
    pub fn new(input: CaptureReader, assets: Assets) -> Self {
        let lost = input.lost_frames();
        Self {
            input,
            assets,
            recordings: Vec::new(),
            lost,
            read: Vec::new(),
            picked: Vec::new(),
        }
    }

    /// Carries out the commands in order, then writes what the input captured since the last
    /// run. Gives what the thread that draws must know.
    pub fn run(&mut self, commands: Vec<RecorderCommand>) -> Vec<RecorderReport> {
        let mut reports = Vec::new();
        for command in commands {
            match command {
                RecorderCommand::Start(id, requests) => {
                    // What runs already starts with the input before this one does.
                    reports.extend(self.take_input());
                    // One under the same number that was never finished is finished first:
                    // its files are closed and given back, not lost.
                    reports.extend(self.finish(id));
                    reports.extend(self.start(id, requests));
                }
                RecorderCommand::Finish { id, frames: None } => {
                    reports.extend(self.take_input());
                    reports.extend(self.finish(id));
                }
                RecorderCommand::Finish { id, frames } => {
                    let recording = self.recordings.iter_mut().find(|each| each.id == id);
                    if let Some(recording) = recording {
                        recording.until = frames;
                    }
                }
            }
        }
        reports.extend(self.take_input());
        let gone = self.input.is_gone();
        let complete: Vec<u64> = (self.recordings.iter())
            .filter(|each| each.until.is_some_and(|until| each.frames >= until || gone))
            .map(|each| each.id)
            .collect();
        for id in complete {
            reports.extend(self.finish(id));
        }
        reports
    }

    /// Opens a file per request. Gives the ones that could not be opened.
    fn start(&mut self, id: u64, requests: Vec<TakeRequest>) -> Vec<RecorderReport> {
        let (rate, count) = (self.input.sample_rate(), self.input.channels());
        let mut failed = Vec::new();
        let mut takes = Vec::new();
        for request in requests {
            let channels = request.channels.len();
            let file = match request.channels.end <= count {
                true => TakeFile::create(&self.assets, &request.name, rate, channels)
                    .map_err(TakeError::from),
                false => Err(TakeError::NoSuchChannel {
                    channels: count,
                    wanted: request.channels.end,
                }),
            };
            let file = file
                .map_err(|error| {
                    let track = request.track.clone();
                    failed.push(RecorderReport::Failed { id, track, error });
                })
                .ok();
            takes.push(Take {
                track: request.track,
                channels: request.channels,
                file,
                kept: None,
            });
        }
        self.recordings.push(Recording {
            id,
            takes,
            first_nanos: None,
            frames: 0,
            until: None,
        });
        failed
    }

    /// Writes everything the ring holds into the takes of the recordings that run.
    fn take_input(&mut self) -> Vec<RecorderReport> {
        self.read.clear();
        let first = self.input.read(&mut self.read);
        let lost = self.input.lost_frames();
        let before = std::mem::replace(&mut self.lost, lost);
        let mut reports = Vec::new();
        if self.recordings.is_empty() {
            return reports;
        }
        if lost != before {
            reports.push(RecorderReport::Behind {
                frames: lost - before,
            });
        }
        if self.read.is_empty() {
            return reports;
        }
        let channels = self.input.channels();
        let first_nanos = self.input.nanos_of(first).unwrap_or_default();
        for recording in &mut self.recordings {
            if recording.first_nanos.is_none() {
                recording.first_nanos = Some(first_nanos);
                let growing = recording.takes.iter().filter_map(|take| {
                    let file = take.file.as_ref()?;
                    Some(StartedTake {
                        track: take.track.clone(),
                        asset: file.asset().clone(),
                        overview: file.overview(),
                    })
                });
                reports.push(RecorderReport::Started {
                    id: recording.id,
                    first_nanos,
                    takes: growing.collect(),
                });
            }
            // Past the end a finish waits for, the input is no part of the recording.
            let read = self.read.len() / channels;
            let wanted = recording.until.map_or(read as u64, |until| {
                until.saturating_sub(recording.frames).min(read as u64)
            });
            recording.frames += wanted;
            let read = (self.read)
                .get(..wanted as usize * channels)
                .unwrap_or_default();
            for take in &mut recording.takes {
                let Some(file) = &mut take.file else {
                    continue;
                };
                self.picked.clear();
                for frame in read.chunks_exact(channels) {
                    let picked = frame.get(take.channels.clone()).unwrap_or_default();
                    self.picked.extend_from_slice(picked);
                }
                if let Err(error) = file.write(&self.picked) {
                    // What was written is closed and kept, and becomes the clip of the take.
                    take.kept = take.file.take().and_then(|file| close(file, &self.assets));
                    let track = take.track.clone();
                    let error = error.into();
                    let id = recording.id;
                    reports.push(RecorderReport::Failed { id, track, error });
                }
            }
        }
        reports
    }

    /// Closes every file of the recording `id`, with what it holds.
    fn finish(&mut self, id: u64) -> Vec<RecorderReport> {
        let mut reports = Vec::new();
        let Some(index) = self.recordings.iter().position(|each| each.id == id) else {
            return reports;
        };
        let recording = self.recordings.remove(index);
        let mut finished = Vec::new();
        for take in recording.takes {
            let kept = match take.file {
                Some(file) => {
                    let asset = file.asset().clone();
                    match file.finish() {
                        Ok(file) => Some(file),
                        Err(error) => {
                            let track = take.track.clone();
                            let error = error.into();
                            reports.push(RecorderReport::Failed { id, track, error });
                            reopen(&self.assets, asset)
                        }
                    }
                }
                None => take.kept,
            };
            if let Some(kept) = kept {
                finished.push((take.track, kept));
            }
        }
        reports.push(RecorderReport::Finished {
            id,
            first_nanos: recording.first_nanos,
            takes: finished,
        });
        reports
    }
}

/// Closes a take whose writing failed, with what it holds.
fn close(file: TakeFile, assets: &Assets) -> Option<Imported> {
    let asset = file.asset().clone();
    file.finish().ok().or_else(|| reopen(assets, asset))
}

/// A take whose last header could not be written, as it is on disk: its header says the length
/// it had at most a second before.
fn reopen(assets: &Assets, asset: AudioAsset) -> Option<Imported> {
    let audio = sound_media::load(assets, &asset).ok()?;
    Some(Imported { asset, audio })
}

/// The device plays project frame `project_frame` at engine frame `engine_frame`, and has for a
/// while: what ties a moment the composer heard to the timeline.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PlayingAt {
    pub engine_frame: u64,
    pub project_frame: Frames,
}

impl PlayingAt {
    /// From two engine statuses one after the other: when the project played through both and
    /// its position moved exactly as far as the engine did. So the playhead was not waiting
    /// for latency after a play, and no seek, stop or tempo change came between them.
    pub(crate) fn between(before: &EngineStatus, after: &EngineStatus) -> Option<Self> {
        let engine = after.frames.checked_sub(before.frames)?;
        let project = after
            .playhead_frame
            .0
            .checked_sub(before.playhead_frame.0)?;
        let steady = before.playing
            && after.playing
            && before.jumps == after.jumps
            && engine > 0
            && engine == project;
        steady.then_some(Self {
            engine_frame: after.frames,
            project_frame: after.playhead_frame,
        })
    }
}

/// The engine frame whose sound started at the device at `nanos`: what the composer heard in
/// that moment. Without a device, as for an engine a test runs by hand, engine frame `n`
/// sounds at `n / sample_rate` seconds on the clock.
fn frame_sounding_at(timing: Option<&StreamTiming>, sample_rate: u32, nanos: u64) -> i64 {
    if let Some(frame) = timing.and_then(|timing| timing.frame_sounding_at(nanos)) {
        return frame;
    }
    let frame = (i128::from(nanos) * i128::from(sample_rate) + 500_000_000) / 1_000_000_000;
    i64::try_from(frame).unwrap_or(i64::MAX)
}

/// Where a take lies: the project frame, at the rate of the engine, at which the composer heard
/// what the first frame of the take holds. `first_heard` is the engine frame sounding when
/// that frame was captured, see [`frame_sounding_at`].
fn take_head(first_heard: i64, playing: PlayingAt) -> i128 {
    let offset = i128::from(playing.project_frame.0) - i128::from(playing.engine_frame);
    i128::from(first_heard) + offset
}

/// The seconds into a take at which the composer heard project frame `frame`, from its head.
pub fn take_seconds_at(head: i128, frame: Frames, clock: &Clock) -> f64 {
    (i128::from(frame.0) - head) as f64 / f64::from(clock.sample_rate())
}

/// Where a take file plays in the song, heard from `start` to `end`: its start, and the part
/// of the file that plays from there, in seconds of the file, to the end of the file with
/// `None`. `head` is the project frame its first frame was heard at, see [`Placement::head`].
/// What the file holds before `start` is trimmed away, and so is what came after `end`, so it
/// plays what was heard in the recording and nothing else. `None` when the file holds nothing
/// of that stretch.
///
/// A take that began before `start` was heard starts at `start`. One whose first frame came
/// later starts at the first tick of its sound. Either way the frame of the file that plays at
/// its start is the one captured when that frame was heard.
pub fn take_place(
    file: &Info,
    head: i128,
    (start, end): (Ticks, Ticks),
    clock: &Clock,
) -> Option<(Ticks, f64, Option<f64>)> {
    let start_frame = i128::from(clock.frame_of(start).0);
    let start = match head <= start_frame {
        true => start,
        false => clock.tick_at(Frames(u64::try_from(head).ok()?)),
    };
    let file_start = take_seconds_at(head, clock.frame_of(start), clock);
    let file_end = take_seconds_at(head, clock.frame_of(end), clock);
    let seconds = file.seconds();
    if start >= end || file_start >= seconds.min(file_end) {
        return None;
    }
    Some((start, file_start, (file_end < seconds).then_some(file_end)))
}

/// A recording on the thread that draws: where it began, the clock it began under, and what
/// ties it to the timeline once it is known. The window keeps one while it records, and a test
/// that plays the part of the window does the same.
///
/// Everything is worked out under the clock the recording began with. A change of the tempo
/// map moves the project frames of the ticks, so the window ends a take at one, as at a seek.
#[derive(Clone, Debug)]
pub struct Placement {
    /// The playhead the recording began at.
    pub start: Ticks,
    /// When the first frame of its takes was captured, from the recorder.
    pub first_nanos: Option<u64>,
    clock: Arc<Clock>,
    playing: Option<PlayingAt>,
    last: Option<EngineStatus>,
}

impl Placement {
    pub fn new(start: Ticks, clock: Arc<Clock>) -> Self {
        Self {
            start,
            first_nanos: None,
            clock,
            playing: None,
            last: None,
        }
    }

    /// The clock the recording began under, which places its takes.
    pub fn clock(&self) -> &Arc<Clock> {
        &self.clock
    }

    /// Takes one engine status, as the window polls it. The first two in a row that show the
    /// project playing steadily tie the recording to the timeline.
    pub fn observe(&mut self, status: EngineStatus) {
        if self.playing.is_none()
            && let Some(last) = &self.last
        {
            self.playing = PlayingAt::between(last, &status);
        }
        self.last = Some(status);
    }

    /// See [`take_head`]. `None` until the first frame came and the project played steadily.
    pub fn head(&self, timing: Option<&StreamTiming>) -> Option<i128> {
        let rate = self.clock.sample_rate();
        let heard = frame_sounding_at(timing, rate, self.first_nanos?);
        Some(take_head(heard, self.playing?))
    }

    /// How many frames of an input at `input_rate` the takes need to hold everything heard up
    /// to `end`: what a finish waits for, see [`RecorderCommand::Finish`]. `None` while the
    /// recording is not tied to the timeline.
    pub fn input_frames_until(
        &self,
        end: Ticks,
        timing: Option<&StreamTiming>,
        input_rate: u32,
    ) -> Option<u64> {
        let clock = &self.clock;
        let head = self.head(timing)?;
        let heard = (i128::from(clock.frame_of(end).0) - head).max(0);
        let rate = i128::from(clock.sample_rate().max(1));
        let frames = (heard * i128::from(input_rate) + rate - 1) / rate;
        u64::try_from(frames).ok()
    }
}
