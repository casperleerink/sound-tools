//! A meter a behaviour declares as the end of a connection, `{ meter: name }`: it keeps the
//! level and the last sound of what reaches it, for live values and waves to show.

use sound_core::{AudioInput, Peaks, Ports, PrepareConfig, ProcessContext, Processor, Scope};

pub(crate) struct Meter {
    peaks: Peaks,
    scope: Scope,
}

impl Meter {
    pub(crate) const INPUT: AudioInput = AudioInput::new(0);

    pub(crate) fn new(peaks: Peaks, scope: Scope) -> Self {
        Self { peaks, scope }
    }
}

impl Processor for Meter {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_input(Self::INPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let inputs = context.audio_inputs.get(Self::INPUT);
        self.peaks.record_block(inputs);
        self.scope.write(inputs);
    }
}
