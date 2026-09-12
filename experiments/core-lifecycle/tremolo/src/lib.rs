use serde::{Deserialize, Serialize};
use sound_core::{Error, Processor, Registry, Result, Tool};

mod view;
pub use view::TremoloEditor;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TremoloState {
    pub frequency_hz: f32,
    pub gain: f32,
    pub rate_hz: f32,
    pub depth: f32,
}

impl Default for TremoloState {
    fn default() -> Self {
        Self {
            frequency_hz: 220.0,
            gain: 0.2,
            rate_hz: 4.0,
            depth: 0.75,
        }
    }
}

fn validate(state: &TremoloState) -> Result<()> {
    if !(20.0..=20_000.0).contains(&state.frequency_hz)
        || !(0.0..=1.0).contains(&state.gain)
        || !(0.1..=20.0).contains(&state.rate_hz)
        || !(0.0..=1.0).contains(&state.depth)
    {
        return Err(Error(
            "Tremolo needs frequency 20..20000 Hz, gain/depth 0..1, and rate 0.1..20 Hz".into(),
        ));
    }
    Ok(())
}

pub fn register(registry: &mut Registry) -> Tool<TremoloState> {
    let tool = registry.register("example.tremolo", validate);
    registry.processor(tool, "audio", |state| {
        Box::new(Voice {
            state: state.clone(),
            phase: 0.0,
            modulation_phase: 0.0,
        })
    });
    tool
}

pub fn register_view(views: &mut sound_ui::Views, tool: Tool<TremoloState>) {
    views.register(tool, TremoloEditor::new);
}

struct Voice {
    state: TremoloState,
    phase: f32,
    modulation_phase: f32,
}

impl Processor<TremoloState> for Voice {
    fn apply(&mut self, state: &TremoloState) {
        self.state = state.clone();
    }

    fn render(&mut self, output: &mut [f32], sample_rate: f32) {
        for sample in output {
            let modulation = (self.modulation_phase * std::f32::consts::TAU).sin();
            let envelope = 1.0 - self.state.depth * (1.0 - modulation) * 0.5;
            *sample = (self.phase * std::f32::consts::TAU).sin() * self.state.gain * envelope;
            self.phase = (self.phase + self.state.frequency_hz / sample_rate).fract();
            self.modulation_phase =
                (self.modulation_phase + self.state.rate_hz / sample_rate).fract();
        }
    }
}
