//! The card of a script in the track rack, with a simulated mouse: an agent writes the record,
//! the card has a knob per `param` line, a drag is one undo step, and new code brings its knobs.

use gpui::{Entity, TestAppContext, point, px};
use script::ScriptState;
use script::view::ScriptView;

use crate::support::{self, Opened, id, write_outside};

const SCRIPT: &str = "arrangement/track-1/tremolo";
const SCRIPT_FILE: &str = "state/arrangement/track-1/tremolo.json";

const TREMOLO: &str = r#"{"tool": "script", "state": {"name": "Tremolo", "code": [
  "param rate = 4 [0.1, 20]",
  "param depth = 0.5 [0, 1]",
  "out = in * (1 - depth * (0.5 + 0.5 * sin(phasor(rate) * tau)))"
]}}"#;

/// One track with no instrument and the tremolo an agent wrote on it, and its panel open.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = sound_core::Changes::new();
        changes.delete(&id("arrangement/track-1/instrument"));
        project.commit("Remove synth", changes).unwrap();
        project.clear_history();
    });
    write_outside(&mut opened, SCRIPT_FILE, TREMOLO);
    write_outside(
        &mut opened,
        "state/arrangement/track-1/instance.json",
        r#"{"tool": "arrangement.track", "state": {"name": "Track 1", "effects": ["tremolo"]}}"#,
    );
    opened.project(|project| assert_eq!(project.problems(), []));
    let header = opened.track_header(0);
    opened.click(header);
    opened
}

fn state(opened: &mut Opened<'_>) -> ScriptState {
    opened.project(|project| {
        let script = project.resolve::<ScriptState>(&id(SCRIPT)).unwrap();
        project.state(&script).unwrap().clone()
    })
}

/// The card of the script: the effect after the instrument slot.
fn card(opened: &mut Opened<'_>) -> Entity<ScriptView> {
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    view.unwrap().downcast::<ScriptView>().ok().unwrap()
}

#[gpui::test]
fn a_knob_of_a_param_writes_its_value_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    card(&mut opened);
    assert_eq!(opened.find("knob-feedback"), None);

    let knob = opened.control("knob-rate");
    opened.drag(knob, point(knob.x, knob.y - px(40.)));
    let rate = state(&mut opened).values["rate"];
    assert!(rate > 4.0, "{rate}");
    assert!(!state(&mut opened).values.contains_key("depth"));
    assert_eq!(opened.undo_label().as_deref(), Some("Change rate"));
    let file = std::fs::read_to_string(opened.path(SCRIPT_FILE)).unwrap();
    assert!(file.contains(r#""values": {"rate": "#), "{file}");

    opened.edit(|project| project.undo().map(|_| ()));
    assert!(state(&mut opened).values.is_empty());
}

#[gpui::test]
fn new_code_brings_the_knobs_of_its_params(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let echo = r#"{"tool": "script", "state": {"name": "Echo", "code": [
      "param time = 350 [1, 2000]",
      "param feedback = 0.45 [0, 0.95]",
      "history echo",
      "wet = delay(in + echo * feedback, time)",
      "echo = wet",
      "out = in + wet"
    ]}}"#;
    write_outside(&mut opened, SCRIPT_FILE, echo);
    assert_eq!(opened.find("knob-rate"), None);
    opened.control("knob-time");
    opened.control("knob-feedback");
}
