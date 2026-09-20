#![allow(clippy::unwrap_used)]
//! The click on exact frames: every onset is on the frame of a beat tick of the tempo map and
//! nowhere else, the downbeat differs from the other beats, and a stop or a seek leaves
//! nothing hanging.

use metronome::{BEAT_HZ, CLICK_SECONDS, Click, DOWNBEAT_HZ, LEVEL};
use sound_core::{
    CHANNELS, Clock, Engine, EngineConfig, EngineControl, Frames, Tempo, TempoChange, TempoMap,
    Ticks,
};

const SAMPLE_RATE: u32 = 48_000;
/// One device buffer. A transport command lands at the start of the next one.
const BUFFER: usize = 512;

fn click_frames() -> usize {
    (CLICK_SECONDS * SAMPLE_RATE as f32) as usize
}

/// The fade after a stop, plus the buffer the command waits for.
fn settled() -> usize {
    BUFFER + (0.002 * SAMPLE_RATE as f32) as usize
}

fn tempo_map(time_signature: &str, changes: &[(u64, f64)]) -> TempoMap {
    let changes = changes
        .iter()
        .map(|&(tick, bpm)| TempoChange {
            tick: Ticks(tick),
            bpm: Tempo::from_bpm(bpm).unwrap(),
        })
        .collect();
    TempoMap::new(time_signature.parse().unwrap(), changes).unwrap()
}

/// An engine with the click attached and a tempo map set. The click starts off.
fn engine_with_click(tempo_map: TempoMap) -> (EngineControl, Engine, Click) {
    let (mut control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, CHANNELS));
    let click = Click::attach(&mut control).unwrap();
    control.set_tempo_map(tempo_map);
    (control, engine, click)
}

/// The left channel of `frames` frames, rendered in device buffers.
fn render(control: &mut EngineControl, engine: &mut Engine, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0_f32; frames * CHANNELS];
    for buffer in output.chunks_mut(BUFFER * CHANNELS) {
        engine.process_block(buffer);
        control.poll().unwrap();
    }
    output.into_iter().step_by(CHANNELS).collect()
}

/// The frame of every click onset: the first sample of a burst after a run of silence. A burst
/// may hold one exact zero where its sine crosses, so a short run is not a new onset.
fn onsets(samples: &[f32]) -> Vec<u64> {
    let mut onsets = Vec::new();
    let mut silent_frames = usize::MAX;
    for (frame, sample) in samples.iter().enumerate() {
        if *sample == 0.0 {
            silent_frames = silent_frames.saturating_add(1);
            continue;
        }
        if silent_frames > 2 {
            onsets.push(frame as u64);
        }
        silent_frames = 0;
    }
    onsets
}

/// Every beat that starts inside `frames`, as its frame and whether it is a downbeat.
fn beats(clock: &Clock, frames: u64) -> Vec<(u64, bool)> {
    let time_signature = clock.tempo_map().time_signature();
    let (per_beat, per_bar) = (
        time_signature.ticks_per_beat(),
        time_signature.ticks_per_bar(),
    );
    let last = clock.tick_at(Frames(frames)).0;
    (0..)
        .map(|beat| beat * per_beat)
        .take_while(|tick| *tick < last)
        .map(|tick| (clock.frame_of(Ticks(tick)).0, tick.is_multiple_of(per_bar)))
        .collect()
}

/// How often a burst crosses zero downwards: its pitch, measured from the samples.
fn zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| pair[0] >= 0.0 && pair[1] < 0.0)
        .count()
}

fn check_beats(time_signature: &str, changes: &[(u64, f64)], seconds: u32) {
    let frames = (seconds * SAMPLE_RATE) as usize;
    let map = tempo_map(time_signature, changes);
    let clock = Clock::new(map.clone(), SAMPLE_RATE);
    let (mut control, mut engine, mut click) = engine_with_click(map);
    click.set_on(&mut control, true).unwrap();
    control.play();
    let samples = render(&mut control, &mut engine, frames);

    let expected = beats(&clock, frames as u64);
    let frames_of_beats: Vec<u64> = expected.iter().map(|(frame, _)| *frame).collect();
    assert_eq!(
        onsets(&samples),
        frames_of_beats,
        "{time_signature} with {changes:?}"
    );
    assert!(expected.len() > 8, "too few beats to be a real check");

    // The first beat of a bar is a higher pitch than the rest, and only the pitch differs.
    for (frame, downbeat) in &expected {
        let start = *frame as usize;
        let Some(burst) = samples.get(start..start + click_frames()) else {
            continue;
        };
        assert!(
            (burst[0] - LEVEL).abs() < 1e-6,
            "a click starts at its peak, not at {}",
            burst[0]
        );
        let pitch = if *downbeat { DOWNBEAT_HZ } else { BEAT_HZ };
        let cycles = (pitch * CLICK_SECONDS) as usize;
        let crossings = zero_crossings(burst);
        assert!(
            crossings.abs_diff(cycles) <= 1,
            "{crossings} zero crossings at frame {frame}: {pitch} Hz means about {cycles}"
        );
    }
}

#[test]
fn every_click_is_on_the_frame_of_its_beat_over_several_tempo_changes() {
    // 120 bpm, then 60 from bar 3, then 93.5 from bar 5. A bar of 4/4 is 3840 ticks.
    check_beats("4/4", &[(0, 120.0), (2 * 3840, 60.0), (4 * 3840, 93.5)], 20);
}

#[test]
fn a_waltz_clicks_three_beats_a_bar() {
    check_beats("3/4", &[(0, 100.0), (3 * 2880, 140.0)], 10);
}

#[test]
fn six_eight_clicks_the_eighth_notes() {
    // A beat is the note value of the lower number, so 6/8 is six clicks per 2880-tick bar.
    check_beats("6/8", &[(0, 90.0), (2 * 2880, 150.0)], 10);
}

#[test]
fn the_click_is_silent_while_it_is_off() {
    let (mut control, mut engine, click) = engine_with_click(TempoMap::default());
    assert!(!click.is_on(), "a click starts off");
    control.play();
    let samples = render(&mut control, &mut engine, SAMPLE_RATE as usize);
    assert!(samples.iter().all(|sample| *sample == 0.0));
}

#[test]
fn a_stop_leaves_no_click_hanging_and_a_seek_clicks_the_beats_it_lands_on() {
    let map = tempo_map("4/4", &[(0, 120.0)]);
    let clock = Clock::new(map.clone(), SAMPLE_RATE);
    let (mut control, mut engine, mut click) = engine_with_click(map);
    click.set_on(&mut control, true).unwrap();
    control.play();

    // Stop a few frames into the click of beat 3, the worst moment to cut one.
    let beat = clock.frame_of(Ticks(2 * 960)).0 as usize;
    render(&mut control, &mut engine, beat + 64);
    control.stop();
    let after = render(&mut control, &mut engine, SAMPLE_RATE as usize);
    assert!(
        after[settled()..].iter().all(|sample| *sample == 0.0),
        "the click went on after the stop"
    );

    // A seek while stopped starts nothing.
    control.seek(Ticks(8 * 3840));
    let stopped = render(&mut control, &mut engine, SAMPLE_RATE as usize);
    assert!(stopped.iter().all(|sample| *sample == 0.0));

    // Playing from there clicks the downbeat it landed on, in the first block.
    control.play();
    let played = render(&mut control, &mut engine, SAMPLE_RATE as usize);
    assert_eq!(onsets(&played).first().copied(), Some(0));
}

#[test]
fn switching_the_click_off_ends_the_sound_and_starts_no_more() {
    let (mut control, mut engine, mut click) = engine_with_click(tempo_map("4/4", &[(0, 120.0)]));
    click.set_on(&mut control, true).unwrap();
    control.play();
    let playing = render(&mut control, &mut engine, SAMPLE_RATE as usize / 2);
    assert!(!onsets(&playing).is_empty());
    click.set_on(&mut control, false).unwrap();
    assert!(!click.is_on());
    let after = render(&mut control, &mut engine, SAMPLE_RATE as usize);
    assert!(after[settled()..].iter().all(|sample| *sample == 0.0));
}
