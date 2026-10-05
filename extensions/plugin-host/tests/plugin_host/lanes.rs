//! Automation lanes on pins, where the host's own timing matters. What a lane sounds like in a
//! whole project is in the runtime's `plugin_lanes` tests.
//!
//! The rack of these tests stands in for a track: it holds a lane at one value for each name
//! in its record, as a lane player does every block. The heard `Level` of each test plugin
//! says what reached it.

use std::time::{Duration, Instant};

use plugin_host::PluginFormat;
use sound_core::Changes;

use crate::pins::{
    is_near, level, one_note, pin, pinned, pinned_value, plugin_value, written_value,
};
use crate::support::{FORMATS, Harness, Played, Rack, id, peak, tell_the_plugin};
use test_plugin_support::EDIT_LEVEL_KEY;

/// The name a lane gives the `Level` of the test plugin of `format`.
fn level_lane(format: PluginFormat) -> String {
    format!("parameters.{}.value", level(format))
}

/// The `Wave` of the test plugin of `format`: three named steps, which take no lane.
fn wave(format: PluginFormat) -> u32 {
    match format {
        PluginFormat::Clap => test_clap_plugin::WAVE,
        PluginFormat::Vst3 => test_vst3_plugin::WAVE,
    }
}

/// The lanes of the rack, as one undo step.
fn set_lanes(harness: &mut Harness, label: &str, lanes: Vec<(String, f32)>) {
    let rack = harness.project.resolve::<Rack>(&id("track")).unwrap();
    let mut changes = Changes::new();
    changes.set(&rack, Rack { lanes });
    harness.project.commit(label, changes).unwrap();
}

/// A track playing one note on the test plugin of `format`, with `Level` pinned at full and
/// `Wave` pinned too.
fn track(format: PluginFormat) -> Harness {
    let mut record = pinned(format, 1.0);
    record.parameters.insert(wave(format), pin("Wave", 0.0));
    let mut harness = Harness::new();
    harness.add_track(record, one_note());
    harness
}

/// The main-thread work of the host, at a time the test says.
fn poll(harness: &mut Harness, now: Instant) {
    harness.plugins.poll_at(&harness.project, now);
    let errors = harness.plugins.follow_pins_at(&mut harness.project, now);
    assert!(errors.is_empty(), "{errors:?}");
}

/// A stepped pin takes no lane. While a lane plays, the poll never writes what it plays into
/// the record, also once any turn of a knob would have ended, and a VST 3 plugin's own window
/// shows it, and the record value again once the lane goes.
#[test]
fn a_lane_is_never_written_into_the_record_and_the_window_follows_it() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let mut harness = track(format);
        let automatable: Vec<String> = harness
            .project
            .automatable(&id("track/instrument"))
            .map(str::to_string)
            .collect();
        assert_eq!(automatable, [level_lane(format)], "{format:?}");
        harness.play(1024);

        set_lanes(&mut harness, "Automate", vec![(level_lane(format), 0.25)]);
        harness.render(4096);
        let later = Instant::now() + Duration::from_secs(10);
        poll(&mut harness, later);
        assert_eq!(pinned_value(&harness, level(format)), 1.0, "{format:?}");
        assert_eq!(written_value(&harness, level(format)), 1.0, "{format:?}");
        assert_eq!(harness.project.undo_label(), Some("Automate"), "{format:?}");
        assert_eq!(plugin_value(&harness, level(format)), 0.25, "{format:?}");

        set_lanes(&mut harness, "Remove the lane", Vec::new());
        harness.render(4096);
        assert_eq!(plugin_value(&harness, level(format)), 1.0, "{format:?}");
        poll(&mut harness, later + Duration::from_secs(10));
        assert_eq!(pinned_value(&harness, level(format)), 1.0, "{format:?}");
        assert_eq!(
            harness.project.undo_label(),
            Some("Remove the lane"),
            "{format:?}"
        );
    }
}

/// A lane that comes and goes between two polls: the poll after it still sees the plugin play
/// the lane, because the engine has not taken the change yet. It is the lane's, not a change
/// of the plugin's own, and the record keeps its value.
#[test]
fn a_lane_that_comes_and_goes_between_two_polls_is_not_written() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let mut harness = track(format);
        let start = Instant::now();
        harness.project.engine().play();
        harness.render_without_polling(1024);
        poll(&mut harness, start);

        set_lanes(&mut harness, "Automate", vec![(level_lane(format), 0.25)]);
        harness.render_without_polling(1024);
        set_lanes(&mut harness, "Remove the lane", Vec::new());
        poll(&mut harness, start + Duration::from_secs(1));
        harness.render_without_polling(1024);
        for second in 2..10 {
            poll(&mut harness, start + Duration::from_secs(second));
        }
        assert_eq!(pinned_value(&harness, level(format)), 1.0, "{format:?}");
        assert_eq!(
            harness.project.undo_label(),
            Some("Remove the lane"),
            "{format:?}"
        );
    }
}

/// The composer turns `Level` in the VST 3 plugin's own window right after its lane went, before
/// the host has put the window back on the record value. The turn is kept and written, not
/// covered by the record value.
#[test]
fn a_turn_in_the_window_just_after_a_lane_goes_is_kept() {
    tell_the_plugin(None, None);
    let format = PluginFormat::Vst3;
    let played = vec![
        Played::On {
            frame: 0,
            pitch: 60,
            velocity: 100,
        },
        Played::On {
            frame: 5200,
            pitch: EDIT_LEVEL_KEY,
            velocity: 100,
        },
    ];
    let mut harness = Harness::new();
    harness.add_track(pinned(format, 1.0), played);
    harness.play(1024);
    set_lanes(&mut harness, "Automate", vec![(level_lane(format), 0.5)]);
    harness.render(4096);
    assert_eq!(plugin_value(&harness, level(format)), 0.5);

    set_lanes(&mut harness, "Remove the lane", Vec::new());
    harness.render_without_polling(512);
    let start = Instant::now();
    for second in 0..5 {
        poll(&mut harness, start + Duration::from_secs(second));
    }
    assert_eq!(plugin_value(&harness, level(format)), 0.25);
    assert_eq!(pinned_value(&harness, level(format)), 0.25);
}

/// Blocks with a lane playing, and the block in which it goes, allocate nothing on the audio
/// thread, inside the plugin's own call included.
#[test]
fn lanes_that_play_and_go_allocate_nothing() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let mut harness = track(format);
        let full = peak(&harness.play(1024).left());
        set_lanes(&mut harness, "Automate", vec![(level_lane(format), 0.25)]);
        harness.render(1024);
        let (render, allocations) = harness.render_counting_allocations(2048);
        assert_eq!(allocations, 0, "the audio thread allocated, {format:?}");
        assert!(is_near(peak(&render.left()), full / 4.0), "{format:?}");
        set_lanes(&mut harness, "Remove the lane", Vec::new());
        let (render, allocations) = harness.render_counting_allocations(2048);
        assert_eq!(allocations, 0, "the audio thread allocated, {format:?}");
        assert!(is_near(peak(&render.left()[1024..]), full), "{format:?}");
    }
}
