//! The project runtime. For now a headless check of the realtime engine: it plays a fixed Tone
//! scenario on the default output device, or renders the same scenario to a WAV file.
//!
//! `runtime` plays. `runtime --render <wav>` renders offline.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use sound_core::{
    Connection, Engine, EngineConfig, EngineControl, EngineStatus, Node, OutputDevice,
};
use tone::{Tone, ToneParameters};

#[derive(Copy, Clone)]
enum Step {
    AddFirstTone,
    ChangeFrequency,
    ChangeGain,
    AddSecondTone,
    RemoveSecondTone,
    End,
}

/// Seconds from the start, and what happens then.
const SCENARIO: [(f64, Step); 6] = [
    (0.0, Step::AddFirstTone),
    (1.0, Step::ChangeFrequency),
    (1.5, Step::ChangeGain),
    (2.0, Step::AddSecondTone),
    (3.0, Step::RemoveSecondTone),
    (4.0, Step::End),
];

const FIRST: ToneParameters = ToneParameters {
    frequency_hz: 220.0,
    gain: 0.2,
};
const RAISED_HZ: f32 = 277.18;
const SECOND: ToneParameters = ToneParameters {
    frequency_hz: 330.0,
    gain: 0.1,
};

#[derive(Default)]
struct Tones {
    first: Option<Node<Tone>>,
    second: Option<Node<Tone>>,
}

impl Tones {
    fn apply(&mut self, step: Step, control: &mut EngineControl) -> Result<()> {
        match step {
            Step::AddFirstTone => self.first = Some(add_tone(control, "first", FIRST)?),
            Step::ChangeFrequency => {
                let first = self.first.context("the first tone is missing")?;
                control.update(
                    first,
                    ToneParameters {
                        frequency_hz: RAISED_HZ,
                        ..FIRST
                    },
                )?;
            }
            Step::ChangeGain => {
                let first = self.first.context("the first tone is missing")?;
                let parameters = ToneParameters {
                    frequency_hz: RAISED_HZ,
                    gain: 0.1,
                };
                control.update(first, parameters)?;
            }
            Step::AddSecondTone => self.second = Some(add_tone(control, "second", SECOND)?),
            Step::RemoveSecondTone => {
                let second = self.second.take().context("the second tone is missing")?;
                let mut edit = control.edit();
                edit.remove_processor(second.id())?;
                edit.commit()?;
            }
            Step::End => {}
        }
        Ok(())
    }
}

/// One edit: the new Tone and its connections to every device channel land in the same block.
fn add_tone(
    control: &mut EngineControl,
    name: &str,
    parameters: ToneParameters,
) -> Result<Node<Tone>> {
    let channels = control.config().channels;
    let mut edit = control.edit();
    let node = edit.add_processor(name, Tone::new(parameters))?;
    for channel in 0..channels {
        edit.connect(Connection::to_device(node.id(), Tone::OUTPUT, channel))?;
    }
    edit.commit()?;
    Ok(node)
}

fn play() -> Result<()> {
    let device = OutputDevice::default_output()?;
    let config = EngineConfig::new(device.sample_rate(), device.channels());
    println!(
        "device: {} Hz, {} channels",
        config.sample_rate, config.channels
    );
    let (mut control, engine) = Engine::new(config);
    let stream = device.start(engine)?;

    let mut tones = Tones::default();
    let started = Instant::now();
    for (seconds, step) in SCENARIO {
        while started.elapsed() < Duration::from_secs_f64(seconds) {
            control.poll()?;
            std::thread::sleep(Duration::from_millis(5));
        }
        tones.apply(step, &mut control)?;
    }
    // Let the last batches come back before the final numbers are read.
    std::thread::sleep(Duration::from_millis(100));
    let status = control.poll()?;
    let device_status = stream.status();
    let errors = stream.take_errors();
    drop(stream);

    report(&status, &control);
    println!("xruns: {}", device_status.xruns);
    println!("late callbacks: {}", device_status.late_callbacks);
    println!("slowest callback: {:?}", device_status.slowest_callback);
    println!("stream errors: {}", errors.len());
    for error in &errors {
        println!("  {error}");
    }
    if device_status.xruns > 0 || !errors.is_empty() || control.pending_edits() > 0 {
        bail!("playback had problems, see the report above");
    }
    Ok(())
}

fn render(path: &Path) -> Result<()> {
    let config = EngineConfig::new(48_000, 2);
    let (mut control, mut engine) = Engine::new(config);
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: config.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;

    // Not a multiple of the engine block size, so short sub-blocks are part of the render.
    let mut buffer = [0.0_f32; 480 * 2];
    let mut tones = Tones::default();
    let mut rendered_frames = 0;
    for (seconds, step) in SCENARIO {
        let step_frame = (seconds * f64::from(config.sample_rate)) as usize;
        while rendered_frames < step_frame {
            let frames = (step_frame - rendered_frames).min(buffer.len() / config.channels);
            let output = &mut buffer[..frames * config.channels];
            engine.process_block(output);
            for sample in output.iter() {
                writer.write_sample(*sample)?;
            }
            rendered_frames += frames;
            control.poll()?;
        }
        tones.apply(step, &mut control)?;
    }
    writer.finalize()?;
    report(&control.poll()?, &control);
    Ok(())
}

fn report(status: &EngineStatus, control: &EngineControl) {
    println!("callbacks: {}", status.blocks);
    println!("frames processed: {}", status.frames);
    println!("edits applied: {}", status.batches_applied);
    println!("edits still waiting: {}", control.pending_edits());
    println!("event overflows: {}", status.event_overflows);
    println!("command ring full: {}", control.command_ring_full());
    println!("return ring full: {}", status.return_ring_full);
}

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [] => play(),
        [flag, path] if flag == "--render" => render(Path::new(path)),
        _ => bail!("usage: runtime [--render <wav>]"),
    }
}
