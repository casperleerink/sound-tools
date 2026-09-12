//! Small rendering workload for build-loop timing, not an audio engine.
pub const GAIN: f32 = 0.20;

pub fn render(output: &mut [f32], frequency: f32, sample_rate: f32) {
    let mut phase = 0.0_f32;
    let mut filtered = 0.0_f32;
    let step = frequency / sample_rate;
    for sample in output {
        let oscillator = (phase * std::f32::consts::TAU).sin();
        filtered += 0.15 * (oscillator - filtered);
        *sample = filtered * GAIN;
        phase = (phase + step).fract();
    }
}

pub fn verify_render() {
    let mut output = vec![0.0; 48_000];
    render(
        std::hint::black_box(&mut output),
        std::hint::black_box(220.0),
        48_000.0,
    );
    assert!(output
        .iter()
        .all(|sample| sample.is_finite() && sample.abs() <= GAIN));
    let energy: f32 = output.iter().map(|sample| sample * sample).sum();
    assert!(energy > 1.0);
    eprintln!("DSP gain={GAIN:.2} energy={energy:.6}");
}
