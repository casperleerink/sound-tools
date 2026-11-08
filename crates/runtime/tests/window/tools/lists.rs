//! Lists in a div that scrolls: which of their rows draw, and what a div keeps of its scroll.

use gpui::{Pixels, Point, ScrollDelta, ScrollEvent, TestAppContext, TouchPhase, point, px};

use super::Opened;
use super::parts::page;

fn scroll(window: &mut Opened, at: Point<Pixels>, down: f32) {
    window.cx.simulate_event(ScrollEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), px(-down))),
        touch_phase: TouchPhase::Moved,
        ..Default::default()
    });
}

fn top(window: &mut Opened, selector: &str) -> Pixels {
    window.bounds(selector).unwrap().top()
}

/// A point in the box of a div that scrolls, by what it scrolls: its middle may lie below.
fn in_box_of(window: &mut Opened, selector: &str) -> Point<Pixels> {
    window.until_found(selector);
    window.bounds(selector).unwrap().origin + point(px(10.), px(10.))
}

/// A tool of one view that `rows` fills: a list 100 points high.
fn list(rows: &str) -> String {
    format!(
        r#"import {{ Button, h, signal, tool }} from "./sdk";

tool({{
  name: "list",
  title: "List",
  when: "Never",
  doc: "A list.",
  state: {{}},
  view: () => {{
    const [tall, setTall] = signal(false);
    return h("div", {{ style: {{ gap: 8 }} }},
      h("div", {{ key: "taller", style: {{ width: 80, height: 20 }}, onClick: () => setTall(true) }}, "Taller"),
      h("div", {{ key: "list", style: () => ({{ width: 200, height: tall() ? 300 : 100 }}) }},
        h("div", {{ style: {{ scroll: "y", grow: true }} }},
          Array.from({{ length: 40 }}, (_, row) => {rows}))));
  }},
}});
"#
    )
}

#[gpui::test]
fn rows_whose_height_a_minimum_can_change_all_draw(cx: &mut TestAppContext) {
    let rows = r#"h("div", { key: `row-${row}`, style: { height: 24, minHeight: 48 } }, `${row}`)"#;
    let mut window = page(cx, "list", &list(rows));
    let list = window.until_found("div-list");
    scroll(&mut window, list, 96.);
    assert_eq!(top(&mut window, "div-row-2"), top(&mut window, "div-list"));
}

#[gpui::test]
fn rows_that_hold_what_tab_goes_to_all_draw(cx: &mut TestAppContext) {
    let rows = r#"h("div", { style: { height: 24 } }, h(Button, { key: `row-${row}`, label: "Go", onClick: () => {} }))"#;
    let mut window = page(cx, "list", &list(rows));
    window.until_found("div-list");
    assert!(window.find("button-row-39").is_some());
}

#[gpui::test]
fn a_list_that_grows_taller_draws_the_rows_it_now_shows(cx: &mut TestAppContext) {
    let rows = r#"h("div", { key: `row-${row}`, style: { height: 20 } }, `${row}`)"#;
    let mut window = page(cx, "list", &list(rows));
    let taller = window.until_found("div-taller");
    assert!(window.find("div-row-10").is_none());
    window.click(taller);
    // Looked up as drawn, not drawn again as `find` does.
    window.until("the list grows", |window| {
        window
            .cx
            .update(|window, cx| window.simulate_next_frame(cx));
        window.cx.run_until_parked();
        (window.cx.debug_bounds("div-list")).is_some_and(|list| list.size.height == px(300.))
    });
    window
        .cx
        .update(|window, cx| window.simulate_next_frame(cx));
    window.cx.run_until_parked();
    assert!(window.cx.debug_bounds("div-row-10").is_some());
}

/// A div with a key with a `#` in it that scrolls, and a click that changes another part.
const KEPT: &str = r##"import { h, tool } from "./sdk";

tool({
  name: "kept",
  title: "Kept",
  when: "Never",
  doc: "A list of a key with a hash, and a label.",
  state: {},
  memory: () => ({ label: "one" }),
  view: ({ memory }) =>
    h("div", { style: { gap: 8 } },
      h("div", { key: "label", style: { width: 80, height: 20 }, onClick: () => { memory.label = "two"; } }, "Label"),
      () => h("div", { key: memory.label }, memory.label),
      h("div", { key: "list#1000000", style: { scroll: "y", width: 80, height: 40 } },
        h("div", { key: "inside", style: { height: 200 } }, "x")),
    ),
});
"##;

#[gpui::test]
fn a_div_keeps_its_scroll_whatever_its_key_says(cx: &mut TestAppContext) {
    let mut window = page(cx, "kept", KEPT);
    let inside = in_box_of(&mut window, "div-inside");
    let unscrolled = top(&mut window, "div-inside");
    scroll(&mut window, inside, 50.);
    assert_eq!(top(&mut window, "div-inside"), unscrolled - px(50.));
    let label = window.until_found("div-label");
    window.click(label);
    window.until_found("div-two");
    assert_eq!(top(&mut window, "div-inside"), unscrolled - px(50.));
}

/// Two divs with no key that scroll, after a div `before`: with two children in the saved
/// code, the first scroller's node has the number the second's had.
fn scrollers(before: &str) -> String {
    format!(
        r#"import {{ h, tool }} from "./sdk";

tool({{
  name: "scrollers",
  title: "Scrollers",
  when: "Never",
  doc: "Two scrollers after a div.",
  state: {{}},
  view: () =>
    h("div", {{ style: {{ gap: 8 }} }},
      {before},
      h("div", {{ style: {{ scroll: "y", width: 80, height: 40 }} }}, h("div", {{ key: "inside-a", style: {{ height: 200 }} }}, "a")),
      h("div", {{ style: {{ scroll: "y", width: 80, height: 40 }} }}, h("div", {{ key: "inside-b", style: {{ height: 200 }} }}, "b", "c")),
    ),
}});
"#
    )
}

#[gpui::test]
fn a_save_of_the_code_gives_no_div_without_a_key_the_scroll_of_another(cx: &mut TestAppContext) {
    let first = scrollers(r#"h("div", { style: { height: 20 } })"#);
    let mut window = page(cx, "scrollers", &first);
    let b = in_box_of(&mut window, "div-inside-b");
    let unscrolled = (
        top(&mut window, "div-inside-a"),
        top(&mut window, "div-inside-b"),
    );
    scroll(&mut window, b, 50.);
    assert_eq!(top(&mut window, "div-inside-b"), unscrolled.1 - px(50.));
    let saved = scrollers(
        r#"h("div", { key: "saved", style: { direction: "row", height: 20 } }, "1", "2")"#,
    );
    window.save_code("extensions/scrollers.ts", &saved);
    window.until_found("div-saved");
    let now = (
        top(&mut window, "div-inside-a"),
        top(&mut window, "div-inside-b"),
    );
    assert_eq!(now, unscrolled);
}
