//! The card of every built-in device, in a rack of the SDK (`crate::rack`) as a chord plays
//! through it:
//!
//! - `cards-effects.png`: the Filter, the Compressor pressed by the chord and the EQ.
//! - `cards-effects-expanded.png`: the same three expanded.
//! - `cards-dynamics.png`: the Saturator driven 18 dB into its tube curve, the Limiter pushing
//!   the chord into a ceiling of -6 dB, and the Gate.
//! - `cards-time.png`, `cards-time-expanded.png`: the Delay synced and dotted, and the Reverb.
//! - `cards-tools.png`, `cards-tools-expanded.png`: the Modulation, the Utility with bass mono
//!   on, and the Vocoder with its formant moved.
//! - `cards-analyzer.png`: the Analyzer hearing the chord.
//! - `cards-pitch.png`, `cards-pitch-expanded.png`: the Pitch correction moving an F down to
//!   the E of C major pentatonic.
//! - `cards-drums.png`, `cards-drums-expanded.png`: the Drum pad playing a beat.
//! - `cards-sampler.png`, `cards-sampler-expanded.png`: the Sampler playing a file.
//! - `cards-plugin.png`: the test plugin as the instrument and as an effect.

use std::path::Path;

use anyhow::Result;
use gpui::HeadlessAppContext;
use plugin_host::PluginFormat;
use sound_core::Ticks;

use super::{Opened, Save, test_plugin};
use crate::rack::{notes, rack_files, record};

/// A chord held for a bar.
fn chord() -> String {
    notes(3840, &[(0, 3840, 48), (0, 3840, 55), (0, 3840, 64)])
}

/// A rack of the synth playing the chord into `effects`, playing.
fn rack(cx: &mut HeadlessAppContext, effects: &[(&str, &str)]) -> Result<Opened> {
    let (synth, chord) = (record("instrument.synth", r#"{"gain": 0.3}"#), chord());
    let mut children = vec![("instrument", synth.as_str()), ("chord", chord.as_str())];
    children.extend(effects.iter().copied());
    let mut opened = Opened::open(cx, &rack_files(&children))?;
    let problems = cx.update(|cx| opened.session.read(cx).project().problems());
    anyhow::ensure!(problems.is_empty(), "{problems:?}");
    opened.play_from(Ticks(0), cx)?;
    Ok(opened)
}

/// Expands the card of `rack/<name>`, a view of type `V` that has `set_expanded`.
macro_rules! expand {
    ($opened:expr, $cx:expr, $view:ty, $name:expr) => {{
        let card = $opened.card::<$view>(&format!("rack/{}", $name), $cx)?;
        $cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(true, cx)));
        $cx.run_until_parked();
    }};
}

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &Save) -> Result<()> {
    let compressor = record("compressor", r#"{"threshold_db": -30.0, "ratio": 4.0}"#);
    let effects = [
        (
            "filter",
            record("filter", r#"{"cutoff_hz": 1200.0, "resonance": 0.4}"#),
        ),
        ("compressor", compressor),
        (
            "eq",
            record("eq", r#"{"bands": [{}, {}, {"gain_db": 4.0}]}"#),
        ),
    ];
    let effects: Vec<(&str, &str)> = (effects.iter())
        .map(|(name, record)| (*name, record.as_str()))
        .collect();
    let opened = rack(cx, &effects)?;
    save(cx, opened.window.into(), "cards-effects")?;
    expand!(opened, cx, eq::view::EqView, "eq");
    expand!(opened, cx, compressor::view::CompressorView, "compressor");
    expand!(opened, cx, filter::view::FilterView, "filter");
    save(cx, opened.window.into(), "cards-effects-expanded")?;
    drop(opened);

    let dynamics = [
        (
            "saturator",
            record("saturator", r#"{"curve": "tube", "drive_db": 18.0}"#),
        ),
        (
            "limiter",
            record("limiter", r#"{"gain_db": 18.0, "ceiling_db": -6.0}"#),
        ),
        ("gate", record("gate", r#"{"threshold_db": -30.0}"#)),
    ];
    let dynamics: Vec<(&str, &str)> = (dynamics.iter())
        .map(|(name, record)| (*name, record.as_str()))
        .collect();
    let opened = rack(cx, &dynamics)?;
    save(cx, opened.window.into(), "cards-dynamics")?;
    drop(opened);

    let time = [
        (
            "delay",
            record("delay", r#"{"division": "1/8", "feel": "dotted"}"#),
        ),
        ("reverb", record("reverb", r#"{"mix": 0.3}"#)),
    ];
    let time: Vec<(&str, &str)> = (time.iter())
        .map(|(name, record)| (*name, record.as_str()))
        .collect();
    let opened = rack(cx, &time)?;
    save(cx, opened.window.into(), "cards-time")?;
    // From the bottom up: a card pushed off the window is not drawn, and has no view.
    expand!(opened, cx, reverb::view::ReverbView, "reverb");
    expand!(opened, cx, delay::view::DelayView, "delay");
    save(cx, opened.window.into(), "cards-time-expanded")?;
    drop(opened);

    let tools = [
        ("modulation", record("modulation", r#"{"mode": "flanger"}"#)),
        (
            "utility",
            record("utility", r#"{"width": 1.5, "bass_mono": true}"#),
        ),
        (
            "vocoder",
            record("vocoder", r#"{"bands": 32, "formant": 3.0}"#),
        ),
    ];
    let tools: Vec<(&str, &str)> = (tools.iter())
        .map(|(name, record)| (*name, record.as_str()))
        .collect();
    let opened = rack(cx, &tools)?;
    save(cx, opened.window.into(), "cards-tools")?;
    expand!(opened, cx, utility::view::UtilityView, "utility");
    expand!(opened, cx, modulation::view::ModulationView, "modulation");
    save(cx, opened.window.into(), "cards-tools-expanded")?;
    drop(opened);

    let opened = rack(cx, &[("analyzer", &record("analyzer", "{}"))])?;
    save(cx, opened.window.into(), "cards-analyzer")?;
    drop(opened);

    let tune = record(
        "pitch-correction",
        r#"{"scale": "major_pentatonic", "retune_ms": 0.0}"#,
    );
    let (synth, melody) = (
        record("instrument.synth", r#"{"gain": 0.3}"#),
        notes(3840, &[(0, 3840, 65)]),
    );
    let children = [
        ("instrument", synth.as_str()),
        ("melody", melody.as_str()),
        ("tune", tune.as_str()),
    ];
    let mut opened = Opened::open(cx, &rack_files(&children))?;
    opened.play_from(Ticks(0), cx)?;
    save(cx, opened.window.into(), "cards-pitch")?;
    expand!(
        opened,
        cx,
        pitch_correction::view::PitchCorrectionView,
        "tune"
    );
    save(cx, opened.window.into(), "cards-pitch-expanded")?;
    drop(opened);

    // A beat: kick on 1 and 3, snare on 2 and 4, hat on the eighths.
    let mut beat: Vec<(u64, u64, u8)> = (0..8).map(|eighth| (eighth * 480, 240, 42)).collect();
    beat.extend([
        (0, 240, 36),
        (1920, 240, 36),
        (960, 240, 38),
        (2880, 240, 38),
    ]);
    let (drums, beat) = (record("drum-pad", "{}"), notes(3840, &beat));
    let children = [("instrument", drums.as_str()), ("beat", beat.as_str())];
    let mut opened = Opened::open(cx, &rack_files(&children))?;
    opened.play_from(Ticks(0), cx)?;
    save(cx, opened.window.into(), "cards-drums")?;
    expand!(opened, cx, drum_pad::view::DrumPadView, "instrument");
    save(cx, opened.window.into(), "cards-drums-expanded")?;
    drop(opened);

    let sampler = record("sampler", r#"{"sample": "kalimba.wav", "root": 72}"#);
    let melody = notes(3840, &[(0, 960, 72), (960, 960, 76), (1920, 1920, 79)]);
    let children = [
        ("instrument", sampler.as_str()),
        ("melody", melody.as_str()),
    ];
    let mut opened = Opened::open_with(cx, &rack_files(&children), |root| write_kalimba(root))?;
    opened.play_from(Ticks(0), cx)?;
    save(cx, opened.window.into(), "cards-sampler")?;
    expand!(opened, cx, sampler::view::SamplerView, "instrument");
    save(cx, opened.window.into(), "cards-sampler-expanded")?;
    drop(opened);

    let (piano, warmth) = (
        test_plugin(PluginFormat::Clap, "piano"),
        test_plugin(PluginFormat::Vst3, "warmth"),
    );
    let chord = chord();
    let children = [
        ("instrument", piano.as_str()),
        ("chord", chord.as_str()),
        ("warmth", warmth.as_str()),
    ];
    let mut opened = Opened::open(cx, &rack_files(&children))?;
    opened.play_from(Ticks(0), cx)?;
    save(cx, opened.window.into(), "cards-plugin")?;
    Ok(())
}

/// Two seconds of a plucked C5, in `assets/audio/kalimba.wav`.
fn write_kalimba(root: &Path) -> Result<()> {
    let path = root.join("assets/audio/kalimba.wav");
    std::fs::create_dir_all(root.join("assets/audio"))?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec)?;
    for frame in 0..2 * 48_000 {
        let time = f64::from(frame) / 48_000.;
        let sample = (std::f64::consts::TAU * 523.25 * time).sin() * (-3. * time).exp() * 0.5;
        writer.write_sample(sample as f32)?;
    }
    writer.finalize()?;
    Ok(())
}
