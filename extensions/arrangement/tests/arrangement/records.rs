//! Records that do not load, and the typed helpers for interfaces.

use arrangement::{Colour, TrackState, add_clip, add_track, clips, move_clip, tracks};
use sound_core::Changes;
use sound_notes::Clip;

use crate::support::{Harness, Probe, TICK, clip, id, level_changes, note};

const CLIP: &str = "state/arrangement/piano/clip-0.json";

fn clip_record(state: &str) -> String {
    format!(r#"{{"tool": "arrangement.clip", "state": {state}}}"#)
}

#[test]
fn an_invalid_clip_names_the_field_and_the_last_valid_clip_keeps_playing() {
    let cases = [
        (
            r#"{"start": 0, "length": 3840, "notes": [{"start": 0, "length": 480, "pitch": 128, "velocity": 100}]}"#,
            "state.notes[0].pitch: pitch must be from 0 to 127, not 128",
        ),
        (
            r#"{"start": 0, "length": 3840, "notes": [{"start": 0, "length": 0, "pitch": 60, "velocity": 100}]}"#,
            "state.notes[0].length: length must be 1 tick or more, not 0",
        ),
        (
            r#"{"start": 0, "length": 0, "notes": []}"#,
            "state.length: length must be 1 tick or more, not 0",
        ),
        (
            r#"{"start": -1, "length": 3840, "notes": []}"#,
            "state.start: invalid value: integer `-1`, expected u64",
        ),
        (
            r#"{"start": 0, "length": 3840, "notes": [], "name": "Verse"}"#,
            "state.name: unknown field `name`, expected one of `start`, `length`, `notes`, `pedal`",
        ),
        (
            r#"{"start": 15360, "length": 3840, "notes": [{"start": 0, "length": 480, "pitch": 60, "velocity": 100}, {"start": 15360, "length": 480, "pitch": 60, "velocity": 100}]}"#,
            "state: notes[1].start must be less than the clip length 3840, not 15360. A note start counts from the start of its clip, not from the start of the project",
        ),
    ];
    for (state, message) in cases {
        let mut harness = Harness::with_clips(vec![clip(0, 3840, vec![note(0, 480, 60)])]);
        assert_eq!(harness.write_and_apply(CLIP, &clip_record(state)), 0);
        assert_eq!(harness.problems(), [format!("{CLIP}: {message}")]);
        assert_eq!(
            level_changes(&harness.play(960 * TICK)),
            [(0, 60.0), (480 * TICK, 0.0)]
        );
    }
}

#[test]
fn an_invalid_track_names_the_field_and_the_track_stays_as_it_was() {
    let cases = [
        (
            r#"{"name": "Piano", "colour": "purple"}"#,
            "state.colour: unknown variant `purple`, expected one of `blue`, `sapphire`, `sky`, `teal`, `green`, `yellow`, `peach`, `red`, `maroon`, `mauve`, `pink`, `lavender`, `rosewater`, `flamingo`",
        ),
        (r#"{"name": " "}"#, "state: name must not be empty"),
        (r#"{"colour": "red"}"#, "state: missing field `name`"),
        (
            r#"{"name": "Piano", "order": -2}"#,
            "state.order: invalid value: integer `-2`, expected u32",
        ),
    ];
    let file = "state/arrangement/piano/instance.json";
    for (state, message) in cases {
        let mut harness = Harness::with_clips(vec![clip(0, 3840, vec![note(0, 480, 60)])]);
        let record = format!(r#"{{"tool": "arrangement.track", "state": {state}}}"#);
        assert_eq!(harness.write_and_apply(file, &record), 0);
        assert_eq!(harness.problems(), [format!("{file}: {message}")]);
        let track = harness
            .project
            .resolve::<TrackState>(&id("arrangement/piano"));
        assert_eq!(
            harness.project.state(&track.unwrap()).unwrap().name,
            "piano"
        );
        assert_eq!(
            level_changes(&harness.play(960 * TICK)),
            [(0, 60.0), (480 * TICK, 0.0)]
        );
    }
}

#[test]
fn a_track_record_needs_only_its_name() {
    let mut harness = Harness::new();
    let record = r#"{"tool": "arrangement.track", "state": {"name": "Strings"}}"#;
    harness.write_and_apply("state/arrangement/strings/instance.json", record);
    assert!(harness.problems().is_empty());
    let track = harness
        .project
        .resolve::<TrackState>(&id("arrangement/strings"));
    // Gain, pan and mute are left out: the middle, no change of level, not muted.
    let expected = TrackState::new("Strings", Colour::Blue, 0);
    assert_eq!(harness.project.state(&track.unwrap()), Some(&expected));
}

#[test]
fn tracks_show_by_order_and_then_by_id() {
    let mut harness = Harness::new();
    for (name, order) in [("b", 1), ("c", 0), ("a", 1)] {
        let record = format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "{name}", "order": {order}}}}}"#
        );
        harness.write_and_apply(&format!("state/arrangement/{name}/instance.json"), &record);
    }
    let shown = tracks(&harness.project, &id("arrangement"));
    let shown: Vec<&str> = shown.iter().map(|(track, _)| track.id().name()).collect();
    assert_eq!(shown, ["c", "a", "b"]);
}

#[test]
fn the_helpers_add_tracks_and_clips_under_free_ids_and_move_a_clip_as_one_step() {
    let mut harness = Harness::new();
    let arrangement = id("arrangement");
    for _ in 0..2 {
        let mut changes = Changes::new();
        let probe = Probe { scale: 1.0 };
        let track = add_track(
            &harness.project,
            &mut changes,
            &arrangement,
            "Warm Pad",
            Colour::Teal,
            probe,
        )
        .unwrap();
        add_clip(
            &harness.project,
            &mut changes,
            &track,
            "Verse A",
            clip(0, 3840, vec![note(0, 480, 60)]),
        )
        .unwrap();
        harness.project.commit("Add track", changes).unwrap();
    }
    let shown = tracks(&harness.project, &arrangement);
    let ids: Vec<(&str, u32)> = shown
        .iter()
        .map(|(track, state)| (track.id().as_str(), state.order))
        .collect();
    assert_eq!(
        ids,
        [("arrangement/warm-pad", 0), ("arrangement/warm-pad-2", 1)]
    );
    assert!(
        harness
            .path("state/arrangement/warm-pad-2/verse-a.json")
            .exists()
    );

    // The other track has a clip of this name already, so the moved one gets a free name.
    let (from, to) = (shown[0].0.clone(), shown[1].0.clone());
    let clip = harness
        .project
        .resolve::<Clip>(&id("arrangement/warm-pad/verse-a"))
        .unwrap();
    let mut changes = Changes::new();
    let moved = move_clip(&harness.project, &mut changes, &clip, &to).unwrap();
    harness.project.commit("Move clip", changes).unwrap();
    assert_eq!(moved.id(), &id("arrangement/warm-pad-2/verse-a-2"));
    assert!(clips(&harness.project, from.id()).is_empty());
    assert_eq!(clips(&harness.project, to.id()).len(), 2);
    assert!(
        !harness
            .path("state/arrangement/warm-pad/verse-a.json")
            .exists()
    );

    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Move clip")
    );
    assert_eq!(clips(&harness.project, from.id()).len(), 1);
    assert!(
        harness
            .path("state/arrangement/warm-pad/verse-a.json")
            .exists()
    );
    assert!(
        !harness
            .path("state/arrangement/warm-pad-2/verse-a-2.json")
            .exists()
    );
}

#[test]
fn a_track_without_an_instrument_loads_and_is_silent() {
    let mut harness = Harness::new();
    let record = r#"{"tool": "arrangement.track", "state": {"name": "Empty"}}"#;
    harness.write_and_apply("state/arrangement/empty/instance.json", record);
    let part = clip(0, 960, vec![note(0, 480, 60)]);
    harness.write_and_apply(
        "state/arrangement/empty/part.json",
        &crate::support::clip_json(&part),
    );
    assert!(harness.problems().is_empty());
    assert_eq!(level_changes(&harness.play(960 * TICK)), []);

    // The instrument arrives later, as one more file.
    let probe = r#"{"tool": "test.probe", "state": {"scale": 1.0}}"#;
    harness.write_and_apply("state/arrangement/empty/instrument.json", probe);
    harness.project.engine().seek(sound_core::Ticks(0));
    assert_eq!(
        level_changes(&harness.render(960 * TICK)),
        [(0, 60.0), (480 * TICK, 0.0)]
    );
}

#[test]
fn a_clip_outside_a_track_and_a_track_outside_an_arrangement_are_reported() {
    let mut harness = Harness::with_clips(vec![]);
    let part = crate::support::clip_json(&clip(0, 960, vec![note(0, 480, 60)]));
    let track = r#"{"tool": "arrangement.track", "state": {"name": "Lost"}}"#;
    let arrangement = r#"{"tool": "arrangement", "state": {}}"#;
    let cases = [
        (
            "state/root-clip.json",
            part.as_str(),
            "an instance of \"arrangement.clip\" belongs directly inside an instance of \"arrangement.track\", not at the top of state/",
        ),
        (
            "state/arrangement/orphan-clip.json",
            part.as_str(),
            "an instance of \"arrangement.clip\" belongs directly inside an instance of \"arrangement.track\", and its owner here is a \"arrangement\"",
        ),
        (
            "state/lost/instance.json",
            track,
            "an instance of \"arrangement.track\" belongs directly inside an instance of \"arrangement\", not at the top of state/",
        ),
        (
            "state/arrangement/piano/nested/instance.json",
            arrangement,
            "an instance of \"arrangement\" belongs at the top of state/, not inside another instance",
        ),
    ];
    for (file, record, message) in cases {
        assert_eq!(harness.write_and_apply(file, record), 0, "{file}");
        assert!(
            harness
                .problems()
                .contains(&format!("{file}: not loaded: {message}")),
            "{:?}",
            harness.problems()
        );
    }
    // In its track the same clip loads.
    assert_eq!(
        harness.write_and_apply("state/arrangement/piano/part.json", &part),
        1
    );
}
