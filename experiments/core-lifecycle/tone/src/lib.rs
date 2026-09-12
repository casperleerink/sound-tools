use serde::{Deserialize, Serialize};
use sound_core::{Error, Processor, Registry, Result, Tool};
mod view;
pub use view::ToneEditor;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ToneState {
    pub frequency_hz: f32,
    pub gain: f32,
}
impl Default for ToneState {
    fn default() -> Self {
        Self {
            frequency_hz: 220.0,
            gain: 0.2,
        }
    }
}
fn validate(state: &ToneState) -> Result<()> {
    if !(20.0..=20_000.0).contains(&state.frequency_hz) || !(0.0..=1.0).contains(&state.gain) {
        return Err(Error(
            "Tone needs frequency 20..20000 Hz and gain 0..1".into(),
        ));
    }
    Ok(())
}
pub fn register(registry: &mut Registry) -> Tool<ToneState> {
    let tool = registry.register("example.tone", validate);
    registry.processor(tool, "audio", |state| {
        Box::new(Oscillator {
            state: state.clone(),
            phase: 0.0,
        })
    });
    tool
}
struct Oscillator {
    state: ToneState,
    phase: f32,
}
impl Processor<ToneState> for Oscillator {
    fn apply(&mut self, state: &ToneState) {
        self.state = state.clone();
    }
    fn render(&mut self, output: &mut [f32], sample_rate: f32) {
        for sample in output {
            *sample = (self.phase * std::f32::consts::TAU).sin() * self.state.gain;
            self.phase = (self.phase + self.state.frequency_hz / sample_rate).fract();
        }
    }
}

pub fn register_view(views: &mut sound_ui::Views, tool: Tool<ToneState>) {
    views.register(tool, ToneEditor::new);
}
