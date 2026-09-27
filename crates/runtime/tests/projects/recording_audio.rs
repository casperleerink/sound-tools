//! Recording audio in a whole project, with a simulated input and a simulated device: a player
//! claps on beats they hear, the input captures the claps late by its own latency, the device
//! plays late by its output latency, and a track with a lookahead makes the project wait after
//! play. The takes land where the claps were heard, to the frame, and are one undo step.
//!
//! Nothing here needs an audio device: the test plays the part of both devices on one clock,
//! calling the same function the input callback calls, and runs the engine by hand.

use std::time::Duration;

use arrangement::AudioClip;
use runtime::recorder::{Placement, Recorder, RecorderCommand, RecorderReport, TakeRequest};
use runtime::window::recording::add_audio_take_clips;
use sound_core::{CaptureWriter, Changes, EngineStatus, InstanceId, StreamTiming, Ticks, capture};
use sound_media::Imported;

use crate::support::{BAR, Harness};

const ENGINE_RATE: u32 = 48_000;
/// The output device: engine frame 0 started to sound 1 s into the clock, and the device adds
/// 7.3 ms after each callback.
const FRAME_ZERO_NANOS: u64 = 1_000_000_000;
const OUTPUT_DELAY: Duration = Duration::from_micros(7_300);
/// The input device has a clock of its own: its frame 0 was captured at this moment.
const INPUT_ZERO_NANOS: u64 = 1_000_123_457;
/// Frames per input callback, and what the input says passes from capture to its callback: its
/// buffer and 2 ms of its own.
const INPUT_BUFFER: u64 = 256;
const INPUT_EXTRA_LATENCY_NANOS: u64 = 2_000_000;
/// The project waits this long after play: the lookahead of the voice's compressor, 10 ms.
const PROJECT_LATENCY: u64 = 480;

const VOICE: &str = "arrangement/voice";
const GUITAR: &str = "arrangement/guitar";

fn nanos(frames: u64, rate: u32) -> u64 {
    frames * 1_000_000_000 / u64::from(rate)
}

fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

/// The default project with two audio tracks: the voice records input 1 through a compressor
/// that only looks ahead, and the guitar records inputs 1 and 2 as a stereo take.
fn studio_project() -> Harness {
    let mut harness = Harness::new();
    let track = |name: &str, order: u32, extra: &str| {
        format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "{name}", "kind": "audio", "order": {order}{extra}}}}}"#
        )
    };
    let paths = [
        harness.write(
            "state/arrangement/voice/instance.json",
            &track("Voice", 1, r#", "effects": [{"name": "compressor"}]"#),
        ),
        harness.write(
            "state/arrangement/voice/compressor.json",
            r#"{"tool": "compressor", "state": {"ratio": 1.0, "threshold_db": 0.0, "lookahead_ms": 10}}"#,
        ),
        harness.write(
            "state/arrangement/guitar/instance.json",
            &track("Guitar", 2, r#", "input": [1, 2]"#),
        ),
    ];
    assert_eq!(harness.apply(&paths), 3);
    assert_eq!(harness.project.problems(), []);
    harness
}

/// A player, an input device and an output device, on one clock.
struct Studio {
    harness: Harness,
    timing: StreamTiming,
    input: Option<CaptureWriter>,
    input_rate: u32,
    /// Input frames delivered so far.
    delivered: u64,
    recorder: Recorder,
    /// Where the player claps: the tick they hear, and the input channel it reaches.
    claps: Vec<(Ticks, usize)>,
    /// The same, as frames of the input, once the test knows when those ticks sound.
    clap_frames: Vec<(u64, usize)>,
    /// How far the project position is ahead of the engine frame while it plays steadily,
    /// worked out here from the engine status alone.
    project_ahead: Option<i128>,
    last: EngineStatus,
    reports: Vec<RecorderReport>,
    /// The recorder does not run while the engine is in these frames, as if the thread that
    /// runs it were held up.
    stalled: std::ops::Range<u64>,
    /// Frames the recorder said were lost.
    lost: u64,
}

impl Studio {
    fn new(input_rate: u32, claps: Vec<(Ticks, usize)>) -> Self {
        let harness = studio_project();
        let (input, reader) = capture(input_rate, 2);
        let recorder = Recorder::new(reader, harness.project.assets().clone());
        Self {
            harness,
            timing: StreamTiming::simulated(ENGINE_RATE, FRAME_ZERO_NANOS, OUTPUT_DELAY),
            input: Some(input),
            input_rate,
            delivered: 0,
            recorder,
            claps,
            clap_frames: Vec::new(),
            project_ahead: None,
            last: EngineStatus::default(),
            reports: Vec::new(),
            stalled: 0..0,
            lost: 0,
        }
    }

    fn status(&mut self) -> EngineStatus {
        self.harness.project.engine().poll().unwrap()
    }

    /// Runs both devices for `frames` of the engine, 512 at a time, as their callbacks would,
    /// and the recorder and the placement at every poll of the window.
    fn run(&mut self, frames: u64, placement: &mut Placement) {
        let mut buffer = vec![0.0_f32; 512 * 2];
        for _ in 0..frames / 512 {
            self.harness.engine.process_block(&mut buffer);
            let status = self.status();
            placement.observe(status);
            self.learn_when_ticks_sound(status);
            // The callback of the next block begins about now.
            self.deliver_input_until(FRAME_ZERO_NANOS + nanos(status.frames, ENGINE_RATE));
            if self.stalled.contains(&status.frames) {
                continue;
            }
            let reports = self.recorder.run(Vec::new());
            self.take_reports(reports, placement);
        }
    }

    fn take_reports(&mut self, reports: Vec<RecorderReport>, placement: &mut Placement) {
        for report in reports {
            if let RecorderReport::Started { first_nanos, .. } = &report {
                placement.first_nanos = Some(*first_nanos);
            }
            if let RecorderReport::Behind { frames } = &report {
                self.lost += frames;
            }
            self.reports.push(report);
        }
    }

    /// When the project position moved exactly as far as the engine did, it plays steadily
    /// and the difference ties the two. Then each clap has the input frame it is captured on:
    /// the moment its tick sounds at the device.
    fn learn_when_ticks_sound(&mut self, status: EngineStatus) {
        let last = std::mem::replace(&mut self.last, status);
        let steady = last.playing
            && status.playing
            && status.frames > last.frames
            && status.playhead_frame.0 - last.playhead_frame.0 == status.frames - last.frames;
        if !steady || self.project_ahead.is_some() {
            return;
        }
        let ahead = i128::from(status.playhead_frame.0) - i128::from(status.frames);
        self.project_ahead = Some(ahead);
        let clock = self.harness.project.clock().clone();
        for (tick, channel) in &self.claps {
            let engine_frame = i128::from(clock.frame_of(*tick).0) - ahead;
            let heard = self
                .timing
                .sound_time_nanos(u64::try_from(engine_frame).unwrap())
                .unwrap();
            let since = u128::from(heard - INPUT_ZERO_NANOS) * u128::from(self.input_rate);
            let frame = (since + 500_000_000) / 1_000_000_000;
            self.clap_frames
                .push((u64::try_from(frame).unwrap(), *channel));
        }
    }

    /// Every input callback that has happened by `now`: a buffer of silence with the claps it
    /// captured, given with the moment of its callback and its latency, as a device gives it.
    fn deliver_input_until(&mut self, now: u64) {
        let latency = nanos(INPUT_BUFFER, self.input_rate) + INPUT_EXTRA_LATENCY_NANOS;
        loop {
            let first = self.delivered;
            let callback = INPUT_ZERO_NANOS + nanos(first, self.input_rate) + latency;
            let Some(input) = self.input.as_mut().filter(|_| callback <= now) else {
                return;
            };
            let mut samples = vec![0.0_f32; INPUT_BUFFER as usize * 2];
            for (frame, channel) in &self.clap_frames {
                if (first..first + INPUT_BUFFER).contains(frame) {
                    samples[(frame - first) as usize * 2 + channel] = 0.5;
                }
            }
            input.write(&samples, callback, latency);
            self.delivered += INPUT_BUFFER;
        }
    }

    /// Records from the start: play from rest, as the record control does.
    fn record(&mut self, bars: u64) -> (Placement, Ticks) {
        let requests = vec![
            TakeRequest {
                track: id(VOICE),
                name: "voice-take".into(),
                channels: 0..1,
            },
            TakeRequest {
                track: id(GUITAR),
                name: "guitar-take".into(),
                channels: 0..2,
            },
        ];
        let clock = std::sync::Arc::new(self.harness.project.clock().clone());
        let mut placement = Placement::new(Ticks(0), clock);
        let reports = self.recorder.run(vec![RecorderCommand::Start(requests)]);
        self.take_reports(reports, &mut placement);
        self.harness.project.engine().play();
        self.run(bars * BAR as u64, &mut placement);
        let end = self.status().playhead_tick;
        self.harness.project.engine().stop();
        (placement, end)
    }

    /// Ends the recording as the window does: the recorder writes the takes up to what was
    /// heard at the end, which the input brings a little later, and closes the files; every
    /// take becomes a clip where it was heard, all in one undo step.
    fn finish(&mut self, mut placement: Placement, end: Ticks) -> Vec<(InstanceId, Imported)> {
        let frames = placement.input_frames_until(end, Some(&self.timing), self.input_rate);
        assert!(frames.is_some(), "the recording is tied to the timeline");
        let finish = RecorderCommand::Finish { frames };
        let reports = self.recorder.run(vec![finish]);
        self.take_reports(reports, &mut placement);
        // The devices go on while the input brings the last of the take.
        let mut finished = self.finished();
        for _ in 0..20 {
            if finished.is_some() {
                break;
            }
            self.run(512, &mut placement);
            finished = self.finished();
        }
        let finished = finished.expect("the recording finished");
        let project = &mut self.harness.project;
        let clips = placement.clips(&finished, end, Some(&self.timing));
        assert_eq!(clips.len(), 2);
        let mut changes = Changes::new();
        add_audio_take_clips(project, &mut changes, clips).unwrap();
        project.commit("Record", changes).unwrap();
        finished
    }

    /// The takes of the finished recording, once it finished. Anything else the recorder said
    /// is a failure of this test, apart from the start.
    fn finished(&mut self) -> Option<Vec<(InstanceId, Imported)>> {
        let mut finished = None;
        for report in std::mem::take(&mut self.reports) {
            match report {
                RecorderReport::Finished { takes, .. } => finished = Some(takes),
                RecorderReport::Started { .. } => {}
                RecorderReport::Behind { frames } => assert!(
                    !self.stalled.is_empty(),
                    "{frames} frames lost with the recorder running"
                ),
                other => panic!("{other:?}"),
            }
        }
        finished
    }

    fn recorder_lost(&self) -> u64 {
        self.lost
    }

    fn clip(&self, track: &str) -> Option<AudioClip> {
        let track = id(track);
        let mut children = self.harness.project.children::<AudioClip>(&track);
        children.next().map(|(_, clip)| clip.clone())
    }
}

/// Where the render is louder than a whisper, per channel.
fn loud_frames(render: &[f32]) -> [Vec<usize>; 2] {
    [0, 1].map(|channel| {
        let samples = render.iter().skip(channel).step_by(2).enumerate();
        samples
            .filter(|(_, sample)| sample.abs() > 0.05)
            .map(|(frame, _)| frame)
            .collect()
    })
}

/// Two armed tracks, mono and stereo, record claps the player made on two beats. Played back,
/// each clap is at the frame of the beat it was played on, although the input captured it
/// 7.3 ms of output and 7.3 ms of input latency after the device played that beat's frame,
/// and the project waited 10 ms after play for its lookahead.
#[test]
fn a_take_lands_where_it_was_heard_to_the_frame() {
    // Bar 2 beat 3 on input 1, and bar 3 beat 3 on input 2 only.
    let (first, second) = (Ticks(3840 + 1920), Ticks(2 * 3840 + 1920));
    let mut studio = Studio::new(ENGINE_RATE, vec![(first, 0), (second, 1)]);
    let (placement, end) = studio.record(4);
    assert_eq!(studio.status().latency, PROJECT_LATENCY);
    let takes = studio.finish(placement, end);

    // The files: a mono and a stereo WAV of 32-bit floats, each a new file.
    let voice = studio.clip(VOICE).unwrap();
    let guitar = studio.clip(GUITAR).unwrap();
    assert_eq!(voice.asset.to_string(), "voice-take-1.wav");
    assert_eq!(guitar.asset.to_string(), "guitar-take-1.wav");
    for (take, channels) in takes.iter().zip([1, 2]) {
        let path = studio
            .harness
            .project
            .assets()
            .path(take.1.asset.asset_name());
        let spec = hound::WavReader::open(path).unwrap().spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (channels, 48_000, 32)
        );
    }
    // Both start where the recording began and end where it ended.
    assert_eq!((voice.start, guitar.start), (Ticks(0), Ticks(0)));
    let clock = studio.harness.project.clock().clone();
    let heard = |clip: &AudioClip| {
        let file = sound_media::info(studio.harness.project.assets(), &clip.asset).unwrap();
        clip.end(Some(&file), &clock)
    };
    assert_eq!((heard(&voice), heard(&guitar)), (end, end));
    // What the files hold before the start is the wait after play and the latency.
    assert!(
        voice.file_start_seconds > 0.01,
        "{}",
        voice.file_start_seconds
    );

    let render = studio.harness.play_from_the_start(4 * BAR);
    let [left, right] = loud_frames(&render);
    let at = |tick: Ticks| clock.frame_of(tick).0 as usize;
    assert_eq!(left, [at(first)], "the voice and the left of the guitar");
    assert_eq!(
        right,
        [at(first), at(second)],
        "the voice and the right of the guitar"
    );
}

/// The whole recording, every track of it, is one undo step, and undo leaves the files: an
/// asset is never deleted.
#[test]
fn one_undo_takes_every_clip_of_the_recording_away_and_leaves_the_files() {
    let mut studio = Studio::new(ENGINE_RATE, vec![(Ticks(3840), 0)]);
    let (placement, end) = studio.record(2);
    let takes = studio.finish(placement, end);
    assert!(studio.clip(VOICE).is_some() && studio.clip(GUITAR).is_some());
    assert_eq!(
        studio.harness.project.undo().unwrap(),
        Some("Record".to_string())
    );
    assert_eq!((studio.clip(VOICE), studio.clip(GUITAR)), (None, None));
    for (_, take) in &takes {
        assert!(
            studio
                .harness
                .project
                .assets()
                .path(take.asset.asset_name())
                .exists()
        );
    }
    studio.harness.project.redo().unwrap();
    assert!(studio.clip(VOICE).is_some() && studio.clip(GUITAR).is_some());
}

/// An input at 44.1 kHz into an engine at 48 kHz: the take is written at the rate of the input
/// and plays at its own speed, and the clap still lands on its beat, within a frame.
#[test]
fn a_take_from_an_input_at_another_rate_lands_within_a_frame() {
    let beat = Ticks(3840 + 960);
    let mut studio = Studio::new(44_100, vec![(beat, 0)]);
    let (placement, end) = studio.record(3);
    let takes = studio.finish(placement, end);
    assert_eq!(takes[0].1.audio.sample_rate(), 44_100);
    let render = studio.harness.play_from_the_start(3 * BAR);
    let left: Vec<f32> = render.iter().step_by(2).copied().collect();
    let loudest = (0..left.len())
        .max_by(|a, b| left[*a].abs().total_cmp(&left[*b].abs()))
        .unwrap();
    let expected = studio.harness.project.clock().frame_of(beat).0 as usize;
    assert!(
        loudest.abs_diff(expected) <= 1,
        "{loudest} against {expected}"
    );
}

/// The recorder is held up for 12 s, longer than the ring holds, so the input loses frames.
/// The take has silence there, and a clap after it still lands on its beat, to the frame.
#[test]
fn a_take_stays_in_time_after_the_recorder_fell_behind() {
    let beat = Ticks(6 * 3840 + 1920);
    let mut studio = Studio::new(ENGINE_RATE, vec![(beat, 0)]);
    studio.stalled = 48_000..48_000 * 13;
    let (placement, end) = studio.record(8);
    assert!(studio.recorder_lost() > 48_000, "the ring overflowed");
    studio.finish(placement, end);
    let render = studio.harness.play_from_the_start(8 * BAR);
    let [left, _] = loud_frames(&render);
    let expected = studio.harness.project.clock().frame_of(beat).0 as usize;
    assert_eq!(left, [expected]);
}
