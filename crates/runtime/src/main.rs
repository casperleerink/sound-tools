//! The project runtime. For now a headless check of the realtime engine and the transport: it
//! plays a fixed scenario of Tone edits and transport operations on the default output device,
//! or renders the same scenario to a WAV file. A click on every beat makes the transport
//! audible: it sounds only while the project plays.
//!
//! `runtime` plays. `runtime --render <wav>` renders offline.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use sound_core::{
    AudioOutput, BarBeat, Connection, Engine, EngineConfig, EngineControl, EngineStatus, Node,
    OutputDevice, Ports, PrepareConfig, ProcessContext, Processor, Tempo, TempoMap, Ticks,
    TimeSignature,
};
use tone::{Tone, ToneParameters};

#[derive(Copy, Clone, Debug)]
enum Step {
    AddFirstTone,
    Play,
    ChangeFrequency,
    ChangeGain,
    AddSecondTone,
    Pause,
    RemoveSecondTone,
    Resume,
    FasterTempo,
    SeekToBarTwo,
    Stop,
    End,
}

/// Seconds from the start, and what happens then.
const SCENARIO: [(f64, Step); 12] = [
    (0.0, Step::AddFirstTone),
    (0.5, Step::Play),
    (1.0, Step::ChangeFrequency),
    (1.5, Step::ChangeGain),
    (2.0, Step::AddSecondTone),
    (2.5, Step::Pause),
    (3.0, Step::RemoveSecondTone),
    (3.25, Step::Resume),
    (3.75, Step::FasterTempo),
    (4.25, Step::SeekToBarTwo),
    (4.75, Step::Stop),
    (5.25, Step::End),
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

const FASTER_BPM: f64 = 180.0;

/// A short click on every beat of the project, louder on the first beat of a bar. It is
/// scheduled from the transport info alone, so it is silent while the project does not play.
struct Click {
    sample_rate: f32,
    /// Frames since the sounding click started.
    age: Option<u32>,
    gain: f32,
    clicks: Arc<AtomicU64>,
}

impl Click {
    const OUTPUT: AudioOutput = AudioOutput::new(0);
    const SECONDS: f32 = 0.04;
}

impl Processor for Click {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
    }

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let transport = &context.transport;
        let time_signature = transport.clock.tempo_map().time_signature();
        let beat = time_signature.ticks_per_beat();
        let mut next_beat = transport.tick_range.start.0.next_multiple_of(beat);
        for (offset, sample) in context
            .audio_outputs
            .get(Self::OUTPUT)
            .iter_mut()
            .enumerate()
        {
            if transport.offset_of(Ticks(next_beat)) == Some(offset) {
                let first_of_bar = next_beat.is_multiple_of(time_signature.ticks_per_bar());
                self.gain = if first_of_bar { 0.5 } else { 0.25 };
                self.age = Some(0);
                self.clicks.fetch_add(1, Ordering::Relaxed);
                next_beat += beat;
            }
            if let Some(age) = self.age {
                let seconds = age as f32 / self.sample_rate;
                let decay = (-seconds * 150.0).exp();
                *sample = self.gain * decay * (std::f32::consts::TAU * 1500.0 * seconds).sin();
                self.age = (seconds < Self::SECONDS).then_some(age + 1);
            }
        }
    }
}

/// The processors of the scenario, and the checks on what the engine reports between steps.
#[derive(Default)]
struct Scenario {
    first: Option<Node<Tone>>,
    second: Option<Node<Tone>>,
    clicks: Arc<AtomicU64>,
    /// The status of the previous poll.
    previous: Option<EngineStatus>,
    playhead_checks: u64,
    playhead_failures: u64,
}

impl Scenario {
    /// Between two polls with no edit applied in between, so with no transport operation, the
    /// playhead must move exactly as far as engine time while playing, and not at all otherwise.
    fn observe(&mut self, status: EngineStatus) {
        if let Some(previous) = self.previous
            && previous.batches_applied == status.batches_applied
        {
            let engine_frames = status.frames - previous.frames;
            let expected = if status.playing { engine_frames } else { 0 };
            let moved = status
                .playhead_frame
                .0
                .checked_sub(previous.playhead_frame.0);
            self.playhead_checks += 1;
            self.playhead_failures += u64::from(moved != Some(expected));
        }
        self.previous = Some(status);
    }

    fn check(&self, control: &EngineControl) -> Result<()> {
        if control.pending_edits() > 0 || self.playhead_failures > 0 {
            bail!("the run had problems, see the report above");
        }
        Ok(())
    }

    fn apply(&mut self, step: Step, control: &mut EngineControl) -> Result<()> {
        match step {
            Step::AddFirstTone => {
                self.first = Some(add_tone(control, "first", FIRST)?);
                add_click(control, self.clicks.clone())?;
            }
            Step::Play | Step::Resume => control.play(),
            Step::Pause => control.pause(),
            Step::Stop => control.stop(),
            Step::FasterTempo => control.set_tempo_map(TempoMap::constant(
                TimeSignature::default(),
                Tempo::from_bpm(FASTER_BPM)?,
            )),
            Step::SeekToBarTwo => {
                let time_signature = control.clock().tempo_map().time_signature();
                control.seek(time_signature.ticks_of(BarBeat {
                    bar: 2,
                    beat: 1,
                    tick: 0,
                })?);
            }
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

fn add_click(control: &mut EngineControl, clicks: Arc<AtomicU64>) -> Result<()> {
    let channels = control.config().channels;
    let mut edit = control.edit();
    let click = Click {
        sample_rate: 0.0,
        age: None,
        gain: 0.0,
        clicks,
    };
    let node = edit.add_processor("click", click)?;
    for channel in 0..channels {
        edit.connect(Connection::to_device(node.id(), Click::OUTPUT, channel))?;
    }
    edit.commit()?;
    Ok(())
}

/// Prints where the playhead is when a step is about to happen.
fn print_step(seconds: f64, step: Step, status: &EngineStatus, control: &EngineControl) {
    let time_signature = control.clock().tempo_map().time_signature();
    println!(
        "{seconds:>5.2} s  {:<17} playhead {:>8} (tick {:>5}, frame {:>6}), {}",
        format!("{step:?}"),
        time_signature.bar_beat_of(status.playhead_tick).to_string(),
        status.playhead_tick.0,
        status.playhead_frame.0,
        if status.playing {
            "playing"
        } else {
            "not playing"
        },
    );
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

    let mut scenario = Scenario::default();
    let started = Instant::now();
    for (seconds, step) in SCENARIO {
        while started.elapsed() < Duration::from_secs_f64(seconds) {
            scenario.observe(control.poll()?);
            std::thread::sleep(Duration::from_millis(5));
        }
        print_step(seconds, step, &control.poll()?, &control);
        scenario.apply(step, &mut control)?;
    }
    // Let the last batches come back before the final numbers are read.
    std::thread::sleep(Duration::from_millis(100));
    let status = control.poll()?;
    let device_status = stream.status();
    let errors = stream.take_errors();
    drop(stream);

    report(&status, &control, &scenario);
    println!("xruns: {}", device_status.xruns);
    println!("late callbacks: {}", device_status.late_callbacks);
    println!("slowest callback: {:?}", device_status.slowest_callback);
    println!("stream errors: {}", errors.len());
    for error in &errors {
        println!("  {error}");
    }
    if device_status.xruns > 0 || !errors.is_empty() {
        bail!("playback had problems, see the report above");
    }
    scenario.check(&control)
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
    let mut scenario = Scenario::default();
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
            scenario.observe(control.poll()?);
        }
        print_step(seconds, step, &control.poll()?, &control);
        scenario.apply(step, &mut control)?;
    }
    writer.finalize()?;
    report(&control.poll()?, &control, &scenario);
    scenario.check(&control)
}

fn report(status: &EngineStatus, control: &EngineControl, scenario: &Scenario) {
    println!("callbacks: {}", status.blocks);
    println!("frames processed: {}", status.frames);
    println!("edits applied: {}", status.batches_applied);
    println!("edits still waiting: {}", control.pending_edits());
    println!("event overflows: {}", status.event_overflows);
    println!("command ring full: {}", control.command_ring_full());
    println!("return ring full: {}", status.return_ring_full);
    println!("port misuses: {}", status.port_misuses);
    println!("clicks: {}", scenario.clicks.load(Ordering::Relaxed));
    println!(
        "playhead checks: {} polls, {} where it did not move with the playing state",
        scenario.playhead_checks, scenario.playhead_failures
    );
}

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [] => play(),
        [flag, path] if flag == "--render" => render(Path::new(path)),
        _ => bail!("usage: runtime [--render <wav>]"),
    }
}
