//! The parts of a device card a view names: a row of steps the keys move in, and a card the
//! window lays out, its display or its rows of steps at the left of its cells, of which the first
//! columns show and the rest wait behind its expand icon. And a page, which plays on space when
//! it shows a transport and puts a part that does not grow in its middle.

use gpui::{TestAppContext, px};
use sound_ui::POLL_INTERVAL;
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
    // Up and down change how strong the step they are on is: a tenth, a fiftieth with shift.
    // Each from what the record holds, also before the row drew the key before.
    for key in ["down", "down", "shift-down"] {
        press_key(&mut window, key);
    }
    window.until("the last step softer", |window| {
        window.record("sequence")["steps"] == serde_json::json!([0, 1, 1, 0.78])
    });
    for key in ["left", "left", "left", "up"] {
        press_key(&mut window, key);
    }
    window.until("the first step on, soft", |window| {
        window.record("sequence")["steps"] == serde_json::json!([0.1, 1, 1, 0.78])
    });
    assert_eq!(window.problems(), []);
}

/// Three rows of steps that each name the step that plays: the first runs with the piece, the
/// second on its own, and so does the third, which only changes the others.
const RINGS: &str = r#"import { h, number, pattern, Steps, tool } from "./sdk";

const row = () => pattern({ length: 4, min: 0, max: 1, default: 0 });
const place = (at: number) => number({ min: 0, max: 3, default: at, integer: true });

tool({
  name: "rings",
  title: "Rings",
  when: "A test",
  doc: "Rows of steps with a ring on the step that plays.",
  state: { timed: row(), free: row(), accents: row(), at: place(2), turn: place(1), third: place(3) },
  sound: () => 0,
  view: () => h("div", {},
    h(Steps, { field: "timed", playing: "at" }),
    h(Steps, { field: "free", playing: "turn", ownClock: true }),
    h(Steps, { field: "accents", playing: "third", ownClock: true, secondary: true }),
  ),
});
"#;

#[gpui::test]
fn the_ring_of_the_step_that_plays_shows_only_while_the_piece_plays(cx: &mut TestAppContext) {
    let mut window = page(cx, "rings", RINGS);
    window.until_found("steps-steps-free");
    window.wait_for_bun();
    // At rest only the row that runs on its own has one.
    assert!(window.find("step-1-playing").is_some());
    assert_eq!(window.find("step-2-playing"), None);

    window.start_playing();
    window.until_found("step-2-playing");
    assert!(window.find("step-1-playing").is_some());
    // A secondary row has none: a stack of rows shows the step once.
    assert_eq!(window.find("step-3-playing"), None);
    assert_eq!(window.problems(), []);
}

/// Sixteen steps whose sound says the sixteenth the piece is in.
const RUNNER: &str = r#"import { beat, floor, h, output, pattern, Steps, tool } from "./sdk";

tool({
  name: "runner",
  title: "Runner",
  when: "A test",
  doc: "Sixteen steps, and the one that plays as its sound says it.",
  state: { steps: pattern({ length: 16, min: 0, max: 1, default: 0 }) },
  sound: () => {
    output("step", floor(beat.times(4)).mod(16));
    return 0;
  },
  view: () => h(Steps, { field: "steps", playing: "step" }),
});
"#;

/// The ring is on the step the playhead is in at every poll, so it and a transport never show
/// two moments.
#[gpui::test]
fn the_ring_is_on_the_step_the_playhead_is_in(cx: &mut TestAppContext) {
    let mut window = page(cx, "runner", RUNNER);
    window.until_found("step-15");
    window.wait_for_bun();
    window.start_playing();
    // Its code hears that the piece plays, and the row shows a ring.
    window.until("a ring", |window| {
        (0..16).any(|step| window.find(&format!("step-{step}-playing")).is_some())
    });
    let session = window.session.clone();
    let mut steps = std::collections::BTreeSet::new();
    for _ in 0..60 {
        // Not a whole number of steps, so the playhead lands inside one.
        window.render(1000);
        window.cx.executor().advance_clock(POLL_INTERVAL);
        window.cx.run_until_parked();
        let tick = (window.cx).read(|cx| session.read(cx).playhead().read(cx).tick.0);
        // On a line the playhead is at the next step and the sound has played only the last.
        if tick % 240 == 0 {
            continue;
        }
        let step = tick / 240 % 16;
        let ring = format!("step-{step}-playing");
        assert!(window.find(&ring).is_some(), "no {ring} at tick {tick}");
        steps.insert(step);
    }
    assert!(steps.len() > 4, "{steps:?}");
}

/// Four accents, which are on or off.
const SWITCHES: &str = r#"import { h, pattern, Steps, tool } from "./sdk";

tool({
  name: "switches",
  title: "Switches",
  when: "A test",
  doc: "Four accents, each on or off, on a page that holds the keys first.",
  state: { accents: pattern({ length: 4, min: 0, max: 1, integer: true, default: [1, 0, 0, 0] }) },
  sound: () => 0,
  onKey: () => {},
  view: () => h(Steps, { field: "accents" }),
});
"#;

/// A step of a row of switches is on or off: a drag up it, which makes a hit of a row of
/// strengths soft, turns it as a click does, and the keys that change a strength do nothing.
#[gpui::test]
fn a_row_of_switches_turns_its_steps_and_no_drag_makes_one_half_strong(cx: &mut TestAppContext) {
    let mut window = page(cx, "switches", SWITCHES);
    let step = window.until_found("step-1");
    window.wait_for_bun();
    // A fifth of the way up a row of strengths.
    window.press(step);
    window.drag_to(step - gpui::point(px(0.), px(16.)));
    window.release(step - gpui::point(px(0.), px(16.)));
    window.until("the step on", |window| {
        window.record("switches")["accents"] == serde_json::json!([1, 1, 0, 0])
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Turn a step on"));
    for key in ["tab", "down", "shift-down", "left", "space"] {
        press_key(&mut window, key);
    }
    window.until("the first step off", |window| {
        window.record("switches")["accents"] == serde_json::json!([0, 1, 0, 0])
    });
    assert_eq!(window.problems(), []);
}

/// A rig that shows the cards of its box and of its beat. The box gives a display, then a knob
/// over a wide control, two more knobs, and two that do not fit beside a display. The beat gives
/// two rows of steps and five knobs, and the plain one has no view: a pattern, a list of pitches,
/// a knob and a switch.
const RIG: &str = r#"import { boolean, choice, Controls, DeviceCard, Display, h, input, Knob, list, number, pattern, SegmentedControl, Steps, tool } from "./sdk";

const level = () => number({ min: 0, max: 1, default: 0.5 });
const knob = (field: string) => h(Knob, { field, label: field, min: 0, max: 1, default: 0.5 });
const kinds = [{ value: "cassette", label: "Cassette" }, { value: "reel", label: "Reel" }] as const;
const row = () => pattern({ length: 4, min: 0, max: 1, default: 0 });

const box = tool({
  name: "box",
  title: "Box",
  when: "A test",
  doc: "A card of a display and six cells.",
  state: { a: level(), b: level(), c: level(), d: level(), e: level(), kind: choice({ options: ["cassette", "reel"], default: "cassette" }) },
  sound: () => input,
  view: () => [
    h(Display, { caption: "Level" }),
    h(Controls, {},
      knob("a"),
      h(SegmentedControl, { key: "kind", field: "kind", options: kinds }),
      knob("b"), knob("c"), knob("d"), knob("e"),
    ),
  ],
});

const beat = tool({
  name: "beat",
  title: "Beat",
  when: "A test",
  doc: "A card of two rows of steps and five knobs.",
  state: { kick: row(), hat: row(), f: level(), g: level(), i: level(), k: level(), l: level() },
  sound: () => input,
  view: () => [
    h(Steps, { key: "kick", field: "kick" }),
    h(Steps, { key: "hat", field: "hat" }),
    h(Controls, {}, knob("f"), knob("g"), knob("i"), knob("k"), knob("l")),
  ],
});

const plain = tool({
  name: "plain",
  title: "Plain",
  when: "A test",
  doc: "A card with no view of its own.",
  state: { steps: row(), pitches: list(number({ min: 0, max: 127 }), { max: 4, default: [60, 62, 64, 65] }), j: level(), hold: boolean({ default: false }) },
  sound: () => input,
});

tool({
  name: "rig",
  title: "Rig",
  when: "A test",
  doc: "Shows the cards of its children.",
  state: {},
  children: { box, beat, plain },
  behaviour: () => ({ connections: [] }),
  view: () => h("div", {}, DeviceCard({ path: "box" }), DeviceCard({ path: "beat" }), DeviceCard({ path: "plain" })),
});
"#;

fn rig(cx: &mut TestAppContext) -> super::Opened<'_> {
    let files = [
        ONLY_THE_TOOL[0],
        ("state/rig/instance.json", r#"{"tool": "rig", "state": {}}"#),
        ("state/rig/box.json", r#"{"tool": "box", "state": {}}"#),
        ("state/rig/beat.json", r#"{"tool": "beat", "state": {}}"#),
        ("state/rig/plain.json", r#"{"tool": "plain", "state": {}}"#),
        ("extensions/rig.ts", RIG),
    ];
    open(cx, &files)
}

#[gpui::test]
fn a_card_shows_its_first_columns_and_the_rest_behind_its_expand_icon(cx: &mut TestAppContext) {
    let mut window = rig(cx);
    window.until_found("space-knob-c");
    window.wait_for_bun();
    assert_eq!(window.find("space-knob-d"), None);
    // A control in a cell is the size of a card's with no size named.
    let kind = window.bounds("segmented-kind").unwrap();
    assert_eq!(kind.size.height, px(24.));

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

#[gpui::test]
fn rows_of_steps_stand_where_a_display_does_and_a_view_of_no_own_shows_them_for_a_pattern(
    cx: &mut TestAppContext,
) {
    let mut window = rig(cx);
    let kick = window.until_found("steps-kick");
    let hat = window.until_found("steps-hat");
    let knob = window.until_found("space-knob-f");
    window.wait_for_bun();
    // One over the other, at the left of the cells, of which two columns show as beside a
    // display.
    assert_eq!(kick.x, hat.x);
    assert!(
        hat.y > kick.y && knob.x > kick.x,
        "{kick:?} {hat:?} {knob:?}"
    );
    assert!(window.find("space-knob-k").is_some());
    assert_eq!(window.find("space-knob-l"), None);
    assert!(window.find("card-rig/beat-expand").is_some());

    // The default view: steps for the pattern, none for a list of numbers that is no pattern.
    let steps = window.until_found("steps-steps-steps");
    let knob = window.until_found("space-knob-j");
    assert!(knob.x > steps.x);
    assert_eq!(window.find("steps-steps-pitches"), None);
    // A switch on a field turns it, as one undo step that names it.
    let hold = window.until_found("toggle-toggle-hold");
    window.click(hold);
    window.until("the switch to turn the field on", |window| {
        window.record("rig/plain")["hold"] == true
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Turn hold on"));
    assert_eq!(window.problems(), []);
}

/// A page with a transport in its header and no action of its own: a panel as wide as what is
/// in it, and after a click on that one that grows.
/// A drum machine: two rows of steps of different lengths, each with its level, and a swing of
/// the whole thing.
const BEATS: &str = r#"import { Controls, h, Knob, number, pattern, StepGrid, Steps, tool } from "./sdk";

const level = () => number({ min: 0, max: 1, default: 0.5 });

tool({
  name: "beats",
  title: "Beats",
  when: "A test",
  doc: "Rows of steps with a knob each.",
  state: {
    kick: pattern({ length: 8, min: 0, max: 1, default: 0 }),
    hat: pattern({ length: 4, min: 0, max: 1, default: 0 }),
    kick_level: level(), hat_level: level(), swing: level(),
  },
  sound: () => 0,
  view: () =>
    h(StepGrid, { columns: ["Level"] },
      h(Steps, { field: "kick" }, h(Knob, { field: "kick_level", text: "value" })),
      h(Steps, { field: "hat" }, h(Knob, { field: "hat_level", text: "value" })),
      h(Controls, {}, h(Knob, { field: "swing" }))),
});
"#;

/// A grid of steps lines up what a view lined up by hand: the controls of rows of different
/// lengths stand in one column, and the controls of the whole thing start level with the
/// first row, under the line that names the columns.
#[gpui::test]
fn a_grid_of_steps_lines_up_the_controls_of_its_rows_and_of_the_whole(cx: &mut TestAppContext) {
    let mut window = page(cx, "beats", BEATS);
    window.until_found("space-knob-swing");
    let mut bounds = |selector: &str| window.bounds(selector).unwrap();
    let [kick, hat] = ["kick", "hat"].map(|drum| bounds(&format!("space-knob-{drum}_level")));
    assert_eq!(kick.left(), hat.left());
    assert!(hat.top() > kick.bottom());
    let [row, swing] = [bounds("steps-steps-kick"), bounds("space-knob-swing")];
    assert_eq!(swing.top(), kick.top());
    assert_eq!(row.top(), kick.top());
    assert!(swing.left() > kick.right());
    // The names of the columns stand over the first row.
    assert!(bounds("steps-steps-kick").top() > bounds("grid").top());
}

const DRONE: &str = r#"import { h, Page, Panel, Show, tool, Transport } from "./sdk";

tool({
  name: "drone",
  title: "Drone",
  when: "A test",
  doc: "A page with a transport.",
  state: {},
  memory: () => ({ grows: false }),
  sound: () => 0,
  view: ({ memory }) =>
    h(Page, { header: h(Transport, {}) },
      h(Show, { when: () => !memory.grows, fallback: () => h(Panel, { key: "wide", grow: true }, "Wide") },
        () => h(Panel, { key: "small" }, h("div", { key: "inside", style: { width: 300, height: 100 }, onClick: () => { memory.grows = true; } }))),
    ),
});
"#;

#[gpui::test]
fn a_page_with_a_transport_plays_on_space_and_keeps_a_small_panel_in_its_middle(
    cx: &mut TestAppContext,
) {
    let mut window = page(cx, "drone", DRONE);
    window.until_found("div-small");
    window.wait_for_bun();
    press_key(&mut window, "space");
    window.until("space to play", |window| window.playing());
    press_key(&mut window, "space");
    window.until("space to pause", |window| !window.playing());

    let viewport = window.cx.update(|window, _| window.viewport_size());
    let top = window.bounds("div-transport").unwrap().bottom();
    let small = window.bounds("div-small").unwrap();
    // 300 and the padding and border of a panel.
    assert_eq!(small.size.width, px(334.));
    assert_eq!(small.center().x, viewport.width / 2.);
    let (above, below) = (small.top() - top, viewport.height - small.bottom());
    assert!((above - below).abs() < px(16.), "{above} {below}");

    let inside = window.until_found("div-inside");
    window.click(inside);
    window.until_found("div-wide");
    let wide = window.bounds("div-wide").unwrap();
    assert_eq!(wide.size.width, viewport.width - px(48.));
    assert!(wide.size.height > viewport.height / 2.);
    assert_eq!(window.problems(), []);
}
