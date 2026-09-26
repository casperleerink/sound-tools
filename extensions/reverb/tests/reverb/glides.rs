//! Every change glides: an edit of any field while a tone plays makes no step in the sound
//! larger than the reverb makes by itself before and after it. A read that jumped to another
//! place in a delay line would step by up to twice the tone.

use reverb::{PRE_DELAY, ReverbState};

use crate::support::{Rig, SAMPLE_RATE, largest_step, sine};

const SECOND: usize = SAMPLE_RATE as usize;
const HZ: f64 = 440.0;
/// The glide and a little after it.
const AROUND: usize = SECOND * 3 / 100;

/// The largest step of the output in the 30 ms after an edit from `before` to `after`, against
/// the largest step of the output before it and in the half second after those 30 ms. After an
/// edit the room is another, and its tone swells and beats while it settles; a click is a step
/// many times larger than any of that.
fn step_ratio(before: ReverbState, after: ReverbState) -> f32 {
    let mut rig = Rig::new(before, sine(HZ, 0.5));
    let [settled, _] = rig.render(2 * SECOND);
    let steady = largest_step(&settled[settled.len() - SECOND / 10..]);
    rig.update(after);
    let [around, _] = rig.render(SECOND / 2 + AROUND);
    let mut edit = settled[settled.len() - 1..].to_vec();
    edit.extend(&around[..AROUND]);
    let later = largest_step(&around[AROUND..]);
    largest_step(&edit) / steady.max(later)
}

#[test]
fn no_edit_steps_the_sound() {
    // Only the reverb, so no dry tone hides a step of it.
    let base = ReverbState {
        decay_seconds: 0.5,
        mix: 1.0,
        ..ReverbState::default()
    };
    let edits = [
        ("size up", ReverbState { size: 1.0, ..base }),
        ("size down", ReverbState { size: 0.0, ..base }),
        (
            "pre-delay up",
            ReverbState {
                pre_delay_ms: PRE_DELAY.max,
                ..base
            },
        ),
        (
            "pre-delay down",
            ReverbState {
                pre_delay_ms: PRE_DELAY.min,
                ..base
            },
        ),
        (
            "decay",
            ReverbState {
                decay_seconds: 10.0,
                ..base
            },
        ),
        ("damping", ReverbState { damping: 1.0, ..base }),
        (
            "diffusion",
            ReverbState {
                diffusion: 0.0,
                ..base
            },
        ),
        (
            "low cut",
            ReverbState {
                low_cut_hz: 2_000.0,
                ..base
            },
        ),
        (
            "high cut",
            ReverbState {
                high_cut_hz: 200.0,
                ..base
            },
        ),
        ("width", ReverbState { width: 0.0, ..base }),
        ("mix", ReverbState { mix: 0.0, ..base }),
        ("freeze", ReverbState { freeze: true, ..base }),
    ];
    for (name, after) in edits {
        let ratio = step_ratio(base, after);
        println!("{name}: largest step {ratio:.2} times the steady one");
        assert!(ratio < 1.5, "{name}: {ratio}");
    }
}

/// The largest step from one sample to the next against the step a sine of the tone's
/// frequency takes at the peak level of the 5 ms around it. A tone is 1 at most, a click
/// many times that.
fn largest_step_for_level(samples: &[f32]) -> f32 {
    let window = SECOND / 200;
    let most_per_amplitude = (std::f64::consts::TAU * HZ / f64::from(SAMPLE_RATE)) as f32;
    samples
        .chunks(window)
        .zip(samples.chunks(window).skip(1))
        .map(|(first, second)| {
            let around: Vec<f32> = first.iter().chain(second).copied().collect();
            let level = crate::support::peak(&around);
            largest_step(&around) / (level * most_per_amplitude)
        })
        .fold(0.0, f32::max)
}

/// A drag of the size or the pre-delay sends a new value with every mouse move. Each one fades
/// from where the last one ended, so a drag is as smooth as one edit.
#[test]
fn a_drag_of_size_and_pre_delay_makes_no_step() {
    let base = ReverbState {
        decay_seconds: 0.5,
        mix: 1.0,
        ..ReverbState::default()
    };
    let mut rig = Rig::new(base, sine(HZ, 0.5));
    let [settled, _] = rig.render(2 * SECOND);
    let mut dragged = settled[settled.len() - 1..].to_vec();
    // A move every 8 ms, as a trackpad sends them, for half a second.
    for step in 0..60 {
        let along = step as f32 / 59.0;
        rig.update(ReverbState {
            size: 0.5 + 0.5 * along,
            pre_delay_ms: 20.0 + 200.0 * along,
            ..base
        });
        let [part, _] = rig.render(SECOND * 8 / 1_000);
        dragged.extend(part);
    }
    // The tone swells and fades as the room changes under it, so each step is measured
    // against the level around it.
    let ratio = largest_step_for_level(&dragged);
    println!("drag: largest step {ratio:.2} times what the tone takes at its level");
    assert!(ratio < 1.5, "{ratio}");
}
