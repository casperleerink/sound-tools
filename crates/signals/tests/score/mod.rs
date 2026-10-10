//! A score that plays everything whose timing a `Signals` processor decides, at a host buffer
//! size: notes that overlap, more notes than voices, a voice that goes idle and plays again, a
//! lane, a turn of a knob, a live control, triggers and notes of the interface at a frame, new
//! code that fades in while it plays, and the transport that stops and plays again.

use sound_core::{
    AudioOutput, Automation, Connection, Engine, EngineConfig, EventOutput, Ports, PrepareConfig,
    ProcessContext, Processor, Watch,
};
use sound_notes::{Bend, NoteEvent, Pitch, Velocity};
use sound_signals::{Code, Kind, Machine, Signals, SignalsUpdate, Values};

const SAMPLE_RATE: u32 = 48_000;
pub(crate) const FRAMES: usize = 72_000;

pub(crate) struct Tool {
    pub kind: Kind,
    pub code: Code,
    pub values: Values,
    /// The code of another choice, which comes in while the score plays.
    pub other: Code,
}

/// Every sample of a render of `tool` at host buffers of `buffer` frames, and the value of each
/// watch after each buffer.
pub(crate) fn render(tool: &Tool, buffer: usize) -> (Vec<f32>, Vec<f32>) {
    let mut watches: Vec<Watch> = tool.code.watches.iter().map(|_| Watch::new()).collect();
    let machine = Box::new(Machine::new(tool.code.clone(), SAMPLE_RATE as f32));
    let signals = Signals::new(tool.kind, machine, tool.values.clone(), watches.clone());
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    let signals = edit.add_processor("signals", signals).unwrap();
    let player = edit.add_processor("player", Player::new(tool)).unwrap();
    edit.connect(Connection::to_device(signals.id(), Signals::OUTPUT, 0))
        .unwrap();
    let id = (player.id(), signals.id());
    edit.connect(Connection::new(id.0, NOTES, id.1, Signals::NOTES))
        .unwrap();
    edit.connect(Connection::new(id.0, LANE, id.1, Signals::AUTOMATION))
        .unwrap();
    edit.connect(Connection::new(id.0, SOUND, id.1, Signals::INPUT))
        .unwrap();
    edit.commit().unwrap();
    control.play();

    let pitch = |number| Pitch::new(number).unwrap();
    let velocity = Velocity::new(90).unwrap();
    let machines = |code: &Code| {
        let made = || Some(Box::new(Machine::new(code.clone(), SAMPLE_RATE as f32)));
        (0..tool.kind.machines()).map(|_| made()).collect()
    };
    let mut turned = tool.values.clone();
    turned.arrays = None;
    for (value, spec) in turned.parameters.iter_mut().zip(&tool.code.parameters) {
        *value = spec.min + (spec.max - spec.min) * 0.3;
    }
    let mut steps: Vec<(usize, SignalsUpdate)> = vec![
        (
            5_000,
            SignalsUpdate::Note {
                pitch: pitch(72),
                velocity,
                at: Some(5_500),
                frames: Some(2_000),
            },
        ),
        (7_000, SignalsUpdate::Trigger { index: 0, at: None }),
        (
            7_000,
            SignalsUpdate::Trigger {
                index: 0,
                at: Some(7_300),
            },
        ),
        (
            7_000,
            SignalsUpdate::Trigger {
                index: 1,
                at: Some(10),
            },
        ),
        (
            8_000,
            SignalsUpdate::Live {
                index: 0,
                value: 0.9,
            },
        ),
        (
            8_000,
            SignalsUpdate::Live {
                index: 1,
                value: 0.1,
            },
        ),
        (
            11_000,
            SignalsUpdate::Note {
                pitch: pitch(74),
                velocity,
                at: None,
                frames: None,
            },
        ),
        (
            11_000,
            SignalsUpdate::Release {
                pitch: pitch(74),
                at: Some(13_000),
            },
        ),
        (
            28_000,
            SignalsUpdate::Set {
                machines: Vec::new(),
                values: Box::new(turned),
                watches: Vec::new(),
            },
        ),
        (
            62_000,
            SignalsUpdate::Trigger {
                index: 0,
                at: Some(62_777),
            },
        ),
    ];
    steps.reverse();
    let mut news = vec![(36_000, &tool.other), (36_200, &tool.code)];
    news.reverse();

    let mut output = Vec::with_capacity(FRAMES * 2);
    let mut shown = Vec::new();
    let mut block = vec![0.0; buffer * 2];
    let mut now = 0;
    let mut stopped = false;
    while now < FRAMES {
        while steps.last().is_some_and(|(at, _)| *at <= now) {
            let (_, update) = steps.pop().unwrap();
            control.update(signals, update).unwrap();
        }
        while news.last().is_some_and(|(at, _)| *at <= now) {
            let (_, code) = news.pop().unwrap();
            watches = code.watches.iter().map(|_| Watch::new()).collect();
            let update = SignalsUpdate::Set {
                machines: machines(code),
                values: Box::new(tool.values.clone()),
                watches: watches.clone(),
            };
            control.update(signals, update).unwrap();
        }
        if !stopped && now >= 45_000 && now < 52_000 {
            control.stop();
            stopped = true;
        } else if stopped && now >= 52_000 {
            control.play();
            stopped = false;
        }
        engine.process_block(&mut block);
        output.extend_from_slice(&block);
        shown.extend(watches.iter().map(Watch::get));
        now += buffer;
    }
    let status = control.poll().unwrap();
    assert_eq!((status.event_overflows, status.port_misuses), (0, 0));
    (output, shown)
}

/// FNV-1a over the bits of every sample, so a test can keep a render in one number.
pub(crate) fn hash(samples: &[f32]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in samples
        .iter()
        .flat_map(|sample| sample.to_bits().to_le_bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);
const LANE: EventOutput<Automation> = EventOutput::new(1);
const SOUND: AudioOutput = AudioOutput::new(0);

/// Plays the notes of the score, a lane over the knob that is automated first, and a sound into
/// an effect.
struct Player {
    notes: Vec<(usize, NoteEvent)>,
    /// The range of the param the first lane moves.
    lane: Option<(f32, f32)>,
    frame: usize,
    noise: u32,
}

impl Player {
    fn new(tool: &Tool) -> Self {
        let on = |number| NoteEvent::On {
            pitch: Pitch::new(number).unwrap(),
            velocity: Velocity::new(100).unwrap(),
        };
        let off = |number| NoteEvent::Off {
            pitch: Pitch::new(number).unwrap(),
        };
        let mut notes = vec![
            (1_001, on(60)),
            (1_001, on(64)),
            (1_517, on(67)),
            (2_950, off(60)),
            (6_001, NoteEvent::Bend(Bend::new(3000).unwrap())),
            (30_000, on(62)),
            (33_000, off(62)),
            (33_000, on(65)),
            (40_123, off(65)),
            (55_000, on(60)),
            (60_000, off(60)),
        ];
        // More notes than voices, so a held one is taken for a new one.
        for step in 0..10 {
            notes.push((4_000 + 13 * step, on(70 + step as u8)));
            notes.push((9_000 + 3 * step, off(70 + step as u8)));
        }
        notes.push((9_100, off(64)));
        notes.push((9_100, off(67)));
        notes.sort_by_key(|(frame, _)| *frame);
        let lane = (tool.values.automated.first())
            .and_then(|param| tool.code.parameters.get(usize::from(*param)))
            .map(|spec| (spec.min, spec.max));
        Self {
            notes,
            lane,
            frame: 0,
            noise: 0x1234_5678,
        }
    }
}

impl Processor for Player {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new()
            .event_output(NOTES)
            .event_output(LANE)
            .audio_output(SOUND)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, (): &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let block = self.frame..self.frame + context.frames;
        for (frame, event) in &self.notes {
            if block.contains(frame) {
                context
                    .event_outputs
                    .push(NOTES, frame - self.frame, *event);
            }
        }
        // Two values per block, so the processor shows which it takes.
        if let Some((min, max)) = self.lane
            && (20_000..26_000).contains(&self.frame)
        {
            let along = (self.frame - 20_000) as f32 / 6_000.0;
            let at = |fraction: f32| Automation {
                parameter: 0,
                value: min + (max - min) * fraction,
            };
            context.event_outputs.push(LANE, 0, at(along * 0.5));
            let last = context.frames - 1;
            context
                .event_outputs
                .push(LANE, last, at(1.0 - along * 0.5));
        }
        let [left, right] = context.audio_outputs.get(SOUND);
        for (index, (left, right)) in left.iter_mut().zip(right).enumerate() {
            let frame = self.frame + index;
            *left = ((frame % 200) as f32 / 100.0 - 1.0) * 0.5;
            self.noise ^= self.noise << 13;
            self.noise ^= self.noise >> 17;
            self.noise ^= self.noise << 5;
            *right = (self.noise >> 8) as f32 / (1 << 24) as f32 * 0.5 - 0.25;
        }
        self.frame = block.end;
    }
}
