//! Every change glides: an edit of any field while a tone plays, or a new tempo, makes no step
//! in the sound larger than the delay makes by itself before and after it. A read that jumped
//! to another place in a line would step by up to twice the tone.

use delay::{DelayState, Division, Feel};

use crate::support::{Rig, SAMPLE_RATE, SECOND, largest_step, peak, sine, tempo, wet};

const HZ: f64 = 440.0;
/// The glide and a little after it.
const AROUND: usize = SECOND * 3 / 100;

/// The largest step of the output in the 30 ms after a change, against the largest step of the
/// output before it and in the half second after those 30 ms. After a change the repeats are
/// others, and the tone swells and beats while they settle; a click is a step many times
/// larger than any of that.
fn step_ratio(before: DelayState, change: impl FnOnce(&mut Rig)) -> f32 {
    let mut rig = Rig::new(before, sine(HZ, 0.5));
    let [settled, _] = rig.render(2 * SECOND);
    let steady = largest_step(&settled[settled.len() - SECOND / 10..]);
    change(&mut rig);
    let [around, _] = rig.render(SECOND / 2 + AROUND);
    let mut edit = settled[settled.len() - 1..].to_vec();
    edit.extend(&around[..AROUND]);
    let later = largest_step(&around[AROUND..]);
    largest_step(&edit) / steady.max(later)
}

#[test]
fn no_edit_and_no_new_tempo_steps_the_sound() {
    // Only the repeats, so no dry tone hides a step of them.
    let base = DelayState {
        feedback: 0.5,
        ..wet()
    };
    let edits = [
        (
            "division",
            DelayState {
                division: Division::Quarter,
                ..base
            },
        ),
        (
            "dotted",
            DelayState {
                feel: Feel::Dotted,
                ..base
            },
        ),
        (
            "triplet",
            DelayState {
                feel: Feel::Triplet,
                ..base
            },
        ),
        (
            "sync off",
            DelayState {
                sync: false,
                time_ms: 90.0,
                ..base
            },
        ),
        (
            "feedback up",
            DelayState {
                feedback: 0.95,
                ..base
            },
        ),
        (
            "feedback down",
            DelayState {
                feedback: 0.0,
                ..base
            },
        ),
        (
            "ping-pong",
            DelayState {
                ping_pong: true,
                ..base
            },
        ),
        (
            "low cut",
            DelayState {
                low_cut_hz: 2_000.0,
                ..base
            },
        ),
        (
            "high cut",
            DelayState {
                high_cut_hz: 200.0,
                ..base
            },
        ),
        ("mix", DelayState { mix: 0.0, ..base }),
    ];
    for (name, after) in edits {
        let ratio = step_ratio(base, |rig| rig.update(after));
        println!("{name}: largest step {ratio:.2} times the steady one");
        assert!(ratio < 1.5, "{name}: {ratio}");
    }
    for bpm in [60.0, 173.0] {
        let ratio = step_ratio(base, |rig| rig.control.set_tempo_map(tempo(bpm)));
        println!("{bpm} bpm: largest step {ratio:.2} times the steady one");
        assert!(ratio < 1.5, "{bpm} bpm: {ratio}");
    }
}

/// The largest step from one sample to the next against the step a sine of the tone's
/// frequency takes at the peak level of the 5 ms around it. A tone is 1 at most, a click many
/// times that.
fn largest_step_for_level(samples: &[f32]) -> f32 {
    let window = SECOND / 200;
    let most_per_amplitude = (std::f64::consts::TAU * HZ / f64::from(SAMPLE_RATE)) as f32;
    samples
        .chunks(window)
        .zip(samples.chunks(window).skip(1))
        .map(|(first, second)| {
            let around: Vec<f32> = first.iter().chain(second).copied().collect();
            largest_step(&around) / (peak(&around) * most_per_amplitude)
        })
        .fold(0.0, f32::max)
}

/// A drag of the time sends a new value with every mouse move. Each one fades from where the
/// last one ended, so a drag is as smooth as one edit.
#[test]
fn a_drag_of_the_time_makes_no_step() {
    let base = DelayState {
        sync: false,
        feedback: 0.5,
        ..wet()
    };
    let mut rig = Rig::new(base, sine(HZ, 0.5));
    let [settled, _] = rig.render(2 * SECOND);
    let mut dragged = settled[settled.len() - 1..].to_vec();
    // A move every 8 ms, as a trackpad sends them, for half a second.
    for step in 0..60 {
        let along = step as f32 / 59.0;
        rig.update(DelayState {
            time_ms: 250.0 + 500.0 * along,
            ..base
        });
        let [part, _] = rig.render(SECOND * 8 / 1_000);
        dragged.extend(part);
    }
    // The tone swells and fades as the repeats change under it, so each step is measured
    // against the level around it.
    let ratio = largest_step_for_level(&dragged);
    println!("drag: largest step {ratio:.2} times what the tone takes at its level");
    assert!(ratio < 1.5, "{ratio}");
}
