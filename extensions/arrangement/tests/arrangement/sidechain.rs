//! A track keys an effect of another track through its `sidechain` input, as the slot in the
//! track record says.
//!
//! The effect is the test `Duck` of `support.rs`: it divides the bass by one plus its key, so
//! every sample says what keyed it. The kick plays 36 on the second beat; the bass plays 60
//! throughout, and the master sums both.

use arrangement::{Sidechain, Tap, TrackState};
use sound_core::Changes;

use crate::support::{Harness, TICK, clip, id, note};

const BEAT: u64 = 960;
const BEAT_FRAMES: usize = BEAT as usize * TICK;
const KICK: f32 = 36.0;
const BASS: f32 = 60.0;
const BASS_FILE: &str = "state/arrangement/bass/instance.json";

/// The bass keyed by `sidechain` (a JSON value), through a duck of this latency, and the kick
/// with `kick` as its track record state after its name.
fn project(sidechain: &str, latency: u32, kick: &str) -> Harness {
    let mut harness = Harness::new();
    harness.add_track("kick", 1.0);
    harness.add_track("bass", 1.0);
    let mut changes = Changes::new();
    let kick_note = clip(BEAT, BEAT, vec![note(0, BEAT, KICK as u8)]);
    changes.create(id("arrangement/kick/hit"), kick_note);
    let bass_note = clip(0, 8 * BEAT, vec![note(0, 8 * BEAT, BASS as u8)]);
    changes.create(id("arrangement/bass/line"), bass_note);
    harness.project.commit("Add clips", changes).unwrap();
    harness.write_and_apply(
        "state/arrangement/bass/duck.json",
        &format!(r#"{{"tool": "test.duck", "state": {{"latency": {latency}}}}}"#),
    );
    harness.write_and_apply(
        BASS_FILE,
        &format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "bass", "order": 1, "effects": [{{"name": "duck", "sidechain": {sidechain}}}]}}}}"#
        ),
    );
    harness.write_and_apply(
        "state/arrangement/kick/instance.json",
        &format!(r#"{{"tool": "arrangement.track", "state": {{"name": "kick"{kick}}}}}"#),
    );
    harness
}

/// What the master plays in the middle of the first beat, and of the second, where the kick is.
fn before_and_during(harness: &mut Harness) -> (f32, f32) {
    let render = harness.play(2 * BEAT_FRAMES);
    (render[BEAT_FRAMES / 2], render[BEAT_FRAMES * 3 / 2])
}

fn ducked_by(key: f32) -> f32 {
    BASS / (1.0 + key)
}

#[test]
fn the_kick_ducks_the_bass_while_it_plays() {
    let mut harness = project(r#"{"track": "kick", "tap": "post_fx"}"#, 0, "");
    assert_eq!(harness.problems(), Vec::<String>::new());
    let (before, during) = before_and_during(&mut harness);
    assert_eq!(before, BASS);
    assert_eq!(during, KICK + ducked_by(KICK));
}

/// Mute comes after the effects: post_fx of a muted kick still keys the bass, and post_mixer
/// is silent, so it keys nothing.
#[test]
fn a_muted_kick_keys_from_post_fx_and_not_from_post_mixer() {
    let muted = r#", "mute": true"#;
    let mut post_fx = project(r#"{"track": "kick", "tap": "post_fx"}"#, 0, muted);
    assert_eq!(before_and_during(&mut post_fx), (BASS, ducked_by(KICK)));
    let mut post_mixer = project(r#"{"track": "kick", "tap": "post_mixer"}"#, 0, muted);
    assert_eq!(post_mixer.problems(), Vec::<String>::new());
    assert_eq!(before_and_during(&mut post_mixer), (BASS, BASS));
}

/// The kick plays through a trim that halves it: pre_fx keys with the kick as it was played,
/// post_fx with the half.
#[test]
fn pre_fx_keys_with_the_sound_before_the_effects_of_the_track() {
    let kick = r#", "mute": true, "effects": ["half"]"#;
    let half = r#"{"tool": "test.trim", "state": {"gain": 0.5}}"#;
    for (tap, key) in [("pre_fx", KICK), ("post_fx", KICK / 2.0)] {
        let mut harness = project(&format!(r#"{{"track": "kick", "tap": "{tap}"}}"#), 0, "");
        harness.write_and_apply("state/arrangement/kick/half.json", half);
        harness.write_and_apply(
            "state/arrangement/kick/instance.json",
            &format!(r#"{{"tool": "arrangement.track", "state": {{"name": "kick"{kick}}}}}"#),
        );
        assert_eq!(harness.problems(), Vec::<String>::new());
        assert_eq!(
            before_and_during(&mut harness),
            (BASS, ducked_by(key)),
            "{tap}"
        );
    }
}

#[test]
fn a_deleted_source_track_is_a_problem_and_the_bass_plays_on() {
    let mut harness = project(r#"{"track": "kick", "tap": "post_fx"}"#, 0, "");
    let mut changes = Changes::new();
    changes.delete(&id("arrangement/kick"));
    harness.project.commit("Delete kick", changes).unwrap();
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/instance.json: the sidechain of \"duck\" in track \"bass\" takes track \"kick\", and this arrangement has no kick/instance.json, so nothing keys it. Name the folder of a track, or take `sidechain` out"
        ]
    );
    assert_eq!(before_and_during(&mut harness), (BASS, BASS));
}

#[test]
fn an_effect_with_no_sidechain_input_is_a_problem_that_names_it() {
    let mut harness = project(r#"{"track": "kick", "tap": "post_fx"}"#, 0, "");
    harness.write_and_apply(
        "state/arrangement/bass/duck.json",
        r#"{"tool": "test.trim", "state": {"gain": 1.0}}"#,
    );
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/bass/instance.json: `effects` keys \"duck\" with a sidechain, and duck.json holds no tool with a `sidechain` input, so nothing keys it. Use an effect that has one, such as the `compressor`, or take `sidechain` out"
        ]
    );
    assert_eq!(before_and_during(&mut harness), (BASS, KICK + BASS));
}

/// Its own track before its effects is a key like any other. After them, the key would come
/// out of the effect itself: a problem, and both tracks play.
#[test]
fn a_track_keys_its_own_effect_only_before_its_effects() {
    let mut pre_fx = project(r#"{"track": "bass", "tap": "pre_fx"}"#, 0, "");
    assert_eq!(pre_fx.problems(), Vec::<String>::new());
    let ducked = ducked_by(BASS);
    assert_eq!(before_and_during(&mut pre_fx), (ducked, KICK + ducked));
    for tap in ["post_fx", "post_mixer"] {
        let mut harness = project(&format!(r#"{{"track": "bass", "tap": "{tap}"}}"#), 0, "");
        assert_eq!(
            harness.problems(),
            [
                "state/arrangement/bass/instance.json: the sidechain of \"duck\" takes the sound of this same track after \"duck\" itself, which would key it in a loop, so nothing keys it. Use \"tap\": \"pre_fx\" to key it with this track before its effects, or take another track"
            ],
            "{tap}"
        );
        assert_eq!(
            before_and_during(&mut harness),
            (BASS, KICK + BASS),
            "{tap}"
        );
    }
}

/// The duck of the bass has a lookahead. The bass is led to stay in time, and the kick that
/// keys it is not: the kick reaches the master as it does with no sidechain, to the sample.
#[test]
fn the_kick_reaches_the_master_on_time_past_a_lookahead_it_keys() {
    let latency = 480;
    let bass_muted = |harness: &mut Harness, sidechain: &str| {
        harness.write_and_apply(
            BASS_FILE,
            &format!(
                r#"{{"tool": "arrangement.track", "state": {{"name": "bass", "order": 1, "mute": true, "effects": [{{"name": "duck", "sidechain": {sidechain}}}]}}}}"#
            ),
        );
    };
    let mut keyed = project("null", latency, "");
    bass_muted(&mut keyed, r#"{"track": "kick", "tap": "post_fx"}"#);
    let mut not_keyed = project("null", latency, "");
    bass_muted(&mut not_keyed, "null");
    let frames = 2 * BEAT_FRAMES + latency as usize;
    let render = keyed.play(frames);
    assert_eq!(render, not_keyed.play(frames));
    assert!(render.contains(&KICK));
}

/// The interface keys a slot through the track record, as one undo step.
#[test]
fn undo_of_a_sidechain_gives_the_sound_before_it() {
    let mut harness = project("null", 0, "");
    let (before, during) = before_and_during(&mut harness);
    let bass = harness
        .project
        .resolve::<TrackState>(&id("arrangement/bass"));
    let bass = bass.unwrap();
    let mut state = harness.project.state(&bass).unwrap().clone();
    let sidechain = Sidechain {
        track: "kick".to_string(),
        tap: Tap::PostFx,
    };
    assert!(state.set_sidechain("duck", Some(sidechain.clone())));
    let mut changes = Changes::new();
    changes.set(&bass, state);
    harness.project.commit("Key duck", changes).unwrap();
    let keyed = harness.project.state(&bass).unwrap();
    assert_eq!(keyed.sidechain("duck"), Some(&sidechain));
    let seek_and_play = |harness: &mut Harness| {
        harness.project.engine().seek(sound_core::Ticks(0));
        before_and_during(harness)
    };
    assert_eq!(
        seek_and_play(&mut harness),
        (before, KICK + ducked_by(KICK))
    );
    harness.project.undo().unwrap();
    assert_eq!(seek_and_play(&mut harness), (before, during));
}
