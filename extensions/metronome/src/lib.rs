//! The click: one processor on the beats of the project's tempo map, and a switch for it.
//!
//! The click is not music. It has no record, no tool and no undo step, so a project from
//! before it existed gets one without anyone editing its files, and `--render` never contains
//! it: only a window attaches a [`Click`]. The processor reads the beats from the transport's
//! clock, so it follows every tempo change and every time signature by itself and keeps no
//! copy of the tempo map.
//!
//! ```no_run
//! # fn main() -> Result<(), sound_core::GraphError> {
//! # let (mut engine, _) = sound_core::Engine::new(sound_core::EngineConfig::new(48_000, 2));
//! let mut click = metronome::Click::attach(&mut engine)?;
//! click.set_on(&mut engine, true)?;
//! # Ok(())
//! # }
//! ```

use std::f32::consts::TAU;

use sound_core::{
    AudioOutput, Connection, EngineControl, GraphError, Node, Ports, PrepareConfig, ProcessContext,
    Processor, Smoothed, Ticks,
};

/// The name of the click's processor in the engine graph. Instance processors are named
/// `<instance id>#<name>`, so a name without `#` can never collide with one.
const PROCESSOR: &str = "metronome";

/// The first device channel the click goes to. The right channel goes to the next one.
const DEVICE_CHANNEL: usize = 0;

/// The pitch of the first beat of a bar, and of every other beat. Two frequencies is the whole
/// difference between a downbeat and the rest: there is nothing to choose and nothing to save.
pub const DOWNBEAT_HZ: f32 = 1600.0;
pub const BEAT_HZ: f32 = 1000.0;

/// How loud a click starts. No control: the click is a reference, not part of the mix.
pub const LEVEL: f32 = 0.25;

/// How long one click sounds. Short enough that two beats never overlap at 1000 bpm.
pub const CLICK_SECONDS: f32 = 0.03;

/// The amplitude a click has decayed to when it ends, so its end is silent.
const END_AMPLITUDE: f32 = 0.001;

/// How long the click takes to fall silent when the transport stops, seeks, or the switch goes
/// off. Short enough that nothing hangs, long enough that the cut is not a second click.
const FADE_SECONDS: f32 = 0.002;

/// The click in a running engine: the processor and its switch.
///
/// It is not project state. Nothing is saved, nothing is written and there is no undo step, so
/// `cmd-z` can never toggle the click. Dropping a `Click` leaves the processor in the engine;
/// the engine goes away with the window.
pub struct Click {
    node: Node<Metronome>,
    on: bool,
}

impl Click {
    /// Adds the click to the engine and connects it to the first device channels. It starts
    /// off and silent, so attaching changes nothing that is heard.
    pub fn attach(engine: &mut EngineControl) -> Result<Self, GraphError> {
        let mut edit = engine.edit();
        let node = edit.add_processor(PROCESSOR, Metronome::default())?;
        edit.connect(Connection::to_device(
            node.id(),
            Metronome::OUTPUT,
            DEVICE_CHANNEL,
        ))?;
        edit.commit()?;
        Ok(Self { node, on: false })
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Turns the click on or off. It is an update, so it costs no compile, and a click that is
    /// sounding fades out instead of being cut.
    pub fn set_on(&mut self, engine: &mut EngineControl, on: bool) -> Result<(), GraphError> {
        self.on = on;
        engine.update(self.node, on)
    }
}

/// One click, from its onset until it has decayed. There is one at a time: a beat is always
/// longer than [`CLICK_SECONDS`] within the tempo bounds of the clock.
#[derive(Default)]
struct Burst {
    /// Frames until the burst is over. 0 is silence.
    frames_left: usize,
    /// In cycles, from 0 to 1.
    phase: f32,
    /// Cycles per frame of the pitch this burst started with.
    step: f32,
    /// The decaying amplitude, 1 at the onset.
    amplitude: f32,
}

/// Clicks the beats of the project's tempo map. A beat is the note value of the time
/// signature's lower number, and the first beat of a bar sounds higher than the rest.
///
/// It holds no tempo map: every block it asks the transport which ticks it covers and the
/// clock on which frame each of them lands. So a tempo change, a tempo map written from
/// outside and another time signature all need no update at all.
pub struct Metronome {
    on: bool,
    burst: Burst,
    /// Falls to 0 on a stop, a seek or a switch off, so no click hangs and none is cut short
    /// with a step in the signal.
    fade: Smoothed,
    /// Whether the fade is already aimed at 0. Aiming again every block would shorten the step
    /// every time, so the fade would approach 0 without reaching it.
    fading: bool,
    click_frames: usize,
    /// What the amplitude is multiplied by per frame, for a burst that ends silent.
    decay_per_frame: f32,
    fade_frames: f32,
    seconds_per_frame: f32,
}

impl Default for Metronome {
    fn default() -> Self {
        Self {
            on: false,
            burst: Burst::default(),
            fade: Smoothed::new(0.0),
            fading: true,
            click_frames: 1,
            decay_per_frame: 0.0,
            fade_frames: 1.0,
            seconds_per_frame: 0.0,
        }
    }
}

impl Metronome {
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    /// Starts a click. The sine starts at its peak, so a click has the sharp attack of a tick
    /// and its first sample is the loudest one, which is also what a test looks for.
    fn strike(&mut self, downbeat: bool) {
        let frequency = if downbeat { DOWNBEAT_HZ } else { BEAT_HZ };
        self.burst = Burst {
            frames_left: self.click_frames,
            phase: 0.25,
            step: frequency * self.seconds_per_frame,
            amplitude: 1.0,
        };
        self.fade.set_target(1.0, 1.0);
        self.fade.snap();
        self.fading = false;
    }

    /// The next sample of the burst, or 0 when none is sounding.
    fn next_sample(&mut self) -> f32 {
        if self.burst.frames_left == 0 {
            return 0.0;
        }
        self.burst.frames_left -= 1;
        let sample = (self.burst.phase * TAU).sin() * self.burst.amplitude * self.fade.advance(1);
        self.burst.phase = (self.burst.phase + self.burst.step).fract();
        self.burst.amplitude *= self.decay_per_frame;
        sample * LEVEL
    }
}

impl Processor for Metronome {
    /// Whether the click sounds.
    type Update = bool;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        let sample_rate = config.sample_rate.max(1) as f32;
        self.seconds_per_frame = 1.0 / sample_rate;
        self.click_frames = (CLICK_SECONDS * sample_rate).max(1.0) as usize;
        self.fade_frames = (FADE_SECONDS * sample_rate).max(1.0);
        self.decay_per_frame = END_AMPLITUDE.powf(1.0 / self.click_frames as f32);
    }

    fn update(&mut self, update: &mut bool) {
        self.on = *update;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let transport = &context.transport;
        // A stop or a seek ends the click that is sounding. After a seek the beats at the new
        // position come through `tick_range` again, so nothing has to be remembered.
        if (!self.on || transport.jumped || transport.stopped_playing) && !self.fading {
            self.fade.set_target(0.0, self.fade_frames);
            self.fading = true;
        }
        let time_signature = transport.clock.tempo_map().time_signature();
        let ticks_per_beat = time_signature.ticks_per_beat();
        let ticks_per_bar = time_signature.ticks_per_bar();
        let mut beat = transport
            .tick_range
            .start
            .0
            .div_ceil(ticks_per_beat)
            .saturating_mul(ticks_per_beat);
        let mut onset = match self.on {
            true => transport.offset_of(Ticks(beat)),
            false => None,
        };
        if onset.is_none() && self.burst.frames_left == 0 {
            // Nothing to write, and every output starts silent.
            return;
        }

        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        for (frame, sample) in left.iter_mut().enumerate() {
            while onset == Some(frame) {
                self.strike(beat.is_multiple_of(ticks_per_bar));
                beat = beat.saturating_add(ticks_per_beat);
                onset = transport.offset_of(Ticks(beat));
            }
            *sample = self.next_sample();
        }
        right.copy_from_slice(left);
    }
}
