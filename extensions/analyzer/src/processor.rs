//! The analyzer processor: the input goes out as it came, bit for bit, and into the scope the
//! card reads.

use sound_core::{
    AudioInput, AudioOutput, Ports, PrepareConfig, ProcessContext, Processor, Scope,
    all_positive_zero,
};

/// How long silence still goes into the scope once the sound stops: longer than the 400 ms the
/// loudness of the card is measured over, so it comes down to silence too.
const REST_SECONDS: u32 = 1;

pub struct Analyzer {
    scope: Scope,
    /// Silent frames in a row written to the scope, up to `rest_frames`. From there the scope
    /// holds only silence, and a silent block has nothing to do.
    quiet: usize,
    rest_frames: usize,
}

impl Analyzer {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    /// The name the behaviour keeps the scope under, for [`sound_core::Project::scope`].
    pub const SCOPE: &str = "sound";

    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
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
        let rest = (REST_SECONDS * config.sample_rate) as usize;
        self.rest_frames = rest.max(Scope::FRAMES);
        self.quiet = 0;
    }

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let inputs = context.audio_inputs.get(Self::INPUT);
        // An output starts as +0.0, so a block of +0.0 in is already the same out.
        let silent = inputs.iter().all(|samples| all_positive_zero(samples));
        if silent && self.quiet >= self.rest_frames {
            return;
        }
        self.quiet = match silent {
            true => self.quiet.saturating_add(context.frames),
            false => 0,
        };
        let outputs = context.audio_outputs.get(Self::OUTPUT);
        for (output, input) in outputs.into_iter().zip(inputs) {
            output.copy_from_slice(input);
        }
        self.scope.write(inputs);
    }
}
