//! The card of the built-in saturator in a rack, with a simulated mouse: every edit of it is one
//! undo step written once.

use gpui::{TestAppContext, point, px};
use saturator::{Curve, DRIVE, SaturatorState};

use crate::rack::record;
use crate::support::{self, Opened, id};

const SATURATOR: &str = "rack/saturator";
const SATURATOR_FILE: &str = "state/rack/saturator.json";

/// A rack of the saturator alone, so its card is the only one with knobs.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_rack(cx, &[("saturator", &record("saturator", "{}"))])
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
    assert_eq!(opened.find("card-rack/saturator-expand"), None);
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
