//! What moves on a page moves in the window, with no Bun: a knob turns its field as one undo
//! step, and a drag moves what binds to it; Bun hears the drag once, at its end.

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, point, px,
};

use super::{ONLY_THE_TOOL, Opened, open};
use crate::support::write_outside;

impl Opened<'_> {
    /// The top left of the space of `key`, once it shows.
    fn space(&mut self, key: &str) -> Point<Pixels> {
        let selector: &'static str = format!("space-{key}").leak();
        self.until(&format!("the space {key}"), |window| {
            window.find(selector).is_some()
        });
        self.cx.debug_bounds(selector).unwrap().origin
    }
}

fn project(name: &str, code: &str) -> [(String, String); 3] {
    [
        (ONLY_THE_TOOL[0].0.into(), ONLY_THE_TOOL[0].1.into()),
        (
            format!("state/{name}.json"),
            format!(r#"{{"tool": "{name}", "state": {{}}}}"#),
        ),
        (format!("extensions/{name}.ts"), code.into()),
    ]
}

fn open_project<'a>(cx: &'a mut TestAppContext, files: &[(String, String)]) -> Opened<'a> {
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    open(cx, &files)
}

const DIAL: &str = r#"import { h, number, Knob, tool } from "./sdk";

tool({
  name: "dial",
  title: "Dial",
  when: "You want a knob",
  doc: "A knob on a page.",
  state: { level: number({ min: 0, max: 1, default: 0.5 }) },
  sound: () => 0,
  view: () => h(Knob, { field: "level", label: "Level", min: 0, max: 1, default: 0.5 }),
});
"#;

#[gpui::test]
fn a_knob_of_a_page_turns_its_field_as_it_drags_as_one_undo_step_and_asks_bun_for_nothing(
    cx: &mut TestAppContext,
) {
    let mut window = open_project(cx, &project("dial", DIAL));
    let dial = window.space("knob-level") + point(px(28.), px(20.));
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    window.press(dial);
    for step in 1..=10 {
        window.drag_to(dial - point(px(0.), px(step as f32 * 10.)));
        if step == 5 {
            // A quarter of the travel up, while the pointer still holds it.
            assert_eq!(window.record("dial")["level"], 0.75);
        }
    }
    window.release(dial - point(px(0.), px(100.)));
    window.wait_for_bun();
    assert_eq!(window.record("dial")["level"], 1.0);
    assert_eq!(window.undo_label().as_deref(), Some("Change level"));
    // No binding reads the field: the knob moved without telling Bun.
    assert_eq!(sound_typescript::sent_updates_and_events(), asked);
    let session = window.session.clone();
    window
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.undo(cx)));
    assert_eq!(window.record("dial").get("level"), None);
}

const MOVER: &str = r##"import { h, number, Rect, Space, tool } from "./sdk";

tool({
  name: "mover",
  title: "Mover",
  when: "You want to move a block",
  doc: "A block that moves along, in steps of 10.",
  state: { x: number({ min: 0, max: 1000, default: 100 }) },
  sound: () => 0,
  view: ({ state, update, now }) =>
    h(Space, { key: "board", width: 400, height: 100, background: "#000000" },
      h(Rect, {
        x: () => now.drag("move").dx.plus(state.x),
        y: 0,
        width: 50,
        height: 50,
        color: "#ffffff",
        drag: {
          id: "move",
          axis: "x",
          snap: { x: 10 },
          onEnd: ({ dx }) => update("Move the block", ({ state }) => { state.x += dx; }),
        },
      }),
    ),
});
"##;

#[gpui::test]
fn a_drag_moves_what_binds_to_it_and_bun_hears_it_once_at_its_end(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("mover", MOVER));
    let block = window.space("board") + point(px(125.), px(25.));
    let (updates, events) = sound_typescript::sent_updates_and_events();
    window.press(block);
    for step in 1..=20 {
        window.drag_to(block + point(px(step as f32 * 5.15), px(step as f32)));
    }
    // Twenty moves, and not one request to Bun.
    assert_eq!(
        sound_typescript::sent_updates_and_events(),
        (updates, events)
    );
    window.release(block + point(px(103.), px(20.)));
    window.until("the block to move", |window| {
        window.record("mover")["x"] == 200
    });
    let (updates_after, events_after) = sound_typescript::sent_updates_and_events();
    assert_eq!(events_after, events + 1);
    // The new `x` a binding reads is told once.
    assert!(
        updates_after - updates <= 1,
        "{} updates",
        updates_after - updates
    );
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Move the block"));
}

const STEPS: &str = r#"import { h, pattern, Steps, tool } from "./sdk";

tool({
  name: "row",
  title: "Row",
  when: "You want steps",
  doc: "A row of steps.",
  state: { steps: pattern({ length: 4, min: 0, max: 1, default: 0 }) },
  sound: () => 0,
  view: () => h(Steps, { field: "steps" }),
});
"#;

#[gpui::test]
fn a_click_on_a_step_turns_it_on_through_its_code(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("row", STEPS));
    let second = window.space("steps-steps") + point(px(17. + 7.), px(7.));
    window.press(second);
    window.release(second);
    window.until("the step to turn on", |window| {
        window.record("row")["steps"] == serde_json::json!([0, 1, 0, 0])
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Turn a step on"));
}

/// A knob on what code keeps and the options of a mode, which keep each value they hear in the
/// record, and a slider on a field.
const KEPT: &str = r#"import { choice, h, Knob, number, SegmentedControl, Slider, tool } from "./sdk";

tool({
  name: "kept",
  title: "Kept",
  when: "You want controls on what code keeps",
  doc: "Controls on its memory.",
  state: {
    level: number({ min: 0, max: 1, default: 0 }),
    heard: number({ min: 0, max: 100, default: 0 }),
    mode: choice({ options: ["soft", "hard"], default: "soft" }),
    amount: number({ min: 0, max: 1, default: 0.5 }),
  },
  memory: () => ({ level: 0.5, mode: "soft" as "soft" | "hard" }),
  sound: () => 0,
  view: ({ memory, update }) => {
    const hear = (level: number) => {
      memory.level = level;
      update("Hear", ({ state }) => { state.level = level; state.heard += 1; });
    };
    return h("div", { style: { gap: 8 } },
      h(Knob, { key: "kept", value: () => memory.level, onChange: hear, min: 0, max: 1, default: 0.5 }),
      h(Slider, { key: "amount", field: "amount", min: 0, max: 1 }),
      h(SegmentedControl, {
        key: "mode", value: () => memory.mode,
        options: [{ value: "soft", label: "Soft" }, { value: "hard", label: "Hard" }],
        onChange: (mode) => {
          memory.mode = mode;
          update("Pick", ({ state }) => { state.mode = mode; });
        },
      }),
      h("div", { key: "quarter", onClick: () => { memory.level = 0.25; } }, "Quarter"),
    );
  },
});
"#;

impl Opened<'_> {
    /// Waits for the record of `kept` to have heard `count` values, and gives the last.
    fn heard(&mut self, count: u64) -> f64 {
        self.until(&format!("{count} values heard"), |window| {
            window.record("kept")["heard"] == count
        });
        self.record("kept")["level"].as_f64().unwrap()
    }

    fn heard_by_the_pointer(&mut self, heard: &str) {
        self.until(heard, |window| window.record("pointer")["heard"] == heard);
    }
}

#[gpui::test]
fn a_knob_on_what_code_keeps_moves_with_no_bun_and_its_code_hears_the_end_once(
    cx: &mut TestAppContext,
) {
    let mut window = open_project(cx, &project("kept", KEPT));
    let knob = window.space("kept") + point(px(28.), px(20.));
    // The press goes to its code, which hears a double click.
    window.press(knob);
    let (_, events) = sound_typescript::sent_updates_and_events();
    for step in 1..=10 {
        window.drag_to(knob - point(px(0.), px(step as f32 * 10.)));
    }
    assert_eq!(sound_typescript::sent_updates_and_events().1, events);
    window.release(knob - point(px(0.), px(100.)));
    // From 0.5, half its travel up.
    assert_eq!(window.heard(1), 1.0);
    assert_eq!(sound_typescript::sent_updates_and_events().1, events + 1);
    // It drags on from what its code gives it.
    let quarter = window.control("div-quarter");
    window.click(quarter);
    window.wait_for_bun();
    window.drag(knob, knob - point(px(0.), px(20.)));
    assert!((window.heard(2) - 0.35).abs() < 1e-6);
}

#[gpui::test]
fn a_slider_turns_its_field_as_one_undo_step_with_no_bun(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("kept", KEPT));
    let slider = window.control("slider-amount");
    let (_, events) = sound_typescript::sent_updates_and_events();
    window.press(slider);
    for step in 1..=4 {
        window.drag_to(slider + point(px(step as f32 * 7.), px(0.)));
    }
    window.release(slider + point(px(28.), px(0.)));
    window.wait_for_bun();
    // From 0.5, a quarter of the 112 points of its line.
    assert_eq!(window.record("kept")["amount"], 0.75);
    assert_eq!(window.undo_label().as_deref(), Some("Change amount"));
    assert_eq!(sound_typescript::sent_updates_and_events().1, events);
}

#[gpui::test]
fn a_pick_of_a_segmented_control_tells_its_code_the_option(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("kept", KEPT));
    let hard = window.control("segment-1");
    window.click(hard);
    window.until("the mode to be hard", |window| {
        window.record("kept")["mode"] == "hard"
    });
}

/// Steps on what code keeps, which starts with none, and two buttons that give it four or two.
const GROWING: &str = r#"import { h, Steps, text, tool } from "./sdk";

tool({
  name: "growing",
  title: "Growing",
  when: "You want steps that come and go",
  doc: "Steps on its memory.",
  state: { heard: text() },
  memory: () => ({ steps: [] as number[] }),
  sound: () => 0,
  view: ({ memory, update }) =>
    h("div", { style: { gap: 8 } },
      h(Steps, {
        key: "kept", value: () => memory.steps,
        onChange: (steps) => {
          memory.steps = [...steps];
          update("Hear", ({ state }) => { state.heard = steps.join(""); });
        },
      }),
      h("div", { key: "four", onClick: () => { memory.steps = [0, 0, 0, 0]; } }, "Four"),
      h("div", { key: "two", onClick: () => { memory.steps = [0, 0]; } }, "Two"),
    ),
});
"#;

#[gpui::test]
fn steps_on_what_code_keeps_grow_and_shrink_with_it(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("growing", GROWING));
    let row = window.space("kept");
    let width = |window: &mut Opened, steps: f32| {
        window.bounds("space-kept").unwrap().size.width == px(steps * 17. - 3.)
    };
    let four = window.control("div-four");
    window.click(four);
    window.until("four steps", |window| width(window, 4.));
    window.click(row + point(px(3. * 17. + 7.), px(7.)));
    window.until("the fourth step to turn on", |window| {
        window.record("growing")["heard"] == "0001"
    });
    let two = window.control("div-two");
    window.click(two);
    window.until("two steps", |window| width(window, 2.));
    window.click(row + point(px(17. + 7.), px(7.)));
    window.until("the second of two steps to turn on", |window| {
        window.record("growing")["heard"] == "01"
    });
}

const POINTER: &str = r##"import { Button, h, type PointerEvents, Rect, Show, signal, Space, text, Toggle, tool, type UiNode } from "./sdk";

tool({
  name: "pointer",
  title: "Pointer",
  when: "A test",
  doc: "Writes down what each node hears of the pointer.",
  state: { heard: text() },
  sound: () => 0,
  view: ({ update }) => {
    const heard: string[] = [];
    const hear = (what: string) => () => {
      heard.push(what);
      const all = heard.join(" ");
      update(what, ({ state }) => { state.heard = all; });
    };
    const pointer = (name: string) => ({ onPress: hear(`${name}-press`), onRelease: hear(`${name}-release`), onClick: hear(`${name}-click`) });
    const vanishing = (name: string, made: (heard: PointerEvents) => UiNode) => {
      const [shown, show] = signal(true);
      return Show({ when: shown, children: () => made({ onPress: () => { hear(`${name}-press`)(); show(false); }, onRelease: hear(`${name}-release`) }) });
    };
    return h("div", {},
      h("div", { key: "pad", style: { width: 100, height: 40 }, ...pointer("div") }),
      vanishing("gone", (heard) => h("div", { key: "gone", style: { width: 100, height: 40 }, ...heard })),
      h(Toggle, { key: "switch", on: false, onClick: (press) => hear(`toggle-click-${press.clicks}`)() }),
      h(Button, { key: "go", label: "Go", onClick: (press) => hear(`button-click-${press.clicks}`)() }),
      h(Space, { key: "board", width: 400, height: 100 },
        h(Rect, { x: 0, y: 0, width: 50, height: 50, color: "#ffffff", ...pointer("shape") }),
        h(Rect, { x: 100, y: 0, width: 50, height: 50, color: "#ffffff", drag: { onEnd: hear("drag-end") }, ...pointer("drag") }),
        vanishing("gone-shape", (heard) => h(Rect, { x: 200, y: 0, width: 50, height: 50, color: "#ffffff", ...heard })),
      ),
    );
  },
});
"##;

#[gpui::test]
fn a_div_and_a_shape_hear_their_press_then_its_release_then_the_click(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("pointer", POINTER));
    let pad = window.until_found("div-pad");
    let shape = window.space("board") + point(px(25.), px(25.));
    window.click(pad);
    window.click(shape);
    window.heard_by_the_pointer(
        "div-press div-release div-click shape-press shape-release shape-click",
    );
}

#[gpui::test]
fn a_release_follows_a_press_off_the_node_after_escape_and_when_the_window_goes_back(
    cx: &mut TestAppContext,
) {
    let mut window = open_project(cx, &project("pointer", POINTER));
    let pad = window.until_found("div-pad");
    let board = window.space("board");
    let (shape, slide) = (
        board + point(px(25.), px(25.)),
        board + point(px(125.), px(25.)),
    );
    let away = board + point(px(300.), px(300.));
    for node in [pad, shape] {
        window.press(node);
        window.drag_to(away);
        window.release(away);
    }
    window.press(slide);
    window.drag_to(slide + point(px(40.), px(0.)));
    window.cx.simulate_keystrokes("escape");
    window.release(slide + point(px(40.), px(0.)));
    window.cx.update(|window, _| window.activate_window());
    window.press(pad);
    window.cx.deactivate_window();
    window.heard_by_the_pointer(
        "div-press div-release shape-press shape-release drag-press drag-release div-press div-release",
    );
}

impl Opened<'_> {
    fn button(&mut self, position: Point<Pixels>, button: MouseButton, down: bool) {
        let modifiers = Modifiers::default();
        if down {
            self.cx.simulate_event(MouseDownEvent {
                position,
                modifiers,
                button,
                click_count: 1,
                first_mouse: false,
            });
        } else {
            self.cx.simulate_event(MouseUpEvent {
                position,
                modifiers,
                button,
                click_count: 1,
            });
        }
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn a_node_its_press_takes_away_still_hears_the_release(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("pointer", POINTER));
    let gone = window.until_found("div-gone");
    window.press(gone);
    window.until("the div gone", |window| window.find("div-gone").is_none());
    window.release(gone);
    let shape = window.space("board") + point(px(225.), px(25.));
    window.press(shape);
    window.release(shape);
    window.heard_by_the_pointer("gone-press gone-release gone-shape-press gone-shape-release");
}

#[gpui::test]
fn the_other_button_neither_clicks_a_held_press_nor_keeps_its_release(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("pointer", POINTER));
    let pad = window.until_found("div-pad");
    let board = window.space("board");
    let (shape, empty) = (
        board + point(px(25.), px(25.)),
        board + point(px(350.), px(80.)),
    );
    window.button(empty, MouseButton::Right, true);
    window.press(shape);
    window.button(empty, MouseButton::Right, false);
    window.release(shape);
    window.press(pad);
    window.button(empty, MouseButton::Right, true);
    window.button(empty, MouseButton::Right, false);
    // The left button came up outside the window.
    window
        .cx
        .simulate_mouse_move(empty, None, Modifiers::default());
    window.cx.run_until_parked();
    window.heard_by_the_pointer("shape-press shape-release shape-click div-press div-release");
}

#[gpui::test]
fn the_middle_button_is_not_heard_while_the_left_holds_a_shape(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("pointer", POINTER));
    let pad = window.until_found("div-pad");
    let shape = window.space("board") + point(px(25.), px(25.));
    window.press(shape);
    window.button(shape, MouseButton::Middle, true);
    window.button(shape, MouseButton::Middle, false);
    window.release(shape);
    window.click(pad);
    window.heard_by_the_pointer(
        "shape-press shape-release shape-click div-press div-release div-click",
    );
}

#[gpui::test]
fn a_toggle_and_a_button_tell_how_many_clicks(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("pointer", POINTER));
    let toggle = window.until_found("toggle-switch");
    let button = window.until_found("button-go");
    window.double_click(toggle);
    window.double_click(button);
    window.heard_by_the_pointer("toggle-click-1 toggle-click-2 button-click-1 button-click-2");
}

#[gpui::test]
fn the_right_button_ends_no_left_click_of_a_div_a_toggle_or_a_button(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("pointer", POINTER));
    let away = window.space("board") + point(px(350.), px(80.));
    for selector in ["div-pad", "toggle-switch", "button-go"] {
        let node = window.until_found(selector);
        window.press(node);
        window.button(node, MouseButton::Right, true);
        window.button(node, MouseButton::Right, false);
        window.release(away);
    }
    let toggle = window.until_found("toggle-switch");
    window.click(toggle);
    window.heard_by_the_pointer("div-press div-press div-release div-release toggle-click-1");
}

#[gpui::test]
fn a_move_with_the_right_button_held_keeps_its_press(cx: &mut TestAppContext) {
    let mut window = open_project(cx, &project("pointer", POINTER));
    let pad = window.until_found("div-pad");
    let toggle = window.until_found("toggle-switch");
    window.button(pad, MouseButton::Right, true);
    window
        .cx
        .simulate_mouse_move(toggle, MouseButton::Right, Modifiers::default());
    window.button(toggle, MouseButton::Left, true);
    window.button(toggle, MouseButton::Left, false);
    window.button(toggle, MouseButton::Right, false);
    window.heard_by_the_pointer("div-press toggle-click-1 div-release");
}

/// A rig that shows the card of its pad until its record hides it.
const RIG: &str = r##"import { boolean, DeviceCard, h, Show, text, tool } from "./sdk";

const pad = tool({
  name: "pad",
  title: "Pad",
  when: "A test",
  doc: "Writes down its press and its release.",
  state: { heard: text() },
  sound: () => 0,
  view: ({ update }) => {
    const hear = (what: string) => () => update(what, (pad) => { pad.state.heard = `${pad.state.heard ?? ""} ${what}`.trim(); });
    return h("div", { key: "key", style: { width: 100, height: 40 }, onPress: hear("press"), onRelease: hear("release") });
  },
});

tool({
  name: "rig",
  title: "Rig",
  when: "A test",
  doc: "Shows the card of its pad.",
  state: { hidden: boolean({ default: false }) },
  children: { pad },
  behaviour: () => ({ connections: [] }),
  view: ({ state }) => Show({ when: () => !state.hidden, children: () => h("div", {}, DeviceCard({ path: "pad" })) }),
});
"##;

#[gpui::test]
fn a_card_that_goes_while_its_pad_is_held_sends_the_release(cx: &mut TestAppContext) {
    let files = [
        (ONLY_THE_TOOL[0].0.into(), ONLY_THE_TOOL[0].1.into()),
        (
            "state/rig/instance.json".into(),
            r#"{"tool": "rig", "state": {}}"#.into(),
        ),
        (
            "state/rig/pad.json".into(),
            r#"{"tool": "pad", "state": {}}"#.into(),
        ),
        ("extensions/rig.ts".into(), RIG.into()),
    ];
    let mut window = open_project(cx, &files);
    let key = window.until_found("div-key");
    window.press(key);
    window.until("the press", |window| {
        window.record("rig/pad")["heard"] == "press"
    });
    write_outside(
        &mut window,
        "state/rig/instance.json",
        r#"{"tool": "rig", "state": {"hidden": true}}"#,
    );
    window.until("the card gone", |window| window.find("div-key").is_none());
    window.until("the release", |window| {
        window.record("rig/pad")["heard"] == "press release"
    });
}
