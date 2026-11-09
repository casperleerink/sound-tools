//! Automation lanes on pins, where the host's own timing matters. The test rack holds each lane
//! at one value every block, and the heard `Level` says what reached the plugin.

use std::time::{Duration, Instant};

use plugin_host::PluginFormat;
use sound_core::{Changes, MAX_BLOCK};

use crate::pins::{
    is_near, level, one_note, pin, pinned, pinned_value, plugin_value, written_value,
};
use crate::support::{FORMATS, Harness, Played, Rack, id, peak, tell_the_plugin};
use test_plugin_support::EDIT_LEVEL_KEY;

fn level_lane(format: PluginFormat) -> String {
    format!("parameters.{}.value", level(format))
}

/// Three named steps, which take no lane.
fn wave(format: PluginFormat) -> u32 {
    match format {
        PluginFormat::Clap => test_clap_plugin::WAVE,
        PluginFormat::Vst3 => test_vst3_plugin::WAVE,
    }
}

fn set_lanes(harness: &mut Harness, label: &str, lanes: Vec<(String, f32)>) {
    let rack = harness.project.resolve::<Rack>(&id("track")).unwrap();
    let mut changes = Changes::new();
    changes.set(&rack, Rack { lanes, bend: None });
    harness.project.commit(label, changes).unwrap();
}

fn track(format: PluginFormat) -> Harness {
    let mut record = pinned(format, 1.0);
    record.parameters.insert(wave(format), pin("Wave", 0.0));
    let mut harness = Harness::new();
    harness.add_track(record, one_note());
    harness
}

fn poll(harness: &mut Harness, now: Instant) {
    harness.plugins.poll_at(&harness.project, now);
    let errors = harness.plugins.follow_pins_at(&mut harness.project, now);
    assert!(errors.is_empty(), "{errors:?}");
}

/// A lane that bends inside a sub-block moves its pin from that frame on, not from the next
/// block: the plugin plays the sub-block in pieces cut where the lane bends.
#[test]
fn a_lane_that_bends_inside_a_sub_block_moves_its_pin_on_that_frame() {
    const BEND: usize = 24;
    for format in FORMATS {
        tell_the_plugin(None, None);
        let left = |bend| {
            let mut harness = track(format);
            harness.play(1024);
            let rack = harness.project.resolve::<Rack>(&id("track")).unwrap();
            let lanes = vec![(level_lane(format), 1.0)];
            let mut changes = Changes::new();
            changes.set(&rack, Rack { lanes, bend });
            harness.project.commit("Automate", changes).unwrap();
            harness.render(1024).left()
        };
        let steady = left(None);
        let bent = left(Some((BEND, 0.25)));
        for frame in 512..1024 {
            let level = if frame % MAX_BLOCK < BEND { 1.0 } else { 0.25 };
            let expected = steady[frame] * level;
            assert!(
                (bent[frame] - expected).abs() < 1e-6,
                "{format:?} frame {frame}: {} and not {expected}",
                bent[frame]
            );
        }
        assert!(peak(&steady[512..]) > 0.1, "{format:?}: the note plays");
    }
}

/// A stepped pin takes no lane. The poll never writes a lane's value, also once any turn of a
/// knob would have ended.
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
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Automate"),
            "{format:?}"
        );
        assert_eq!(plugin_value(&harness, level(format)), 0.25, "{format:?}");

        set_lanes(&mut harness, "Remove the lane", Vec::new());
        harness.render(4096);
        assert_eq!(plugin_value(&harness, level(format)), 1.0, "{format:?}");
        poll(&mut harness, later + Duration::from_secs(10));
        assert_eq!(pinned_value(&harness, level(format)), 1.0, "{format:?}");
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Remove the lane"),
            "{format:?}"
        );
    }
}

/// The poll after it still sees the plugin play the lane, as the engine has not taken the
/// change yet. That is not a change of the plugin's own.
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
            harness.project.undo_label().as_deref(),
            Some("Remove the lane"),
            "{format:?}"
        );
    }
}

/// Before the host has put the window back on the record value.
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

/// The test plugin keeps hundredths: playing 0.12 is it taking the record, not an edit.
#[test]
fn a_record_value_the_plugin_rounds_after_a_lane_is_not_written() {
    tell_the_plugin(None, None);
    let format = PluginFormat::Clap;
    let mut harness = Harness::new();
    harness.add_track(pinned(format, 0.123), one_note());
    harness.play(1024);
    set_lanes(&mut harness, "Automate", vec![(level_lane(format), 0.25)]);
    harness.render(2048);
    set_lanes(&mut harness, "Remove the lane", Vec::new());
    harness.render(2048);
    let start = Instant::now();
    for second in 0..5 {
        poll(&mut harness, start + Duration::from_secs(second));
    }
    assert_eq!(plugin_value(&harness, level(format)), 0.12);
    assert_eq!(pinned_value(&harness, level(format)), 0.123);
    assert_eq!(
        harness.project.undo_label().as_deref(),
        Some("Remove the lane")
    );
}

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
