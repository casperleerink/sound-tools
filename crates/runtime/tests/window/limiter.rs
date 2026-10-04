//! The card of the built-in limiter in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, every edit of it is one undo step written once, and its
//! display shows the reduction while the track plays.

use gpui::{Entity, TestAppContext, point, px};
use limiter::view::LimiterView;
use limiter::{LimiterState, Lookahead};
use sound_ui::POLL_INTERVAL;
use sound_ui::components::limiter_display::POLLS_PER_COLUMN;

use crate::support::{self, Opened, clip, id, note};

const LIMITER: &str = "arrangement/track-1/limiter";
const LIMITER_FILE: &str = "state/arrangement/track-1/limiter.json";

/// Adds a limiter the way a composer does, `Add effect`, then `Limiter`, to the open panel.
fn add_limiter(opened: &mut Opened<'_>) {
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control("menu-limiter");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Limiter"));
    opened.project(|project| assert_eq!(project.problems(), []));
}

/// One track with no instrument, so the only card with knobs is the limiter, and its panel
/// open.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = sound_core::Changes::new();
        changes.delete(&id("arrangement/track-1/instrument"));
        project.commit("Remove synth", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    add_limiter(&mut opened);
    opened
}

fn state(opened: &mut Opened<'_>) -> LimiterState {
    opened.project(|project| {
        let limiter = project.resolve::<LimiterState>(&id(LIMITER)).unwrap();
        *project.state(&limiter).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(LIMITER_FILE)).unwrap()
}

/// The card of the limiter: the effect after the instrument slot.
fn card(opened: &mut Opened<'_>) -> Entity<LimiterView> {
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    view.unwrap().downcast::<LimiterView>().ok().unwrap()
}

/// The ceiling handle moves up and down only, and stops at the ends of the range.
#[gpui::test]
fn a_drag_of_the_ceiling_handle_changes_the_ceiling(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    // Every control is shown, so there is nothing to expand.
    assert_eq!(opened.find("card-limiter-expand"), None);
    let default = LimiterState::default();
    let handle = opened.control("handle-ceiling");
    opened.drag(handle, point(handle.x - px(30.), handle.y + px(20.)));
    let after = state(&mut opened);
    assert!(after.ceiling_db < default.ceiling_db, "{after:?}");
    assert_eq!(after.gain_db, default.gain_db);
    assert_eq!(opened.undo_label().as_deref(), Some("Change ceiling"));
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), default);

    let handle = opened.control("handle-ceiling");
    opened.drag(handle, point(handle.x, handle.y + px(300.)));
    assert_eq!(state(&mut opened).ceiling_db, limiter::CEILING.min);
    let handle = opened.control("handle-ceiling");
    opened.drag(handle, point(handle.x, handle.y - px(300.)));
    assert_eq!(state(&mut opened).ceiling_db, limiter::CEILING.max);
}

/// The lookahead is a select: a pick is one undo step, and an outside edit shows in it.
#[gpui::test]
fn the_lookahead_is_picked_and_follows_an_outside_edit(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let select = opened.control("lookahead");
    opened.click(select);
    let five = opened.control("menu-5");
    opened.click(five);
    assert_eq!(state(&mut opened).lookahead, Lookahead::Five);
    assert_eq!(opened.undo_label().as_deref(), Some("Change lookahead"));
    assert!(file(&mut opened).contains(r#""lookahead_ms": 5"#));

    let path = opened.path(LIMITER_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "limiter", "state": {"lookahead_ms": 0}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    assert_eq!(state(&mut opened).lookahead, Lookahead::Off);
    // The select shows what the file names: picking it again is no edit.
    let label = opened.undo_label();
    let select = opened.control("lookahead");
    opened.click(select);
    let off = opened.control("menu-0");
    opened.click(off);
    assert_eq!(opened.undo_label(), label);
}

/// While a loud track plays into a low ceiling, the display shows how much the limiter takes.
/// It comes from the audio thread.
#[gpui::test]
fn the_reduction_shows_while_the_track_plays(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let chord = vec![note(0, 3840, 48), note(0, 3840, 55), note(0, 3840, 64)];
        let mut changes = sound_core::Changes::new();
        changes.create(id("arrangement/track-1/chord"), clip(0, 3840, chord));
        project.commit("Add chord", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    add_limiter(&mut opened);
    let path = opened.path(LIMITER_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "limiter", "state": {"ceiling_db": -24.0}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    opened.settle();
    let view = card(&mut opened);
    assert_eq!(opened.cx.read(|cx| view.read(cx).reduction_db()), 0.0);

    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| session.engine().play());
    });
    opened.settle();
    for _ in 0..POLLS_PER_COLUMN {
        opened.render(2_400);
        opened.cx.executor().advance_clock(POLL_INTERVAL);
        opened.cx.run_until_parked();
    }
    let reduction = opened.cx.read(|cx| view.read(cx).reduction_db());
    assert!(reduction > 3.0, "{reduction}");
}
