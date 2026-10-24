//! Correcting inside `Engine::process_block`, which the realtime sanitizer checks when the build
//! sets `RTSAN_ENABLE=1`: no allocation, lock or system call. Every part runs here: both ways of
//! reading, formants, held notes, a change of voice range and a sample rate other than 48 kHz.

use pitch_correction::{PitchCorrectionState, Voice};

use crate::support::{Rig, SAMPLE_RATE, held, voice};

#[test]
fn correcting_runs_on_the_audio_thread_without_allocating_locking_or_calling_the_system() {
    // From a third under A3 up past it, so the shift reads both slower and faster.
    let glide = |time: f64| Some(56.6 + 0.8 * (time % 1.0));
    let state = PitchCorrectionState {
        retune_ms: 0.0,
        keep_formants: true,
        ..PitchCorrectionState::default()
    };
    let notes = [held(59, 0.5, 0.9), held(55, 0.7, 1.4)].concat();
    let mut rig = Rig::with(state, voice(glide), false, notes, false);
    let output = rig.render(SAMPLE_RATE as usize);
    let state = PitchCorrectionState {
        voice: Voice::High,
        mix: 0.5,
        ..state
    };
    rig.control.update(rig.correction, state).unwrap();
    let output = [output, rig.render(SAMPLE_RATE as usize)].concat();
    rig.control.poll().unwrap();
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(output.iter().any(|sample| sample.abs() > 0.1));
}
