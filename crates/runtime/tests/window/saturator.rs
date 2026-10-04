//! The card of the built-in saturator in the track rack, with a simulated mouse: it is added
//! from the control at the end of the rack, and every edit of it is one undo step written once.

use gpui::{TestAppContext, point, px};
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

/// The handle sits where the curve bends. To the left the bend comes earlier: more drive.
#[gpui::test]
fn the_handle_drags_the_drive(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    // Everything fits on the card: it has nothing to expand.
    assert_eq!(opened.find("card-saturator-expand"), None);
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
