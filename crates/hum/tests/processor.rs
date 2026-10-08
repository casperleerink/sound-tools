#![allow(clippy::unwrap_used)]

//! The Hum processor in an engine: notes into an instrument and a source, a trigger and a live
//! control from the interface, a watch back to it, and the beat of the transport.

use sound_core::{
    Automation, Connection, Engine, EngineConfig, EngineControl, EventOutput, Node, Ports,
    PrepareConfig, ProcessContext, Processor, Watch,
};
use sound_hum::{Hum, HumUpdate, Kind, Machine, Values, compile};
use sound_notes::{NoteEvent, Pitch, Velocity};

const SAMPLE_RATE: u32 = 48_000;

/// Plays notes at frames from the start.
struct Score {
    notes: Vec<(usize, NoteEvent)>,
    frame: usize,
}

const SCORE_OUT: EventOutput<NoteEvent> = EventOutput::new(0);

impl Processor for Score {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_output(SCORE_OUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, (): &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let block = self.frame..self.frame + context.frames;
        for (frame, event) in &self.notes {
            if block.contains(frame) {
                context
                    .event_outputs
                    .push(SCORE_OUT, frame - self.frame, *event);
            }
        }
        self.frame += context.frames;
    }
}

fn on(pitch: u8) -> NoteEvent {
    NoteEvent::On {
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    }
}

fn off(pitch: u8) -> NoteEvent {
    NoteEvent::Off {
        pitch: Pitch::new(pitch).unwrap(),
    }
}

struct Played {
    control: EngineControl,
    engine: Engine,
    hum: Node<Hum>,
    watches: Vec<Watch>,
}

/// An engine that plays `code` as `kind` to the device, with `notes` into it.
fn play(kind: Kind, code: &[&str], notes: Vec<(usize, NoteEvent)>) -> Played {
    let lines: Vec<String> = code.iter().map(|line| line.to_string()).collect();
    let code = compile(&lines).unwrap();
    let watches: Vec<Watch> = code.watches.iter().map(|_| Watch::new()).collect();
    let machine = Box::new(Machine::new(code, SAMPLE_RATE as f32));
    let hum = Hum::new(kind, machine, Values::default(), watches.clone());
    let (mut control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    let hum = edit.add_processor("hum", hum).unwrap();
    edit.connect(Connection::to_device(hum.id(), Hum::OUTPUT, 0))
        .unwrap();
    if kind != Kind::Effect {
        let score = edit
            .add_processor("score", Score { notes, frame: 0 })
            .unwrap();
        edit.connect(Connection::new(score.id(), SCORE_OUT, hum.id(), Hum::NOTES))
            .unwrap();
    }
    edit.commit().unwrap();
    Played {
        control,
        engine,
        hum,
        watches,
    }
}

impl Played {
    /// The left channel of the next `frames` frames.
    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        for block in output.chunks_mut(64 * 2) {
            self.engine.process_block(block);
        }
        let status = self.control.poll().unwrap();
        assert_eq!((status.event_overflows, status.port_misuses), (0, 0));
        output.iter().step_by(2).copied().collect()
    }
}

fn loudest(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

/// The frequency of a steady tone, from the first to the last upward zero crossing.
fn frequency(samples: &[f32]) -> f32 {
    let rises: Vec<usize> = (samples.windows(2).enumerate())
        .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
        .map(|(frame, _)| frame)
        .collect();
    match (rises.first(), rises.last()) {
        (Some(first), Some(last)) if last > first => {
            (rises.len() - 1) as f32 * SAMPLE_RATE as f32 / (last - first) as f32
        }
        _ => 0.0,
    }
}

const SINE_VOICE: &[&str] = &[
    "level = adsr(gate, 5, 50, 0.5, 100)",
    "out = 0.2 * velocity * level * sin(phasor(freq) * tau)",
];

#[test]
fn an_instrument_plays_each_note_at_its_pitch_until_its_release_is_over() {
    let notes = vec![(4_800, on(69)), (28_800, off(69))];
    let mut played = play(Kind::Instrument { voices: 4 }, SINE_VOICE, notes);
    let output = played.render(48_000);
    assert_eq!(loudest(&output[..4_800]), 0.0);
    let held = &output[9_600..28_800];
    assert!((frequency(held) - 440.0).abs() < 1.0, "{}", frequency(held));
    // The release is 100 ms: 4800 frames, and the voice is quiet after it.
    assert_eq!(loudest(&output[28_800 + 4_900..]), 0.0);
}

#[test]
fn an_instrument_plays_a_chord_as_one_voice_per_note() {
    let one = loudest(
        &play(
            Kind::Instrument { voices: 4 },
            SINE_VOICE,
            vec![(0, on(60))],
        )
        .render(9_600)[4_800..],
    );
    let chord = vec![(0, on(60)), (0, on(64)), (0, on(67))];
    let three = play(Kind::Instrument { voices: 4 }, SINE_VOICE, chord).render(9_600);
    // Three sines add up louder than one, and less than three times as loud.
    let three = loudest(&three[4_800..]);
    assert!(three > one * 1.5 && three < one * 3.01, "{one} and {three}");
}

#[test]
fn a_note_that_takes_a_held_voice_starts_its_envelope_again() {
    let code = [
        "level = adsr(gate, 10, 10, 0.5, 10)",
        "watch shown = level",
        "out = 0.01 * level",
    ];
    // Eight voices, all held at their sustain: the ninth note takes the oldest.
    let mut notes: Vec<(usize, NoteEvent)> = (60..68).map(|pitch| (0, on(pitch))).collect();
    notes.push((4_800, on(72)));
    let mut played = play(Kind::Instrument { voices: 8 }, &code, notes);
    played.render(4_800);
    assert_eq!(played.watches[0].get(), 0.5);
    // 5 ms of a 10 ms attack from 0.5 reaches the top.
    played.render(256);
    assert!(played.watches[0].get() > 0.9, "{}", played.watches[0].get());
}

#[test]
fn a_source_sounds_with_no_note_and_follows_the_newest_held_one() {
    // Before any note `gate` is 0, so this drone plays 110 Hz until a note comes.
    let code = ["out = 0.2 * sin(phasor(mix(110, freq, gate)) * tau)"];
    let notes = vec![(24_000, on(69)), (36_000, on(81)), (42_000, off(81))];
    let mut played = play(Kind::Source, &code, notes);
    let output = played.render(48_000);
    assert!((frequency(&output[..24_000]) - 110.0).abs() < 1.0);
    assert!((frequency(&output[24_100..36_000]) - 440.0).abs() < 1.0);
    assert!((frequency(&output[36_100..42_000]) - 880.0).abs() < 1.0);
    // Letting go of the newest goes back to the one still held.
    assert!((frequency(&output[42_100..]) - 440.0).abs() < 1.0);
}

#[test]
fn a_trigger_fires_in_one_frame_and_a_watch_shows_what_it_counted() {
    let code = [
        "trigger hit",
        "history count",
        "count = count + hit",
        "watch total = count",
        "out = 0",
    ];
    let mut played = play(Kind::Source, &code, Vec::new());
    played.render(64);
    for _ in 0..3 {
        played
            .control
            .update(played.hum, HumUpdate::Trigger { index: 0 })
            .unwrap();
        played.render(640);
    }
    assert_eq!(played.watches[0].get(), 3.0);
}

#[test]
fn a_note_from_the_interface_plays_a_voice_as_a_key_would() {
    let code = ["out = 0.2 * sin(phasor(freq) * tau) * adsr(gate, 1, 1, 1, 1)"];
    let mut played = play(Kind::Instrument { voices: 8 }, &code, Vec::new());
    assert_eq!(loudest(&played.render(4_800)), 0.0);
    played
        .control
        .update(played.hum, HumUpdate::Note(on(69)))
        .unwrap();
    let output = played.render(24_000);
    assert!((frequency(&output[2_400..]) - 440.0).abs() < 1.0);
    played
        .control
        .update(played.hum, HumUpdate::Note(off(69)))
        .unwrap();
    assert_eq!(loudest(&played.render(24_000)[4_800..]), 0.0);
}

#[test]
fn a_live_control_glides_to_where_the_interface_puts_it() {
    let mut played = play(Kind::Source, &["live x = 0 [0, 1]", "out = x"], Vec::new());
    assert_eq!(played.render(64)[63], 0.0);
    let live = HumUpdate::Live {
        index: 0,
        value: 1.0,
    };
    played.control.update(played.hum, live).unwrap();
    let output = played.render(1_920);
    // It takes 20 ms, 960 frames, and does not jump.
    assert!(output[479] > 0.4 && output[479] < 0.6, "{}", output[479]);
    assert_eq!(output[1_919], 1.0);
    let steps = output.windows(2).map(|pair| (pair[1] - pair[0]).abs());
    assert!(steps.fold(0.0_f32, f32::max) < 0.002);
}

#[test]
fn the_beat_counts_quarter_notes_while_the_transport_plays() {
    let mut played = play(Kind::Source, &["out = beat / 100"], Vec::new());
    assert_eq!(played.render(4_800)[4_799], 0.0);
    played.control.play();
    // 120 bpm is two quarter notes a second.
    let output = played.render(48_000);
    assert!(
        (output[47_999] * 100.0 - 2.0).abs() < 0.01,
        "{}",
        output[47_999] * 100.0
    );
}

/// The update the behaviour sends when the code of a tool of `kind` is new.
fn new_code(kind: Kind, line: &str) -> HumUpdate {
    let code = compile(&[line.to_string()]).unwrap();
    let made = || Some(Box::new(Machine::new(code.clone(), SAMPLE_RATE as f32)));
    HumUpdate::Set {
        machines: (0..kind.machines()).map(|_| made()).collect(),
        values: Box::default(),
        watches: Vec::new(),
    }
}

#[test]
fn new_code_fades_in_without_a_jump() {
    let mut played = play(Kind::Source, &["out = 0.5"], Vec::new());
    played.render(640);
    let update = new_code(Kind::Source, "out = -0.5");
    played.control.update(played.hum, update).unwrap();
    let output = played.render(1_920);
    let steps = output.windows(2).map(|pair| (pair[1] - pair[0]).abs());
    // From 0.5 to -0.5 over 10 ms, 480 frames.
    assert!(steps.fold(0.0_f32, f32::max) < 0.003);
    assert_eq!(output[1_919], -0.5);
}

/// Sends one lane of automation, as an arrangement does, for its first `blocks` blocks.
struct Lane {
    value: f32,
    blocks: usize,
}

const LANE_OUT: EventOutput<Automation> = EventOutput::new(0);

impl Processor for Lane {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_output(LANE_OUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, (): &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if self.blocks > 0 {
            self.blocks -= 1;
            let lane = Automation {
                parameter: 0,
                value: self.value,
            };
            context.event_outputs.push(LANE_OUT, 0, lane);
        }
    }
}

#[test]
fn a_lane_moves_a_param_and_the_record_takes_it_back_when_the_lane_stops() {
    let lines = [
        "param tone = 0.25 [0, 1]".to_string(),
        "out = tone".to_string(),
    ];
    let code = compile(&lines).unwrap();
    let mut values = Values::default();
    values.parameters[0] = 0.25;
    values.automated = vec![0];
    let machine = Box::new(Machine::new(code, SAMPLE_RATE as f32));
    let hum = Hum::new(Kind::Source, machine, values, Vec::new());
    let (mut control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    let hum = edit.add_processor("hum", hum).unwrap();
    let lane = edit
        .add_processor(
            "lane",
            Lane {
                value: 1.0,
                blocks: 100,
            },
        )
        .unwrap();
    edit.connect(Connection::to_device(hum.id(), Hum::OUTPUT, 0))
        .unwrap();
    edit.connect(Connection::new(
        lane.id(),
        LANE_OUT,
        hum.id(),
        Hum::AUTOMATION,
    ))
    .unwrap();
    edit.commit().unwrap();
    let mut played = Played {
        control,
        engine,
        hum,
        watches: Vec::new(),
    };
    // 100 blocks of 64 frames: the lane holds it at 1 after its 20 ms glide.
    let output = played.render(6_400);
    assert_eq!(output[6_399], 1.0);
    // Then no lane: back to the record.
    let output = played.render(1_920);
    assert_eq!(output[1_919], 0.25);
}

#[test]
fn a_note_after_new_code_plays_only_the_new_code() {
    let kind = Kind::Instrument { voices: 1 };
    let notes = vec![(0, on(60)), (64, off(60)), (9_600, on(60))];
    let mut played = play(kind, &["out = 0.5 * gate"], notes);
    // Released at frame 64 and silent since, so the voice is idle when the code changes.
    played.render(4_800);
    played
        .control
        .update(played.hum, new_code(kind, "out = -0.5 * gate"))
        .unwrap();
    let output = played.render(9_600);
    // The note at frame 9600 starts with the new code, not with a fade from the old.
    assert_eq!(output[4_800], -0.5);
}
