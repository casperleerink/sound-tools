//! What moves on a page moves in the window, with no Bun: a knob turns its field as one undo
//! step, and a drag moves what binds to it; Bun hears the drag once, at its end.

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, point, px,
};

use super::{ONLY_THE_TOOL, Window, open};

impl Window<'_> {
    pub(super) fn press(&mut self, position: Point<Pixels>) {
        self.cx
            .simulate_mouse_move(position, None, Modifiers::default());
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    pub(super) fn drag_to(&mut self, position: Point<Pixels>) {
        self.cx
            .simulate_mouse_move(position, MouseButton::Left, Modifiers::default());
        self.cx.run_until_parked();
    }

    pub(super) fn release(&mut self, position: Point<Pixels>) {
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

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

fn open_project<'a>(cx: &'a mut TestAppContext, files: &[(String, String)]) -> Option<Window<'a>> {
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
  view: () => h(Knob, { value: "level", label: "Level", min: 0, max: 1, default: 0.5 }),
});
"#;

#[gpui::test]
fn a_knob_of_a_page_turns_its_field_as_it_drags_as_one_undo_step_and_asks_bun_for_nothing(
    cx: &mut TestAppContext,
) {
    let Some(mut window) = open_project(cx, &project("dial", DIAL)) else {
        return;
    };
    let dial = window.space("knob-level") + point(px(28.), px(20.));
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
    let Some(mut window) = open_project(cx, &project("mover", MOVER)) else {
        return;
    };
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
  view: () => h(Steps, { path: "steps" }),
});
"#;

#[gpui::test]
fn a_click_on_a_step_turns_it_on_through_its_code(cx: &mut TestAppContext) {
    let Some(mut window) = open_project(cx, &project("row", STEPS)) else {
        return;
    };
    let second = window.space("steps-steps") + point(px(17. + 7.), px(7.));
    window.press(second);
    window.release(second);
    window.until("the step to turn on", |window| {
        window.record("row")["steps"] == serde_json::json!([0, 1, 0, 0])
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Turn a step on"));
}
