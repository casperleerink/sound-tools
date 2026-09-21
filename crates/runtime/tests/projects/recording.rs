//! Recording a whole real project: a keyboard plays into the track's synth, the take becomes a
//! clip, and playing the clip back renders what was heard.
//!
//! No device and no MIDI hardware. Every message is one the test sends, at the frame it
//! chooses, through the same ring a keyboard writes into.

use std::path::PathBuf;

use metronome::Click;
use midi::{Keyboard, Played, RawEvent, RawTake, Take};
use sound_core::{InstanceId, Ticks};
use sound_notes::{Clip, Pedal, Pitch, Velocity};

use crate::support::{BAR, Harness, difference};

/// The track that a take is recorded into, and its synth.
const TRACK: &str = "arrangement/track-1";
const CLIP: &str = "arrangement/track-1/take";
const TAKE_FILE: &str = "assets/takes/take-1.json";

fn on(pitch: u8, velocity: u8) -> Played {
    Played::On {
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(velocity).unwrap(),
    }
}

fn off(pitch: u8) -> Played {
    Played::Off {
        pitch: Pitch::new(pitch).unwrap(),
        velocity: 64,
    }
}

fn pedal(value: u8) -> Played {
    Played::Pedal(Pedal::new(value).unwrap())
}

/// A project of one track with a synth, with the MIDI input wired to it, as the window does.
struct Recorder {
    harness: Harness,
    keyboard: Keyboard,
    click: Option<Click>,
}

impl Recorder {
    fn new() -> Self {
        let mut harness = Harness::new();
        let mut keyboard = Keyboard::attach(harness.project.engine()).unwrap();
        let track = InstanceId::new(TRACK).unwrap();
        let track = harness.project.resolve(&track).unwrap();
        let notes = runtime::window::recording::notes_input(&harness.project, &track).unwrap();
        keyboard
            .play_into(harness.project.engine(), Some(notes))
            .unwrap();
        // The poll after it makes the port the one the live input reaches, as the window does.
        keyboard.poll(harness.project.engine(), None).unwrap();
        Self {
            harness,
            keyboard,
            click: None,
        }
    }

    /// The click, as the window attaches it. A render then has it in the signal.
    fn with_click(mut self) -> Self {
        let mut click = Click::attach(self.harness.project.engine()).unwrap();
        click.set_on(self.harness.project.engine(), true).unwrap();
        self.click = Some(click);
        self
    }

    /// Renders `frames` frames and drains what the engine reported, as the window polls.
    fn render(&mut self, frames: usize) -> Vec<f32> {
        let output = self.harness.render(frames);
        self.keyboard
            .poll(self.harness.project.engine(), None)
            .unwrap();
        output
    }

    /// Plays and records `messages`, each at the frame given, and renders `frames` in all.
    fn record(&mut self, messages: &[(usize, Played)], frames: usize) -> (Take, Vec<f32>) {
        self.harness.project.engine().play();
        let mut heard = self.render(64);
        let from = self.harness.project.engine().poll().unwrap().playhead_tick;
        self.keyboard.start_recording(from);
        let mut at = 64;
        for (frame, played) in messages {
            heard.extend(self.render(frame.saturating_sub(at)));
            at = (*frame).max(at);
            assert!(self.keyboard.input().send(*played));
        }
        heard.extend(self.render(frames.saturating_sub(at)));
        let until = self.harness.project.engine().poll().unwrap().playhead_tick;
        let take = self.keyboard.finish_recording(until).unwrap();
        (take, heard)
    }

    /// Saves the take as the window does: the raw take first, then the clip that names it, as
    /// one undo step.
    fn save(&mut self, take: &Take) -> InstanceId {
        let name = runtime::window::recording::write_take(&self.harness.project, take).unwrap();
        let track = InstanceId::new(TRACK).unwrap();
        let track = self.harness.project.resolve(&track).unwrap();
        runtime::window::recording::add_take_clip(
            &mut self.harness.project,
            &track,
            take,
            Some(name),
        )
        .unwrap()
        .unwrap()
    }

    fn take_file(&self) -> PathBuf {
        self.harness.path(TAKE_FILE)
    }

    /// Plays the project from the start again and renders, with nothing on the keyboard.
    fn play_back(&mut self, frames: usize) -> Vec<f32> {
        self.harness.project.engine().stop();
        self.render(64);
        self.harness.project.engine().play();
        self.render(frames)
    }

    fn clip(&self) -> Clip {
        let id = InstanceId::new(CLIP).unwrap();
        let clip = self.harness.project.resolve::<Clip>(&id).unwrap();
        self.harness.project.state(&clip).unwrap().clone()
    }
}

/// A chord and a melody note, played over a tempo map that changes in the middle. The clip
/// must render what was heard live, note for note, within one tick.
#[test]
fn a_take_becomes_a_clip_that_renders_what_was_heard() {
    let mut recorder = Recorder::new();
    // 150 bpm from bar 2, written into project.json as an agent would.
    recorder.harness.write_and_apply(
        "project.json",
        r#"{"format": 1, "extensions": ["arrangement", "instrument", "tone"], "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}, {"tick": 3840, "bpm": 150.0}]}, "connections": []}"#,
    );
    assert_eq!(recorder.harness.project.problems(), []);

    let messages = [
        (12_000, on(60, 88)),
        (12_000, on(64, 70)),
        (36_000, off(60)),
        (36_000, off(64)),
        (48_000, on(67, 110)),
        (84_000, off(67)),
    ];
    let (take, heard) = recorder.record(&messages, 2 * BAR);
    assert_eq!(take.events.len(), 6);
    let mut clip = take.clip().unwrap();
    recorder.save(&take);
    // The saved clip is the take's clip, plus the name of the raw take it came from.
    clip.take = Some("take-1".to_string());
    assert_eq!(recorder.clip(), clip);
    assert_eq!(recorder.harness.project.problems(), []);

    // Every note is at the tick the engine sounded it. Each message here is sent while the
    // engine stands on the frame the test names, so the block that begins there carries it.
    let ticks: Vec<u64> = take.events.iter().map(|event| event.tick.0).collect();
    let clock = recorder.harness.project.clock().clone();
    for (index, (frame, _)) in messages.iter().enumerate() {
        let sounded = clock.tick_at(sound_core::Frames(*frame as u64));
        assert_eq!(ticks[index], sounded.0, "message {index}");
    }

    // Playing the clip back renders what was heard, note for note. The one difference the
    // rules allow is that a note lands on the exact frame of its tick instead of on the start
    // of the block that carried it: less than one tick.
    let played = recorder.play_back(2 * BAR);
    let tick_frames = 25;
    for (index, (frame, _)) in messages.iter().enumerate() {
        let live = *frame;
        let back = clock.frame_of(Ticks(ticks[index])).0 as usize;
        assert!(
            back.abs_diff(live) < tick_frames,
            "message {index}: live at {live}, played back at {back}"
        );
    }
    // Both renders sound: the take is not silence compared with silence.
    let loudest = |samples: &[f32]| samples.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
    assert!(loudest(&heard) > 0.01, "{}", loudest(&heard));
    assert!(loudest(&played) > 0.01, "{}", loudest(&played));
}

/// The pedal is recorded and holds notes when the clip plays back: the render goes on after
/// the note off and stops after the pedal comes up.
#[test]
fn the_pedal_is_in_the_clip_and_holds_notes_on_playback() {
    let mut recorder = Recorder::new();
    let messages = [
        (6_000, pedal(127)),
        (12_000, on(60, 100)),
        (24_000, off(60)),
        (60_000, pedal(0)),
    ];
    let (take, _) = recorder.record(&messages, BAR);
    recorder.save(&take);
    let clip = recorder.clip();
    assert_eq!(clip.pedal.len(), 2);
    assert_eq!(clip.pedal[0].value, Pedal::new(127).unwrap());
    assert_eq!(clip.pedal[1].value, Pedal::UP);
    assert_eq!(clip.notes.len(), 1);

    let played = recorder.play_back(BAR);
    let loudest = |range: std::ops::Range<usize>| {
        played[range.start * 2..range.end * 2]
            .iter()
            .fold(0.0_f32, |peak, s| peak.max(s.abs()))
    };
    // The key came up at frame 24 000 and the note sounds on: the pedal holds it.
    assert!(
        loudest(30_000..40_000) > 0.01,
        "{}",
        loudest(30_000..40_000)
    );
    assert!(
        loudest(50_000..58_000) > 0.01,
        "{}",
        loudest(50_000..58_000)
    );
    // The pedal came up at frame 60 000 and the release has run within a second.
    assert_eq!(loudest(90_000..BAR), 0.0);
}

/// The whole recording is one undo step. Undo removes the clip and leaves the raw take: it is
/// the only copy of what was played.
#[test]
fn a_recording_is_one_undo_step_and_undo_leaves_the_raw_take() {
    let mut recorder = Recorder::new();
    let messages = [(6_000, on(60, 100)), (30_000, off(60))];
    let (take, _) = recorder.record(&messages, BAR);
    let clip = recorder.save(&take);
    assert_eq!(recorder.harness.project.undo_label(), Some("Record"));
    assert!(recorder.take_file().exists());
    assert!(
        recorder
            .harness
            .path("state/arrangement/track-1/take.json")
            .exists()
    );

    recorder.harness.project.undo().unwrap();
    assert!(recorder.harness.project.resolve::<Clip>(&clip).is_none());
    assert!(
        !recorder
            .harness
            .path("state/arrangement/track-1/take.json")
            .exists()
    );
    assert!(recorder.take_file().exists(), "undo removed the raw take");
    assert_eq!(recorder.harness.project.undo_label(), None);

    // Redo brings the clip back at the same id, so it finds its take again.
    recorder.harness.project.redo().unwrap();
    assert!(recorder.harness.project.resolve::<Clip>(&clip).is_some());
    assert!(recorder.take_file().exists());
}

/// The raw take holds what arrived, in real time, with both velocities and the pedal.
#[test]
fn the_raw_take_holds_what_was_played_in_real_time() {
    let mut recorder = Recorder::new();
    let messages = [
        (6_000, pedal(127)),
        (12_000, on(60, 88)),
        (
            24_000,
            Played::Off {
                pitch: Pitch::new(60).unwrap(),
                velocity: 31,
            },
        ),
        (36_000, pedal(0)),
    ];
    let (take, _) = recorder.record(&messages, BAR);
    let clip = recorder.save(&take);
    assert_eq!(clip.as_str(), CLIP);

    // The clip names its take, and that name is the file.
    assert_eq!(recorder.clip().take.as_deref(), Some("take-1"));
    let text = std::fs::read_to_string(recorder.take_file()).unwrap();
    let raw: RawTake = serde_json::from_str(&text).unwrap();
    assert_eq!(raw.start_tick, recorder.clip().start.0);
    assert_eq!(raw.pedal_at_start, 0);
    let kinds: Vec<RawEvent> = raw
        .events
        .iter()
        .map(|event| match *event {
            RawEvent::On {
                pitch, velocity, ..
            } => RawEvent::On {
                time_us: 0,
                pitch,
                velocity,
            },
            RawEvent::Off {
                pitch, velocity, ..
            } => RawEvent::Off {
                time_us: 0,
                pitch,
                velocity,
            },
            RawEvent::Pedal { value, .. } => RawEvent::Pedal { time_us: 0, value },
        })
        .collect();
    assert_eq!(
        kinds,
        [
            RawEvent::Pedal {
                time_us: 0,
                value: 127
            },
            RawEvent::On {
                time_us: 0,
                pitch: 60,
                velocity: 88
            },
            // The key up velocity is in the take and nowhere else.
            RawEvent::Off {
                time_us: 0,
                pitch: 60,
                velocity: 31
            },
            RawEvent::Pedal {
                time_us: 0,
                value: 0
            },
        ]
    );
    // The times are real and in order, counted from the start of the recording.
    let times: Vec<u64> = raw
        .events
        .iter()
        .map(|event| match *event {
            RawEvent::On { time_us, .. } | RawEvent::Off { time_us, .. } => time_us,
            RawEvent::Pedal { time_us, .. } => time_us,
        })
        .collect();
    assert!(times.windows(2).all(|pair| pair[0] <= pair[1]), "{times:?}");
    assert!(!recorder.clip().notes.is_empty());
}

/// The click is not part of the piece and is not part of a take either: it makes no MIDI
/// message and no note. The same playing gives the same clip with it on and with it off.
#[test]
fn recording_with_the_click_on_and_off_gives_the_same_clip() {
    let messages = [
        (6_000, pedal(127)),
        (12_000, on(60, 88)),
        (24_000, off(60)),
        (36_000, on(64, 70)),
        (60_000, off(64)),
        (72_000, pedal(0)),
    ];
    let take_of = |click: bool| {
        let mut recorder = match click {
            true => Recorder::new().with_click(),
            false => Recorder::new(),
        };
        let (take, heard) = recorder.record(&messages, BAR);
        recorder.save(&take);
        (recorder.clip(), heard)
    };
    let (with_click, clicking) = take_of(true);
    let (without_click, quiet) = take_of(false);
    assert_eq!(with_click, without_click);
    assert!(!with_click.notes.is_empty());
    // The click was really in the signal, so the two runs were not the same by accident.
    assert!(difference(&clicking, &quiet).is_some());
}
