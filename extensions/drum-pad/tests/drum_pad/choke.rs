//! The choke group: a closed or pedal hat cuts the open hat, fast and without a click. A new hit
//! of a pad cuts its last one the same way.

use drum_pad::{DrumPadState, FADE_SECONDS};

use crate::support::{Harness, QUARTER, SAMPLE_RATE, Stereo, hit, largest_step, note, peak};

const HAT: usize = 6;
const PEDAL_HAT: usize = 8;
const OPEN_HAT: usize = 10;

/// An eighth note at 120 bpm: where the second hit comes.
const EIGHTH: u64 = QUARTER / 2;
const EIGHTH_FRAMES: usize = 12_000;

fn render(notes: Vec<sound_notes::Note>, drums: DrumPadState) -> Stereo {
    let mut harness = Harness::with_track(notes, drums);
    harness.play(48_000)
}

/// What is left of the open hat once the other hat plays: the render of both less the render
/// of the other hat alone.
fn open_hat_part(cutter: usize, drums: &DrumPadState) -> Vec<f32> {
    let open = hit(0, note(OPEN_HAT), 110);
    let cut = hit(EIGHTH, note(cutter), 110);
    let both = render(vec![open, cut], drums.clone()).sum();
    let cutter = render(vec![cut], drums.clone()).sum();
    both.iter()
        .zip(cutter)
        .map(|(both, cutter)| both - cutter)
        .collect()
}

#[test]
fn a_closed_or_pedal_hat_cuts_the_open_hat_in_five_ms_without_a_click() {
    let drums = DrumPadState::default();
    let alone = render(vec![hit(0, note(OPEN_HAT), 110)], drums.clone()).sum();
    let fade = (FADE_SECONDS * SAMPLE_RATE as f32).round() as usize;
    for cutter in [HAT, PEDAL_HAT] {
        let part = open_hat_part(cutter, &drums);
        // Before the cut, the open hat as it is alone.
        assert_eq!(part[..EIGHTH_FRAMES], alone[..EIGHTH_FRAMES]);
        // Over the cut, the open hat faded out in a straight line, and nothing after it.
        let cut = &part[EIGHTH_FRAMES..];
        let silent_from = cut.iter().rposition(|sample| *sample != 0.0).unwrap() + 1;
        assert!(silent_from <= fade, "{silent_from}");
        for (frame, sample) in cut[..fade].iter().enumerate() {
            let expected = alone[EIGHTH_FRAMES + frame] * (fade - frame) as f32 / fade as f32;
            assert!(
                (sample - expected).abs() < 1e-6,
                "{frame}: {sample} {expected}"
            );
        }
        // No click: over the cut, no step between two frames is larger than the open hat
        // takes by itself there.
        let around = EIGHTH_FRAMES - 480..EIGHTH_FRAMES + fade;
        let (with_cut, by_itself) = (
            largest_step(&part[around.clone()]),
            largest_step(&alone[around]),
        );
        let level = peak(&alone[EIGHTH_FRAMES..EIGHTH_FRAMES + fade]);
        println!(
            "cut by pad {}: silent {:.2} ms after the hit, open hat at {level:.4} there; largest step {with_cut:.4}, {by_itself:.4} without the cut",
            note(cutter),
            silent_from as f64 * 1000.0 / f64::from(SAMPLE_RATE)
        );
        assert!(with_cut <= by_itself);
        // What the open hat would have played in the next 100 ms is gone.
        let rest = &alone[EIGHTH_FRAMES + fade..EIGHTH_FRAMES + 4_800];
        assert!(peak(rest) > 0.01);
    }
}

#[test]
fn a_pad_out_of_the_choke_group_rings_on() {
    let mut drums = DrumPadState::default();
    drums.pads[OPEN_HAT].choke = false;
    let alone = render(vec![hit(0, note(OPEN_HAT), 110)], drums.clone()).sum();
    assert_same(&open_hat_part(HAT, &drums), &alone);
    // A pad that is not in the group cuts nothing either.
    let mut drums = DrumPadState::default();
    drums.pads[HAT].choke = false;
    let alone = render(vec![hit(0, note(OPEN_HAT), 110)], drums.clone()).sum();
    assert_same(&open_hat_part(HAT, &drums), &alone);
}

/// The same, but for the rounding of a sum less one of its parts.
fn assert_same(part: &[f32], alone: &[f32]) {
    let difference = part
        .iter()
        .zip(alone)
        .fold(0.0_f32, |largest, (a, b)| largest.max((a - b).abs()));
    assert!(difference < 1e-6, "{difference}");
    assert!(peak(&alone[EIGHTH_FRAMES + 480..]) > 0.001);
}

#[test]
fn a_new_hit_of_a_pad_fades_its_last_one_out() {
    let crash = 13;
    let drums = DrumPadState::default();
    let first = render(vec![hit(0, note(crash), 110)], drums.clone()).sum();
    let second = hit(EIGHTH, note(crash), 110);
    let both = render(vec![hit(0, note(crash), 110), second], drums.clone()).sum();
    let later = render(vec![second], drums).sum();
    let fade = (FADE_SECONDS * SAMPLE_RATE as f32).round() as usize;
    let part: Vec<f32> = both
        .iter()
        .zip(&later)
        .map(|(both, later)| both - later)
        .collect();
    assert_eq!(part[..EIGHTH_FRAMES], first[..EIGHTH_FRAMES]);
    assert!(
        part[EIGHTH_FRAMES + fade..]
            .iter()
            .all(|sample| sample.abs() < 1e-6)
    );
}

#[test]
fn a_stop_fades_every_pad_out() {
    let crash = 13;
    let notes = vec![hit(0, note(crash), 110)];
    let mut going_on = Harness::with_track(notes.clone(), DrumPadState::default());
    going_on.play(4_800);
    let continued = going_on.render(4_800).sum();
    let mut harness = Harness::with_track(notes, DrumPadState::default());
    harness.play(4_800);
    harness.project.engine().stop();
    let stopped = harness.render(4_800).sum();
    let fade = (FADE_SECONDS * SAMPLE_RATE as f32).round() as usize;
    for frame in 0..fade {
        let expected = continued[frame] * (fade - frame) as f32 / fade as f32;
        assert!((stopped[frame] - expected).abs() < 1e-6, "{frame}");
    }
    assert!(stopped[fade..].iter().all(|sample| *sample == 0.0));
}
