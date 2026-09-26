//! Every change glides: an edit of any field while a tone plays makes no step in the sound
//! larger than the tone itself takes, where a switch would jump by a large part of its level.
//! Also a change of shape, and a band turned on or off.

use eq::{EqState, Shape};

use crate::support::{Rig, SAMPLE_RATE, band, largest_step, peak, sine, with_bands};

const HZ: f64 = 440.0;
const AMPLITUDE: f32 = 0.25;

/// The largest step of the output around an edit from `before` to `after`, against the largest
/// step of the steady output before and after it. A sine's step is its amplitude times
/// `2π f / sample rate`; a click is many times that.
fn step_ratio(before: EqState, after: EqState) -> f32 {
    let mut rig = Rig::new(before, sine(HZ, AMPLITUDE));
    // A quarter of a cycle past a whole number of them, so the edit lands at the top of the
    // tone, where a switch jumps the most, and not at a zero crossing.
    let [settled, _] = rig.render(SAMPLE_RATE as usize / 2 + 27);
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
    let base = with_bands(&[
        band(Shape::LowShelf, 100.0, 0.0, 0.71),
        band(Shape::Bell, 440.0, 9.0, 2.0),
        band(Shape::Bell, 2_000.0, 0.0, 0.71),
        band(Shape::HighShelf, 8_000.0, 0.0, 0.71),
    ]);
    let edit = |change: &dyn Fn(&mut EqState)| {
        let mut after = base;
        change(&mut after);
        after
    };
    let edits: [(&str, EqState); 12] = [
        ("frequency", edit(&|state| state.bands[1].frequency_hz = 3_000.0)),
        ("gain", edit(&|state| state.bands[1].gain_db = -15.0)),
        ("q", edit(&|state| state.bands[1].q = 18.0)),
        ("bell to high cut", edit(&|state| state.bands[1].shape = Shape::HighCut)),
        ("bell to low cut", edit(&|state| state.bands[1].shape = Shape::LowCut)),
        ("bell to notch", edit(&|state| state.bands[1].shape = Shape::Notch)),
        ("bell to low shelf", edit(&|state| state.bands[1].shape = Shape::LowShelf)),
        ("bell to high shelf", edit(&|state| state.bands[1].shape = Shape::HighShelf)),
        ("band off", edit(&|state| state.bands[1].on = false)),
        ("band on", edit(&|state| {
            state.bands[2] = band(Shape::Notch, 440.0, 0.0, 4.0);
        })),
        ("output", edit(&|state| state.output_gain_db = -12.0)),
        ("everything", edit(&|state| {
            // Nothing glides across the tone, and every change makes it quieter: a resonance
            // that glides over it, or a gain that rises while another falls, makes it louder on
            // the way than at either end. That is a sweep and not a click, and it is not what
            // this measures.
            state.bands[0] = band(Shape::LowCut, 60.0, 0.0, 8.0);
            state.bands[1].on = false;
            state.bands[3] = band(Shape::HighCut, 12_000.0, 0.0, 0.3);
            state.output_gain_db = -6.0;
        })),
    ];
    for (name, after) in edits {
        let ratio = step_ratio(base, after);
        println!("{name}: largest step {ratio:.2} times the steady one");
        // With a ramp of one frame, close to a switch, the output edit steps by 13 times, the
        // frequency by 3.6 and the bell to a high shelf by 3.3; measured by hand.
        assert!(ratio < 1.5, "{name}: {ratio}");
    }
}

/// A glide of the frequency over the whole range is done in 20 ms.
#[test]
fn a_glide_takes_twenty_milliseconds() {
    let base = with_bands(&[band(Shape::LowCut, 20.0, 0.0, 0.71)]);
    let mut rig = Rig::new(base, sine(HZ, AMPLITUDE));
    rig.render(SAMPLE_RATE as usize / 2);
    rig.update(with_bands(&[band(Shape::LowCut, 20_000.0, 0.0, 0.71)]));
    let [output, _] = rig.render(SAMPLE_RATE as usize / 10);
    // 440 Hz under a low cut at 20 kHz is 70 dB down. The glide is 960 frames, and 1.5 ms after
    // it the tone has gone.
    let after = &output[960 + 72..];
    assert!(peak(after) < AMPLITUDE * 0.001, "{}", peak(after));
    assert!(peak(&output[..480]) > AMPLITUDE * 0.5);
}

/// A change of shape arrives in 20 ms too, and then the band is exactly the new shape.
#[test]
fn a_change_of_shape_arrives_in_twenty_milliseconds() {
    let before = with_bands(&[band(Shape::Bell, 440.0, 12.0, 1.0)]);
    let after = with_bands(&[band(Shape::Notch, 440.0, 12.0, 1.0)]);
    let mut rig = Rig::new(before, sine(HZ, AMPLITUDE));
    rig.render(SAMPLE_RATE as usize / 2);
    rig.update(after);
    let [output, _] = rig.render(SAMPLE_RATE as usize / 5);
    // After the glide the notch rings out; 100 ms later the tone is gone.
    let after = &output[960 + 4_800..];
    assert!(peak(after) < AMPLITUDE * 0.001, "{}", peak(after));
}
