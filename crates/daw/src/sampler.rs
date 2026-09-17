use crate::sample_asset::{SampleAsset, validate_asset_reference};
use serde::{Deserialize, Serialize};
use sound_core::{
    Error, Result,
    audio::{AudioBuffer, Event, EventData, Prepare, ProcessContext, Processor},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SamplerConfig {
    pub asset: String,
    #[serde(default = "default_root_key")]
    pub root_key: u8,
}

fn default_root_key() -> u8 {
    60
}

impl SamplerConfig {
    pub fn validate(&self) -> Result<()> {
        validate_asset_reference(&self.asset)?;
        if self.root_key > 127 {
            return Err(Error("Sampler root key must be 0..127".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    key: u8,
    velocity: f32,
    position: f64,
    step: f64,
}

pub struct Sampler {
    sample: SampleAsset,
    root_key: u8,
    rate_ratio: f64,
    voices: [Voice; 32],
}

impl Sampler {
    pub fn new(sample: SampleAsset, root_key: u8) -> Result<Self> {
        if root_key > 127 {
            return Err(Error("Sampler root key must be 0..127".into()));
        }
        Ok(Self {
            rate_ratio: f64::from(sample.sample_rate()) / 48_000.0,
            sample,
            root_key,
            voices: [Voice::default(); 32],
        })
    }

    fn note_off(&mut self, key: u8) {
        for voice in &mut self.voices {
            if voice.key == key {
                voice.active = false;
            }
        }
    }

    fn event(&mut self, event: &Event) {
        let EventData::Custom { kind, data } = event.data else {
            return;
        };
        if kind == 3 {
            self.reset();
            return;
        }
        if !(0.0..=127.0).contains(&data[0]) || data[0].fract() != 0.0 {
            return;
        }
        let key = data[0] as u8;
        match kind {
            1 if (0.0..=1.0).contains(&data[1]) && data[2].is_finite() && data[2] >= 0.0 => {
                if data[1] == 0.0 {
                    self.note_off(key);
                    return;
                }
                let slot = self
                    .voices
                    .iter()
                    .position(|voice| voice.active && voice.key == key)
                    .or_else(|| self.voices.iter().position(|voice| !voice.active));
                if let Some(slot) = slot {
                    let step = self.rate_ratio
                        * ((f64::from(key) - f64::from(self.root_key)) / 12.0).exp2();
                    self.voices[slot] = Voice {
                        active: true,
                        key,
                        velocity: data[1],
                        position: f64::from(data[2]) * step,
                        step,
                    };
                }
            }
            2 => self.note_off(key),
            _ => {}
        }
    }
}

impl Processor for Sampler {
    fn input_channels(&self) -> usize {
        0
    }

    fn output_channels(&self) -> usize {
        2
    }

    fn prepare(&mut self, settings: Prepare) -> Result<()> {
        settings.validate()?;
        self.rate_ratio = f64::from(self.sample.sample_rate()) / settings.sample_rate;
        self.reset();
        Ok(())
    }

    fn process(
        &mut self,
        _: ProcessContext,
        _: &AudioBuffer,
        output: &mut AudioBuffer,
        events: &[Event],
    ) {
        let mut event_index = 0;
        for frame in 0..output.frames() {
            while event_index < events.len() && events[event_index].frame <= frame {
                self.event(&events[event_index]);
                event_index += 1;
            }
            let mut mixed = [0.0; 2];
            let samples = self.sample.frames();
            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }
                let index = voice.position as usize;
                if index >= samples.len() {
                    voice.active = false;
                    continue;
                }
                let fraction = (voice.position - index as f64) as f32;
                let next = samples.get(index + 1).copied().unwrap_or([0.0; 2]);
                for channel in 0..2 {
                    mixed[channel] += (samples[index][channel]
                        + (next[channel] - samples[index][channel]) * fraction)
                        * voice.velocity;
                }
                voice.position += voice.step;
            }
            output.channel_mut(0)[frame] = mixed[0];
            output.channel_mut(1)[frame] = mixed[1];
        }
    }

    fn reset(&mut self) {
        self.voices.fill(Voice::default());
    }
}

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;
