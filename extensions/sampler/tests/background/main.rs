#![allow(clippy::unwrap_used)]
//! Instruments that load in the background, as in the window: a Sampler plays what it played
//! until the new instrument is in memory, then the new one. A process of its own, because
//! loading in the background is for the whole process.

#[path = "../sampler/support.rs"]
#[allow(dead_code)]
mod support;

use sampler::{SamplerState, SfzPath};
use sound_core::Changes;

use support::{Harness, SAMPLE_RATE, id, note, write_wav};

/// Writes `assets/instruments/<name>/<name>.sfz` with one steady sample at `level`.
fn write_pack(harness: &Harness, name: &str, level: f32) -> SamplerState {
    let folder = harness.path(&format!("assets/instruments/{name}"));
    write_wav(
        &folder.join("a.wav"),
        SAMPLE_RATE,
        &vec![level; SAMPLE_RATE as usize],
    );
    let sfz = "<region> sample=a.wav key=60 amp_veltrack=0";
    std::fs::write(folder.join(format!("{name}.sfz")), sfz).unwrap();
    SamplerState {
        sfz: Some(SfzPath::new(&format!("{name}/{name}.sfz")).unwrap()),
        ..SamplerState::default()
    }
}

/// Renders the first note again from the start.
fn level(harness: &mut Harness) -> f32 {
    harness.project.engine().seek(sound_core::Ticks(0));
    harness.play(4_800)[2_400]
}

fn take_ready(harness: &mut Harness) {
    sampler::instrument::wait_for_loading();
    for instance in sampler::take_ready(harness.project.assets()) {
        harness.project.rebind(&instance).unwrap();
    }
}

#[test]
fn a_new_instrument_plays_once_it_is_loaded_and_the_old_one_until_then() {
    sound_media::load_in_background();
    let mut harness = Harness::with_samples(&[]);
    let first = write_pack(&harness, "first", 0.1);
    let second = write_pack(&harness, "second", 0.2);
    harness.add_track(vec![note(0, 4_800, 60, 100)], first);
    // Loading is no problem, and nothing plays yet.
    assert_eq!(harness.project.problems(), []);
    assert_eq!(level(&mut harness), 0.0);
    take_ready(&mut harness);
    assert_eq!(level(&mut harness), 0.1);

    let instrument = id("track/instrument");
    let sampler = harness
        .project
        .resolve::<SamplerState>(&instrument)
        .unwrap();
    let mut changes = Changes::new();
    changes.set(&sampler, second);
    harness.project.commit("Load instrument", changes).unwrap();
    assert_eq!(level(&mut harness), 0.1);
    take_ready(&mut harness);
    assert_eq!(level(&mut harness), 0.2);
    assert_eq!(harness.project.problems(), []);
}
