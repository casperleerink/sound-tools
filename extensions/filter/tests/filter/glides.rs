//! Every change glides: an edit of any field while a tone plays makes no step in the sound
//! larger than the tone itself takes, where a switch would jump by a large part of its level.

use filter::{FilterState, FilterType, Slope};

use crate::support::{Rig, SAMPLE_RATE, largest_step, sine};

const HZ: f64 = 440.0;
const AMPLITUDE: f32 = 0.5;

/// The largest step of the output around an edit from `before` to `after`, against the largest
/// step of the steady output before and after it. A sine's step is its amplitude times
/// `2π f / sample rate`; a click is many times that.
fn step_ratio(before: FilterState, after: FilterState) -> f32 {
    let mut rig = Rig::new(before, sine(HZ, AMPLITUDE));
    let [settled, _] = rig.render(SAMPLE_RATE as usize / 2);
    let steady_before = largest_step(&settled[settled.len() - 4_800..]);
    rig.update(after);
    let [around, _] = rig.render(SAMPLE_RATE as usize / 2);
    let steady_after = largest_step(&around[around.len() - 4_800..]);
    let mut edit = settled[settled.len() - 1..].to_vec();
    edit.extend(&around[..4_800]);
    largest_step(&edit) / steady_before.max(steady_after)
}

#[test]
fn no_edit_steps_the_sound() {
    let base = FilterState {
        cutoff_hz: 440.0,
        resonance: 0.3,
        ..FilterState::default()
    };
    let edits = [
        (
            "cutoff",
            FilterState {
                cutoff_hz: 5_000.0,
                ..base
            },
        ),
        (
            "cutoff down",
            FilterState {
                cutoff_hz: 60.0,
                ..base
            },
        ),
        (
            "resonance",
            FilterState {
                resonance: 1.0,
                ..base
            },
        ),
        (
            "type",
            FilterState {
                kind: FilterType::HighPass,
                ..base
            },
        ),
        (
            "notch",
            FilterState {
                kind: FilterType::Notch,
                ..base
            },
        ),
        (
            "slope",
            FilterState {
                slope: Slope::TwentyFour,
                ..base
            },
        ),
        (
            "drive",
            FilterState {
                drive_db: 24.0,
                ..base
            },
        ),
        ("mix", FilterState { mix: 0.0, ..base }),
        (
            "lfo",
            FilterState {
                lfo_depth_octaves: 3.0,
                lfo_rate_hz: 8.0,
                ..base
            },
        ),
    ];
    for (name, after) in edits {
        let ratio = step_ratio(base, after);
        println!("{name}: largest step {ratio:.2} times the steady one");
        // Without the glides the cutoff edit alone steps by 7.8 times; measured by hand.
        assert!(ratio < 1.5, "{name}: {ratio}");
    }
}

/// A change of slope at high resonance glides between the two peaks and never passes the larger
/// of them: during the glide, no section rings at a Q that neither end has.
#[test]
fn a_change_of_slope_at_high_resonance_does_not_peak_during_its_glide() {
    let steady_peak = |state: FilterState| {
        let mut rig = Rig::new(state, sine(1_000.0, 0.05));
        let [left, _] = rig.render(SAMPLE_RATE as usize / 2);
        crate::support::peak(&left[left.len() - 4_800..])
    };
    for resonance in [0.8, 1.0] {
        for (from, to) in [
            (Slope::Twelve, Slope::TwentyFour),
            (Slope::TwentyFour, Slope::Twelve),
        ] {
            let before = FilterState {
                cutoff_hz: 1_000.0,
                resonance,
                slope: from,
                ..FilterState::default()
            };
            let after = FilterState {
                slope: to,
                ..before
            };
            let limit = steady_peak(before).max(steady_peak(after)) * 1.05;
            let mut rig = Rig::new(before, sine(1_000.0, 0.05));
            rig.render(SAMPLE_RATE as usize / 2);
            rig.update(after);
            let [glide, _] = rig.render(SAMPLE_RATE as usize / 4);
            let peak = crate::support::peak(&glide);
            println!("resonance {resonance}, {from:?} to {to:?}: peak {peak:.4}, limit {limit:.4}");
            assert!(
                peak <= limit,
                "resonance {resonance}, {from:?} to {to:?}: {peak} over {limit}"
            );
        }
    }
}

/// A glide of the cutoff over the whole range is done in 20 ms.
#[test]
fn a_glide_takes_twenty_milliseconds() {
    let base = FilterState {
        kind: FilterType::HighPass,
        cutoff_hz: 20.0,
        resonance: 0.0,
        ..FilterState::default()
    };
    let mut rig = Rig::new(base, sine(HZ, AMPLITUDE));
    rig.render(SAMPLE_RATE as usize / 2);
    rig.update(FilterState {
        cutoff_hz: 20_000.0,
        ..base
    });
    let [output, _] = rig.render(SAMPLE_RATE as usize / 10);
    // 440 Hz under a high pass at 20 kHz is 70 dB down. The glide is 960 frames, and 1.5 ms
    // after it the tone has gone.
    let after = &output[960 + 72..];
    assert!(crate::support::peak(after) < AMPLITUDE * 0.001);
    assert!(crate::support::peak(&output[..480]) > AMPLITUDE * 0.5);
}
