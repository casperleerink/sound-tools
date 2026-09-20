//! The same track on the real output device. CI has no device, so this is run by hand.

use std::time::{Duration, Instant};

use instrument::SynthState;
use sound_core::{EngineConfig, OutputDevice};

use crate::support::{Harness, id, note};

/// Run with `cargo nextest run -p instrument --run-ignored only real_device --no-capture`.
/// It plays a rising C major arpeggio and then a chord, for four seconds, and prints what the
/// device reported. Halfway a cutoff edit arrives the way an agent's file edit does.
#[test]
#[ignore = "needs an audio device and makes sound"]
fn a_track_plays_the_synth_on_the_real_device() {
    let device = OutputDevice::default_output().unwrap();
    let config = EngineConfig::new(device.sample_rate(), device.channels());
    println!(
        "device: {} Hz, {} channels",
        config.sample_rate, config.channels
    );
    let mut harness = Harness::with_config(config);

    let arpeggio = [60, 64, 67, 72].into_iter().enumerate();
    let mut notes: Vec<_> = arpeggio
        .map(|(step, pitch)| note(step as u64 * 480, 480, pitch, 100))
        .collect();
    notes.extend([60, 64, 67, 72].map(|pitch| note(1_920, 3_840, pitch, 90)));
    harness.add_track("track", notes, SynthState::default());

    // The folder binding keeps the temporary folder until the end of the test.
    let Harness {
        mut project,
        engine,
        folder: _folder,
    } = harness;
    let stream = device.start(engine).unwrap();
    project.engine().play();
    let started = Instant::now();
    let mut edited = false;
    while started.elapsed() < Duration::from_secs(4) {
        project.engine().poll().unwrap();
        if !edited && started.elapsed() > Duration::from_secs(2) {
            let synth = project
                .resolve::<SynthState>(&id("track/instrument"))
                .unwrap();
            let mut edit = project.begin("Open the filter");
            project
                .update(&mut edit, &synth, |state| state.cutoff_hz = 6_000.0)
                .unwrap();
            project.finish(edit).unwrap();
            edited = true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let engine_status = project.engine().poll().unwrap();
    let status = stream.status();
    let errors = stream.take_errors();
    drop(stream);
    println!(
        "callbacks: {}, frames: {}",
        engine_status.blocks, engine_status.frames
    );
    println!(
        "xruns: {}, late callbacks: {}, slowest callback: {:?}",
        status.xruns, status.late_callbacks, status.slowest_callback
    );
    println!(
        "event overflows: {}, port misuses: {}, stream errors: {}",
        engine_status.event_overflows,
        engine_status.port_misuses,
        errors.len()
    );
    assert_eq!(status.xruns, 0);
    assert_eq!(
        engine_status.event_overflows + engine_status.port_misuses,
        0
    );
    assert!(errors.is_empty());
}
