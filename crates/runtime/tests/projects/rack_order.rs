//! Reordering the rack of a track, step 9b of the third milestone: after a reorder through the
//! editing path a project renders exactly what the same project renders when its files were
//! written in that order from the start. Bypass and latency stay right: a bypassed plugin with
//! latency adds none to the chain wherever it moves, and one that is on keeps its latency
//! compensated. The wait after a play still counts the bypassed one, a known gap of step 2
//! ("taking a bypassed plugin's latency out of the wait after a play"), the same in both.
//!
//! The chain holds a plugin and a built-in effect alike: the repository's test plugin with a
//! latency and an offset of its own, the Filter, and a second test plugin that is bypassed.

use arrangement::{EffectSlot, TrackState};
use plugin_host::PluginFormat;
use sound_core::{Changes, InstanceId};
use test_plugin_support::SavedState;

use crate::support::{Harness, plugin_state_of, test_plugin_of};

const FOLDER: &str = "state/arrangement/piano";
/// The latency of the plugin that is on, and of the one that is bypassed, in frames.
const LATE: i32 = 300;
const HELD: i32 = 700;

/// The order written first, and the order the reorder makes: `late` from the front to the end.
const FIRST: &str = r#"["late", "filter", {"name": "held", "bypass": true}]"#;
const MOVED: &str = r#"["filter", {"name": "held", "bypass": true}, "late"]"#;

/// A project with one track: the synth, a clip of two notes, and these effects in this order.
fn project(format: PluginFormat, effects: &str) -> Harness {
    let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
    harness.write(
        &format!("{FOLDER}/instance.json"),
        &format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "piano", "order": 1, "effects": {effects}}}}}"#
        ),
    );
    harness.write(
        &format!("{FOLDER}/instrument.json"),
        r#"{"tool": "instrument.synth", "state": {}}"#,
    );
    harness.write(
        &format!("{FOLDER}/notes.json"),
        r#"{"tool": "arrangement.clip", "state": {"start": 0, "length": 3840, "notes": [
            {"start": 0, "length": 960, "pitch": 60, "velocity": 100},
            {"start": 1920, "length": 960, "pitch": 67, "velocity": 80}]}}"#,
    );
    harness.write(
        &format!("{FOLDER}/filter.json"),
        r#"{"tool": "filter", "state": {"cutoff_hz": 900.0, "resonance": 0.4}}"#,
    );
    let plugins = [("late", LATE, 10), ("held", HELD, 30)];
    for (name, latency, offset) in plugins
        .into_iter()
        .filter(|(name, ..)| effects.contains(name))
    {
        harness.write(
            &format!("{FOLDER}/{name}.json"),
            &test_plugin_of(format, name),
        );
        let asset = harness.path(&format!("assets/plugin-state/{name}.bin"));
        std::fs::create_dir_all(asset.parent().unwrap()).unwrap();
        let state = SavedState {
            latency,
            offset,
            ..SavedState::default()
        };
        std::fs::write(&asset, plugin_state_of(format, state)).unwrap();
    }
    let path = harness.path(FOLDER);
    harness.apply(&[path]);
    assert_eq!(harness.project.problems(), [], "{format:?}");
    harness
}

fn effects(harness: &Harness) -> Vec<EffectSlot> {
    let track = harness
        .project
        .resolve::<TrackState>(&InstanceId::new("arrangement/piano").unwrap())
        .unwrap();
    harness.project.state(&track).unwrap().effects.clone()
}

/// The longest latency of the project, which the engine waits for after a play. It counts the
/// bypassed plugin too, see the module.
fn latency(harness: &mut Harness) -> u64 {
    harness.render(64);
    harness.project.engine().poll().unwrap().latency
}

const FRAMES: usize = 72_000;

#[test]
fn a_reorder_renders_what_a_project_written_in_that_order_renders() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        // What the first order plays, from a project of its own: a render leaves tails in the
        // processors, and the reordered project must start as fresh as the written one.
        let before = project(format, FIRST).play_from_the_start(FRAMES);
        let mut reordered = project(format, FIRST);
        assert_eq!(latency(&mut reordered), HELD as u64, "{format:?}");

        // The reorder, through the helper the rack calls, as one undo step.
        let track = reordered
            .project
            .resolve::<TrackState>(&InstanceId::new("arrangement/piano").unwrap())
            .unwrap();
        let late = InstanceId::new("arrangement/piano/late").unwrap();
        let mut changes = Changes::new();
        assert!(
            arrangement::move_effect(&reordered.project, &mut changes, &track, &late, 2).unwrap()
        );
        reordered.project.commit("Move late", changes).unwrap();
        let bypassed = |name: &str, bypass| EffectSlot {
            name: name.into(),
            bypass,
        };
        assert_eq!(
            effects(&reordered),
            [
                bypassed("filter", false),
                bypassed("held", true),
                bypassed("late", false)
            ]
        );
        assert_eq!(latency(&mut reordered), HELD as u64, "{format:?}");
        let after = reordered.play_from_the_start(FRAMES);

        let mut written = project(format, MOVED);
        assert_eq!(latency(&mut written), HELD as u64, "{format:?}");
        let expected = written.play_from_the_start(FRAMES);
        assert!(after.iter().any(|sample| sample.abs() > 0.01), "{format:?}");
        assert_eq!(
            after, expected,
            "{format:?}: the reorder plays as the written order"
        );
        // A bypassed effect is as if it were not there, wherever it is, latency and all.
        let without = project(format, r#"["filter", "late"]"#).play_from_the_start(FRAMES);
        assert_eq!(after, without, "{format:?}");
        // The order is audible: the offset of `late` goes through the filter or not.
        assert_ne!(before, after, "{format:?}");

        // Undo gives the first order back, with its bypass and its latency.
        assert_eq!(
            reordered.project.undo().unwrap().as_deref(),
            Some("Move late")
        );
        assert_eq!(
            effects(&reordered),
            [
                bypassed("late", false),
                bypassed("filter", false),
                bypassed("held", true)
            ]
        );
        assert_eq!(latency(&mut reordered), HELD as u64, "{format:?}");
    }
}
