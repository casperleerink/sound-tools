//! Every change glides: an edit of any field while a tone plays, a change of mode too, adds no
//! click. What the effect makes of a tone of 440 Hz stays a tone near 440 Hz, whose level swells
//! and falls as the notches move over it; a click is a step or a corner, heard as a crack
//! across the whole band.

use modulation::{Mode, ModulationState};

use crate::support::{Rig, SECOND, crackle, sine};

const HZ: f64 = 440.0;
const AMPLITUDE: f32 = 0.5;

/// The most crackle a tone through the effect may have. What the tone leaves is about 1e-5. A
/// glide leaves up to about 5e-3, from the corners where its straight line starts and ends, or
/// a sudden small bend of the pitch as depth starts to glide: no click is heard. A step of a
/// thousandth, far under what a switch of any field makes, leaves 2e-2.
const CLICK: f32 = 1e-2;

/// The output of both sides from the frames before an edit from `before` to `after` to 200 ms
/// after it.
fn around_an_edit(before: ModulationState, after: ModulationState) -> [Vec<f32>; 2] {
    let mut rig = Rig::new(before, sine(HZ, AMPLITUDE));
    let settled = rig.render(SECOND / 2);
    rig.update(after);
    let around = rig.render(SECOND / 5);
    [0, 1].map(|side| {
        let mut window = settled[side][settled[side].len() - 6..].to_vec();
        window.extend(&around[side]);
        window
    })
}

fn crackle_of_both([left, right]: &[Vec<f32>; 2]) -> f32 {
    crackle(left).max(crackle(right))
}

#[test]
fn no_edit_clicks() {
    for mode in Mode::ALL {
        let base = ModulationState {
            mode,
            feedback: 0.5,
            ..ModulationState::default()
        };
        let other = |other: Mode| ModulationState {
            mode: other,
            ..base
        };
        let edits = [
            ("chorus", other(Mode::Chorus)),
            ("flanger", other(Mode::Flanger)),
            ("phaser", other(Mode::Phaser)),
            (
                "rate",
                ModulationState {
                    rate_hz: 10.0,
                    ..base
                },
            ),
            ("depth", ModulationState { depth: 1.0, ..base }),
            ("depth to 0", ModulationState { depth: 0.0, ..base }),
            (
                "feedback",
                ModulationState {
                    feedback: 1.0,
                    ..base
                },
            ),
            (
                "feedback to 0",
                ModulationState {
                    feedback: 0.0,
                    ..base
                },
            ),
            (
                "spread",
                ModulationState {
                    spread: 1.0,
                    ..base
                },
            ),
            ("mix", ModulationState { mix: 0.0, ..base }),
            ("mix to 1", ModulationState { mix: 1.0, ..base }),
        ];
        let steady = crackle_of_both(&around_an_edit(base, base));
        println!("{mode:?}, no edit: crackle {steady:.2e}");
        for (name, after) in edits {
            let heard = crackle_of_both(&around_an_edit(base, after));
            println!("{mode:?}, {name}: crackle {heard:.2e}");
            assert!(heard < CLICK, "{mode:?}, {name}: {heard}");
        }
    }
}

/// The measure hears a click: a step of a thousandth in the steady tone.
#[test]
fn a_step_of_a_thousandth_is_a_click() {
    let state = ModulationState::default();
    let [mut window, _] = around_an_edit(state, state);
    assert!(crackle(&window) < CLICK / 100.0);
    for sample in &mut window[4_800..] {
        *sample += 0.001;
    }
    assert!(crackle(&window) > CLICK);
}

/// A change of mode fades over 20 ms: 1 ms after it the sound is still mostly the old mode, and
/// 25 ms after it the new one alone.
#[test]
fn a_change_of_mode_fades_over_twenty_milliseconds() {
    let chorus = ModulationState {
        mode: Mode::Chorus,
        mix: 1.0,
        depth: 0.0,
        feedback: 0.0,
        ..ModulationState::default()
    };
    let phaser = ModulationState {
        mode: Mode::Phaser,
        ..chorus
    };
    let mut switched = Rig::new(chorus, sine(HZ, AMPLITUDE));
    switched.render(SECOND / 2);
    switched.update(phaser);
    let [heard, _] = switched.render(SECOND / 10);
    let [only_phaser, _] = {
        let mut rig = Rig::new(phaser, sine(HZ, AMPLITUDE));
        rig.render(SECOND / 2);
        rig.render(SECOND / 10)
    };
    let [only_chorus, _] = {
        let mut rig = Rig::new(chorus, sine(HZ, AMPLITUDE));
        rig.render(SECOND / 2);
        rig.render(SECOND / 10)
    };
    let early = 48;
    assert!((heard[early] - only_chorus[early]).abs() < 0.1 * AMPLITUDE);
    let late = 25 * SECOND / 1_000;
    for index in late..late + 480 {
        assert!((heard[index] - only_phaser[index]).abs() < 1e-4, "{index}");
    }
}
