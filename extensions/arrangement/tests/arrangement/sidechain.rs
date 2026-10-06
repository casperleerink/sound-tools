//! A track keys an effect of another track through its `sidechain` input, as the slot in the
//! track record says.
//!
//! The effect is the test `Duck` of `support.rs`: it divides the bass by one plus its key, so
//! every sample says what keyed it. The kick plays 36 on the second beat; the bass plays 60
//! throughout, and the master sums both.

use sound_core::Changes;

use crate::support::{Harness, TICK, clip, id, note};

const BEAT: u64 = 960;
const BEAT_FRAMES: usize = BEAT as usize * TICK;
const KICK: f32 = 36.0;
const BASS: f32 = 60.0;
const DUCK: &str = r#"{"tool": "test.duck", "state": {}}"#;

/// The bass with a duck whose slot says `"sidechain": <sidechain>`, and the kick with `kick`
/// in its track record after its name.
fn project(sidechain: &str, kick: &str) -> Harness {
    let mut harness = Harness::new();
    harness.add_track("kick", 1.0);
    harness.add_track("bass", 1.0);
    let mut changes = Changes::new();
    let kick_note = clip(BEAT, BEAT, vec![note(0, BEAT, KICK as u8)]);
    changes.create(id("arrangement/kick/hit"), kick_note);
    let bass_note = clip(0, 8 * BEAT, vec![note(0, 8 * BEAT, BASS as u8)]);
    changes.create(id("arrangement/bass/line"), bass_note);
    harness.project.commit("Add clips", changes).unwrap();
    harness.write_and_apply("state/arrangement/bass/duck.json", DUCK);
    harness.write_and_apply(
        "state/arrangement/bass/instance.json",
        &format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "bass", "order": 1, "effects": [{{"name": "duck", "sidechain": {sidechain}}}]}}}}"#
        ),
    );
    write_kick(&mut harness, kick);
    harness
}

fn write_kick(harness: &mut Harness, kick: &str) {
    harness.write_and_apply(
        "state/arrangement/kick/instance.json",
        &format!(r#"{{"tool": "arrangement.track", "state": {{"name": "kick"{kick}}}}}"#),
    );
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
    let mut harness = project(r#"{"track": "kick", "tap": "post_fx"}"#, "");
    assert_eq!(harness.problems(), Vec::<String>::new());
    let (before, during) = before_and_during(&mut harness);
    assert_eq!(before, BASS);
    assert_eq!(during, KICK + ducked_by(KICK));
}

/// The kick is muted and plays through a trim that halves it. pre_fx keys with the kick as it
/// was played, post_fx with the half, and post_mixer with the silence after the mute.
#[test]
fn each_tap_keys_with_the_kick_at_its_point() {
    let half = r#"{"tool": "test.trim", "state": {"gain": 0.5}}"#;
    for (tap, key) in [
        ("pre_fx", KICK),
        ("post_fx", KICK / 2.0),
        ("post_mixer", 0.0),
    ] {
        let mut harness = project(&format!(r#"{{"track": "kick", "tap": "{tap}"}}"#), "");
        harness.write_and_apply("state/arrangement/kick/half.json", half);
        write_kick(&mut harness, r#", "mute": true, "effects": ["half"]"#);
        assert_eq!(harness.problems(), Vec::<String>::new());
        assert_eq!(
            before_and_during(&mut harness),
            (BASS, ducked_by(key)),
            "{tap}"
        );
    }
}

/// A deleted source is a problem, and the bass plays on. A bypassed slot is keyed by nothing,
/// so the same name there is no problem.
#[test]
fn a_deleted_source_track_is_a_problem_and_the_bass_plays_on() {
    let mut harness = project(r#"{"track": "kick", "tap": "post_fx"}"#, "");
    let mut changes = Changes::new();
    changes.delete(&id("arrangement/kick"));
    harness.project.commit("Delete kick", changes).unwrap();
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/instance.json: the sidechain of \"duck\" in track \"bass\" takes track \"kick\", and this arrangement has no kick/instance.json, so \"duck\" in track \"bass\" follows its own sound. Name the folder of a track, or take `sidechain` out"
        ]
    );
    assert_eq!(before_and_during(&mut harness), (BASS, BASS));
    harness.write_and_apply(
        "state/arrangement/bass/instance.json",
        r#"{"tool": "arrangement.track", "state": {"name": "bass", "effects": [{"name": "duck", "bypass": true, "sidechain": {"track": "kick", "tap": "post_fx"}}]}}"#,
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
}

#[test]
fn an_effect_with_no_sidechain_input_is_a_problem_that_names_it() {
    let mut harness = project(r#"{"track": "kick", "tap": "post_fx"}"#, "");
    harness.write_and_apply(
        "state/arrangement/bass/duck.json",
        r#"{"tool": "test.trim", "state": {"gain": 1.0}}"#,
    );
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/bass/instance.json: `effects` keys \"duck\" with a sidechain, and duck.json holds no tool with a `sidechain` input, so the sidechain is not used. Use an effect that has one, such as the `compressor`, or take `sidechain` out"
        ]
    );
    assert_eq!(before_and_during(&mut harness), (BASS, KICK + BASS));
}

/// A source with no sound at its tap leaves the duck with its own sound, and says so.
#[test]
fn a_source_with_no_instrument_is_a_problem() {
    let mut harness = project(r#"{"track": "kick", "tap": "pre_fx"}"#, "");
    let mut changes = Changes::new();
    changes.delete(&id("arrangement/kick/instrument"));
    harness
        .project
        .commit("Delete instrument", changes)
        .unwrap();
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/instance.json: the sidechain of \"duck\" in track \"bass\" takes track \"kick\", which has no instrument that plays, so \"duck\" in track \"bass\" follows its own sound"
        ]
    );
    assert_eq!(before_and_during(&mut harness), (BASS, BASS));
}

/// Its own track before its effects is a key like any other. After them, the key would come
/// out of the duck itself: a loop of one, and both tracks play.
#[test]
fn a_track_keys_its_own_effect_only_before_its_effects() {
    let mut pre_fx = project(r#"{"track": "bass", "tap": "pre_fx"}"#, "");
    assert_eq!(pre_fx.problems(), Vec::<String>::new());
    let ducked = ducked_by(BASS);
    assert_eq!(before_and_during(&mut pre_fx), (ducked, KICK + ducked));
    for tap in ["post_fx", "post_mixer"] {
        let mut harness = project(&format!(r#"{{"track": "bass", "tap": "{tap}"}}"#), "");
        assert_eq!(
            harness.problems(),
            [
                "state/arrangement/instance.json: the sidechain of \"duck\" in track \"bass\" closes a loop: \"bass\" is keyed by \"bass\", each after the effects of the track that keys it. A sound cannot key itself, so \"duck\" in track \"bass\" follows its own sound. Use \"tap\": \"pre_fx\" for one of them, or take one out"
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

/// The bass is keyed by the kick and the kick by the bass, both after their effects: neither
/// key is used, each says so, and both tracks play.
#[test]
fn two_tracks_that_key_each_other_are_a_loop_and_both_play() {
    let mut harness = project(r#"{"track": "kick", "tap": "post_fx"}"#, "");
    harness.write_and_apply("state/arrangement/kick/duck.json", DUCK);
    write_kick(
        &mut harness,
        r#", "effects": [{"name": "duck", "sidechain": {"track": "bass", "tap": "post_fx"}}]"#,
    );
    assert_eq!(
        harness.problems(),
        [
            "state/arrangement/instance.json: the sidechain of \"duck\" in track \"bass\" closes a loop: \"bass\" is keyed by \"kick\" and \"kick\" is keyed by \"bass\", each after the effects of the track that keys it. A sound cannot key itself, so \"duck\" in track \"bass\" follows its own sound. Use \"tap\": \"pre_fx\" for one of them, or take one out",
            "state/arrangement/instance.json: the sidechain of \"duck\" in track \"kick\" closes a loop: \"kick\" is keyed by \"bass\" and \"bass\" is keyed by \"kick\", each after the effects of the track that keys it. A sound cannot key itself, so \"duck\" in track \"kick\" follows its own sound. Use \"tap\": \"pre_fx\" for one of them, or take one out",
        ]
    );
    assert_eq!(before_and_during(&mut harness), (BASS, KICK + BASS));
}
