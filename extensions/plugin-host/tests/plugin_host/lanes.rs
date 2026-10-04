//! Automation lanes on pins: a pin the plugin says a host may automate, and that is not
//! stepped, takes a lane by its path in the record. The lane plays into the plugin on the audio
//! thread and is never written into the record; when it goes, the record value plays again.
//!
//! The rack of these tests stands in for a track: it holds a lane at one value for each name
//! in its record, as a lane player does every block. The heard `Level` of each test plugin
//! says what reached it.

use std::time::{Duration, Instant};

use plugin_host::PluginFormat;
use sound_core::Changes;

use crate::pins::{
    is_near, level, one_note, pin, pinned, pinned_value, plugin_value, set_pin, written_value,
};
use crate::support::{FORMATS, Harness, Rack, id, peak, tell_the_plugin};

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

#[test]
fn a_lane_moves_its_pin_never_writes_the_record_and_the_record_plays_again_when_it_goes() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let mut harness = track(format);
        // A stepped pin takes no lane.
        let automatable: Vec<String> = harness
            .project
            .automatable(&id("track/instrument"))
            .map(str::to_string)
            .collect();
        assert_eq!(automatable, [level_lane(format)], "{format:?}");
        let full = peak(&harness.play(1024).left());

        set_lanes(&mut harness, "Automate", vec![(level_lane(format), 0.25)]);
        let left = harness.render(4096).left();
        let laned = peak(&left[2048..]);
        assert!(is_near(laned, full / 4.0), "{format:?}: {laned} of {full}");
        // The plugin plays the lane, and the record and the undo history do not know it, also
        // once any turn of a knob would have ended.
        let later = Instant::now() + Duration::from_secs(10);
        let errors = harness.plugins.follow_pins_at(&mut harness.project, later);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(pinned_value(&harness, level(format)), 1.0, "{format:?}");
        assert_eq!(written_value(&harness, level(format)), 1.0, "{format:?}");
        assert_eq!(harness.project.undo_label(), Some("Automate"), "{format:?}");

        // A record value under a lane does not sound, and is the one that comes back.
        set_pin(&mut harness, level(format), 0.5);
        let left = harness.render(4096).left();
        assert!(is_near(peak(&left[2048..]), full / 4.0), "{format:?}");
        set_lanes(&mut harness, "Remove the lane", Vec::new());
        let left = harness.render(4096).left();
        let back = peak(&left[2048..]);
        assert!(is_near(back, full / 2.0), "{format:?}: {back} of {full}");
        if format == PluginFormat::Clap {
            assert_eq!(plugin_value(&harness, level(format)), 0.5);
        }
        harness.plugins.follow_pins_at(&mut harness.project, later);
        assert_eq!(pinned_value(&harness, level(format)), 0.5, "{format:?}");
        assert_eq!(
            harness.project.undo_label(),
            Some("Remove the lane"),
            "{format:?}"
        );
    }
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
