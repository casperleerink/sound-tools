//! Automation lanes of a track on the pins of a hosted plugin, in the track record as an agent
//! writes them: the lane plays where its points say, the same to the byte in every render, the
//! record wins again when the lane goes, playing writes nothing, and a lane on a pin that takes
//! none is reported while the rest plays.
//!
//! The `Level` of each test plugin is heard: how loud its note plays.

use plugin_host::PluginFormat;

use crate::support::{Harness, difference};

/// Frames per tick at 120 bpm and 48 kHz.
const TICK: usize = 25;
const BAR: u64 = 3840;
const FORMATS: [PluginFormat; 2] = [PluginFormat::Clap, PluginFormat::Vst3];

/// The `Level` of the test plugin of `format`.
fn level(format: PluginFormat) -> u32 {
    match format {
        PluginFormat::Clap => test_clap_plugin::LEVEL,
        PluginFormat::Vst3 => test_vst3_plugin::LEVEL,
    }
}

/// A lane on the pin `pin` of the instrument, as `(tick, value)` points.
fn lane(pin: u32, points: &[(u64, f32)]) -> String {
    let points: Vec<String> = points
        .iter()
        .map(|(tick, value)| format!(r#"{{"tick": {tick}, "value": {value:?}}}"#))
        .collect();
    format!(
        r#"{{"device": "instrument", "parameter": "parameters.{pin}.value", "points": [{}]}}"#,
        points.join(", ")
    )
}

/// Writes the track `piano`: one note through two bars on the test plugin of `format`, with
/// `Level` pinned at `record`, `Wave` pinned too, and `lanes` in the track record.
fn write_track(harness: &mut Harness, format: PluginFormat, record: f64, lanes: &[String]) {
    let (plugin_id, wave) = match format {
        PluginFormat::Clap => (test_clap_plugin::PLUGIN_ID, test_clap_plugin::WAVE),
        PluginFormat::Vst3 => (test_vst3_plugin::PLUGIN_ID, test_vst3_plugin::WAVE),
    };
    let level = level(format);
    let folder = "state/arrangement/piano";
    let track = format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "piano", "order": 1, "automation": [{}]}}}}"#,
        lanes.join(", ")
    );
    let track = track.replace(r#", "automation": []"#, "");
    let instrument = format!(
        r#"{{"tool": "plugin", "state": {{"format": "{}", "plugin_id": "{plugin_id}", "state_asset": "piano", "parameters": {{"{level}": {{"name": "Level", "value": {record:?}}}, "{wave}": {{"name": "Wave", "value": 0.0}}}}}}}}"#,
        format.as_str()
    );
    let clip = r#"{"tool": "arrangement.clip", "state": {"start": 0, "length": 7680, "notes": [{"start": 0, "length": 7680, "pitch": 60, "velocity": 100}]}}"#;
    let paths = [
        harness.write(&format!("{folder}/instance.json"), &track),
        harness.write(&format!("{folder}/instrument.json"), &instrument),
        harness.write(&format!("{folder}/note.json"), clip),
    ];
    assert!(harness.apply(&paths) >= 1);
}

/// The loudest sample of the left channel of a stereo render, from tick `from` to tick `to`.
fn peak(render: &[f32], from: u64, to: u64) -> f32 {
    let (from, to) = (from as usize * TICK, to as usize * TICK);
    let left = render[2 * from..2 * to].iter().step_by(2);
    left.fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// How loud the note plays with `Level` pinned at full and no lane.
fn full_level(harness: &mut Harness, format: PluginFormat) -> f32 {
    write_track(harness, format, 1.0, &[]);
    let full = peak(&harness.play_from_the_start(BAR as usize * TICK), 0, BAR);
    assert!(full > 0.5, "{format:?}: the plugin is silent");
    full
}

fn is_near(level: f32, expected: f32) -> bool {
    (level - expected).abs() < 0.03
}

/// The value of the pin `Level` in the record of the instrument.
fn pinned(harness: &Harness, format: PluginFormat) -> f64 {
    let text = std::fs::read_to_string(harness.path("state/arrangement/piano/instrument.json"));
    let json: serde_json::Value = serde_json::from_str(&text.unwrap()).unwrap();
    let value = &json["state"]["parameters"][level(format).to_string()]["value"];
    value.as_f64().unwrap()
}

/// A rise of `Level` over the first bar, from silence to full: the note is as loud as the lane
/// is at each quarter, every render is the same, and nothing of it reaches the record.
#[test]
fn a_lane_on_a_pin_moves_the_plugin_as_its_points_say_and_renders_the_same_every_time() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let (mut harness, _plugins) = Harness::with_test_plugin(folder);
        let full = full_level(&mut harness, format);

        let rise = lane(level(format), &[(0, 0.0), (BAR, 1.0)]);
        write_track(&mut harness, format, 1.0, &[rise]);
        assert_eq!(harness.project.problems(), [], "{format:?}");
        let undo = harness.project.undo_label().map(str::to_string);
        let first = harness.play_from_the_start(2 * BAR as usize * TICK);
        for quarter in [1, 2, 3] {
            let tick = quarter * BAR / 4;
            let heard = peak(&first, tick - 48, tick + 48);
            let expected = full * quarter as f32 / 4.0;
            assert!(
                is_near(heard, expected),
                "{format:?}: {heard} at quarter {quarter}, not {expected}"
            );
        }
        assert!(
            is_near(peak(&first, BAR + 960, 2 * BAR), full),
            "{format:?}"
        );
        let second = harness.play_from_the_start(2 * BAR as usize * TICK);
        assert_eq!(difference(&first, &second), None, "{format:?}");
        assert_eq!(pinned(&harness, format), 1.0, "{format:?}");
        assert_eq!(harness.project.undo_label(), undo.as_deref(), "{format:?}");
    }
}

/// The lane holds a quarter over a record of a half. Without the lane the half plays again.
#[test]
fn taking_the_lane_out_plays_the_record_again() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let (mut harness, _plugins) = Harness::with_test_plugin(folder);
        let full = full_level(&mut harness, format);

        let quarter = lane(level(format), &[(0, 0.25)]);
        write_track(&mut harness, format, 0.5, &[quarter]);
        let laned = harness.play_from_the_start(BAR as usize * TICK);
        assert!(is_near(peak(&laned, 960, BAR), full / 4.0), "{format:?}");
        write_track(&mut harness, format, 0.5, &[]);
        let back = harness.play_from_the_start(BAR as usize * TICK);
        assert!(is_near(peak(&back, 960, BAR), full / 2.0), "{format:?}");
        assert_eq!(pinned(&harness, format), 0.5, "{format:?}");
    }
}

/// A lane on a pin with named steps and one on a parameter the record does not pin are each
/// a problem that names the lane, and the lane on `Level` plays.
#[test]
fn a_lane_on_a_pin_that_takes_none_is_reported_and_the_rest_plays() {
    let format = PluginFormat::Clap;
    let folder = tempfile::tempdir().unwrap();
    let (mut harness, _plugins) = Harness::with_test_plugin(folder);
    let full = full_level(&mut harness, format);

    let lanes = [
        lane(test_clap_plugin::WAVE, &[(0, 1.0)]),
        lane(test_clap_plugin::CUTOFF, &[(0, 400.0)]),
        lane(level(format), &[(0, 0.25)]),
    ];
    write_track(&mut harness, format, 1.0, &lanes);
    let problems: Vec<String> = harness
        .project
        .problems()
        .into_iter()
        .map(|problem| problem.message)
        .collect();
    let level = level(format);
    assert_eq!(
        problems,
        [
            format!(
                r#"automation[0].parameter is "parameters.1.value", and instrument takes no automation of a number of that name. It takes parameters.{level}.value, so the lane moves nothing"#
            ),
            format!(
                r#"automation[1].parameter is "parameters.0.value", and instrument takes no automation of a number of that name. It takes parameters.{level}.value, so the lane moves nothing"#
            ),
        ]
    );
    let render = harness.play_from_the_start(BAR as usize * TICK);
    assert!(is_near(peak(&render, 960, BAR), full / 4.0));
}
