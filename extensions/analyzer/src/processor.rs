//! The analyzer processor: the input goes out as it came, bit for bit, into the scope the card
//! reads and into the peaks of its meter.

use sound_core::{
    AudioInput, AudioOutput, Peaks, Ports, PrepareConfig, ProcessContext, Processor, Scope,
    all_positive_zero,
};

pub struct Analyzer {
    scope: Scope,
    peaks: Peaks,
    /// Silent frames in a row written to the scope, up to `rest_frames`: a second, longer than
    /// the 400 ms the card measures loudness over, so its loudness comes down to silence too.
    /// From there a silent block has nothing to do.
    quiet: usize,
    rest_frames: usize,
}

impl Analyzer {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    /// The names the behaviour keeps the scope and the peaks under, for
    /// [`sound_core::Project::scope`] and [`sound_core::Project::peaks`].
    pub const SCOPE: &str = "sound";
    pub const PEAKS: &str = "level";

    pub fn new(scope: Scope, peaks: Peaks) -> Self {
        Self {
            scope,
            peaks,
            quiet: 0,
            rest_frames: 0,
        }
    }
}

impl Processor for Analyzer {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.rest_frames = config.sample_rate as usize;
        self.quiet = 0;
    }

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let inputs = context.audio_inputs.get(Self::INPUT);
        // An output starts as +0.0, so a block of +0.0 in is already the same out.
        if inputs.iter().all(|samples| all_positive_zero(samples)) {
            if self.quiet < self.rest_frames {
                self.quiet += context.frames;
                self.scope.write(inputs);
            }
            return;
        }
        self.quiet = 0;
        let outputs = context.audio_outputs.get(Self::OUTPUT);
        for (output, input) in outputs.into_iter().zip(inputs) {
            output.copy_from_slice(input);
        }
        self.scope.write(inputs);
        self.peaks.record_block(inputs);
    }
}
