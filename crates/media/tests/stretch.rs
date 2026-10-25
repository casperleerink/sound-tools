#![allow(clippy::unwrap_used)]
//! Regions with warp points, played as a parent plays them: one player for the regions of one
//! place, in a song at 120 bpm, where a beat is half a second. Through `Engine::process_block`,
//! which the realtime sanitizer checks when the build sets `RTSAN_ENABLE=1`.

use std::f64::consts::TAU;
use std::path::Path;
use std::sync::Arc;

use sound_core::{
    Assets, Connection, Engine, EngineConfig, EngineControl, Node, Tempo, TempoMap, Ticks,
    TimeSignature,
};
use sound_media::analysis::{Pitch, PitchSettings, PitchTracker};
use sound_media::{AudioAsset, AudioPlayer, AudioUpdate, Periods, Region, WarpPoint};

const RATE: u32 = 48_000;
const BEAT: u64 = 960;
/// A beat at 120 bpm, in frames.
const BEAT_FRAMES: usize = 24_000;

/// A float WAV at 48 kHz under `assets/audio/` of `folder`, `seconds` long.
fn file(folder: &Path, name: &str, seconds: f64, sound: impl Fn(f64) -> f64) {
    let path = folder.join("assets/audio").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..(seconds * f64::from(RATE)) as usize {
        let sample = sound(frame as f64 / f64::from(RATE)) as f32;
        writer.write_sample(sample).unwrap();
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
}

/// A tone at `hz` with a few harmonics, as a voice or a string has.
fn tone(hz: f64) -> impl Fn(f64) -> f64 {
    move |time| {
        let phase = TAU * hz * time;
        0.3 * phase.sin() + 0.15 * (2.0 * phase).sin() + 0.08 * (3.0 * phase).sin()
    }
}

fn region(asset: &str, start: u64, warp: &[(u64, f64)]) -> Region {
    Region {
        asset: AudioAsset::try_from(asset.to_string()).unwrap(),
        start: Ticks(start),
        file_start_seconds: 0.0,
        file_end_seconds: None,
        gain: 1.0,
        fade_in_ms: 0.0,
        fade_out_ms: 0.0,
        warp: (warp.iter())
            .map(|(ticks, seconds)| WarpPoint {
                ticks: Ticks(*ticks),
                seconds: *seconds,
            })
            .collect(),
    }
}

fn tempo(bpm: f64) -> TempoMap {
    let signature = TimeSignature::new(4, 4).unwrap();
    TempoMap::constant(signature, Tempo::from_bpm(bpm).unwrap())
}

/// A player of `regions` in an engine, playing from the start.
struct Song {
    control: EngineControl,
    engine: Engine,
}

impl Song {
    fn new(folder: &Path, regions: Vec<Region>) -> Self {
        let assets = Assets::new(folder);
        let (mut control, engine) = Engine::new(EngineConfig::new(RATE, 2));
        let regions = regions.into_iter().enumerate().map(|(index, region)| {
            let audio = sound_media::load(&assets, &region.asset).unwrap();
            let periods = (!region.warp.is_empty()).then(|| Arc::new(Periods::of(&audio)));
            (format!("region {index}"), region, periods)
        });
        let (snapshot, problems) = sound_media::snapshot(regions, &assets, RATE);
        assert_eq!(problems, Vec::<String>::new());
        let mut edit = control.edit();
        let node: Node<AudioPlayer> = edit.add_processor("regions", AudioPlayer::new()).unwrap();
        edit.update(node, AudioUpdate::new(snapshot)).unwrap();
        edit.connect(Connection::to_device(node.id(), AudioPlayer::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        control.set_tempo_map(tempo(120.0));
        control.play();
        Self { control, engine }
    }

    /// The left channel of the next `frames`, in blocks of `block` frames.
    fn play(&mut self, frames: usize, block: usize) -> Vec<f32> {
        let mut output = vec![0.0_f32; 2 * frames];
        for buffer in output.chunks_mut(2 * block) {
            self.engine.process_block(buffer);
            self.control.poll().unwrap();
        }
        output.iter().step_by(2).copied().collect()
    }
}

/// The frequency of a sound, from the first and the last of its rising zero crossings.
fn frequency(samples: &[f32]) -> f64 {
    let crossings: Vec<f64> = (samples.windows(2).enumerate())
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(frame, pair)| frame as f64 + f64::from(pair[0] / (pair[0] - pair[1])))
        .collect();
    let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
    (crossings.len() - 1) as f64 / (last - first) * f64::from(RATE)
}

/// The first frame from `from` on whose level goes over a tenth.
fn onset(samples: &[f32], from: usize) -> usize {
    from + (samples[from..].iter())
        .position(|sample| sample.abs() > 0.1)
        .unwrap()
}

/// The largest step from one sample to the next in `samples`.
fn largest_step(samples: &[f32]) -> f32 {
    (samples.windows(2))
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

/// Without warp points a region plays its file sample for sample, as it did before they were
/// there.
#[test]
fn a_region_without_warp_points_plays_its_file_sample_for_sample() {
    let folder = tempfile::tempdir().unwrap();
    let noise = |time: f64| ((time * 48_000.0 * 7_919.0) % 1_000.0) / 1_000.0 - 0.5;
    file(folder.path(), "noise.wav", 1.0, noise);
    let mut song = Song::new(folder.path(), vec![region("noise.wav", BEAT, &[])]);
    let played = song.play(2 * RATE as usize, 512);
    let file: Vec<f32> = (0..RATE as usize)
        .map(|frame| noise(frame as f64 / 48_000.0) as f32)
        .collect();
    // Past the ramp at its start and before the one at its end.
    let ramp = 96;
    let inside = BEAT_FRAMES + ramp..BEAT_FRAMES + RATE as usize - ramp;
    assert_eq!(played[inside], file[ramp..RATE as usize - ramp]);
}

/// A tone stretched to twice its length, or squeezed to half, keeps its pitch and lasts as
/// long as its point says.
#[test]
fn a_tone_stretched_or_squeezed_keeps_its_pitch_and_lasts_as_long_as_its_point_says() {
    let folder = tempfile::tempdir().unwrap();
    file(folder.path(), "tone.wav", 2.0, tone(220.0));
    // Two seconds of the file over eight beats, four seconds, and over two beats, one second.
    for beats in [8, 2] {
        let warped = region("tone.wav", 0, &[(beats * BEAT, 2.0)]);
        let mut song = Song::new(folder.path(), vec![warped]);
        let length = beats as usize * BEAT_FRAMES;
        let played = song.play(length + RATE as usize / 2, 512);
        let heard = frequency(&played[RATE as usize / 10..length - RATE as usize / 10]);
        let cents = 1200.0 * (heard / 220.0).log2();
        assert!(cents.abs() < 3.0, "{beats} beats: {cents} cents off");
        // Loud up to its end, then silent after its ramp.
        let last_loud = played
            .iter()
            .rposition(|sample| sample.abs() > 0.05)
            .unwrap();
        assert!(
            length - last_loud < 500,
            "{beats} beats: {last_loud} for {length}"
        );
        assert!(played[length + 100..].iter().all(|sample| *sample == 0.0));
        // And it is the tone it was: a pitch tracker reads the note it read in the file.
        let mut tracker = PitchTracker::new(f64::from(RATE), PitchSettings::FULL);
        tracker.push(played[RATE as usize / 4..RATE as usize / 2].iter().copied());
        let Pitch::Note(note) = tracker.pitch() else {
            panic!("{beats} beats: {:?}", tracker.pitch());
        };
        assert!((note - 57.0).abs() < 0.05, "{beats} beats: {note}");
    }
}

/// A period of the bursts: a head is never further than that from where the file should be.
const PERIOD: i64 = 48_000 / 440 + 1;

/// Bursts of a tone every half second of the file, which its points pin to beats of their own.
fn bursts(folder: &Path) {
    let burst = tone(440.0);
    file(folder, "bursts.wav", 2.0, move |time| {
        match time % 0.5 < 0.1 {
            true => burst(time),
            false => 0.0,
        }
    });
}

/// Each burst starts on the frame of the tick its point pins its second to, within a period of
/// its tone.
#[test]
fn a_sound_starts_on_the_tick_its_point_says() {
    let folder = tempfile::tempdir().unwrap();
    bursts(folder.path());
    // The bursts at 0.5, 1 and 1.5 s land on beats 1, 3 and 3.5 of the region, which starts
    // on beat 1 of the song.
    let points = [
        (BEAT, 0.5),
        (3 * BEAT, 1.0),
        (7 * BEAT / 2, 1.5),
        (4 * BEAT, 2.0),
    ];
    let mut song = Song::new(folder.path(), vec![region("bursts.wav", BEAT, &points)]);
    let played = song.play(6 * BEAT_FRAMES, 512);
    for (ticks, _) in &points[..3] {
        let expected = (BEAT + ticks) as usize * BEAT_FRAMES / BEAT as usize;
        let heard = onset(&played, expected - BEAT_FRAMES / 8);
        let late = heard as i64 - expected as i64;
        assert!(late.abs() < PERIOD, "{late} frames off at {ticks}");
    }
}

/// A tempo change moves the sound of a region with warp points, which are in ticks, and not
/// the sound of one without, which plays at the speed of its file.
#[test]
fn a_tempo_change_moves_a_warped_region_and_not_one_without_warp_points() {
    let folder = tempfile::tempdir().unwrap();
    bursts(folder.path());
    // Where the second burst, at 0.5 s of the file, starts: after the first one ended.
    let onsets = |warp: &[(u64, f64)], bpm: f64| {
        let mut song = Song::new(folder.path(), vec![region("bursts.wav", 0, warp)]);
        song.control.set_tempo_map(tempo(bpm));
        let played = song.play(5 * BEAT_FRAMES, 512);
        let slower = if warp.is_empty() { 1.0 } else { 120.0 / bpm };
        onset(&played, (0.3 * slower * f64::from(RATE)) as usize)
    };
    // Pinned to beat 1 of the region at either tempo.
    let warp = [(4 * BEAT, 2.0)];
    let (warped, slower) = (onsets(&warp, 120.0), onsets(&warp, 60.0));
    assert!(
        (warped as i64 - BEAT_FRAMES as i64).abs() < PERIOD,
        "{warped}"
    );
    assert!(
        (slower as i64 - 2 * BEAT_FRAMES as i64).abs() < PERIOD,
        "{slower}"
    );
    let (plain, slower) = (onsets(&[], 120.0), onsets(&[], 60.0));
    assert_eq!(plain, slower);
}

/// A tone with a vibrato that moves, a run of noise, then a lower tone: what a voice does.
fn phrase(folder: &Path) {
    let (first, second) = (tone(220.0), tone(165.0));
    file(folder, "phrase.wav", 2.0, move |time| {
        let wobble = 0.004 * (TAU * 5.0 * time).sin();
        if time < 0.8 {
            first(time * (1.0 + wobble))
        } else if time < 1.0 {
            0.2 * (((time * 48_000.0 * 7_919.0) % 1_000.0) / 500.0 - 1.0)
        } else {
            second(time)
        }
    });
}

/// From a seek into the middle of a stretched region, it plays what playing through played
/// there, once the ramp after the jump is over, in any blocks.
#[test]
fn a_seek_into_a_warped_region_plays_what_playing_through_plays() {
    let folder = tempfile::tempdir().unwrap();
    phrase(folder.path());
    let warped = || vec![region("phrase.wav", 0, &[(3 * BEAT, 0.7), (6 * BEAT, 2.0)])];
    let mut song = Song::new(folder.path(), warped());
    let through = song.play(6 * BEAT_FRAMES, 512);
    for (from, block) in [(5 * BEAT / 2, 64), (4 * BEAT + 17, 333)] {
        let mut song = Song::new(folder.path(), warped());
        song.control.seek(Ticks(from));
        let played = song.play(BEAT_FRAMES, block);
        let at = from as usize * BEAT_FRAMES / BEAT as usize;
        let ramp = 96;
        let worst = (played[ramp..].iter().zip(&through[at + ramp..]))
            .map(|(played, through)| (played - through).abs())
            .fold(0.0, f32::max);
        assert!(worst < 1e-6, "from {from}: {worst}");
    }
}

/// A loop over a stretched region goes back with no step in the sound, and every time round
/// plays the same.
#[test]
fn a_loop_over_a_warped_region_does_not_click_and_repeats_the_same() {
    let folder = tempfile::tempdir().unwrap();
    file(folder.path(), "tone.wav", 2.0, tone(220.0));
    let warped = region("tone.wav", 0, &[(8 * BEAT, 2.0)]);
    let mut song = Song::new(folder.path(), vec![warped]);
    // A loop of a beat and a third, which ends in the middle of a period.
    let (from, to) = (BEAT, 7 * BEAT / 3);
    song.control.set_loop(Some(Ticks(from)..Ticks(to)));
    let length = (to - from) as usize * BEAT_FRAMES / BEAT as usize;
    let played = song.play(BEAT_FRAMES + 4 * length, 512);
    let steady = largest_step(&played[BEAT_FRAMES / 2..BEAT_FRAMES]);
    let after_loop = largest_step(&played[BEAT_FRAMES..]);
    assert!(
        after_loop < 1.2 * steady,
        "{after_loop} at the loop, {steady} steady"
    );
    let start = BEAT_FRAMES + length;
    let (second, third) = (start..start + length, start + length..start + 2 * length);
    assert_eq!(played[second], played[third]);
}

/// Where one stretched region ends and the next starts, the level holds: the one player
/// crossfades them as it does regions without warp points.
#[test]
fn two_warped_regions_that_meet_hand_over_with_no_dip() {
    let folder = tempfile::tempdir().unwrap();
    file(folder.path(), "level.wav", 1.0, |_| 0.25);
    for end in [0.3, 0.9] {
        let first = region("level.wav", 0, &[(BEAT, end)]);
        let second = region("level.wav", BEAT, &[(2 * BEAT, 0.9)]);
        let mut song = Song::new(folder.path(), vec![first, second]);
        let played = song.play(3 * BEAT_FRAMES, 512);
        let join = &played[BEAT_FRAMES / 2..3 * BEAT_FRAMES / 2];
        let lowest = join.iter().fold(f32::MAX, |low, sample| low.min(*sample));
        assert!(lowest > 0.25 * 0.95, "{lowest} at the join after {end} s");
    }
}

/// How alike `samples` is to itself `lag` frames later, from -1 to 1.
fn alike(samples: &[f32], lag: usize) -> f64 {
    let pairs = samples.iter().zip(&samples[lag..]);
    let together: f64 = pairs.map(|(a, b)| f64::from(*a) * f64::from(*b)).sum();
    let power: f64 = samples.iter().map(|a| f64::from(*a).powi(2)).sum();
    together / power
}

/// Noise has no clear pitch, so it is spliced every 10 ms, as drums or a mix would be: not at
/// the period of a note, which would repeat it every few milliseconds and buzz at that pitch.
#[test]
fn noise_stretched_is_spliced_every_ten_milliseconds() {
    let folder = tempfile::tempdir().unwrap();
    // Splitmix64 of the frame: white, the same every run.
    let noise = |time: f64| {
        let mut bits = ((time * 48_000.0).round() as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
        bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        bits ^= bits >> 31;
        0.5 * ((bits >> 11) as f64 / (1_u64 << 53) as f64 - 0.5)
    };
    file(folder.path(), "noise.wav", 1.0, noise);
    // A second over four beats: half its speed, so half of what plays is played twice.
    let warped = region("noise.wav", 0, &[(4 * BEAT, 1.0)]);
    let mut song = Song::new(folder.path(), vec![warped]);
    let played = song.play(4 * BEAT_FRAMES, 512);
    let middle = &played[BEAT_FRAMES..3 * BEAT_FRAMES];
    let at_a_note = alike(middle, 48_000 / 440);
    let at_ten_milliseconds = alike(middle, 480);
    assert!(
        at_a_note.abs() < 0.05,
        "{at_a_note} at the period of 440 Hz"
    );
    assert!(at_ten_milliseconds > 0.2, "{at_ten_milliseconds} at 10 ms");
}
