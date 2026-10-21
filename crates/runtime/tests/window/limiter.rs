//! The card of the built-in limiter in a rack, with a simulated mouse: every edit of it is one
//! undo step written once, and its display shows the reduction while it plays.

use gpui::{TestAppContext, point, px};
use limiter::view::LimiterView;
use limiter::{LimiterState, Lookahead};
use sound_ui::POLL_INTERVAL;
use sound_ui::components::limiter_display::POLLS_PER_COLUMN;

use crate::rack::{notes, record};
use crate::support::{self, Opened, id};

const LIMITER: &str = "rack/limiter";
const LIMITER_FILE: &str = "state/rack/limiter.json";

/// A rack of the limiter alone, so its card is the only one with knobs.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_rack(cx, &[("limiter", &record("limiter", "{}"))])
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

/// The ceiling handle moves up and down only, and stops at the ends of the range.
#[gpui::test]
fn a_drag_of_the_ceiling_handle_changes_the_ceiling(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    // Every control is shown, so there is nothing to expand.
    assert_eq!(opened.find("card-rack/limiter-expand"), None);
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

/// While a loud chord plays into a low ceiling, the display shows how much the limiter takes.
/// It comes from the audio thread.
#[gpui::test]
fn the_reduction_shows_while_it_plays(cx: &mut TestAppContext) {
    let chord = notes(3840, &[(0, 3840, 48), (0, 3840, 55), (0, 3840, 64)]);
    let low = record("limiter", r#"{"ceiling_db": -24.0}"#);
    let mut opened = support::open_rack(
        cx,
        &[
            ("instrument", &record("instrument.synth", "{}")),
            ("chord", &chord),
            ("limiter", &low),
        ],
    );
    let view = opened.card::<LimiterView>(LIMITER);
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
