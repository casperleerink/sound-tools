#![allow(clippy::unwrap_used)]

//! The `Signals` processor in an engine: notes into an instrument and a source, a trigger and a
//! live control from the interface, a watch back to it, and the beat of the transport.

use std::f32::consts::TAU;

use sound_core::{
    Automation, Connection, Engine, EngineConfig, EngineControl, EventOutput, Node, Ports,
    PrepareConfig, ProcessContext, Processor, Watch,
};
use sound_notes::{NoteEvent, Pitch, Velocity};
use sound_signals::{Code, Kind, Machine, Signals, SignalsUpdate, Values};

mod score;
mod sound;

use sound::*;

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
    signals: Node<Signals>,
    watches: Vec<Watch>,
}

/// An engine that plays `code` as `kind` to the device, with `notes` into it.
fn play(kind: Kind, code: Code, notes: Vec<(usize, NoteEvent)>) -> Played {
    let watches: Vec<Watch> = code.watches.iter().map(|_| Watch::new()).collect();
    let machine = Box::new(Machine::new(code, SAMPLE_RATE as f32));
    let signals = Signals::new(kind, machine, Values::default(), watches.clone());
    let (mut control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    let signals = edit.add_processor("signals", signals).unwrap();
    edit.connect(Connection::to_device(signals.id(), Signals::OUTPUT, 0))
        .unwrap();
    if kind != Kind::Effect {
        let score = edit
            .add_processor("score", Score { notes, frame: 0 })
            .unwrap();
        edit.connect(Connection::new(
            score.id(),
            SCORE_OUT,
            signals.id(),
            Signals::NOTES,
        ))
        .unwrap();
    }
    edit.commit().unwrap();
    Played {
        control,
        engine,
        signals,
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

fn sine_voice() -> Code {
    compile(|| {
        let level = adsr(gate(), 5.0, 50.0, 0.5, 100.0);
        0.2 * velocity() * level * sin(phasor(freq()) * TAU)
    })
}

/// A sine at the note that plays while the gate is up.
fn sine_note() -> Code {
    compile(|| 0.2 * sin(phasor(freq()) * TAU) * adsr(gate(), 1.0, 1.0, 1.0, 1.0))
}

#[test]
fn an_instrument_plays_each_note_at_its_pitch_until_its_release_is_over() {
    let notes = vec![(4_800, on(69)), (28_800, off(69))];
    let mut played = play(Kind::Instrument { voices: 4 }, sine_voice(), notes);
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
            sine_voice(),
            vec![(0, on(60))],
        )
        .render(9_600)[4_800..],
    );
    let chord = vec![(0, on(60)), (0, on(64)), (0, on(67))];
    let three = play(Kind::Instrument { voices: 4 }, sine_voice(), chord).render(9_600);
    // Three sines add up louder than one, and less than three times as loud.
    let three = loudest(&three[4_800..]);
    assert!(three > one * 1.5 && three < one * 3.01, "{one} and {three}");
}

#[test]
fn two_voices_started_on_the_same_frame_play_different_noise() {
    let noise = || compile(|| 0.2 * noise() * adsr(gate(), 1.0, 1.0, 1.0, 1.0));
    let loudness = |notes| {
        let output = play(Kind::Instrument { voices: 4 }, noise(), notes).render(9_600);
        let held = &output[4_800..];
        (held.iter().map(|sample| sample * sample).sum::<f32>() / held.len() as f32).sqrt()
    };
    let one = loudness(vec![(0, on(60))]);
    let two = loudness(vec![(0, on(60)), (0, on(64))]);
    // The same noise twice is twice as loud; two different ones are about 1.41 times as loud.
    assert!(two > one * 1.2 && two < one * 1.6, "{one} and {two}");
}

#[test]
fn a_note_that_takes_a_held_voice_starts_its_envelope_again() {
    let code = compile(|| {
        let level = adsr(gate(), 10.0, 10.0, 0.5, 10.0);
        watch("shown", level);
        0.01 * level
    });
    // Eight voices, all held at their sustain: the ninth note takes the oldest.
    let mut notes: Vec<(usize, NoteEvent)> = (60..68).map(|pitch| (0, on(pitch))).collect();
    notes.push((4_800, on(72)));
    let mut played = play(Kind::Instrument { voices: 8 }, code, notes);
    played.render(4_800);
    assert_eq!(played.watches[0].get(), 0.5);
    // 5 ms of a 10 ms attack from 0.5 reaches the top.
    played.render(256);
    assert!(played.watches[0].get() > 0.9, "{}", played.watches[0].get());
}

#[test]
fn a_source_sounds_with_no_note_and_follows_the_newest_held_one() {
    // Before any note `gate` is 0, so this drone plays 110 Hz until a note comes.
    let code = compile(|| 0.2 * sin(phasor(mix(110.0, freq(), gate())) * TAU));
    let notes = vec![(24_000, on(69)), (36_000, on(81)), (42_000, off(81))];
    let mut played = play(Kind::Source, code, notes);
    let output = played.render(48_000);
    assert!((frequency(&output[..24_000]) - 110.0).abs() < 1.0);
    assert!((frequency(&output[24_100..36_000]) - 440.0).abs() < 1.0);
    assert!((frequency(&output[36_100..42_000]) - 880.0).abs() < 1.0);
    // Letting go of the newest goes back to the one still held.
    assert!((frequency(&output[42_100..]) - 440.0).abs() < 1.0);
}

#[test]
fn a_trigger_fires_in_one_frame_and_a_watch_shows_what_it_counted() {
    let code = compile(|| {
        let hit = trigger("hit");
        let count = feedback();
        count.set(count.read() + hit);
        watch("total", count.read());
        0.0
    });
    let mut played = play(Kind::Source, code, Vec::new());
    played.render(64);
    for _ in 0..3 {
        played
            .control
            .update(
                played.signals,
                SignalsUpdate::Trigger { index: 0, at: None },
            )
            .unwrap();
        played.render(640);
    }
    assert_eq!(played.watches[0].get(), 3.0);
}

#[test]
fn a_note_from_the_interface_plays_a_voice_for_its_length() {
    let mut played = play(Kind::Instrument { voices: 8 }, sine_note(), Vec::new());
    assert_eq!(loudest(&played.render(4_800)), 0.0);
    let note = SignalsUpdate::Note {
        pitch: Pitch::new(69).unwrap(),
        velocity: Velocity::new(100).unwrap(),
        at: None,
        frames: Some(24_000),
    };
    played.control.update(played.signals, note).unwrap();
    let output = played.render(48_000);
    assert!((frequency(&output[2_400..24_000]) - 440.0).abs() < 1.0);
    // Let go after its length, and over 1 ms of release.
    assert_eq!(loudest(&output[30_000..]), 0.0);
}

#[test]
fn a_note_with_no_length_plays_until_it_is_released() {
    let mut played = play(Kind::Instrument { voices: 8 }, sine_note(), Vec::new());
    let pitch = Pitch::new(69).unwrap();
    let note = SignalsUpdate::Note {
        pitch,
        velocity: Velocity::new(100).unwrap(),
        at: None,
        frames: None,
    };
    played.control.update(played.signals, note).unwrap();
    assert!(loudest(&played.render(96_000)[48_000..]) > 0.1);
    let release = SignalsUpdate::Release { pitch, at: None };
    played.control.update(played.signals, release).unwrap();
    assert_eq!(loudest(&played.render(9_600)[4_800..]), 0.0);
}

#[test]
fn a_release_lets_go_of_its_note_when_the_list_of_what_waits_for_its_frame_is_full() {
    let mut played = play(Kind::Instrument { voices: 8 }, sine_note(), Vec::new());
    let pitch = Pitch::new(69).unwrap();
    let note = SignalsUpdate::Note {
        pitch,
        velocity: Velocity::new(100).unwrap(),
        at: None,
        frames: None,
    };
    played.control.update(played.signals, note).unwrap();
    // A trigger far ahead waits in the list, which holds 256.
    for _ in 0..256 {
        let hit = SignalsUpdate::Trigger {
            index: 0,
            at: Some(u64::MAX),
        };
        played.control.update(played.signals, hit).unwrap();
        played.render(64);
    }
    assert!(loudest(&played.render(4_800)) > 0.1);
    let release = SignalsUpdate::Release { pitch, at: None };
    played.control.update(played.signals, release).unwrap();
    assert_eq!(loudest(&played.render(9_600)[4_800..]), 0.0);
}

#[test]
fn a_trigger_with_a_time_fires_on_that_frame_and_one_whose_time_passed_at_once() {
    let mut played = play(Kind::Source, compile(|| trigger("hit")), Vec::new());
    // Engine time is 640 from here.
    played.render(640);
    for at in [1_000, 100] {
        let hit = SignalsUpdate::Trigger {
            index: 0,
            at: Some(at),
        };
        played.control.update(played.signals, hit).unwrap();
    }
    let output = played.render(1_280);
    let fired: Vec<usize> = (0..output.len()).filter(|&at| output[at] == 1.0).collect();
    assert_eq!(fired, [0, 360]);
}

#[test]
fn a_live_control_glides_to_where_the_interface_puts_it() {
    let mut played = play(
        Kind::Source,
        compile(|| live("x", 0.0, [0.0, 1.0])),
        Vec::new(),
    );
    assert_eq!(played.render(64)[63], 0.0);
    let live = SignalsUpdate::Live {
        index: 0,
        value: 1.0,
    };
    played.control.update(played.signals, live).unwrap();
    let output = played.render(1_920);
    // It takes 20 ms, 960 frames, and does not jump.
    assert!(output[479] > 0.4 && output[479] < 0.6, "{}", output[479]);
    assert_eq!(output[1_919], 1.0);
    let steps = output.windows(2).map(|pair| (pair[1] - pair[0]).abs());
    assert!(steps.fold(0.0_f32, f32::max) < 0.002);
}

#[test]
fn the_beat_counts_quarter_notes_while_the_transport_plays() {
    let mut played = play(Kind::Source, compile(|| beat() / 100.0), Vec::new());
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

#[test]
fn an_instrument_silent_until_the_transport_stops_hears_the_beat_where_it_stopped() {
    // A silent instrument does not move the beat frame by frame while it plays; a source does.
    let stopped = |kind: Kind, notes: Vec<(usize, NoteEvent)>| {
        let mut played = play(kind, compile(|| beat() * gate()), notes);
        played.control.play();
        played.render(4_800);
        played.control.stop();
        played.render(4_800)[4_799]
    };
    let instrument = stopped(Kind::Instrument { voices: 1 }, vec![(4_900, on(60))]);
    let source = stopped(Kind::Source, vec![(0, on(60))]);
    assert!(source > 0.0);
    assert_eq!(instrument, source);
}

#[test]
fn a_live_control_stays_where_it_is_when_the_code_is_new() {
    let mut played = play(
        Kind::Source,
        compile(|| live("x", 0.0, [0.0, 1.0])),
        Vec::new(),
    );
    let moved = SignalsUpdate::Live {
        index: 0,
        value: 1.0,
    };
    played.control.update(played.signals, moved).unwrap();
    played.render(1_920);
    // New code, with another live control first: `x` keeps its value under its name.
    let code = compile(|| {
        let y = live("y", 0.5, [0.0, 1.0]);
        live("x", 0.0, [0.0, 1.0]) + y
    });
    let new = SignalsUpdate::Set {
        machines: vec![Some(Box::new(Machine::new(code, SAMPLE_RATE as f32)))],
        values: Box::default(),
        watches: Vec::new(),
    };
    played.control.update(played.signals, new).unwrap();
    assert_eq!(played.render(1_920)[1_919], 1.5);
}

/// The update the behaviour sends when the code of a tool of `kind` is new.
fn new_code(kind: Kind, code: Code) -> SignalsUpdate {
    let made = || Some(Box::new(Machine::new(code.clone(), SAMPLE_RATE as f32)));
    SignalsUpdate::Set {
        machines: (0..kind.machines()).map(|_| made()).collect(),
        values: Box::default(),
        watches: Vec::new(),
    }
}

#[test]
fn new_code_fades_in_without_a_jump() {
    let mut played = play(Kind::Source, compile(|| 0.5), Vec::new());
    played.render(640);
    let update = new_code(Kind::Source, compile(|| -0.5));
    played.control.update(played.signals, update).unwrap();
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
    let code = compile(|| param("tone", 0.25, [0.0, 1.0]));
    let mut values = Values::default();
    values.parameters[0] = 0.25;
    values.automated = vec![0];
    let machine = Box::new(Machine::new(code, SAMPLE_RATE as f32));
    let signals = Signals::new(Kind::Source, machine, values, Vec::new());
    let (mut control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    let signals = edit.add_processor("signals", signals).unwrap();
    let lane = edit
        .add_processor(
            "lane",
            Lane {
                value: 1.0,
                blocks: 100,
            },
        )
        .unwrap();
    edit.connect(Connection::to_device(signals.id(), Signals::OUTPUT, 0))
        .unwrap();
    edit.connect(Connection::new(
        lane.id(),
        LANE_OUT,
        signals.id(),
        Signals::AUTOMATION,
    ))
    .unwrap();
    edit.commit().unwrap();
    let mut played = Played {
        control,
        engine,
        signals,
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
    let mut played = play(kind, compile(|| 0.5 * gate()), notes);
    // Released at frame 64 and silent since, so the voice is idle when the code changes.
    played.render(4_800);
    played
        .control
        .update(played.signals, new_code(kind, compile(|| -0.5 * gate())))
        .unwrap();
    let output = played.render(9_600);
    // The note at frame 9600 starts with the new code, not with a fade from the old.
    assert_eq!(output[4_800], -0.5);
}

/// Every param at its default, and the first one moved by a lane.
fn defaults(code: &Code) -> Values {
    let mut values = Values::default();
    for (value, spec) in values.parameters.iter_mut().zip(&code.parameters) {
        *value = spec.default;
    }
    values.automated = vec![0];
    values
}

fn score_tool(kind: Kind, code: Code, other: Code) -> score::Tool {
    score::Tool {
        kind,
        values: defaults(&code),
        code,
        other,
    }
}

fn scored_instrument(bright: bool) -> Code {
    compile(|| {
        let cutoff = param("cutoff", 2000.0, [200.0, 8000.0]);
        let level = param("level", 0.5, [0.0, 1.0]);
        let x = live("x", 0.2, [0.0, 1.0]);
        let hit = trigger("hit");
        let level_of_note = adsr(gate(), 3.0, 30.0, 0.6, 40.0);
        let count = feedback();
        count.set(count.read() + onset() + hit);
        watch("count", count.read());
        watch("level", level_of_note);
        let mut tone = sin(phasor(freq()) * TAU) + 0.3 * phasor(pitch() * 2.0);
        if bright {
            tone = saturate(tone * 3.0);
        }
        0.2 * level * velocity() * level_of_note * lowpass(tone, cutoff, 0.7)
            + 0.01 * x * hold(noise(), hit)
    })
}

fn scored_source(low: f32) -> Code {
    compile(|| {
        let rate = param("rate", 3.0, [0.5, 10.0]);
        let x = live("x", 0.5, [0.0, 1.0]);
        let hits = trigger("hit") + trigger("other");
        let tick = rise(wrap(beat()).lt(0.5));
        let level = adsr(gate(), 2.0, 20.0, 0.5, 30.0);
        watch("beat", beat());
        let tone = sin(phasor(mix(low, freq(), gate())) * TAU) * (0.5 + 0.5 * level);
        0.2 * tone * (1.0 + x) + 0.3 * (tick + hits + onset()) + 0.01 * sin(phasor(rate) * TAU)
    })
}

fn scored_effect(stereo: bool) -> Code {
    if stereo {
        return compile(|| {
            let width = param("width", 0.5, [0.0, 1.0]);
            (input_right() * width, delay(input_left(), 3.0))
        });
    }
    compile(|| {
        let time = param("time", 120.0, [10.0, 400.0]);
        let again = param("again", 0.4, [0.0, 0.9]);
        let echo = feedback();
        let wet = delay(input() + echo.read() * again, time);
        echo.set(lowpass(wet, 3000.0, 0.7));
        mix(input(), wet, live("wet", 0.3, [0.0, 1.0]))
    })
}

/// A tape: a feedback through a filter and a buffer, read before it is set, and a lookup that
/// plays the tape back.
fn scored_tape(behind: f32) -> Code {
    compile(|| {
        let again = param("again", 0.6, [0.0, 0.9]);
        let tape = buffer(0.05);
        let echo = feedback();
        let back = echo.read();
        let head = phasor(20.0) * 2400.0;
        tape.write(head, input() + back * again);
        let old = tape.at(head - behind);
        echo.set(lowpass(old, 2500.0, 0.7));
        mix(input(), old + 0.2 * lookup(&tape, phasor(3.0)), 0.5)
    })
}

/// The hashes are of renders on macOS on Apple silicon: `sin` and `tan` are of the platform,
/// and may differ in their last bit elsewhere.
#[test]
#[cfg_attr(
    not(all(target_os = "macos", target_arch = "aarch64")),
    ignore = "hashes of macOS on Apple silicon"
)]
fn a_score_of_every_timing_renders_as_it_did() {
    let tools = [
        score_tool(
            Kind::Instrument { voices: 4 },
            scored_instrument(false),
            scored_instrument(true),
        ),
        score_tool(Kind::Source, scored_source(110.0), scored_source(220.0)),
        score_tool(Kind::Effect, scored_effect(false), scored_effect(true)),
        score_tool(Kind::Effect, scored_tape(600.0), scored_tape(1_000.0)),
    ];
    let mut found = Vec::new();
    for tool in &tools {
        for buffer in [37, 512] {
            let (output, shown) = score::render(tool, buffer);
            found.push((score::hash(&output), score::hash(&shown)));
        }
    }
    let expected = [
        (5776360306822237945, 6672061644954794190),
        (9782693442129799630, 6622498506930331513),
        (2862924557966494273, 1517712285991712962),
        (1203857699514163717, 7353583470673154908),
        (8521250857642533575, 14695981039346656037),
        (12097547745870752178, 14695981039346656037),
        (12019199482111183270, 14695981039346656037),
        (1782942339727100491, 14695981039346656037),
    ];
    assert_eq!(found, expected);
}
