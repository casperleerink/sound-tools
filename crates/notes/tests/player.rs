//! Launched patterns, played offline through the engine into a probe whose level says which
//! notes it holds: a launch lands on its frame, a pattern keeps its phase through a seek, the
//! loop and a swap, and every note that started gets its off.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use proptest::prelude::*;
use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, EventInput, Node, Ports,
    PrepareConfig, ProcessContext, Processor, SignatureRun, Tempo, TempoMap, Ticks, TimeSignature,
    TimeSignatures,
};
use sound_notes::{
    Launch, Length, NO_PATTERN, Note, NoteEvent, NotePlayer, NotePlayerUpdate, Pattern, Pitch,
    Placing, PlayerWatches, QUEUED_STOP, Quantize, Velocity,
};

const SAMPLE_RATE: u32 = 48_000;
/// Frames per tick at 120 bpm and 48 kHz.
const TICK: usize = 25;
const BAR: u64 = 3840;
/// The phase shows the tick heard at the start of the last sub-block, which is at most 64
/// frames, so up to 3 ticks of a round of 960 before the end of what was rendered.
const SUB_BLOCK_PHASE: f32 = 3.0 / 960.0;

/// Makes no sound but a level: the sum of the pitches it holds, once per held note. An off
/// releases every note of its pitch, as the note contract says.
struct Probe {
    held: [u32; 128],
    /// What it says its output lags by, so the player before it is led by that much.
    latency: u32,
}

impl Probe {
    const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    const OUTPUT: AudioOutput = AudioOutput::new(0);

    fn level(&self) -> f32 {
        let pitches = self.held.iter().enumerate();
        pitches
            .map(|(pitch, held)| (pitch as u32 * held) as f32)
            .sum()
    }
}

impl Processor for Probe {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn latency(&self) -> u32 {
        self.latency
    }

    fn update(&mut self, (): &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let events = context.event_inputs.get(Self::NOTES);
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        let mut events = events.iter().peekable();
        for (frame, sample) in left.iter_mut().enumerate() {
            while let Some(timed) = events.next_if(|timed| timed.offset <= frame) {
                match timed.event {
                    NoteEvent::On { pitch, .. } => self.held[usize::from(pitch.number())] += 1,
                    NoteEvent::Off { pitch } => self.held[usize::from(pitch.number())] = 0,
                    NoteEvent::AllOff => self.held = [0; 128],
                    _ => {}
                }
            }
            *sample = self.level();
        }
        right.copy_from_slice(left);
    }
}

struct Harness {
    control: EngineControl,
    engine: Engine,
    player: Node<NotePlayer>,
    watches: PlayerWatches,
}

impl Harness {
    fn new(patterns: Vec<Pattern>) -> Self {
        Self::with_latency(patterns, 0)
    }

    /// A player into a probe that says its output lags by `latency` frames.
    fn with_latency(patterns: Vec<Pattern>, latency: u32) -> Self {
        let (mut control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
        let watches = PlayerWatches::default();
        let mut edit = control.edit();
        let player = NotePlayer::watched(watches.clone());
        let player = edit.add_processor("player", player).unwrap();
        let probe = edit
            .add_processor(
                "probe",
                Probe {
                    held: [0; 128],
                    latency,
                },
            )
            .unwrap();
        let notes = Connection::new(player.id(), NotePlayer::NOTES, probe.id(), Probe::NOTES);
        edit.connect(notes).unwrap();
        edit.connect(Connection::to_device(probe.id(), Probe::OUTPUT, 0))
            .unwrap();
        edit.update(player, NotePlayerUpdate::Patterns(Arc::new(patterns)))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            player,
            watches,
        }
    }

    fn launch(&mut self, pattern: Option<usize>, quantize: Quantize) {
        let launch = NotePlayerUpdate::Launch(Launch {
            pattern,
            quantize,
            once: false,
        });
        self.control.update(self.player, launch).unwrap();
    }

    fn once(&mut self, quantize: Quantize) {
        let launch = Launch {
            pattern: Some(0),
            quantize,
            once: true,
        };
        (self.control)
            .update(self.player, NotePlayerUpdate::Launch(launch))
            .unwrap();
    }

    fn place(&mut self, at: u64, once: bool) {
        let placing = Placing {
            pattern: Some(0),
            at: Ticks(at),
            once,
        };
        (self.control)
            .update(self.player, NotePlayerUpdate::Place(placing))
            .unwrap();
    }

    fn swap(&mut self, patterns: Vec<Pattern>) {
        let patterns = NotePlayerUpdate::Patterns(Arc::new(patterns));
        self.control.update(self.player, patterns).unwrap();
    }

    /// In device buffers of 100 frames, so changes land on sub-block edges and inside them.
    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        for buffer in output.chunks_mut(100) {
            self.engine.process_block(buffer);
        }
        output
    }

    fn render_ticks(&mut self, ticks: u64) -> Vec<f32> {
        self.render(ticks as usize * TICK)
    }

    fn watched(&self) -> (f32, f32, f32) {
        let watches = &self.watches;
        (
            watches.playing.get(),
            watches.queued.get(),
            watches.phase.get(),
        )
    }
}

fn note(start: u64, length: u64, pitch: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    }
}

fn pattern(length: u64, notes: &[Note]) -> Pattern {
    Pattern::new(Length::new(Ticks(length)).unwrap(), notes).unwrap()
}

/// Two notes in a beat of 960 ticks: 60 from 0, 62 from 480, each 240 long.
fn two_notes() -> Pattern {
    pattern(960, &[note(0, 240, 60), note(480, 240, 62)])
}

/// The frames where the level changes, with the new level.
fn level_changes(samples: &[f32]) -> Vec<(usize, f32)> {
    let mut changes = Vec::new();
    let mut last = 0.0;
    for (frame, sample) in samples.iter().enumerate() {
        if *sample != last {
            changes.push((frame, *sample));
            last = *sample;
        }
    }
    changes
}

/// Level changes given in ticks, as frames.
fn at_ticks(changes: &[(u64, f32)]) -> Vec<(usize, f32)> {
    let frames = changes
        .iter()
        .map(|(tick, level)| (*tick as usize * TICK, *level));
    frames.collect()
}

#[test]
fn a_bar_launch_lands_on_the_next_bar_line_and_a_beat_launch_on_the_next_beat() {
    for (quantize, lands) in [(Quantize::Bar, BAR), (Quantize::Beat, 960)] {
        let mut harness = Harness::new(vec![two_notes()]);
        harness.control.play();
        let mut output = harness.render_ticks(100);
        harness.launch(Some(0), quantize);
        output.extend(harness.render_ticks(lands + 960 - 100));
        let expected = [
            (lands, 60.0),
            (lands + 240, 0.0),
            (lands + 480, 62.0),
            (lands + 720, 0.0),
        ];
        assert_eq!(level_changes(&output), at_ticks(&expected), "{quantize:?}");
    }
}

#[test]
fn a_bar_launch_lands_on_the_bar_line_after_a_time_signature_change() {
    let mut harness = Harness::new(vec![two_notes()]);
    // Bar 1 in 4/4 from 0, then 7/8 from bar 2 on: bar 2 starts at 3840, bar 3 at 7200.
    let runs = vec![
        SignatureRun {
            signature: TimeSignature::default(),
            bars: 1.try_into().unwrap(),
        },
        SignatureRun {
            signature: TimeSignature::new(7, 8).unwrap(),
            bars: 1.try_into().unwrap(),
        },
    ];
    let time_signatures = TimeSignatures::new(runs).unwrap();
    let tempo = Tempo::from_bpm(120.0).unwrap();
    harness
        .control
        .set_tempo_map(TempoMap::constant(time_signatures, tempo));
    harness.control.play();
    let mut output = harness.render_ticks(4000);
    harness.launch(Some(0), Quantize::Bar);
    output.extend(harness.render_ticks(7200 + 960 - 4000));
    assert_eq!(
        level_changes(&output),
        at_ticks(&[(7200, 60.0), (7440, 0.0), (7680, 62.0), (7920, 0.0)])
    );
}

#[test]
fn a_launch_while_stopped_takes_effect_at_once_and_a_seek_before_play_moves_it_along() {
    let mut harness = Harness::new(vec![two_notes()]);
    harness.control.seek(Ticks(1000));
    let mut output = harness.render(100);
    harness.launch(Some(0), Quantize::Bar);
    output.extend(harness.render(100));
    assert_eq!(harness.watched(), (0.0, NO_PATTERN, 0.0));
    harness.control.seek(Ticks(0));
    output.extend(harness.render(100));
    harness.control.play();
    output.extend(harness.render_ticks(960));
    let played = |tick: usize| 300 + tick * TICK;
    assert_eq!(
        level_changes(&output),
        [
            (played(0), 60.0),
            (played(240), 0.0),
            (played(480), 62.0),
            (played(720), 0.0)
        ]
    );
}

#[test]
fn a_launch_while_stopped_is_anchored_where_play_starts_also_with_latency_after_the_player() {
    // 2400 frames are 96 ticks: the player sees the transport that far ahead while stopped.
    let mut harness = Harness::with_latency(vec![two_notes()], 2400);
    let mut output = harness.render(100);
    harness.launch(Some(0), Quantize::Bar);
    output.extend(harness.render(100));
    harness.control.play();
    output.extend(harness.render_ticks(960));
    let played = |tick: usize| 200 + tick * TICK;
    assert_eq!(
        level_changes(&output),
        [
            (played(0), 60.0),
            (played(240), 0.0),
            (played(480), 62.0),
            (played(720), 0.0)
        ]
    );
}

#[test]
fn a_seek_keeps_the_phase() {
    let mut harness = Harness::new(vec![two_notes()]);
    harness.control.play();
    harness.render_ticks(100);
    harness.launch(Some(0), Quantize::Now);
    harness.render_ticks(100);
    // Launched on 100, so a round starts on 100 + 960 * n: 2020 is the round after 2000.
    harness.control.seek(Ticks(2000));
    let output = harness.render_ticks(960);
    assert_eq!(
        level_changes(&output),
        at_ticks(&[(20, 60.0), (260, 0.0), (500, 62.0), (740, 0.0)])
    );
}

#[test]
fn the_loop_keeps_the_phase() {
    let mut harness = Harness::new(vec![two_notes()]);
    harness.control.set_loop(Some(Ticks(0)..Ticks(1000)));
    harness.control.play();
    harness.render_ticks(100);
    harness.launch(Some(0), Quantize::Now);
    let first = harness.render_ticks(900);
    let rounds = [(0, 60.0), (240, 0.0), (480, 62.0), (720, 0.0)];
    assert_eq!(level_changes(&first), at_ticks(&rounds));
    // Back at 0, the pattern is where it was 100 ticks before its launch: its next round
    // starts on 100.
    let second = harness.render_ticks(1000);
    let shifted = rounds.map(|(tick, level)| (tick + 100, level));
    assert_eq!(level_changes(&second), at_ticks(&shifted));
}

#[test]
fn a_switch_and_a_stop_end_the_held_notes() {
    let long = |pitch| pattern(960, &[note(0, 960, pitch)]);
    let mut harness = Harness::new(vec![long(60), long(64)]);
    harness.launch(Some(0), Quantize::Now);
    harness.control.play();
    let mut output = harness.render_ticks(500);
    harness.launch(Some(1), Quantize::Now);
    output.extend(harness.render_ticks(200));
    harness.launch(None, Quantize::Bar);
    output.extend(harness.render_ticks(100));
    let (playing, queued, phase) = harness.watched();
    assert_eq!((playing, queued), (1.0, QUEUED_STOP));
    assert!((phase - 0.3125).abs() < SUB_BLOCK_PHASE, "{phase}");
    output.extend(harness.render_ticks(2 * BAR - 100));
    assert_eq!(
        level_changes(&output),
        at_ticks(&[(0, 60.0), (500, 64.0), (BAR, 0.0)])
    );
    assert_eq!(harness.watched(), (NO_PATTERN, NO_PATTERN, 0.0));
}

#[test]
fn a_swap_while_playing_keeps_the_phase_and_the_held_notes_follow_it() {
    let mut harness = Harness::new(vec![two_notes()]);
    harness.launch(Some(0), Quantize::Now);
    harness.control.play();
    let mut output = harness.render_ticks(600);
    let (playing, queued, phase) = harness.watched();
    assert_eq!((playing, queued), (0.0, NO_PATTERN));
    assert!((phase - 0.625).abs() < SUB_BLOCK_PHASE, "{phase}");
    // The note of 62 is shorter now, so it is over: it ends at once. The new note plays in
    // the same round.
    let edited = [note(0, 240, 60), note(480, 100, 62), note(720, 120, 67)];
    harness.swap(vec![pattern(960, &edited)]);
    output.extend(harness.render_ticks(1200 - 600));
    assert_eq!(
        level_changes(&output),
        at_ticks(&[
            (0, 60.0),
            (240, 0.0),
            (480, 62.0),
            (600, 0.0),
            (720, 67.0),
            (840, 0.0),
            (960, 60.0)
        ])
    );
}

#[test]
fn a_launch_outside_the_set_plays_nothing_and_shows_what_was_launched() {
    let mut harness = Harness::new(vec![two_notes()]);
    harness.control.play();
    let mut before = harness.render_ticks(100);
    harness.launch(Some(3), Quantize::Bar);
    before.extend(harness.render_ticks(100));
    assert_eq!(harness.watched(), (NO_PATTERN, 3.0, 0.0));
    let after = harness.render_ticks(2 * BAR);
    assert_eq!(harness.watched(), (3.0, NO_PATTERN, 0.0));
    assert_eq!(level_changes(&before), []);
    assert_eq!(level_changes(&after), []);
}

#[derive(Clone, Debug)]
enum Step {
    Render(usize),
    Launch(Option<usize>, Quantize),
    Seek(u64),
    Swap(Vec<Pattern>),
    Pause,
    Play,
}

fn any_pattern() -> impl Strategy<Value = Pattern> {
    let pitch = prop::sample::select(vec![60_u8, 62, 64, 67]);
    let notes = prop::collection::vec((0.0..1.0_f64, 1..3_000_u64, pitch), 0..6);
    (1..2_000_u64, notes).prop_map(|(length, notes)| {
        let notes: Vec<Note> = notes
            .into_iter()
            .map(|(start, duration, pitch)| note((start * length as f64) as u64, duration, pitch))
            .collect();
        pattern(length, &notes)
    })
}

fn any_step() -> impl Strategy<Value = Step> {
    let quantize = prop::sample::select(vec![Quantize::Now, Quantize::Beat, Quantize::Bar]);
    // One index past the set, which plays nothing.
    let launched = prop::option::of(0..4_usize);
    prop_oneof![
        4 => (1..10_000_usize).prop_map(Step::Render),
        3 => (launched, quantize).prop_map(|(pattern, quantize)| Step::Launch(pattern, quantize)),
        1 => (0..20_000_u64).prop_map(Step::Seek),
        1 => prop::collection::vec(any_pattern(), 3).prop_map(Step::Swap),
        1 => Just(Step::Pause),
        1 => Just(Step::Play),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Whatever is launched, swapped and sought, a stop that lands while playing leaves no
    /// note held. A stop sends offs, no `AllOff`, so a note without its off would show.
    #[test]
    fn every_note_that_started_gets_its_off(
        patterns in prop::collection::vec(any_pattern(), 3),
        steps in prop::collection::vec(any_step(), 1..40),
    ) {
        let mut harness = Harness::new(patterns);
        harness.control.play();
        for step in steps {
            match step {
                Step::Render(frames) => {
                    harness.render(frames);
                }
                Step::Launch(pattern, quantize) => harness.launch(pattern, quantize),
                Step::Seek(tick) => harness.control.seek(Ticks(tick)),
                Step::Swap(patterns) => harness.swap(patterns),
                Step::Pause => harness.control.pause(),
                Step::Play => harness.control.play(),
            }
        }
        harness.control.play();
        harness.render(100);
        harness.launch(None, Quantize::Now);
        let output = harness.render(100);
        prop_assert_eq!(output.last().copied(), Some(0.0));
        let status = harness.control.poll().unwrap();
        prop_assert_eq!(status.event_overflows, 0);
    }
}

#[test]
fn a_placed_pattern_loops_from_its_tick_through_a_seek_and_plays_nothing_before_it() {
    let mut harness = Harness::new(vec![two_notes()]);
    harness.place(1000, false);
    harness.control.play();
    let mut output = harness.render_ticks(2000);
    // A seek while stopped moves no placed pattern, as it moves a launched one.
    harness.control.pause();
    harness.control.seek(Ticks(500));
    output.extend(harness.render(100));
    harness.control.play();
    output.extend(harness.render_ticks(600));
    let expected = [
        (1000, 60.0),
        (1240, 0.0),
        (1480, 62.0),
        (1720, 0.0),
        (1960, 60.0),
    ];
    let mut changes = at_ticks(&expected);
    // The pause ends the note of 1960; from 500 the next note is the one of 1000.
    changes.push((2000 * TICK, 0.0));
    changes.push((2000 * TICK + 100 + 500 * TICK, 60.0));
    assert_eq!(level_changes(&output), changes);
}

#[test]
fn a_pattern_played_once_stops_after_its_round_placed_or_launched() {
    let mut harness = Harness::new(vec![two_notes()]);
    harness.place(960, true);
    harness.control.play();
    let output = harness.render_ticks(3000);
    let expected = [(960, 60.0), (1200, 0.0), (1440, 62.0), (1680, 0.0)];
    assert_eq!(level_changes(&output), at_ticks(&expected));
    assert_eq!(harness.watched().0, NO_PATTERN);

    let mut harness = Harness::new(vec![two_notes()]);
    harness.control.play();
    let mut output = harness.render_ticks(100);
    harness.once(Quantize::Beat);
    output.extend(harness.render_ticks(2900));
    let expected = [(960, 60.0), (1200, 0.0), (1440, 62.0), (1680, 0.0)];
    assert_eq!(level_changes(&output), at_ticks(&expected));
}

#[test]
fn placing_again_ends_the_notes_the_pattern_holds() {
    let mut harness = Harness::new(vec![pattern(960, &[note(0, 900, 60)])]);
    harness.place(0, false);
    harness.control.play();
    let mut output = harness.render_ticks(100);
    harness.place(5000, false);
    output.extend(harness.render_ticks(200));
    let changes = level_changes(&output);
    assert_eq!(changes.first(), Some(&(0, 60.0)));
    let (ended, level) = changes[1];
    assert_eq!(level, 0.0);
    assert!((100 * TICK..100 * TICK + 100).contains(&ended), "{ended}");
}
