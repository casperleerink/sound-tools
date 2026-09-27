//! The default kit: every synthesized sound of the Drum pad, made from oscillators, noise,
//! filters and envelopes. No sample file ships with the product.
//!
//! A sound is rendered whole on the control thread, at the pitch and decay of its pad and the
//! rate of the engine, and played from memory like a sample. So the audio thread does the same
//! for a synthesized pad as for a sample pad, and a render is the same bytes every time: the
//! noise is a fixed sequence that starts from the same seed for every hit.
//!
//! What each sound is made of, and why, is written next to it. The recipes follow the analog
//! drum machines everyone knows (a sine that falls in pitch for a kick, a drum tone under
//! filtered noise for a snare, bursts of noise for a clap, clusters of square waves for the
//! metal of hats and cymbals), with the details tuned by measuring the renders.

use std::f64::consts::{PI, TAU};

use crate::Sound;

/// -60 dB, as the exponent of `e`: a level falls by this in the decay time.
const SIXTY_DB: f64 = 6.907_755_278_982_137;

/// The six square waves of the metal of an analog hi-hat and cymbal, in Hz: close to each
/// other and to no harmonic series, so together they ring like metal and not like a chord.
const METAL_HZ: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// Renders `sound` at `pitch` (a ratio, 1 is as the kit tunes it) for `decay_seconds`, at
/// `sample_rate`: left and right, and exactly `decay_seconds` long. The loudest sample of
/// either channel is [`Sound::level`].
pub(crate) fn synthesize(
    sound: Sound,
    pitch: f64,
    decay_seconds: f64,
    sample_rate: u32,
) -> Vec<[f32; 2]> {
    let rate = f64::from(sample_rate.max(1));
    let frames = (decay_seconds * rate).ceil().max(1.0) as usize;
    let voice = Voice {
        pitch,
        decay: decay_seconds.max(1.0 / rate),
        rate,
    };
    let mut out = vec![[0.0_f64; 2]; frames];
    match sound {
        Sound::Kick => voice.kick(&mut out),
        Sound::Snare => voice.snare(&mut out),
        Sound::Clap => voice.clap(&mut out),
        Sound::Rim => voice.rim(&mut out),
        Sound::Hat => voice.hat(&mut out, Hat::Closed),
        Sound::OpenHat => voice.hat(&mut out, Hat::Open),
        Sound::Tom => voice.tom(&mut out),
        Sound::Crash => voice.crash(&mut out),
        Sound::Ride => voice.ride(&mut out),
    }
    let peak = out
        .iter()
        .flatten()
        .fold(0.0_f64, |peak, sample| peak.max(sample.abs()));
    let scale = match peak > 0.0 {
        true => f64::from(sound.level()) / peak,
        false => 0.0,
    };
    out.iter()
        .map(|frame| frame.map(|sample| (sample * scale) as f32))
        .collect()
}

/// What every sound is rendered for.
struct Voice {
    /// A ratio: every frequency of the sound is multiplied by it.
    pitch: f64,
    /// Seconds from the hit to -60 dB.
    decay: f64,
    rate: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Hat {
    Closed,
    Open,
}

impl Voice {
    fn seconds(&self, frame: usize) -> f64 {
        frame as f64 / self.rate
    }

    /// A frequency of the sound at this pitch, kept below the Nyquist frequency with room.
    fn hz(&self, hz: f64) -> f64 {
        (hz * self.pitch).min(0.45 * self.rate)
    }

    /// A filter of the sound at this pitch.
    fn filter(&self, hz: f64, q: f64) -> Svf {
        Svf::new(self.hz(hz), q, self.rate)
    }

    /// The kick: a sine that falls from about 300 Hz to its note in the first 50 ms, which is
    /// the punch, and rings on at the note. Driven into a soft clip while it is loud, which
    /// rounds the first cycles into more of a square and makes it cut through; the tail stays a
    /// clean sine. A short band of noise on top is the beater.
    fn kick(&self, out: &mut [[f64; 2]]) {
        let note = self.hz(48.0);
        let mut phase = 0.0_f64;
        let mut noise = Noise::new(0x6b69_636b);
        let mut beater = self.filter(3_200.0, 0.9);
        const DRIVE: f64 = 2.0;
        let drive_scale = 1.0 / DRIVE.tanh();
        for (frame, sample) in out.iter_mut().enumerate() {
            let t = self.seconds(frame);
            let sweep = 1.0 + 2.2 * (-t / 0.03).exp() + 3.0 * (-t / 0.006).exp();
            phase += TAU * note * sweep / self.rate;
            let body = phase.sin() * shaped_fall(t, self.decay, 1.5);
            let body = (DRIVE * body).tanh() * drive_scale;
            let click = beater.band(noise.next()) * (-t / 0.0022).exp();
            *sample = mono(body + 0.5 * click);
        }
    }

    /// The snare: the tone of the drum head, two sines that drop a little in pitch and die
    /// early, under the rattle of the snares, which is noise between about 1.8 and 9.5 kHz for
    /// the whole decay. A short band of noise at 4 kHz is the crack of the stick.
    fn snare(&self, out: &mut [[f64; 2]]) {
        let (low, high) = (self.hz(180.0), self.hz(310.0));
        let tone_decay = (0.7 * self.decay).max(0.03);
        let (mut low_phase, mut high_phase) = (0.0_f64, 0.0_f64);
        let mut noise = Noise::new(0x736e_6172);
        let mut snares_high = self.filter(1_800.0, 0.7);
        let mut snares_low = self.filter(9_500.0, 0.7);
        let mut crack = self.filter(4_200.0, 1.2);
        for (frame, sample) in out.iter_mut().enumerate() {
            let t = self.seconds(frame);
            let bend = 1.0 + 0.35 * (-t / 0.012).exp();
            low_phase += TAU * low * bend / self.rate;
            high_phase += TAU * high * bend / self.rate;
            let tone = (low_phase.sin() + 0.55 * high_phase.sin()) * fall(t, tone_decay);
            let white = noise.next();
            let snares = snares_low.low(snares_high.high(white));
            let snares = snares * rise(t, 0.0008) * shaped_fall(t, self.decay, 1.1);
            let crack = crack.band(white) * (-t / 0.008).exp();
            let mixed = tone + 0.7 * snares + 0.3 * crack;
            *sample = mono((1.4 * mixed).tanh());
        }
    }

    /// The clap: four bursts of band-passed noise about 11 ms apart, as several hands that do
    /// not quite meet, then a softer tail of the same noise, the room. Each side has noise of
    /// its own in part, so the clap is wide.
    fn clap(&self, out: &mut [[f64; 2]]) {
        const BURSTS: [(f64, f64); 4] = [(0.0, 0.8), (0.011, 0.95), (0.023, 0.75), (0.034, 1.0)];
        let tail_start = 0.034;
        let tail_decay = (self.decay - tail_start).max(0.02);
        let mut common = Noise::new(0x636c_6170);
        let mut sides = [Noise::new(0x6c65_6674), Noise::new(0x7269_6768)];
        let mut bursts = [self.filter(950.0, 1.4), self.filter(950.0, 1.4)];
        let mut bursts_high = [self.filter(500.0, 0.7), self.filter(500.0, 0.7)];
        let mut tails = [self.filter(850.0, 1.0), self.filter(850.0, 1.0)];
        for (frame, sample) in out.iter_mut().enumerate() {
            let t = self.seconds(frame);
            let hands: f64 = BURSTS
                .iter()
                .filter(|(start, _)| t >= *start)
                .map(|(start, level)| {
                    level * rise(t - start, 0.0003) * (-(t - start) / 0.0045).exp()
                })
                .sum();
            let room = match t >= tail_start {
                true => 0.7 * fall(t - tail_start, tail_decay),
                false => 0.0,
            };
            let shared = common.next();
            for side in 0..2 {
                let white = 0.8 * shared + 0.35 * sides[side].next();
                let burst = bursts_high[side].high(bursts[side].band(white));
                sample[side] = burst * hands + tails[side].band(white) * room;
            }
        }
    }

    /// The rim: two short sines at about 470 Hz and 1.7 kHz, the ring of the rim and the shell,
    /// with a click of high noise on top, high-passed so it has no thump and driven hard, which
    /// makes it woody.
    fn rim(&self, out: &mut [[f64; 2]]) {
        let (low, high) = (self.hz(470.0), self.hz(1_700.0));
        let mut noise = Noise::new(0x7269_6d73);
        let mut click = self.filter(4_000.0, 0.7);
        let mut thin = self.filter(350.0, 0.7);
        for (frame, sample) in out.iter_mut().enumerate() {
            let t = self.seconds(frame);
            let ring = 0.6 * (TAU * low * t).sin() * fall(t, 0.5 * self.decay)
                + (TAU * high * t).sin() * fall(t, self.decay);
            let click = click.high(noise.next()) * (-t / 0.0007).exp();
            let shaped = thin.high(ring + 0.6 * click);
            *sample = mono((2.2 * shaped).tanh());
        }
    }

    /// The hats: the metal of six square waves with some noise, band-passed high. A closed hat
    /// falls at once; an open hat holds for a moment before it falls, and is a little lower.
    /// Each side has part of its noise of its own, so the hat is a little wide.
    fn hat(&self, out: &mut [[f64; 2]], hat: Hat) {
        let (band_hz, high_hz, shape) = match hat {
            Hat::Closed => (7_000.0, 5_000.0, 1.0),
            Hat::Open => (6_500.0, 4_500.0, 1.35),
        };
        let mut metal = Metal::new(self, &METAL_HZ, 1.0);
        let mut common = Noise::new(0x6861_7473);
        let mut sides = [Noise::new(0x6861_746c), Noise::new(0x6861_7472)];
        let mut bands = [self.filter(band_hz, 1.0), self.filter(band_hz, 1.0)];
        let mut highs = [self.filter(high_hz, 0.7), self.filter(high_hz, 0.7)];
        for (frame, sample) in out.iter_mut().enumerate() {
            let t = self.seconds(frame);
            let level = rise(t, 0.0002) * shaped_fall(t, self.decay, shape);
            let (ring, shared) = (metal.next(), common.next());
            for side in 0..2 {
                let noise = 0.75 * shared + 0.25 * sides[side].next();
                let source = 0.55 * ring + 0.45 * noise;
                sample[side] = highs[side].high(bands[side].band(source)) * level;
            }
        }
    }

    /// A tom: a sine that drops a third of its pitch in the first 50 ms, as a struck head
    /// does, with a second mode of the head a fifth above that dies sooner, and the soft
    /// thud of the stick. The pads tune the toms apart.
    fn tom(&self, out: &mut [[f64; 2]]) {
        let note = self.hz(120.0);
        let overtone = self.hz(120.0 * 1.52);
        let (mut phase, mut overtone_phase) = (0.0_f64, 0.0_f64);
        let mut noise = Noise::new(0x746f_6d73);
        let mut mallet = self.filter(2_500.0, 0.7);
        for (frame, sample) in out.iter_mut().enumerate() {
            let t = self.seconds(frame);
            let bend = 1.0 + 0.35 * (-t / 0.05).exp();
            phase += TAU * note * bend / self.rate;
            overtone_phase += TAU * overtone * bend / self.rate;
            let head = phase.sin() * shaped_fall(t, self.decay, 1.3)
                + 0.3 * overtone_phase.sin() * fall(t, 0.5 * self.decay);
            let thud = mallet.low(noise.next()) * (-t / 0.004).exp();
            *sample = mono((1.3 * (head + 0.25 * thud)).tanh());
        }
    }

    /// The crash: twelve square waves of metal and plenty of noise, a splash of bright noise
    /// at the hit and a long wash that falls fast at first and then slowly, getting darker as
    /// it rings, as a cymbal does. The two sides are tuned a hair apart and have noise of their
    /// own, so the crash is wide.
    fn crash(&self, out: &mut [[f64; 2]]) {
        let mut metals = [
            Metal::new(self, &METAL_HZ, 1.0).and(self, &METAL_HZ, 1.41),
            Metal::new(self, &METAL_HZ, 1.007).and(self, &METAL_HZ, 1.423),
        ];
        let mut common = Noise::new(0x6372_6173);
        let mut sides = [Noise::new(0x6372_736c), Noise::new(0x6372_7372)];
        let mut highs = [self.filter(3_500.0, 0.7), self.filter(3_500.0, 0.7)];
        let mut mids = [self.filter(3_500.0, 0.8), self.filter(3_500.0, 0.8)];
        let mut splashes = [self.filter(2_000.0, 0.7), self.filter(2_000.0, 0.7)];
        let mut darkening = [self.filter(12_000.0, 0.7), self.filter(12_000.0, 0.7)];
        let mid_decay = 0.35 * self.decay;
        for (frame, sample) in out.iter_mut().enumerate() {
            let t = self.seconds(frame);
            if frame % 32 == 0 {
                let hz = 5_000.0 + 7_000.0 * (-t / (0.4 * self.decay)).exp();
                darkening
                    .iter_mut()
                    .for_each(|filter| filter.tune(self.hz(hz), 0.7, self.rate));
            }
            let attack = rise(t, 0.001);
            let (wash, middle, splash) = (
                shaped_fall(t, self.decay, 1.4),
                fall(t, mid_decay),
                fall(t, 0.12),
            );
            let shared = common.next();
            for side in 0..2 {
                let noise = 0.5 * shared + 0.5 * sides[side].next();
                let source = 0.45 * metals[side].next() + 0.55 * noise;
                let sound = 0.9 * highs[side].high(source) * wash
                    + 0.6 * mids[side].band(source) * middle
                    + 0.25 * splashes[side].high(noise) * splash;
                sample[side] = darkening[side].low(sound) * attack;
            }
        }
    }

    /// The ride: the ping of the stick, seven bell partials a little out of tune with each
    /// other that ring for part of the decay, over a quiet wash of metal and noise that rings
    /// for all of it, and the tick of the stick.
    fn ride(&self, out: &mut [[f64; 2]]) {
        const PARTIALS: [(f64, f64, f64); 7] = [
            (1.0, 1.0, 0.5),
            (1.505, 0.7, 0.45),
            (2.14, 0.55, 0.4),
            (2.77, 0.5, 0.35),
            (3.41, 0.4, 0.3),
            (4.23, 0.3, 0.25),
            (5.02, 0.25, 0.2),
        ];
        let bell = 620.0;
        let tunings = [1.0, 1.003];
        let mut metals = [
            Metal::new(self, &METAL_HZ, 1.2),
            Metal::new(self, &METAL_HZ, 1.205),
        ];
        let mut common = Noise::new(0x7269_6465);
        let mut sides = [Noise::new(0x7269_646c), Noise::new(0x7269_6472)];
        let mut bands = [self.filter(7_000.0, 0.7), self.filter(7_000.0, 0.7)];
        let mut highs = [self.filter(4_500.0, 0.7), self.filter(4_500.0, 0.7)];
        let mut ticks = [self.filter(3_000.0, 0.7), self.filter(3_000.0, 0.7)];
        for (frame, sample) in out.iter_mut().enumerate() {
            let t = self.seconds(frame);
            let attack = rise(t, 0.0005);
            let wash_level = shaped_fall(t, self.decay, 1.25);
            let tick_level = (-t / 0.0025).exp();
            let shared = common.next();
            for side in 0..2 {
                let ping: f64 = PARTIALS
                    .iter()
                    .map(|(ratio, level, decay)| {
                        let hz = self.hz(bell * ratio * tunings[side]);
                        level * (TAU * hz * t).sin() * fall(t, decay * self.decay)
                    })
                    .sum();
                let noise = 0.6 * shared + 0.4 * sides[side].next();
                let source = 0.5 * metals[side].next() + 0.5 * noise;
                let wash = highs[side].high(bands[side].band(source)) * wash_level;
                let tick = ticks[side].high(noise) * tick_level;
                sample[side] = (0.05 * ping + 0.7 * wash + 0.5 * tick) * attack;
            }
        }
    }
}

fn mono(sample: f64) -> [f64; 2] {
    [sample; 2]
}

/// From 1 at the hit to -60 dB after `length` seconds, falling evenly in dB.
fn fall(t: f64, length: f64) -> f64 {
    (-SIXTY_DB * t / length).exp()
}

/// The same fall with a shape: above 1 it holds longer before it falls, below 1 it drops
/// faster first and rings longer after. It still reaches -60 dB after `length` seconds.
fn shaped_fall(t: f64, length: f64, shape: f64) -> f64 {
    (-SIXTY_DB * (t / length).powf(shape)).exp()
}

/// From 0 at the hit to 1 after `length` seconds, so nothing starts with a step.
fn rise(t: f64, length: f64) -> f64 {
    (t / length).min(1.0)
}

/// White noise, from a fixed seed: the same sequence every time (xorshift32).
struct Noise(u32);

impl Noise {
    fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// The next sample, from -1 to 1.
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        f64::from(x) / f64::from(u32::MAX) * 2.0 - 1.0
    }
}

/// A state variable filter in the trapezoidal form (Simper): low, band and high pass from one
/// memory, stable when it is tuned while it runs.
struct Svf {
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    ic1: f64,
    ic2: f64,
}

impl Svf {
    fn new(hz: f64, q: f64, rate: f64) -> Self {
        let mut filter = Self {
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        filter.tune(hz, q, rate);
        filter
    }

    fn tune(&mut self, hz: f64, q: f64, rate: f64) {
        let g = (PI * hz.min(0.45 * rate) / rate).tan();
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// Low, band and high pass of one sample.
    fn run(&mut self, input: f64) -> (f64, f64, f64) {
        let v3 = input - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, input - self.k * v1 - v2)
    }

    fn low(&mut self, input: f64) -> f64 {
        self.run(input).0
    }

    fn band(&mut self, input: f64) -> f64 {
        self.run(input).1
    }

    fn high(&mut self, input: f64) -> f64 {
        self.run(input).2
    }
}

/// Square waves summed, the metal of hats and cymbals. Each square has its steps rounded off
/// over two frames (PolyBLEP), so little folds back from above the Nyquist frequency.
struct Metal {
    /// Phase and phase step per square, in cycles.
    squares: Vec<(f64, f64)>,
}

impl Metal {
    fn new(voice: &Voice, hz: &[f64], tuning: f64) -> Self {
        Self {
            squares: Vec::new(),
        }
        .and(voice, hz, tuning)
    }

    fn and(mut self, voice: &Voice, hz: &[f64], tuning: f64) -> Self {
        self.squares.extend(
            hz.iter()
                .map(|hz| (0.0, voice.hz(hz * tuning) / voice.rate)),
        );
        self
    }

    fn next(&mut self) -> f64 {
        let mut sum = 0.0;
        for (phase, step) in &mut self.squares {
            let naive = if *phase < 0.5 { 1.0 } else { -1.0 };
            let half = (*phase + 0.5).fract();
            sum += naive + blep(*phase, *step) - blep(half, *step);
            *phase = (*phase + *step).fract();
        }
        sum / self.squares.len().max(1) as f64
    }
}

/// What to add near a step of a waveform, at `phase` of a cycle, to round it off.
fn blep(phase: f64, step: f64) -> f64 {
    if phase < step {
        let t = phase / step;
        t + t - t * t - 1.0
    } else if phase > 1.0 - step {
        let t = (phase - 1.0) / step;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}
