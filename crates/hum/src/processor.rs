//! The Hum processor: runs the [`Machine`] of the code that plays, and fades from the old
//! code to new code over a few milliseconds, so an edit of the code while it plays does not
//! click.

use sound_core::{AudioInput, AudioOutput, Ports, PrepareConfig, ProcessContext, Processor};

use crate::machine::{Machine, Values};

/// How long the old code fades out while the new one fades in.
const FADE_SECONDS: f32 = 0.01;

/// What the behaviour sends on every run: a machine when the code is new, and where every
/// param stands.
pub struct HumUpdate {
    pub machine: Option<Box<Machine>>,
    pub values: Values,
}

pub struct Hum {
    current: Box<Machine>,
    /// The machine of the code before, while it fades out. It goes back to the control thread
    /// with the next update.
    fading: Option<Box<Machine>>,
    fade_frames: usize,
    fade_left: usize,
}

impl Hum {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub fn new(machine: Box<Machine>) -> Self {
        Self {
            current: machine,
            fading: None,
            fade_frames: 1,
            fade_left: 0,
        }
    }
}

impl Processor for Hum {
    type Update = HumUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.fade_frames = ((FADE_SECONDS * config.sample_rate as f32) as usize).max(1);
    }

    fn update(&mut self, update: &mut HumUpdate) {
        if let Some(machine) = update.machine.take() {
            let old = std::mem::replace(&mut self.current, machine);
            // The one that faded before rides back with this update, to be dropped there.
            update.machine = self.fading.replace(old);
            self.fade_left = self.fade_frames;
        }
        self.current.aim(&update.values);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        for frame in 0..context.frames {
            let input = [
                left_in.get(frame).copied().unwrap_or(0.0),
                right_in.get(frame).copied().unwrap_or(0.0),
            ];
            let mut output = self.current.frame(input);
            if self.fade_left > 0
                && let Some(fading) = &mut self.fading
            {
                let old = fading.frame(input);
                let new = 1.0 - self.fade_left as f32 / self.fade_frames as f32;
                for (sample, old) in output.iter_mut().zip(old) {
                    *sample = old + (*sample - old) * new;
                }
                self.fade_left -= 1;
            }
            if let Some(left) = left_out.get_mut(frame) {
                *left = output[0];
            }
            if let Some(right) = right_out.get_mut(frame) {
                *right = output[1];
            }
        }
    }
}
