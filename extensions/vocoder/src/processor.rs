//! The vocoder processor: a spectrum of the track and one of the voice every hop, from one
//! [`Stft`] per channel with the voice as its side input.
//!
//! Both spectra are split into the same bands, spaced evenly in octaves from [`LOWEST_HZ`] to
//! [`HIGHEST_HZ`]; the bins under and over them belong to the first and the last band. Each
//! band has a level, the root mean square of its magnitudes. A bin of the track goes out at
//! its magnitude times the level of the voice over the level of the track there: the track
//! loses its own shape and takes the voice's. Between the middles of two bands the levels are
//! read on a line, so the shape has no steps at the edges of the bands. The phase of the track
//! stays, so with a voice of the same shape the track comes out as it went in.
//!
//! The level of a band of the voice rises at once and falls over the release. The formant
//! moves where a band reads the voice: up by a semitone, each band hears the voice a
//! semitone lower, so the voice's shape sits higher.

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, Ports, PrepareConfig, ProcessContext,
    Processor, SpectralBin, Stft, all_held_silent, held,
};

use crate::{BANDS, PARAMETERS, VocoderState};

/// The size of a spectrum: 21 ms at 48 kHz, short enough to follow speech, and bins 47 Hz
/// apart, close enough for the lowest bands.
pub const SIZE: usize = 1024;
/// The bins of a spectrum.
const BINS: usize = SIZE / 2 + 1;
/// The most bands: the top of the range of `bands`.
const MAX_BANDS: usize = 64;

/// Where the bands start and end.
pub const LOWEST_HZ: f32 = 100.0;
pub const HIGHEST_HZ: f32 = 12_000.0;

/// The track's level in a band is taken as at least this, -60 dB: a band where the track is
/// quieter is not raised to the level of the voice, so its noise stays down.
const TRACK_FLOOR: f32 = 1e-3;

/// Silent frames in and out after which the transforms hold nothing: the window in, and the
/// sound still on its way out.
const REST_FRAMES: usize = 2 * SIZE;

pub struct Vocoder {
    sample_rate: f32,
    /// The record, with the values of the lanes that automate it.
    state: Automated<VocoderState, { PARAMETERS.len() }>,
    layout: Layout,
    /// How much of its level a band of the voice keeps from one spectrum to the next as it
    /// falls.
    release: f32,
    channels: [Channel; 2],
    /// Frames in a row of silence in the input and the sidechain.
    quiet: usize,
}

/// The bands, for a number of them, a formant and a sample rate.
struct Layout {
    bands: usize,
    formant: f32,
    sample_rate: f32,
    /// The bins of each band of the track, from and to.
    track: [(usize, usize); MAX_BANDS],
    /// The bins of the voice each band hears, moved by the formant.
    voice: [(usize, usize); MAX_BANDS],
    /// For each bin, the band whose middle is at or under it, and how far it is on the way to
    /// the middle of the next, 0 to 1.
    places: Box<[(usize, f32)]>,
}

struct Channel {
    stft: Stft,
    levels: Levels,
}

struct Levels {
    /// The level of each band of the voice, as it falls over the release.
    voice: [f32; MAX_BANDS],
    /// The level of each band of the track in this spectrum.
    track: [f32; MAX_BANDS],
}

impl Vocoder {
    pub const INPUT: AudioInput = AudioInput::new(0);
    /// The voice, while anything feeds it. Else the vocoder hears its own input.
    pub const SIDECHAIN: AudioInput = AudioInput::new(1);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<VocoderState, { PARAMETERS.len() }> =
        AutomationInput::new(0, PARAMETERS);

    pub fn new(state: VocoderState) -> Self {
        let mut vocoder = Self {
            sample_rate: 48_000.0,
            state: Automated::new(Self::AUTOMATION, state),
            layout: Layout {
                bands: 0,
                formant: f32::NAN,
                sample_rate: 0.0,
                track: [(0, 0); MAX_BANDS],
                voice: [(0, 0); MAX_BANDS],
                places: vec![(0, 0.0); BINS].into(),
            },
            release: 0.0,
            channels: [Channel::new(), Channel::new()],
            quiet: REST_FRAMES,
        };
        vocoder.aim();
        vocoder
    }

    /// Sets the bands and the release from the record and its lanes. The tables and the `exp`
    /// are worked out here, when a value moves, and never per spectrum.
    fn aim(&mut self) {
        let state = *self.state;
        let bands = (state.bands.round() as usize).clamp(BANDS.min as usize, MAX_BANDS);
        self.layout.set(bands, state.formant, self.sample_rate);
        let hop_seconds = (SIZE / 4) as f32 / self.sample_rate;
        self.release = (-hop_seconds / (state.release_ms / 1_000.0).max(1e-3)).exp();
    }

    /// The state silence leads to: no level held in any band.
    fn rest(&mut self) {
        for channel in &mut self.channels {
            channel.levels.voice.fill(0.0);
        }
    }
}

impl Channel {
    fn new() -> Self {
        Self {
            stft: Stft::with_side(SIZE),
            levels: Levels {
                voice: [0.0; MAX_BANDS],
                track: [0.0; MAX_BANDS],
            },
        }
    }
}

impl Levels {
    /// Gives `track` the shape of `voice`, `mix` of the way.
    #[inline]
    fn vocode(
        &mut self,
        layout: &Layout,
        release: f32,
        mix: f32,
        track: &mut [SpectralBin],
        voice: &[SpectralBin],
    ) {
        let bands = layout.bands;
        let ranges = layout.track.iter().zip(&layout.voice).take(bands);
        let levels = self.track.iter_mut().zip(self.voice.iter_mut());
        for ((track_range, voice_range), (track_level, voice_level)) in ranges.zip(levels) {
            *track_level = level(track, *track_range).max(TRACK_FLOOR);
            let now = level(voice, *voice_range);
            *voice_level = match now >= *voice_level {
                true => now,
                false => now + release * (*voice_level - now),
            };
        }
        let last = bands.saturating_sub(1);
        let read = |levels: &[f32; MAX_BANDS], band: usize, along: f32| {
            let low = levels.get(band).copied().unwrap_or(0.0);
            let high = levels.get((band + 1).min(last)).copied().unwrap_or(0.0);
            low + (high - low) * along
        };
        for (bin, (band, along)) in track.iter_mut().zip(layout.places.iter()) {
            let gain = read(&self.voice, *band, *along) / read(&self.track, *band, *along);
            bin.magnitude *= 1.0 + mix * (gain - 1.0);
        }
    }
}

/// The root mean square of the magnitudes of `bins` from and to.
#[inline]
fn level(bins: &[SpectralBin], (from, to): (usize, usize)) -> f32 {
    let Some(bins) = bins.get(from..to) else {
        return 0.0;
    };
    let power: f32 = bins.iter().map(|bin| bin.magnitude * bin.magnitude).sum();
    (power / bins.len().max(1) as f32).sqrt()
}

impl Layout {
    /// The tables of `bands` bands, the voice moved by `formant` semitones. Nothing allocates,
    /// so it runs on the audio thread when a value moves.
    fn set(&mut self, bands: usize, formant: f32, sample_rate: f32) {
        if (bands, formant, sample_rate) == (self.bands, self.formant, self.sample_rate) {
            return;
        }
        (self.bands, self.formant, self.sample_rate) = (bands, formant, sample_rate);
        let bin_hz = sample_rate / SIZE as f32;
        let step = (HIGHEST_HZ / LOWEST_HZ).powf(1.0 / bands as f32);
        // Each band is at least one bin wide: at the bottom a band is narrower than a bin.
        let mut from = 0;
        for (band, range) in self.track.iter_mut().take(bands).enumerate() {
            let to = match band + 1 == bands {
                true => BINS,
                false => {
                    let edge = LOWEST_HZ * step.powi(band as i32 + 1) / bin_hz;
                    (edge.round() as usize).max(from + 1).min(BINS - 1)
                }
            };
            *range = (from, to);
            from = to;
        }
        let shift = 2_f32.powf(-formant / 12.0);
        let moved = |bin: usize| ((bin as f32 * shift) as usize).min(BINS - 1);
        for ((from, to), voice) in self.track.iter().zip(&mut self.voice).take(bands) {
            let start = moved(*from);
            *voice = (start, moved(*to).max(start + 1));
        }
        let middle = |band: usize| {
            let (from, to) = self.track.get(band).copied().unwrap_or((0, 0));
            (from + to) as f32 / 2.0 - 0.5
        };
        let mut band = 0;
        for (bin, place) in self.places.iter_mut().enumerate() {
            let bin = bin as f32;
            while band + 1 < bands && middle(band + 1) <= bin {
                band += 1;
            }
            let (low, high) = (middle(band), middle(band + 1));
            *place = match band + 1 < bands && bin > low {
                true => (band, (bin - low) / (high - low)),
                false => (band, 0.0),
            };
        }
    }
}

impl Processor for Vocoder {
    type Update = VocoderState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .side_audio_input(Self::SIDECHAIN)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.aim();
    }

    fn update(&mut self, update: &mut VocoderState) {
        self.state.set_record(update, 1.0);
        self.aim();
    }

    fn latency(&self) -> u32 {
        Stft::latency_of(SIZE) as u32
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if self.state.follow(context, 1.0).is_some() {
            self.aim();
        }
        let inputs = &context.audio_inputs;
        let input = inputs.get(Self::INPUT);
        let key = match inputs.is_connected(Self::SIDECHAIN) {
            true => inputs.get(Self::SIDECHAIN),
            false => input,
        };
        let silent = input.iter().chain(&key).all(|samples| all_held_silent(samples));
        if !silent {
            if self.quiet >= REST_FRAMES {
                self.rest();
            }
            self.quiet = 0;
        } else if self.quiet >= REST_FRAMES {
            // Silence in, and the transforms hold nothing: the output is silent already.
            return;
        } else {
            self.quiet += context.frames;
        }
        let Self {
            layout,
            release,
            channels,
            state,
            ..
        } = self;
        let mix = state.mix;
        let outputs = context.audio_outputs.get(Self::OUTPUT);
        let channels = channels.iter_mut().zip(outputs).zip(input.iter().zip(&key));
        for ((channel, output), (input, key)) in channels {
            let frames = output.iter_mut().zip(input.iter().zip(key.iter()));
            let Channel { stft, levels } = channel;
            for (out, (input, key)) in frames {
                *out = stft.next_with_side(held(*input), held(*key), |track, voice| {
                    levels.vocode(layout, *release, mix, track, voice)
                });
            }
        }
    }
}
