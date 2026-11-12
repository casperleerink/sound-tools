//! The parts of a device card a view names: a row of steps the keys move in, and the cells of a
//! card, of which the first columns show and the rest wait behind its expand icon.

use gpui::{TestAppContext, px};
use sound_ui::components::cell::CELL_WIDTH;

use super::gestures::press_key;
use super::parts::page;
use super::{ONLY_THE_TOOL, open};

/// Four steps on a page that counts each space it hears.
const SEQUENCE: &str = r#"import { h, number, pattern, Steps, tool } from "./sdk";

tool({
  name: "sequence",
  title: "Sequence",
  when: "A test",
  doc: "Four steps, and how often its page heard space.",
  state: { steps: pattern({ length: 4, min: 0, max: 1, default: 0 }), heard: number({ min: 0, max: 100, default: 0 }) },
  sound: () => 0,
  globalKeys: ["space"],
  onKey: ({ update }, { key, down }) => {
    if (key === "space" && down) update("Hear space", ({ state }) => { state.heard = (state.heard ?? 0) + 1; });
  },
  view: () => h(Steps, { field: "steps" }),
});
"#;

#[gpui::test]
fn the_arrows_move_along_steps_and_space_or_enter_turns_the_one_they_are_on(
    cx: &mut TestAppContext,
) {
    let mut window = page(cx, "sequence", SEQUENCE);
    window.until_found("step-3");
    window.wait_for_bun();
    // Space is the page's until the steps have the keys.
    press_key(&mut window, "space");
    window.until("the page to hear space", |window| {
        window.record("sequence")["heard"] == 1
    });

    press_key(&mut window, "tab");
    for key in ["right", "right", "space"] {
        press_key(&mut window, key);
    }
    let steps = |on: [u8; 4]| serde_json::json!(on);
    window.until("the third step on", |window| {
        window.record("sequence")["steps"] == steps([0, 0, 1, 0])
    });
    for key in ["left", "enter"] {
        press_key(&mut window, key);
    }
    window.until("the second step on", |window| {
        window.record("sequence")["steps"] == steps([0, 1, 1, 0])
    });
    // The arrows stop at the ends of the row.
    for key in ["right", "right", "right", "right", "space"] {
        press_key(&mut window, key);
    }
    window.until("the last step on", |window| {
        window.record("sequence")["steps"] == steps([0, 1, 1, 1])
    });
    press_key(&mut window, "space");
    window.until("the last step off", |window| {
        window.record("sequence")["steps"] == steps([0, 1, 1, 0])
    });
    // A step a key turned is one undo step.
    window.undo();
    assert_eq!(window.record("sequence")["steps"], steps([0, 1, 1, 1]));
    assert_eq!(window.record("sequence")["heard"], 1);
    assert_eq!(window.problems(), []);
}

/// A rig that shows the card of its box: a display, then a knob over a wide control, two more
/// knobs, and two that do not fit beside a display.
const RIG: &str = r#"import { Cell, choice, Controls, DeviceCard, Display, h, input, Knob, number, SegmentedControl, tool } from "./sdk";

const level = () => number({ min: 0, max: 1, default: 0.5 });
const knob = (field: string) => h(Knob, { field, label: field, min: 0, max: 1, default: 0.5 });
const kinds = [{ value: "cassette", label: "Cassette" }, { value: "reel", label: "Reel" }] as const;

const box = tool({
  name: "box",
  title: "Box",
  when: "A test",
  doc: "A card of a display and six cells.",
  state: { a: level(), b: level(), c: level(), d: level(), e: level(), kind: choice({ options: ["cassette", "reel"], default: "cassette" }) },
  sound: () => input,
  view: () =>
    h("div", { style: { direction: "row" } },
      h(Display, { caption: "Level" }),
      h(Controls, {},
        knob("a"),
        h(Cell, { label: "Kind" }, h(SegmentedControl, { key: "kind", field: "kind", size: "sm", options: kinds })),
        knob("b"), knob("c"), knob("d"), knob("e"),
      ),
    ),
});

tool({
  name: "rig",
  title: "Rig",
  when: "A test",
  doc: "Shows the card of its box.",
  state: {},
  children: { box },
  behaviour: () => ({ connections: [] }),
  view: () => h("div", {}, DeviceCard({ path: "box" })),
});
"#;

#[gpui::test]
fn a_card_shows_its_first_columns_and_the_rest_behind_its_expand_icon(cx: &mut TestAppContext) {
    let files = [
        ONLY_THE_TOOL[0],
        ("state/rig/instance.json", r#"{"tool": "rig", "state": {}}"#),
        ("state/rig/box.json", r#"{"tool": "box", "state": {}}"#),
        ("extensions/rig.ts", RIG),
    ];
    let mut window = open(cx, &files);
    window.until_found("space-knob-c");
    window.wait_for_bun();
    assert_eq!(window.find("space-knob-d"), None);

    // The wide control takes whole columns, and the knob over it stays in the first of them.
    let left = |window: &mut super::Opened<'_>, selector: &str| {
        f32::from(window.bounds(selector).unwrap().origin.x)
    };
    let kind = f32::from(window.bounds("segmented-kind").unwrap().size.width);
    let columns = (kind / CELL_WIDTH).ceil();
    assert!(columns >= 2., "{kind}");
    let (a, b) = (
        left(&mut window, "space-knob-a"),
        left(&mut window, "space-knob-b"),
    );
    assert_eq!(b - a, CELL_WIDTH * columns);

    let expand = window.until_found("card-rig/box-expand");
    window.click(expand);
    let d = window.until_found("space-knob-d");
    assert!(d.x > px(b), "the hidden columns come after the others");
    let expand = window.until_found("card-rig/box-expand");
    window.click(expand);
    window.until("the card to fold", |window| {
        window.find("space-knob-d").is_none()
    });
    assert_eq!(window.problems(), []);
}
