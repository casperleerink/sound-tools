//! Every change glides: an edit of any field while a tone plays makes no step in the sound larger
//! than the tone itself takes, where a switch would jump by a large part of its level.

use utility::{Channels, UtilityState};

use crate::support::{Rig, SAMPLE_RATE, Signal, largest_step, peak, two_sines};

/// A tone on each side, one a fourth under the other, low enough that bass mono has something
/// to do.
fn tones() -> Signal {
    two_sines(110.0, 82.5, 0.5)
}

/// The largest step of the output around an edit from `before` to `after`, against the largest
/// step of the steady output before and after it, in either channel. A sine's step is its
/// amplitude times `2π f / sample rate`; a click is many times that.
fn step_ratio(before: UtilityState, after: UtilityState) -> f32 {
    let mut rig = Rig::new(before, tones());
    let settled = rig.render(SAMPLE_RATE as usize / 2);
    rig.update(after);
    let around = rig.render(SAMPLE_RATE as usize / 2);
    let steady = |channel: &Vec<f32>| largest_step(&channel[channel.len() - 4_800..]);
    let (settled, around) = (&settled, &around);
    (0..2)
        .map(|channel| {
            let steady = steady(&settled[channel]).max(steady(&around[channel]));
            let mut edit = settled[channel][settled[channel].len() - 1..].to_vec();
            edit.extend(&around[channel][..4_800]);
            largest_step(&edit) / steady.max(f32::MIN_POSITIVE)
        })
        .fold(0.0, f32::max)
}

#[test]
fn no_edit_steps_the_sound() {
    let default = UtilityState::default();
    let bass_mono = UtilityState {
        bass_mono: true,
        ..default
    };
    let edits = [
        (
            "gain up",
            default,
            UtilityState {
                gain_db: 12.0,
                ..default
            },
        ),
        (
            "gain down",
            default,
            UtilityState {
                gain_db: -36.0,
                ..default
            },
        ),
        (
            "pan",
            default,
            UtilityState {
                pan: -1.0,
                ..default
            },
        ),
        (
            "mono",
            default,
            UtilityState {
                width: 0.0,
                ..default
            },
        ),
        (
            "wide",
            default,
            UtilityState {
                width: 2.0,
                ..default
            },
        ),
        (
            "left",
            default,
            UtilityState {
                channels: Channels::Left,
                ..default
            },
        ),
        (
            "swap",
            default,
            UtilityState {
                channels: Channels::Swap,
                ..default
            },
        ),
        (
            "invert",
            default,
            UtilityState {
                invert_left: true,
                ..default
            },
        ),
        (
            "mute",
            default,
            UtilityState {
                mute: true,
                ..default
            },
        ),
        (
            "unmute",
            UtilityState {
                mute: true,
                ..default
            },
            default,
        ),
        ("bass mono on", default, bass_mono),
        ("bass mono off", bass_mono, default),
        (
            "bass mono frequency",
            bass_mono,
            UtilityState {
                bass_mono_hz: 500.0,
                ..bass_mono
            },
        ),
    ];
    for (name, before, after) in edits {
        let ratio = step_ratio(before, after);
        println!("{name}: largest step {ratio:.2} times the steady one");
        assert!(ratio < 1.5, "{name}: {ratio}");
    }
}

/// Mute takes 20 ms, and after it the output is exactly silent.
#[test]
fn a_glide_takes_twenty_milliseconds() {
    let mut rig = Rig::new(UtilityState::default(), tones());
    rig.render(SAMPLE_RATE as usize / 2);
    rig.update(UtilityState {
        mute: true,
        ..UtilityState::default()
    });
    let [left, right] = rig.render(SAMPLE_RATE as usize / 10);
    assert!(peak(&left[..480]) > 0.1);
    for channel in [&left, &right] {
        assert!(channel[960..].iter().all(|sample| *sample == 0.0));
    }
}
