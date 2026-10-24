//! How much faster than realtime many vocoders render.

use std::time::Instant;

use sound_core::{Connection, Engine, EngineConfig};
use vocoder::{Vocoder, VocoderState};

use crate::support::{SAMPLE_RATE, Signal, Source, noise, silence};

const VOCODERS: usize = 100;
const SECONDS: usize = 10;

/// 100 vocoders, each keyed, with 32 bands and the formant moved, so every part of one runs.
fn render(label: &str, track: impl Fn() -> Signal, voice: impl Fn() -> Signal) {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    let state = VocoderState {
        bands: 32.0,
        formant: 3.0,
        ..VocoderState::default()
    };
    for index in 0..VOCODERS {
        let track = edit.add_processor(&format!("track-{index}"), Source::new(track()));
        let voice = edit.add_processor(&format!("voice-{index}"), Source::new(voice()));
        let vocoder = Vocoder::new(state);
        let vocoder = edit
            .add_processor(&format!("vocoder-{index}"), vocoder)
            .unwrap();
        let (track, voice) = (track.unwrap(), voice.unwrap());
        let connections = [
            Connection::new(track.id(), Source::OUTPUT, vocoder.id(), Vocoder::INPUT),
            Connection::new(voice.id(), Source::OUTPUT, vocoder.id(), Vocoder::SIDECHAIN),
            Connection::to_device(vocoder.id(), Vocoder::OUTPUT, 0),
        ];
        for connection in connections {
            edit.connect(connection).unwrap();
        }
    }
    edit.commit().unwrap();
    let mut output = vec![0.0; SECONDS * SAMPLE_RATE as usize * 2];
    let started = Instant::now();
    for buffer in output.chunks_mut(512 * 2) {
        engine.process_block(buffer);
    }
    let elapsed = started.elapsed().as_secs_f64();
    let ratio = SECONDS as f64 / elapsed;
    println!("100 vocoders, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime");
}

/// Run with
/// `cargo nextest run -p vocoder --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_vocoders() {
    render("playing", || noise(0.25), || noise(0.25));
    render("idle", silence, silence);
}
