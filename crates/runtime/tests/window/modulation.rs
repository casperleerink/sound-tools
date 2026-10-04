//! The card of the built-in modulation in the track rack, with a simulated mouse: it is added
//! from the control at the end of the rack, and every edit of it is one undo step written once.

use gpui::{TestAppContext, point, px};
use modulation::{Mode, ModulationState};

use crate::support::{self, Opened, id};

const MODULATION: &str = "arrangement/track-1/modulation";
const MODULATION_FILE: &str = "state/arrangement/track-1/modulation.json";

/// One track with no instrument, so the only card with knobs is the modulation, and its panel
/// open. The modulation is added the way a composer adds it: `Add effect`, then `Modulation`.
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
    let row = opened.control("menu-modulation");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Modulation"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> ModulationState {
    opened.project(|project| {
        let modulation = project.resolve::<ModulationState>(&id(MODULATION)).unwrap();
        *project.state(&modulation).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(MODULATION_FILE)).unwrap()
}

/// The mode is one click and one step, and the file says it.
#[gpui::test]
fn a_click_on_a_mode_is_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let phaser = opened.control("segment-phaser");
    opened.click(phaser);
    assert_eq!(state(&mut opened).mode, Mode::Phaser);
    assert_eq!(opened.undo_label().as_deref(), Some("Change mode"));
    assert!(file(&mut opened).contains(r#""mode": "phaser""#));
    let flanger = opened.control("segment-flanger");
    opened.click(flanger);
    assert_eq!(state(&mut opened).mode, Mode::Flanger);
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened).mode, Mode::Phaser);
}

/// The peak of the line drags the depth up and down; the peak of the dashed line drags the
/// spread sideways. Each is one step with the name of its knob.
#[gpui::test]
fn the_handles_drag_the_depth_and_the_spread(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let default = ModulationState::default();
    let depth = opened.control("handle-depth");
    opened.drag(depth, point(depth.x, depth.y - px(10.)));
    let after = state(&mut opened);
    assert!(after.depth > default.depth, "{after:?}");
    assert_eq!(after.spread, default.spread);
    assert_eq!(opened.undo_label().as_deref(), Some("Change depth"));

    let spread = opened.control("handle-spread");
    opened.drag(spread, point(spread.x + px(10.), spread.y));
    let after = state(&mut opened);
    assert!(after.spread > default.spread, "{after:?}");
    assert_eq!(opened.undo_label().as_deref(), Some("Change spread"));

    // Far past the right: the spread stops at its most.
    let spread = opened.control("handle-spread");
    opened.drag(spread, point(spread.x + px(300.), spread.y));
    assert_eq!(state(&mut opened).spread, modulation::SPREAD.max);

    opened.edit(|project| project.undo().map(|_| ()));
    opened.edit(|project| project.undo().map(|_| ()));
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), default);
}

/// Expand shows the spread, is no edit, and is not saved.
#[gpui::test]
fn expand_shows_the_spread(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-modulation-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Modulation"));
    let spread = opened.control("knob-spread");
    opened.drag(spread, point(spread.x, spread.y - px(50.)));
    assert!(state(&mut opened).spread > ModulationState::default().spread);
    assert_eq!(opened.undo_label().as_deref(), Some("Change spread"));

    let expand = opened.control("card-modulation-expand");
    opened.click(expand);
    assert_eq!(opened.find("knob-spread"), None);
}
