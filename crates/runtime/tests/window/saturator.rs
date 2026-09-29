//! The card of the built-in saturator in the track rack, with a simulated mouse: it is added
//! from the control at the end of the rack, and every edit of it is one undo step written once.

use gpui::{TestAppContext, point, px};
use saturator::view::SaturatorView;
use saturator::{Curve, DRIVE, SaturatorState};

use crate::support::{self, Opened, id};

const SATURATOR: &str = "arrangement/track-1/saturator";
const SATURATOR_FILE: &str = "state/arrangement/track-1/saturator.json";

/// One track with no instrument, so the only card with knobs is the saturator, and its panel
/// open. The saturator is added the way a composer adds it: `Add effect`, then `Saturator`.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = sound_core::Changes::new();
        changes.delete(&id("arrangement/track-1/instrument"));
        project.commit("Remove synth", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control("menu-saturator");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Saturator"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> SaturatorState {
    opened.project(|project| {
        let saturator = project.resolve::<SaturatorState>(&id(SATURATOR)).unwrap();
        *project.state(&saturator).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(SATURATOR_FILE)).unwrap()
}

#[gpui::test]
fn add_effect_puts_a_saturator_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(state(&mut opened), SaturatorState::default());
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    assert!(view.unwrap().downcast::<SaturatorView>().is_ok());
    for shown in [
        "knob-drive_db",
        "knob-tone_db",
        "knob-output_db",
        "knob-mix",
        "handle-drive",
        "segment-soft",
        "segment-tape",
        "segment-tube",
        "segment-clip",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    // Everything fits on the card: it has nothing to expand.
    assert_eq!(opened.find("card-saturator-expand"), None);

    // One undo takes it off again.
    opened.edit(|project| project.undo().map(|_| ()));
    assert!(!opened.path(SATURATOR_FILE).exists());
}

#[gpui::test]
fn a_knob_drag_is_one_undo_step_written_once(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let before = file(&mut opened);
    let knob = opened.control("knob-drive_db");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    // Heard during the drag, not written until it ends.
    let moving = state(&mut opened).drive_db;
    assert!(moving > SaturatorState::default().drive_db);
    assert_eq!(file(&mut opened), before);
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    opened.release(point(knob.x, knob.y - px(40.)));
    let after = state(&mut opened).drive_db;
    assert!(after > moving);
    assert_eq!(opened.undo_label().as_deref(), Some("Change drive"));
    assert!(file(&mut opened).contains(&format!("\"drive_db\": {after:?}")));

    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), SaturatorState::default());
    assert_eq!(opened.undo_label().as_deref(), Some("Add Saturator"));
}

/// The handle sits where the curve bends. To the left the bend comes earlier: more drive.
#[gpui::test]
fn the_handle_drags_the_drive(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let default = SaturatorState::default();
    let handle = opened.control("handle-drive");
    opened.drag(handle, point(handle.x - px(30.), handle.y));
    let after = state(&mut opened);
    assert!(after.drive_db > default.drive_db, "{after:?}");
    assert_eq!(opened.undo_label().as_deref(), Some("Change drive"));
    // The handle moved with the bend.
    assert!(opened.control("handle-drive").x < handle.x);

    // Far past the right: the drive stops at its least.
    let handle = opened.control("handle-drive");
    opened.drag(handle, point(handle.x + px(300.), handle.y));
    assert_eq!(state(&mut opened).drive_db, DRIVE.min);

    opened.edit(|project| project.undo().map(|_| ()));
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), default);
}

#[gpui::test]
fn the_curve_is_one_click_and_one_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let tube = opened.control("segment-tube");
    opened.click(tube);
    assert_eq!(state(&mut opened).curve, Curve::Tube);
    assert_eq!(opened.undo_label().as_deref(), Some("Change curve"));
    assert!(file(&mut opened).contains(r#""curve": "tube""#));
}

/// An agent edits the file while the card is open: the card shows it at once.
#[gpui::test]
fn an_outside_edit_shows_on_the_card(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let handle = opened.control("handle-drive");
    let path = opened.path(SATURATOR_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "saturator", "state": {"curve": "clip", "drive_db": 24.0}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    assert_eq!(state(&mut opened).curve, Curve::Clip);
    // The bend moved left with the drive.
    assert!(opened.control("handle-drive").x < handle.x);
    // The selected segment is the one the file names: clicking it again is no edit.
    let label = opened.undo_label();
    let clip = opened.control("segment-clip");
    opened.click(clip);
    assert_eq!(opened.undo_label(), label);
}

/// The peak of what the track plays over its mean level, from its left channel: a sound
/// clipped hard is nearly as loud everywhere as at its peak. It plays from the start, because
/// the saturator has a latency: when it comes or goes, the notes that sound end.
fn crest(opened: &mut Opened<'_>) -> f32 {
    opened.settle();
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| {
            let engine = session.engine();
            engine.stop();
            engine.seek(sound_core::Ticks(0));
            engine.play();
        });
    });
    opened.settle();
    opened.render(12_000);
    let render = opened.render(12_000);
    let left: Vec<f32> = render.iter().step_by(2).copied().collect();
    let peak = left
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    let level = left.iter().map(|sample| sample.abs()).sum::<f32>() / left.len() as f32;
    peak / level
}

/// The saturator gets the power icon of every effect: it bypasses the slot, the record of the
/// saturator stays, and the track sounds as it does without it. One undo step each way.
#[gpui::test]
fn the_power_icon_bypasses_the_saturator_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = sound_core::Changes::new();
        let note = support::note(0, 4 * support::BAR, 64);
        let part = support::clip(0, 4 * support::BAR, vec![note]);
        changes.create(id("arrangement/track-1/part"), part);
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    let plain = crest(&mut opened);
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control("menu-saturator");
    opened.click(row);
    let path = opened.path(SATURATOR_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "saturator", "state": {"curve": "clip", "drive_db": 36.0}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    let clipped = crest(&mut opened);
    assert!(clipped < plain * 0.8, "{clipped} {plain}");

    let power = opened.control("card-saturator-power");
    opened.click(power);
    assert_eq!(opened.undo_label().as_deref(), Some("Turn off Saturator"));
    let track = std::fs::read_to_string(opened.path("state/arrangement/track-1/instance.json"));
    assert!(
        track
            .unwrap()
            .contains(r#"{"name": "saturator", "bypass": true}"#)
    );
    assert!(opened.path(SATURATOR_FILE).exists());
    let bypassed = crest(&mut opened);
    assert!(
        (bypassed - plain).abs() < plain * 0.05,
        "{bypassed} {plain}"
    );

    opened.keys("cmd-z");
    assert!(crest(&mut opened) < plain * 0.8);
    let power = opened.control("card-saturator-power");
    opened.click(power);
    opened.click(power);
    assert_eq!(opened.undo_label().as_deref(), Some("Turn on Saturator"));
}
