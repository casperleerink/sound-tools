//! A rig of one source into one filter into the device, and what the tests measure with.

use filter::{Filter, FilterState};
use sound_core::{Connection, Engine, EngineConfig, EngineControl, Node};
pub(crate) use sound_dsp::testing::{
    Signal, Source, amplitude_at, largest_step, noise, peak, sine_at,
};

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// A sine of this frequency and amplitude in both channels, from phase 0.
pub(crate) fn sine(hz: f64, amplitude: f32) -> Signal {
    sine_at(hz, amplitude, SAMPLE_RATE)
}

/// A source, a filter and the device, rendering offline.
pub(crate) struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub filter: Node<Filter>,
}

impl Rig {
    pub(crate) fn new(state: FilterState, signal: Signal) -> Self {
        Self::at_rate(state, signal, SAMPLE_RATE)
    }

    pub(crate) fn at_rate(state: FilterState, signal: Signal, sample_rate: u32) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(sample_rate, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let filter = edit.add_processor("filter", Filter::new(state)).unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            filter.id(),
            Filter::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(filter.id(), Filter::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            filter,
        }
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    /// Left and right.
    pub(crate) fn render(&mut self, frames: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(480 * 2) {
            self.engine.process_block(buffer);
        }
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    pub(crate) fn update(&mut self, state: FilterState) {
        self.control.update(self.filter, state).unwrap();
    }
}

/// The gain of a filter with this record at `hz`, measured: a quiet sine through it, once the
/// filter has settled, in dB.
pub(crate) fn measured_db(state: FilterState, hz: f64) -> f64 {
    measured_db_at(state, hz, SAMPLE_RATE)
}

/// The same at another sample rate.
pub(crate) fn measured_db_at(state: FilterState, hz: f64, sample_rate: u32) -> f64 {
    const AMPLITUDE: f32 = 0.01;
    // Long enough for the slowest ring of these tests to die away, then whole cycles over about
    // a quarter of a second, so the correlation sees no part of a cycle.
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
