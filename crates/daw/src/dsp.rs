use sound_core::{
    Error, Result,
    audio::{AudioBuffer, Event, EventData, Prepare, ProcessContext, Processor},
};

const MAX_VOICES: usize = 32;
const WAVE_SINE: f32 = 0.0;
const WAVE_SAW: f32 = 1.0;
const WAVE_SQUARE: f32 = 2.0;

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    key: u8,
    velocity: f32,
    phase: f32,
    frequency: f32,
    released: bool,
    level: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            key: 0,
            velocity: 0.0,
            phase: 0.0,
            frequency: 0.0,
            released: false,
            level: 0.0,
        }
    }
}

pub struct Synth {
    sample_rate: f32,
    gain: f32,
    attack: f32,
    release: f32,
    waveform: f32,
    voices: Vec<Voice>,
}

fn key_frequency(key: u8) -> f32 {
    440.0 * ((f32::from(key) - 69.0) / 12.0).exp2()
}

impl Synth {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            gain: 0.5,
            attack: 0.01,
            release: 0.2,
            waveform: WAVE_SINE,
            voices: vec![Voice::default(); MAX_VOICES],
        }
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        if let Some(voice) = self
            .voices
            .iter_mut()
            .find(|voice| voice.active && voice.key == key)
        {
            voice.velocity = velocity;
            voice.released = false;
            return;
        }
        if let Some(voice) = self.voices.iter_mut().find(|voice| !voice.active) {
            *voice = Voice {
                active: true,
                key,
                velocity,
                phase: 0.0,
                frequency: key_frequency(key),
                released: false,
                level: 0.0,
            };
        }
    }
    fn note_off(&mut self, key: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.key == key {
                voice.released = true;
            }
        }
    }
}

impl Default for Synth {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for Synth {
    fn input_channels(&self) -> usize {
        0
    }
    fn output_channels(&self) -> usize {
        2
    }
    fn prepare(&mut self, settings: Prepare) -> Result<()> {
        settings.validate()?;
        self.sample_rate = settings.sample_rate as f32;
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
        let waveform = self.waveform;
        for frame in 0..output.frames() {
            while event_index < events.len() && events[event_index].frame <= frame {
                if let EventData::Custom { kind, data } = events[event_index].data {
                    match kind {
                        1 => self.note_on(data[0] as u8, data[1].clamp(0.0, 1.0)),
                        2 => self.note_off(data[0] as u8),
                        3 => {
                            for voice in &mut self.voices {
                                voice.released = true;
                            }
                        }
                        _ => {}
                    }
                }
                event_index += 1;
            }
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }
                let target = 1.0f32
                    .min(self.attack * self.sample_rate * voice.velocity)
                    .min(1.0);
                voice.level = if voice.released {
                    voice.level - 1.0 / (self.release * self.sample_rate).max(1.0)
                } else {
                    voice.level
                        + (target - voice.level)
                            .min(1.0 / (self.attack * self.sample_rate).max(1.0))
                };
                if voice.level <= 0.0 && voice.released {
                    voice.active = false;
                    continue;
                }
                voice.phase = (voice.phase + voice.frequency / self.sample_rate).fract();
                let sample = match waveform {
                    WAVE_SAW => 2.0 * voice.phase - 1.0,
                    WAVE_SQUARE => {
                        if voice.phase < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    _ => (voice.phase * std::f32::consts::TAU).sin(),
                } * voice.level
                    * voice.velocity;
                left += sample;
                right += sample;
            }
            output.channel_mut(0)[frame] = left * self.gain;
            output.channel_mut(1)[frame] = right * self.gain;
        }
        for event in &events[event_index.min(events.len())..] {
            if let EventData::Custom { kind: 1, data } = event.data {
                self.note_on(data[0] as u8, data[1].clamp(0.0, 1.0));
            }
        }
    }
    fn reset(&mut self) {
        for voice in &mut self.voices {
            *voice = Voice::default();
        }
    }
}

pub struct Gain {
    gain: f32,
    pan: f32,
    target_gain: f32,
    target_pan: f32,
}

impl Gain {
    pub fn new(gain: f32, pan: f32) -> Self {
        Self {
            gain: gain.clamp(0.0, 2.0),
            pan: pan.clamp(-1.0, 1.0),
            target_gain: gain.clamp(0.0, 2.0),
            target_pan: pan.clamp(-1.0, 1.0),
        }
    }
}

impl Processor for Gain {
    fn input_channels(&self) -> usize {
        2
    }
    fn output_channels(&self) -> usize {
        2
    }
    fn prepare(&mut self, settings: Prepare) -> Result<()> {
        settings.validate()?;
        Ok(())
    }
    fn process(
        &mut self,
        _: ProcessContext,
        input: &AudioBuffer,
        output: &mut AudioBuffer,
        events: &[Event],
    ) {
        for event in events {
            if let EventData::Parameter { id, value } = event.data {
                match id {
                    0 => self.target_gain = value.clamp(0.0, 2.0),
                    1 => self.target_pan = value.clamp(-1.0, 1.0),
                    _ => {}
                }
            }
        }
        let step = 1.0 / output.frames().max(1) as f32;
        for frame in 0..output.frames() {
            self.gain += (self.target_gain - self.gain)
                .min(step.abs() * 2.0)
                .max(-step.abs() * 2.0);
            self.pan += (self.target_pan - self.pan)
                .min(step.abs())
                .max(-step.abs());
            let angle = (self.pan + 1.0) * std::f32::consts::FRAC_PI_4;
            output.channel_mut(0)[frame] = input.channel(0)[frame] * self.gain * angle.cos();
            output.channel_mut(1)[frame] = input.channel(1)[frame] * self.gain * angle.sin();
        }
    }
    fn reset(&mut self) {}
}

pub struct Filter {
    cutoff: f32,
    state: [f32; 2],
}

impl Filter {
    pub fn new(cutoff: f32) -> Self {
        Self {
            cutoff: cutoff.clamp(20.0, 20_000.0),
            state: [0.0; 2],
        }
    }
}

impl Processor for Filter {
    fn input_channels(&self) -> usize {
        2
    }
    fn output_channels(&self) -> usize {
        2
    }
    fn prepare(&mut self, settings: Prepare) -> Result<()> {
        settings.validate()?;
        Ok(())
    }
    fn process(
        &mut self,
        context: ProcessContext,
        input: &AudioBuffer,
        output: &mut AudioBuffer,
        events: &[Event],
    ) {
        let mut cutoff = self.cutoff;
        let mut event_index = 0;
        let alpha_base =
            1.0 - (-std::f64::consts::TAU * self.cutoff as f64 / context.sample_rate).exp();
        for frame in 0..output.frames() {
            while event_index < events.len() && events[event_index].frame <= frame {
                if let EventData::Parameter { id: 0, value } = events[event_index].data {
                    cutoff = value.clamp(20.0, 20_000.0);
                }
                event_index += 1;
            }
            let alpha = if cutoff == self.cutoff {
                alpha_base as f32
            } else {
                (1.0 - (-std::f64::consts::TAU * cutoff as f64 / context.sample_rate).exp()) as f32
            };
            for channel in 0..2 {
                self.state[channel] +=
                    alpha * (input.channel(channel)[frame] - self.state[channel]);
                output.channel_mut(channel)[frame] = self.state[channel];
            }
        }
        self.cutoff = cutoff;
    }
    fn reset(&mut self) {
        self.state = [0.0; 2];
    }
}

pub struct Delay {
    sample_rate: f32,
    seconds: f32,
    feedback: f32,
    mix: f32,
    buffer: Vec<f32>,
    write: usize,
}

impl Delay {
    pub fn new(seconds: f32, feedback: f32, mix: f32) -> Self {
        Self {
            sample_rate: 48_000.0,
            seconds: seconds.clamp(0.001, 4.0),
            feedback: feedback.clamp(0.0, 0.95),
            mix: mix.clamp(0.0, 1.0),
            buffer: Vec::new(),
            write: 0,
        }
    }
}

impl Processor for Delay {
    fn input_channels(&self) -> usize {
        2
    }
    fn output_channels(&self) -> usize {
        2
    }
    fn prepare(&mut self, settings: Prepare) -> Result<()> {
        settings.validate()?;
        self.sample_rate = settings.sample_rate as f32;
        let frames = (4.0 * self.sample_rate).ceil() as usize + 1;
        self.buffer = vec![0.0; frames * 2];
        self.write = 0;
        Ok(())
    }
    fn process(
        &mut self,
        _: ProcessContext,
        input: &AudioBuffer,
        output: &mut AudioBuffer,
        events: &[Event],
    ) {
        let capacity = self.buffer.len() / 2;
        let mut event_index = 0;
        for frame in 0..output.frames() {
            while event_index < events.len() && events[event_index].frame <= frame {
                if let EventData::Parameter { id, value } = events[event_index].data
                    && value.is_finite()
                {
                    match id {
                        0 => self.seconds = value.clamp(0.001, 4.0),
                        1 => self.feedback = value.clamp(0.0, 0.95),
                        2 => self.mix = value.clamp(0.0, 1.0),
                        _ => {}
                    }
                }
                event_index += 1;
            }
            let delay_frames =
                ((self.seconds * self.sample_rate).round() as usize).clamp(1, capacity - 1);
            let read = (self.write + capacity - delay_frames) % capacity;
            for channel in 0..2 {
                let offset = channel * capacity;
                let delayed = self.buffer[offset + read];
                let input_sample = input.channel(channel)[frame];
                self.buffer[offset + self.write] = input_sample + delayed * self.feedback;
                output.channel_mut(channel)[frame] =
                    input_sample * (1.0 - self.mix) + delayed * self.mix;
            }
            self.write = (self.write + 1) % capacity;
        }
    }
    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }
}

pub struct Reverb {
    mix: f32,
    combs: Vec<Vec<f32>>,
    comb_index: [usize; 8],
    allpass: Vec<Vec<f32>>,
    allpass_index: [usize; 4],
}

const COMB_LENGTHS: [f32; 8] = [
    0.0297, 0.0371, 0.0411, 0.0437, 0.0297, 0.0371, 0.0411, 0.0437,
];
const ALLPASS_LENGTHS: [f32; 4] = [0.005, 0.0017, 0.0039, 0.0011];

impl Reverb {
    pub fn new(mix: f32) -> Self {
        Self {
            mix: mix.clamp(0.0, 1.0),
            combs: Vec::new(),
            comb_index: [0; 8],
            allpass: Vec::new(),
            allpass_index: [0; 4],
        }
    }
}

impl Processor for Reverb {
    fn input_channels(&self) -> usize {
        2
    }
    fn output_channels(&self) -> usize {
        2
    }
    fn prepare(&mut self, settings: Prepare) -> Result<()> {
        settings.validate()?;
        let rate = settings.sample_rate as f32;
        self.combs = COMB_LENGTHS
            .iter()
            .map(|length| vec![0.0; (length * rate) as usize + 1])
            .collect();
        self.allpass = ALLPASS_LENGTHS
            .iter()
            .map(|length| vec![0.0; (length * rate) as usize + 1])
            .collect();
        self.comb_index = [0; 8];
        self.allpass_index = [0; 4];
        Ok(())
    }
    fn process(
        &mut self,
        _: ProcessContext,
        input: &AudioBuffer,
        output: &mut AudioBuffer,
        events: &[Event],
    ) {
        let mut event_index = 0;
        for frame in 0..output.frames() {
            while event_index < events.len() && events[event_index].frame <= frame {
                if let EventData::Parameter { id: 0, value } = events[event_index].data
                    && value.is_finite()
                {
                    self.mix = value.clamp(0.0, 1.0);
                }
                event_index += 1;
            }
            for channel in 0..2 {
                let input_sample = input.channel(channel)[frame];
                let mut wet = 0.0f32;
                for slot in channel * 4..channel * 4 + 4 {
                    let comb = &mut self.combs[slot];
                    let index = self.comb_index[slot];
                    let delayed = comb[index];
                    comb[index] = input_sample * 0.25 + delayed * 0.84;
                    self.comb_index[slot] = (index + 1) % comb.len();
                    wet += delayed;
                }
                wet *= 0.25;
                for slot in channel * 2..channel * 2 + 2 {
                    let buffer = &mut self.allpass[slot];
                    let index = self.allpass_index[slot];
                    let delayed = buffer[index];
                    let next = delayed - wet * 0.5;
                    buffer[index] = wet + next * 0.5;
                    self.allpass_index[slot] = (index + 1) % buffer.len();
                    wet = next;
                }
                output.channel_mut(channel)[frame] =
                    input_sample * (1.0 - self.mix) + wet * self.mix;
            }
        }
    }
    fn reset(&mut self) {
        for comb in &mut self.combs {
            comb.fill(0.0);
        }
        for buffer in &mut self.allpass {
            buffer.fill(0.0);
        }
        self.comb_index = [0; 8];
        self.allpass_index = [0; 4];
    }
}

pub fn validate_gain(gain: f32) -> Result<()> {
    if (0.0..=2.0).contains(&gain) {
        Ok(())
    } else {
        Err(Error("Gain must be 0..2".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Prepare {
        Prepare {
            sample_rate: 48_000.0,
            max_frames: 128,
        }
    }

    fn run(processor: &mut impl Processor, events: &[Event]) -> Vec<f32> {
        processor.prepare(settings()).unwrap();
        let mut input = AudioBuffer::new(processor.input_channels(), 128);
        let mut output = AudioBuffer::new(processor.output_channels(), 128);
        let context = ProcessContext {
            sample_rate: 48_000.0,
            engine_frame: 0,
            project_frame: 0,
            playing: true,
        };
        input.set_frames(128).unwrap();
        output.set_frames(128).unwrap();
        processor.process(context, &input, &mut output, events);
        output.channel(0).to_vec()
    }

    fn note_on(frame: usize, key: u8) -> Event {
        Event {
            frame,
            data: EventData::Custom {
                kind: 1,
                data: [f32::from(key), 0.8, 0.0, 0.0],
            },
        }
    }

    fn impulse_response(processor: &mut impl Processor, block: usize) -> Vec<[f32; 2]> {
        processor.prepare(settings()).unwrap();
        let mut input = AudioBuffer::new(2, 128);
        let mut output = AudioBuffer::new(2, 128);
        let mut response = Vec::new();
        while response.len() < 8192 {
            let frames = block.min(8192 - response.len());
            input.set_frames(frames).unwrap();
            output.set_frames(frames).unwrap();
            input.clear();
            output.clear();
            if response.is_empty() {
                input.channel_mut(0)[0] = 1.0;
            }
            processor.process(
                ProcessContext {
                    sample_rate: 48_000.0,
                    engine_frame: response.len() as u64,
                    project_frame: response.len() as u64,
                    playing: true,
                },
                &input,
                &mut output,
                &[],
            );
            response.extend(
                (0..frames).map(|frame| [output.channel(0)[frame], output.channel(1)[frame]]),
            );
        }
        response
    }

    #[test]
    fn delay_impulse_is_stereo_isolated_and_block_independent() {
        let response = impulse_response(&mut Delay::new(0.01, 0.5, 1.0), 128);
        let single = impulse_response(&mut Delay::new(0.01, 0.5, 1.0), 1);
        assert_eq!(response, single);
        assert_eq!(response[480], [1.0, 0.0]);
        assert_eq!(response[960], [0.5, 0.0]);
        assert!(response[..480].iter().all(|sample| *sample == [0.0; 2]));
        assert!(response.iter().all(|sample| sample[1] == 0.0));
    }

    #[test]
    fn reverb_impulse_is_block_independent_and_has_a_tail() {
        let response = impulse_response(&mut Reverb::new(1.0), 128);
        let single = impulse_response(&mut Reverb::new(1.0), 1);
        assert_eq!(response, single);
        assert!(
            response[1500..]
                .iter()
                .any(|sample| sample[0].abs() > 0.001)
        );
        assert!(
            response
                .iter()
                .flatten()
                .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
        );
    }

    #[test]
    fn synth_plays_notes_at_sample_offsets_and_resets() {
        let mut synth = Synth::new();
        let output = run(&mut synth, &[note_on(32, 60)]);
        assert!(output[..32].iter().all(|sample| sample.abs() < 1e-6));
        assert!(output[33..].iter().any(|sample| sample.abs() > 0.01));
        assert!(
            output
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 2.0)
        );
        synth.reset();
        let output = run(&mut synth, &[]);
        assert!(output.iter().all(|sample| sample.abs() < 1e-6));
    }

    #[test]
    fn gain_applies_and_smoothes_parameter_changes() {
        let mut gain = Gain::new(0.0, 0.0);
        let mut input = AudioBuffer::new(2, 128);
        input.set_frames(4).unwrap();
        for frame in 0..4 {
            input.channel_mut(0)[frame] = 0.5;
            input.channel_mut(1)[frame] = 0.5;
        }
        let mut output = AudioBuffer::new(2, 128);
        output.set_frames(4).unwrap();
        let events = [Event {
            frame: 0,
            data: EventData::Parameter { id: 0, value: 1.0 },
        }];
        let context = ProcessContext {
            sample_rate: 48_000.0,
            engine_frame: 0,
            project_frame: 0,
            playing: true,
        };
        gain.prepare(settings()).unwrap();
        gain.process(context, &input, &mut output, &events);
        let samples: Vec<f32> = output.channel(0).to_vec();
        assert!(samples[0] < 0.5 && samples.last().unwrap() > &samples[0]);
        assert!(samples.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn filter_and_delay_change_signal_shape() {
        let mut filter = Filter::new(500.0);
        let mut input = AudioBuffer::new(2, 128);
        input.set_frames(128).unwrap();
        for frame in 0..128 {
            let value = if frame < 64 { 1.0 } else { 0.0 };
            input.channel_mut(0)[frame] = value;
            input.channel_mut(1)[frame] = value;
        }
        let mut output = AudioBuffer::new(2, 128);
        output.set_frames(128).unwrap();
        let context = ProcessContext {
            sample_rate: 48_000.0,
            engine_frame: 0,
            project_frame: 0,
            playing: true,
        };
        filter.prepare(settings()).unwrap();
        filter.process(context, &input, &mut output, &[]);
        let filtered = output.channel(0);
        assert!(filtered[63] > filtered[0]);
        assert!(filtered[127] > 0.01 && filtered[127] < filtered[63]);
        assert!(
            output
                .channel(0)
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
        );
        let mut delay = Delay::new(0.01, 0.3, 0.5);
        delay.prepare(settings()).unwrap();
        let mut output = AudioBuffer::new(2, 128);
        output.set_frames(128).unwrap();
        delay.process(context, &input, &mut output, &[]);
        assert!(output.channel(0).iter().any(|sample| sample.abs() > 0.001));
        delay.reset();
    }

    #[test]
    fn reverb_adds_tail_and_resets() {
        let mut reverb = Reverb::new(0.5);
        let mut input = AudioBuffer::new(2, 128);
        input.set_frames(128).unwrap();
        for frame in 0..128 {
            input.channel_mut(0)[frame] = 0.5;
            input.channel_mut(1)[frame] = 0.5;
        }
        let mut output = AudioBuffer::new(2, 128);
        output.set_frames(128).unwrap();
        let context = ProcessContext {
            sample_rate: 48_000.0,
            engine_frame: 0,
            project_frame: 0,
            playing: true,
        };
        reverb.prepare(settings()).unwrap();
        reverb.process(context, &input, &mut output, &[]);
        assert!(output.channel(0).iter().any(|sample| sample.abs() > 0.01));
        reverb.reset();
    }

    #[test]
    fn gain_parameter_validation_reports_range() {
        assert!(validate_gain(1.0).is_ok());
        assert!(validate_gain(5.0).is_err());
    }
}
