//! Pins: parameters of a plugin that its record holds.
//!
//! Each test plugin has a `Level`: how loud its notes play. Each sets it itself on `LEVEL_KEY`.

use std::time::{Duration, Instant};

use plugin_host::{Pin, PluginFormat, PluginRecord};
use sound_core::{Changes, Instance};
use test_plugin_support::{EDIT_LEVEL_KEY, LATENCY_KEY, LEVEL_KEY, LIST_LEVEL_KEY, SavedState};

use crate::support::{
    FORMATS, Harness, Played, id, lifecycle, peak, record, state_asset, tell_the_plugin,
    tell_the_plugin_to_smooth, vst3_state,
};

pub(crate) fn level(format: PluginFormat) -> u32 {
    match format {
        PluginFormat::Clap => test_clap_plugin::LEVEL,
        PluginFormat::Vst3 => test_vst3_plugin::LEVEL,
    }
}

pub(crate) fn pin(name: &str, value: f64) -> Pin {
    Pin {
        name: name.to_string(),
        value,
    }
}

pub(crate) fn pinned(format: PluginFormat, value: f64) -> PluginRecord {
    let mut record = record(format, "piano");
    record.parameters.insert(level(format), pin("Level", value));
    record
}

fn on(frame: u64, pitch: u8, velocity: u8) -> Played {
    Played::On {
        frame,
        pitch,
        velocity,
    }
}

/// One long note, so a render is one level to read.
pub(crate) fn one_note() -> Vec<Played> {
    vec![on(0, 60, 100)]
}

fn instrument(harness: &Harness) -> Instance<PluginRecord> {
    let instance = harness.project.resolve(&id("track/instrument"));
    instance.expect("the plugin record")
}

fn record_of(harness: &Harness) -> PluginRecord {
    let record = harness.project.state(&instrument(harness));
    record.expect("the plugin record").clone()
}

pub(crate) fn pinned_value(harness: &Harness, pin: u32) -> f64 {
    record_of(harness).parameters[&pin].value
}

pub(crate) fn written_value(harness: &Harness, pin: u32) -> f64 {
    let text = std::fs::read_to_string(harness.path("state/track/instrument.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["state"]["parameters"][pin.to_string()]["value"]
        .as_f64()
        .expect("a pinned value in the file")
}

pub(crate) fn plugin_value(harness: &Harness, parameter: u32) -> f64 {
    let value = harness
        .plugins
        .parameter_value(&id("track/instrument"), parameter);
    value.expect("the plugin says").value
}

pub(crate) fn set_pin(harness: &mut Harness, pin: u32, value: f64) {
    let mut record = record_of(harness);
    record.parameters.get_mut(&pin).expect("the pin").value = value;
    let mut changes = Changes::new();
    changes.set(&instrument(harness), record);
    harness.project.commit("Set level", changes).unwrap();
}

fn full_level(format: PluginFormat) -> f32 {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), one_note());
    peak(&harness.play(512).left())
}

/// At a time the test says, so the end of a turn of a knob does not depend on how fast the
/// machine is.
fn step(harness: &mut Harness, frames: usize, now: Instant) -> Vec<f32> {
    let render = harness.render_without_polling(frames);
    harness.plugins.poll_at(&harness.project, now);
    harness.plugins.send_restarts(&mut harness.project);
    let errors = harness.plugins.follow_pins_at(&mut harness.project, now);
    assert!(errors.is_empty(), "{errors:?}");
    render.left()
}

pub(crate) fn is_near(level: f32, expected: f32) -> bool {
    (level - expected).abs() < 0.01
}

/// The record wins over the state the plugin saved.
#[test]
fn a_pinned_value_reaches_the_plugin_before_its_first_block_and_is_heard() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let full = full_level(format);
        let mut harness = Harness::new();
        let saved = test_plugin_support::save_state(SavedState {
            edit_level: 50,
            ..Default::default()
        });
        let bytes = match format {
            PluginFormat::Clap => saved,
            PluginFormat::Vst3 => vst3_state(&saved, b""),
        };
        harness
            .project
            .assets()
            .write(&state_asset("piano"), &bytes)
            .unwrap();
        harness.add_track(pinned(format, 0.25), one_note());
        let first = peak(&harness.play(512).left());
        assert!(is_near(first, full / 4.0), "{format:?}: {first} of {full}");
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
        harness.play(4096);
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Add track"),
            "{format:?}"
        );
        assert_eq!(pinned_value(&harness, level(format)), 0.25);
    }
}

/// A smoothing plugin glides to a value sent while it plays, but a pin it has before it is
/// activated has nothing to glide from.
#[test]
fn a_plugin_that_smooths_plays_its_pins_from_the_first_frame() {
    tell_the_plugin_to_smooth();
    for format in FORMATS {
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 0.25), one_note());
        let left = harness.play(4096).left();
        let (first, steady) = (peak(&left[..48]), peak(&left[1024..]));
        assert!(is_near(first, steady), "{format:?}: {first} then {steady}");

        // An edit while it plays glides, so the check above is not true of any plugin.
        set_pin(&mut harness, level(format), 1.0);
        let left = harness.render(4096).left();
        let (first, steady) = (peak(&left[..48]), peak(&left[1024..]));
        assert!(first < steady * 0.6, "{format:?}: {first} then {steady}");
    }
}

/// A short note starts every block, so whichever block comes first after the restart has one.
#[test]
fn a_pin_changed_while_the_plugin_is_started_again_is_played_from_the_first_frame() {
    tell_the_plugin_to_smooth();
    for format in FORMATS {
        let mut harness = Harness::new();
        let mut played = vec![on(0, 60, 100), on(1024, LATENCY_KEY, 1)];
        for block in 3..40 {
            let frame = block * 512;
            played.push(on(frame, 64, 100));
            played.push(Played::Off {
                frame: frame + 256,
                pitch: 64,
            });
        }
        harness.add_track(pinned(format, 1.0), played);
        let now = Instant::now();
        for _ in 0..3 {
            step(&mut harness, 512, now);
        }
        set_pin(&mut harness, level(format), 0.25);
        let left: Vec<f32> = (0..32).flat_map(|_| step(&mut harness, 512, now)).collect();
        let gap = left
            .windows(512)
            .position(|block| block.iter().all(|it| *it == 0.0));
        let gap = gap.expect("a silent block while the plugin is started again") + 512;
        let start = gap
            + left[gap..]
                .iter()
                .position(|it| *it != 0.0)
                .expect("a note");
        let (first, later) = (peak(&left[start..start + 48]), peak(&left[start + 2048..]));
        assert!(is_near(first, later), "{format:?}: {first} then {later}");
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    }
}

/// Six Sines takes a state it reads only at a flush or in a block, and taken in the first block
/// it ends the notes of that block, so a render with a note at its start is silent.
#[test]
fn a_flush_a_plugin_asks_for_as_it_reads_its_state_comes_before_its_activation() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    tell_the_plugin(Some(&log), None);
    let mut harness = Harness::new();
    let saved = test_plugin_support::save_state(SavedState::default());
    let asset = state_asset("piano");
    harness.project.assets().write(&asset, &saved).unwrap();
    harness.add_track(record(PluginFormat::Clap, "piano"), one_note());
    harness.play(512);
    let calls: Vec<String> = lifecycle(&log).into_iter().map(|call| call.call).collect();
    let at = |name: &str| calls.iter().position(|call| call == name);
    assert!(at("flush").is_some(), "{calls:?}");
    assert!(at("flush") < at("activate"), "{calls:?}");
}

/// The test plugin rounds to hundredths, which must not be written back over the record's value.
#[test]
fn an_edit_of_a_pin_moves_the_plugin_without_loading_it_again_and_is_not_written_back() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        tell_the_plugin(Some(&log), None);
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 1.0), one_note());
        let full = peak(&harness.play(1024).left());

        set_pin(&mut harness, level(format), 0.333);
        let left = harness.render(4096).left();
        let after = peak(&left[2048..]);
        assert!(is_near(after, full * 0.33), "{format:?}: {after} of {full}");
        let loads = lifecycle(&log)
            .into_iter()
            .filter(|call| call.call == "activate");
        assert_eq!(loads.count(), 1, "{format:?}: the plugin was loaded again");

        // Also once any turn of a knob would have ended.
        assert_eq!(plugin_value(&harness, level(format)), 0.33, "{format:?}");
        let later = Instant::now() + Duration::from_secs(1);
        harness.plugins.follow_pins_at(&mut harness.project, later);
        assert_eq!(pinned_value(&harness, level(format)), 0.333, "{format:?}");
        assert_eq!(written_value(&harness, level(format)), 0.333, "{format:?}");
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Set level"),
            "{format:?}"
        );

        harness.project.undo().unwrap();
        let left = harness.render(4096).left();
        assert!(is_near(peak(&left[2048..]), full), "{format:?}");
    }
}

/// Three moves a block or more apart, then a rest: one undo step.
#[test]
fn a_value_the_plugin_changes_itself_lands_in_the_record_as_one_undo_step() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let played = vec![
            on(0, 60, 100),
            on(1024, LEVEL_KEY, 100),
            on(2048, LEVEL_KEY, 64),
            on(3072, LEVEL_KEY, 32),
        ];
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 1.0), played);
        harness.project.engine().play();
        let start = Instant::now();
        for block in 0..8 {
            step(
                &mut harness,
                512,
                start + Duration::from_millis(100 * block),
            );
        }
        let own = plugin_value(&harness, level(format));
        assert_eq!(own, 0.25, "{format:?}");
        assert_eq!(pinned_value(&harness, level(format)), own, "{format:?}");
        // Still turning, as far as the host can tell.
        assert_eq!(written_value(&harness, level(format)), 1.0, "{format:?}");

        step(&mut harness, 512, start + Duration::from_secs(2));
        assert_eq!(written_value(&harness, level(format)), own, "{format:?}");
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Change Level"),
            "{format:?}"
        );
        harness.project.undo().unwrap();
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Add track"),
            "{format:?}"
        );
        assert_eq!(pinned_value(&harness, level(format)), 1.0, "{format:?}");
        let later = start + Duration::from_secs(3);
        step(&mut harness, 512, later);
        let left = step(&mut harness, 512, later);
        assert_eq!(plugin_value(&harness, level(format)), 1.0, "{format:?}");
        assert!(peak(&left) > 0.5, "{format:?}");
    }
}

/// With `endEdit`, so no time has to pass.
#[test]
fn a_turn_of_a_knob_in_a_vst3_window_is_one_undo_step_that_ends_where_the_hand_lets_go() {
    tell_the_plugin(None, None);
    let format = PluginFormat::Vst3;
    let mut harness = Harness::new();
    harness.add_track(
        pinned(format, 1.0),
        vec![on(0, 60, 100), on(1024, EDIT_LEVEL_KEY, 1)],
    );
    harness.project.engine().play();
    let now = Instant::now();
    for _ in 0..6 {
        step(&mut harness, 512, now);
    }
    assert_eq!(written_value(&harness, level(format)), 0.25);
    assert_eq!(
        harness.project.undo_label().as_deref(),
        Some("Change Level")
    );
    harness.project.undo().unwrap();
    assert_eq!(harness.project.undo_label().as_deref(), Some("Add track"));
}

#[test]
fn an_unknown_pin_and_a_value_out_of_range_are_problems_and_the_rest_plays() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let full = full_level(format);
        let mut record = pinned(format, 0.25);
        let (wave, outside) = match format {
            PluginFormat::Clap => (test_clap_plugin::WAVE, 3.0),
            PluginFormat::Vst3 => (test_vst3_plugin::WAVE, 2.0),
        };
        record.parameters.insert(999, pin("Nothing", 0.5));
        record.parameters.insert(wave, pin("Wave", outside));
        let mut harness = Harness::new();
        harness.add_track(record, one_note());

        let problems = harness.problems();
        assert_eq!(problems.len(), 2, "{format:?}: {problems:?}");
        let says = |text: &str| problems.iter().any(|problem| problem.contains(text));
        assert!(says("no parameter with the id 999"), "{problems:?}");
        let outside = format!("`parameters.{wave}.value` is {outside}, outside the range");
        assert!(says(&outside), "{problems:?}");
        let first = peak(&harness.play(512).left());
        assert!(is_near(first, full / 4.0), "{format:?}: {first} of {full}");

        let mut fixed = record_of(&harness);
        fixed.parameters.retain(|pin, _| *pin == level(format));
        let mut changes = Changes::new();
        changes.set(&instrument(&harness), fixed);
        harness.project.commit("Fix pins", changes).unwrap();
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    }
}

/// CLAP says so with a `rescan`, VST 3 with `kParamIDMappingChanged`.
#[test]
fn a_pin_of_a_parameter_the_plugin_lists_later_moves_it_once_it_is_listed() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        // SAFETY: as in `tell_the_plugin`.
        unsafe { std::env::set_var(test_plugin_support::LATE_LEVEL_VARIABLE, "1") };
        let full = full_level(format);
        let mut harness = Harness::new();
        let played = vec![on(0, 60, 100), on(512, LIST_LEVEL_KEY, 100)];
        harness.add_track(pinned(format, 0.25), played);
        let problems = harness.problems();
        assert_eq!(problems.len(), 1, "{format:?}: {problems:?}");
        assert!(
            problems[0].contains("no parameter with the id"),
            "{problems:?}"
        );

        let left = harness.play(2048).left();
        assert!(is_near(peak(&left[..512]), full), "{format:?}");
        // What the runtime does at every tick.
        for retry in harness.plugins.take_retries() {
            harness.project.rebind(&retry).unwrap();
        }
        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
        let left = harness.render(2048).left();
        assert!(is_near(peak(&left[1024..]), full / 4.0), "{format:?}");
        // SAFETY: as in `tell_the_plugin`.
        unsafe { std::env::remove_var(test_plugin_support::LATE_LEVEL_VARIABLE) };
    }
}

#[test]
fn a_pin_that_changes_while_the_plugin_plays_allocates_nothing() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 1.0), one_note());
        let full = peak(&harness.play(1024).left());
        // Sent now, so the block that takes it is one that is counted.
        set_pin(&mut harness, level(format), 0.5);
        assert!(harness.plugins.follow_pins(&mut harness.project).is_empty());
        let (render, allocations) = harness.render_counting_allocations(2048);
        assert_eq!(allocations, 0, "the audio thread allocated, {format:?}");
        assert!(is_near(peak(&render.left()), full / 2.0), "{format:?}");
    }
}

fn count(log: &std::path::Path, call: &str) -> usize {
    lifecycle(log)
        .iter()
        .filter(|line| line.call == call)
        .count()
}

/// One [`step`] per block, a tenth of a second apart.
fn steps(harness: &mut Harness, blocks: std::ops::Range<u32>, start: Instant) -> Vec<f32> {
    let mut left = Vec::new();
    for block in blocks {
        let now = start + Duration::from_millis(100) * block;
        left.extend(step(harness, 512, now));
    }
    left
}

/// The plugin moves its own `Level` to a half, then asks to be started again. It must not get
/// the older value that was sent before.
#[test]
fn a_plugin_started_again_gets_the_pins_as_the_record_has_them_now() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let full = full_level(format);
        let log = tempfile::NamedTempFile::new().unwrap();
        tell_the_plugin(Some(log.path()), None);
        let played = vec![
            on(0, 60, 100),
            on(1024, LEVEL_KEY, 64),
            on(2048, LATENCY_KEY, 10),
            on(6144, 60, 100),
        ];
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 1.0), played);
        harness.project.engine().play();
        let start = Instant::now();
        let left = steps(&mut harness, 0..16, start);
        assert_eq!(count(log.path(), "activate"), 2, "{format:?}");
        assert_eq!(count(log.path(), "mode[realtime]"), 1, "{format:?}");
        assert_eq!(plugin_value(&harness, level(format)), 0.5, "{format:?}");
        assert_eq!(pinned_value(&harness, level(format)), 0.5, "{format:?}");
        let after = peak(&left[6656..]);
        assert!(is_near(after, full / 2.0), "{format:?}: {after} of {full}");
        steps(&mut harness, 16..40, start);
        assert_eq!(written_value(&harness, level(format)), 0.5, "{format:?}");
    }
}

#[test]
fn a_pin_that_changes_while_the_plugin_starts_again_does_not_load_it_again() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let full = full_level(format);
        let log = tempfile::NamedTempFile::new().unwrap();
        tell_the_plugin(Some(log.path()), None);
        let played = vec![on(0, 60, 100), on(1024, LATENCY_KEY, 10), on(4096, 60, 100)];
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 1.0), played);
        harness.project.engine().play();
        let start = Instant::now();
        steps(&mut harness, 0..3, start);
        set_pin(&mut harness, level(format), 0.25);
        let left = steps(&mut harness, 3..12, start);
        assert_eq!(count(log.path(), "activate"), 2, "{format:?}");
        assert_eq!(count(log.path(), "mode[realtime]"), 1, "{format:?}");
        assert_eq!(plugin_value(&harness, level(format)), 0.25, "{format:?}");
        let after = peak(&left[3072..]);
        assert!(is_near(after, full / 4.0), "{format:?}: {after} of {full}");
    }
}

/// On a device the new audio side may play before the next poll of the pins.
#[test]
fn a_plugin_started_again_has_its_pins_before_its_first_block() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let full = full_level(format);
        let played = vec![on(0, 60, 100), on(1024, LATENCY_KEY, 10), on(4096, 60, 100)];
        let mut harness = Harness::new();
        harness.add_track(pinned(format, 1.0), played);
        harness.project.engine().play();
        let start = Instant::now();
        steps(&mut harness, 0..3, start);
        // Never followed by the poll of the pins.
        set_pin(&mut harness, level(format), 0.25);
        let mut left = Vec::new();
        for _ in 3..12 {
            left.extend(harness.render_without_polling(512).left());
            harness.plugins.poll(&harness.project);
            harness.plugins.send_restarts(&mut harness.project);
        }
        let after = peak(&left[3072..]);
        assert!(is_near(after, full / 4.0), "{format:?}: {after} of {full}");
    }
}

#[test]
fn a_turn_whose_record_is_deleted_ends_and_the_delete_is_the_one_step() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let mut harness = Harness::new();
        let played = vec![on(0, 60, 100), on(1024, LEVEL_KEY, 64)];
        harness.add_track(pinned(format, 1.0), played);
        harness.project.engine().play();
        let start = Instant::now();
        steps(&mut harness, 0..4, start);
        assert_eq!(pinned_value(&harness, level(format)), 0.5, "{format:?}");
        let mut changes = Changes::new();
        changes.delete(&id("track/instrument"));
        harness.project.commit("Delete", changes).unwrap();
        // Well inside the quiet time of a turn.
        steps(&mut harness, 4..5, start);
        let record = harness
            .project
            .resolve::<PluginRecord>(&id("track/instrument"));
        assert!(record.is_none(), "{format:?}");
        assert!(!harness.path("state/track/instrument.json").exists());
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Delete"),
            "{format:?}"
        );
        harness.project.undo().unwrap();
        assert_eq!(written_value(&harness, level(format)), 1.0, "{format:?}");
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Add track"),
            "{format:?}"
        );
    }
}

/// A write of another pin mid-turn is the one step for everything before it, the turn so far
/// included.
#[test]
fn a_write_of_the_record_during_a_turn_is_one_step_and_the_moves_after_it_another() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let wave = match format {
            PluginFormat::Clap => test_clap_plugin::WAVE,
            PluginFormat::Vst3 => test_vst3_plugin::WAVE,
        };
        let mut record = pinned(format, 1.0);
        record.parameters.insert(wave, pin("Wave", 0.0));
        let played = vec![
            on(0, 60, 100),
            on(1024, LEVEL_KEY, 64),
            on(12288, LEVEL_KEY, 32),
        ];
        let mut harness = Harness::new();
        harness.add_track(record, played);
        harness.project.engine().play();
        let start = Instant::now();
        steps(&mut harness, 0..4, start);
        let mut written = record_of(&harness);
        written.parameters.get_mut(&wave).expect("the pin").value = 1.0;
        let mut changes = Changes::new();
        changes.set(&instrument(&harness), written);
        harness.project.commit("Set wave", changes).unwrap();
        steps(&mut harness, 4..40, start);
        assert_eq!(written_value(&harness, level(format)), 0.25, "{format:?}");

        let undo = |harness: &mut Harness| {
            let label = harness.project.undo().unwrap();
            let record = record_of(harness);
            let values = (
                record.parameters[&level(format)].value,
                record.parameters[&wave].value,
            );
            (label, values)
        };
        assert_eq!(
            undo(&mut harness),
            (Some("Change Level".to_string()), (0.5, 1.0)),
            "{format:?}"
        );
        assert_eq!(
            undo(&mut harness),
            (Some("Set wave".to_string()), (1.0, 0.0)),
            "{format:?}"
        );
        assert_eq!(
            harness.project.undo_label().as_deref(),
            Some("Add track"),
            "{format:?}"
        );
    }
}

/// More values than the ring holds, sent before a block plays.
#[test]
fn a_clap_value_that_finds_the_ring_full_waits_and_arrives() {
    tell_the_plugin(None, None);
    let format = PluginFormat::Clap;
    let full = full_level(format);
    let mut harness = Harness::new();
    harness.add_track(pinned(format, 1.0), one_note());
    harness.project.engine().play();
    let start = Instant::now();
    steps(&mut harness, 0..1, start);
    for hundredths in 1..=70 {
        set_pin(&mut harness, level(format), f64::from(hundredths) / 100.0);
        let errors = harness.plugins.follow_pins(&mut harness.project);
        assert!(errors.is_empty(), "{errors:?}");
    }
    let left = steps(&mut harness, 1..4, start);
    assert_eq!(plugin_value(&harness, level(format)), 0.7);
    let last = peak(&left[1024..]);
    assert!(is_near(last, full * 0.7), "{last} of {full}");
}

/// The processor reports a `Level` of its own in a block the host has not polled yet, and the
/// record then sends another. The controller must not be put back on the older report.
#[test]
fn a_vst3_report_older_than_a_value_the_host_sent_is_left_out() {
    tell_the_plugin(None, None);
    let format = PluginFormat::Vst3;
    let mut harness = Harness::new();
    let played = vec![on(0, 60, 100), on(1024, LEVEL_KEY, 64)];
    harness.add_track(pinned(format, 1.0), played);
    harness.project.engine().play();
    let start = Instant::now();
    steps(&mut harness, 0..2, start);
    harness.render_without_polling(512);
    set_pin(&mut harness, level(format), 0.25);
    let errors = harness.plugins.follow_pins_at(&mut harness.project, start);
    assert!(errors.is_empty(), "{errors:?}");
    steps(&mut harness, 3..30, start);
    assert_eq!(plugin_value(&harness, level(format)), 0.25);
    assert_eq!(written_value(&harness, level(format)), 0.25);
}

/// The same with the edit ring full: a report from a block that took every edit that had room
/// is still older than the value that waits.
#[test]
fn a_vst3_report_made_while_a_sent_value_waits_for_room_is_left_out() {
    tell_the_plugin(None, None);
    let format = PluginFormat::Vst3;
    let mut harness = Harness::new();
    let played = vec![on(0, 60, 100), on(1024, LEVEL_KEY, 64)];
    harness.add_track(pinned(format, 1.0), played);
    harness.project.engine().play();
    let start = Instant::now();
    steps(&mut harness, 0..2, start);
    // Each value differs from the one before, so each is sent.
    for step in 0..520 {
        let value = match step {
            519 => 0.25,
            even if even % 2 == 0 => 0.75,
            _ => 0.7,
        };
        set_pin(&mut harness, level(format), value);
        let errors = harness.plugins.follow_pins_at(&mut harness.project, start);
        assert!(errors.is_empty(), "{errors:?}");
    }
    steps(&mut harness, 2..30, start);
    assert_eq!(plugin_value(&harness, level(format)), 0.25);
    assert_eq!(written_value(&harness, level(format)), 0.25);
}
