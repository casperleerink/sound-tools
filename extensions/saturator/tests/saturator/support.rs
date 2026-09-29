//! A rig of one source into one saturator into the device, and what the tests measure with.

use std::f64::consts::TAU;

use saturator::{Saturator, SaturatorState};
use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, Node, Ports, PrepareConfig,
    ProcessContext, Processor,
};

pub const SAMPLE_RATE: u32 = 48_000;

/// Makes the next frame of a signal, left and right. Called on the audio thread.
pub type Signal = Box<dyn FnMut() -> [f32; 2] + Send>;

/// A processor that plays a signal. The closure is made on the control thread; calling it
/// allocates nothing.
pub struct Source {
    signal: Signal,
}

impl Source {
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub fn new(signal: Signal) -> Self {
        Self { signal }
    }
}

impl Processor for Source {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            [*left, *right] = (self.signal)();
        }
    }
}

/// A sine of this frequency and amplitude in both channels, from phase 0.
pub fn sine(hz: f64, amplitude: f32) -> Signal {
    sine_at(hz, amplitude, SAMPLE_RATE)
}

/// The same at another sample rate.
pub fn sine_at(hz: f64, amplitude: f32, sample_rate: u32) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(sample_rate);
    Box::new(move || {
        let sample = (TAU * phase).sin() as f32 * amplitude;
        phase = (phase + step).fract();
        [sample; 2]
    })
}

/// White noise from -amplitude to amplitude, the same every run, other in each channel.
pub fn noise(amplitude: f32) -> Signal {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    Box::new(move || [next() * amplitude, next() * amplitude])
}

/// A source, a saturator and the device, rendering offline.
pub struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub saturator: Node<Saturator>,
}

impl Rig {
    pub fn new(state: SaturatorState, signal: Signal) -> Self {
        Self::at_rate(state, signal, SAMPLE_RATE)
    }

    pub fn at_rate(state: SaturatorState, signal: Signal, sample_rate: u32) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(sample_rate, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let saturator = edit
            .add_processor("saturator", Saturator::new(state))
            .unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            saturator.id(),
            Saturator::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(saturator.id(), Saturator::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            saturator,
        }
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    /// Left and right.
    pub fn render(&mut self, frames: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(480 * 2) {
            self.engine.process_block(buffer);
        }
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    pub fn update(&mut self, state: SaturatorState) {
        self.control.update(self.saturator, state).unwrap();
    }
}

/// The amplitude of the part of `samples` at `hz`, by correlation with a sine and a cosine of
/// that frequency. `samples` start at frame `start` of a signal whose phase was 0 at frame 0.
pub fn amplitude_at(samples: &[f32], start: usize, hz: f64, sample_rate: u32) -> f64 {
    let (mut sine, mut cosine) = (0.0, 0.0);
    for (offset, sample) in samples.iter().enumerate() {
        let angle = TAU * hz * (start + offset) as f64 / f64::from(sample_rate);
        sine += f64::from(*sample) * angle.sin();
        cosine += f64::from(*sample) * angle.cos();
    }
    2.0 * sine.hypot(cosine) / samples.len() as f64
}

/// The gain of a saturator with this record at `hz`, measured: a quiet sine through it, once it
/// has settled, in dB.
pub fn measured_db_at(state: SaturatorState, hz: f64, sample_rate: u32) -> f64 {
    const AMPLITUDE: f32 = 0.001;
    // Long enough for the DC blocker to settle, then whole cycles over about a quarter of a
    // second, so the correlation sees no part of a cycle.
    let settle = sample_rate as usize;
    let cycles = (hz * 0.25).ceil();
    let window = (cycles * f64::from(sample_rate) / hz).round() as usize;
    let mut rig = Rig::at_rate(state, sine_at(hz, AMPLITUDE, sample_rate), sample_rate);
    rig.render(settle);
    let [left, right] = rig.render(window);
    assert_eq!(left, right);
    let amplitude = amplitude_at(&left, settle, hz, sample_rate);
    20.0 * (amplitude / f64::from(AMPLITUDE)).log10()
}

/// The sound of a steady tone at `bin` times `sample_rate / window`, split into what is in tune
/// with it and what is not: the amplitude of each harmonic under 20 kHz, the fundamental first,
/// and the level of everything else under 20 kHz but DC against the fundamental, in dB. What is
/// not in tune is what folded back from above the Nyquist frequency. `samples` is one window
/// of the settled sound, starting at a whole number of cycles.
pub struct Spectrum {
    pub harmonics: Vec<f64>,
    pub out_of_tune_db: f64,
}

impl Spectrum {
    pub fn of(samples: &[f32], bin: usize, sample_rate: u32) -> Self {
        let window = samples.len();
        let bin_hz = f64::from(sample_rate) / window as f64;
        let top = (20_000.0 / bin_hz) as usize;
        let (mut harmonics, mut out_of_tune) = (Vec::new(), 0.0);
        for each in 1..=top {
            let amplitude = amplitude_at(samples, 0, each as f64 * bin_hz, sample_rate);
            match each % bin == 0 {
                true => harmonics.push(amplitude),
                false => out_of_tune += amplitude * amplitude,
            }
        }
        let out_of_tune_db = 10.0 * (out_of_tune / (harmonics[0] * harmonics[0])).log10();
        Self {
            harmonics,
            out_of_tune_db,
        }
    }

    /// A harmonic against the fundamental, in dB: 2 is the second.
    pub fn harmonic_db(&self, number: usize) -> f64 {
        20.0 * (self.harmonics[number - 1] / self.harmonics[0]).log10()
    }
}

pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// The largest step from one sample to the next.
pub fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}
