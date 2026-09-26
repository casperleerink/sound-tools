//! The card of the built-in EQ in the track rack, with a simulated mouse and keys: it is added
//! from the control at the end of the rack, a press on a handle selects its band, and every
//! edit of it is one undo step written once.

use eq::view::EqView;
use eq::{Band, EqState, Shape};
use gpui::{Entity, TestAppContext, point, px};

use crate::support::{self, Opened, id};

const EQ: &str = "arrangement/track-1/eq";
const EQ_FILE: &str = "state/arrangement/track-1/eq.json";

/// One track with no instrument, so the only card with knobs is the EQ, and its panel open.
/// The EQ is added the way a composer adds it: `Add effect`, then `EQ`.
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
    let row = opened.control("menu-eq");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add EQ"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> EqState {
    opened.project(|project| {
        let eq = project.resolve::<EqState>(&id(EQ)).unwrap();
        *project.state(&eq).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(EQ_FILE)).unwrap()
}

fn view(opened: &mut Opened<'_>) -> Entity<EqView> {
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    view.unwrap().downcast::<EqView>().ok().unwrap()
}

fn selected(opened: &mut Opened<'_>) -> usize {
    let view = view(opened);
    opened.cx.read(|cx| view.read(cx).selected())
}

#[gpui::test]
fn add_effect_puts_an_eq_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(state(&mut opened), EqState::default());
    view(&mut opened);
    // The shown controls, and none of the hidden ones.
    for shown in [
        "knob-frequency_hz",
        "knob-gain_db",
        "knob-q",
        "shape",
        "handle-band-1",
        "handle-band-2",
        "handle-band-3",
        "handle-band-4",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    assert_eq!(opened.find("knob-output_gain_db"), None);
    assert_eq!(opened.find("toggle-band-1-on"), None);
    assert_eq!(selected(&mut opened), 0);

    // One undo takes it off again.
    opened.edit(|project| project.undo().map(|_| ()));
    assert!(!opened.path(EQ_FILE).exists());
}

/// A click on a handle selects its band and is no edit. The knobs then change that band only.
#[gpui::test]
fn a_click_on_a_handle_selects_its_band_and_the_knobs_follow(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let handle = opened.control("handle-band-3");
    opened.click(handle);
    assert_eq!(selected(&mut opened), 2);
    assert_eq!(opened.undo_label().as_deref(), Some("Add EQ"));

    let knob = opened.control("knob-gain_db");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    // Heard during the drag, not written until it ends.
    let moving = state(&mut opened).bands[2].gain_db;
    assert!(moving > 0.0, "{moving}");
    assert!(!file(&mut opened).contains(&format!("{moving:?}")));
    opened.release(point(knob.x, knob.y - px(20.)));
    let after = state(&mut opened);
    assert_eq!(opened.undo_label().as_deref(), Some("Change gain"));
    for band in [0, 1, 3] {
        assert_eq!(after.bands[band], Band::default_at(band), "band {}", band + 1);
    }
    assert!(file(&mut opened).contains(&format!("\"gain_db\": {:?}", after.bands[2].gain_db)));

    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), EqState::default());
}

/// Sideways is frequency and up is gain, both one step; the press also selects the band.
#[gpui::test]
fn a_drag_of_a_handle_changes_frequency_and_gain_as_one_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let handle = opened.control("handle-band-2");
    opened.drag(handle, point(handle.x + px(30.), handle.y - px(10.)));
    let after = state(&mut opened).bands[1];
    let default = Band::default_at(1);
    assert!(after.frequency_hz > default.frequency_hz, "{after:?}");
    assert!(after.gain_db > 0.0, "{after:?}");
    assert_eq!(selected(&mut opened), 1);
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Change frequency and gain")
    );
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), EqState::default());

    // Up past the top of the display: the gain stops at +15 dB.
    let handle = opened.control("handle-band-2");
    opened.drag(handle, point(handle.x, handle.y - px(200.)));
    assert_eq!(state(&mut opened).bands[1].gain_db, 15.0);
}

/// A shape from the select is one step. A cut has no gain, so its handle moves only sideways.
#[gpui::test]
fn a_shape_is_one_step_and_a_cut_moves_only_sideways(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let trigger = opened.control("shape");
    opened.click(trigger);
    let row = opened.control("menu-low_cut");
    opened.click(row);
    assert_eq!(state(&mut opened).bands[0].shape, Shape::LowCut);
    assert_eq!(opened.undo_label().as_deref(), Some("Change shape"));
    assert!(file(&mut opened).contains(r#""shape": "low_cut""#));

    // The select shows the icon of every shape, so it fits in its cell of 56 pt.
    for shape in ["high_shelf", "low_shelf", "high_cut", "notch", "bell"] {
        let trigger = opened.control("shape");
        opened.click(trigger);
        let row = opened.control(&format!("menu-{shape}"));
        opened.click(row);
        let width = opened.bounds("shape").unwrap().size.width;
        assert!(width <= px(56.), "{shape}: {width:?}");
    }
    let trigger = opened.control("shape");
    opened.click(trigger);
    let row = opened.control("menu-low_cut");
    opened.click(row);

    let handle = opened.control("handle-band-1");
    opened.drag(handle, point(handle.x + px(20.), handle.y - px(30.)));
    let after = state(&mut opened).bands[0];
    assert!(after.frequency_hz > 100.0, "{after:?}");
    assert_eq!(after.gain_db, 0.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change frequency"));
}

/// Expand shows the on and off of the bands and the output. It is no edit and is not saved.
#[gpui::test]
fn expand_shows_the_switches_of_the_bands_and_the_output(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-eq-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label().as_deref(), Some("Add EQ"));
    for hidden in [
        "toggle-band-1-on",
        "toggle-band-2-on",
        "toggle-band-3-on",
        "toggle-band-4-on",
        "knob-output_gain_db",
    ] {
        assert!(opened.find(hidden).is_some(), "{hidden}");
    }
    let switch = opened.control("toggle-band-2-on");
    opened.click(switch);
    assert!(!state(&mut opened).bands[1].on);
    assert_eq!(opened.undo_label().as_deref(), Some("Turn band off"));
    assert!(file(&mut opened).contains(r#""on": false"#));

    let output = opened.control("knob-output_gain_db");
    opened.drag(output, point(output.x, output.y + px(50.)));
    assert!(state(&mut opened).output_gain_db < 0.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change output"));

    let expand = opened.control("card-eq-expand");
    opened.click(expand);
    assert_eq!(opened.find("toggle-band-1-on"), None);
}

/// The keys 1 to 4 select a band from a control of the card that has the focus, so the
/// keyboard reaches every band.
#[gpui::test]
fn the_number_keys_select_a_band(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let knob = opened.control("knob-q");
    opened.click(knob);
    opened.keys("4");
    assert_eq!(selected(&mut opened), 3);
    // And the arrows on the focused knob now step the Q of band 4.
    opened.keys("up");
    let after = state(&mut opened);
    assert!(after.bands[3].q > 0.71, "{after:?}");
    assert_eq!(after.bands[0], Band::default_at(0));
    assert_eq!(opened.undo_label().as_deref(), Some("Change Q"));
    opened.keys("1");
    assert_eq!(selected(&mut opened), 0);
}

/// An agent edits the file while the card is open: the card shows it at once. Band 1 is now a
/// notch, which has no gain, so its handle moves only sideways.
#[gpui::test]
fn an_outside_edit_shows_on_the_card(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let path = opened.path(EQ_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "eq", "state": {"bands": [{"shape": "notch", "frequency_hz": 300.0}]}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    assert_eq!(state(&mut opened).bands[0].shape, Shape::Notch);
    let handle = opened.control("handle-band-1");
    opened.drag(handle, point(handle.x, handle.y - px(40.)));
    assert_eq!(state(&mut opened).bands[0].gain_db, 0.0);
}
