//! Parts of a view that come and go: a popover and a dialog its code opens and closes, a timer
//! that goes with the part that made it, and the one branch of a `Switch` that shows.

use gpui::{TestAppContext, point, px};

use super::{ONLY_THE_TOOL, Opened, open};

fn page<'a>(cx: &'a mut TestAppContext, name: &str, code: &str) -> Opened<'a> {
    let state = format!(r#"{{"tool": "{name}", "state": {{}}}}"#);
    let (state_path, code_path) = (
        format!("state/{name}.json"),
        format!("extensions/{name}.ts"),
    );
    let files = [
        ONLY_THE_TOOL[0],
        (state_path.as_str(), state.as_str()),
        (code_path.as_str(), code),
    ];
    open(cx, &files)
}

/// A `Popover` under the div that holds its trigger, and a `Dialog`: each opens on a click of
/// its trigger and shows a div `inside`.
const OVERLAYS: &str = r#"import { Dialog, h, Popover, signal, tool } from "./sdk";

tool({
  name: "overlays",
  title: "Overlays",
  when: "Never",
  doc: "A popover and a dialog.",
  state: {},
  view: () => {
    const [popover, setPopover] = signal(false);
    const [dialog, setDialog] = signal(false);
    const box = { width: 80, height: 20 };
    return h("div", { style: { padding: 40, gap: 40 } },
      h("div", {},
        h("div", { key: "popover", style: box, onClick: () => setPopover(!popover()) }, "Popover"),
        h(Popover, { open: popover, onClose: () => setPopover(false) }, () => h("div", { key: "inside" }, "Inside")),
      ),
      h("div", { key: "dialog", style: box, onClick: () => setDialog(true) }, "Dialog"),
      h(Dialog, { open: dialog, onClose: () => setDialog(false) }, () => h("div", { key: "inside" }, "Inside")),
      h("div", { key: "elsewhere", style: box }, "Elsewhere"),
    );
  },
});
"#;

impl Opened<'_> {
    fn closes(&mut self, what: &str) {
        self.until(what, |window| window.find("div-inside").is_none());
    }
}

#[gpui::test]
fn a_popover_shows_its_content_until_escape_or_a_press_outside_it_closes_it(
    cx: &mut TestAppContext,
) {
    let mut window = page(cx, "overlays", OVERLAYS);
    let trigger = window.until_found("div-popover");
    let elsewhere = window.until_found("div-elsewhere");
    window.click(trigger);
    let inside = window.until_found("div-inside");
    assert!(inside.y > trigger.y, "it hangs under its trigger");
    window.key_down("escape", false);
    window.closes("escape to close it");
    window.click(trigger);
    window.until_found("div-inside");
    window.click(elsewhere);
    window.closes("a press outside to close it");
    // A click of its trigger is the trigger's own, which closes it and does not open it again.
    window.click(trigger);
    window.until_found("div-inside");
    window.click(trigger);
    window.closes("its trigger to close it");
    window.wait_for_bun();
    assert!(window.find("div-inside").is_none());
}

#[gpui::test]
fn a_dialog_shows_its_content_until_escape_or_a_press_on_its_backdrop_closes_it(
    cx: &mut TestAppContext,
) {
    let mut window = page(cx, "overlays", OVERLAYS);
    let trigger = window.until_found("div-dialog");
    let elsewhere = window.until_found("div-elsewhere");
    window.click(trigger);
    window.until_found("div-inside");
    window.key_down("escape", false);
    window.closes("escape to close it");
    window.click(trigger);
    let inside = window.until_found("div-inside");
    // A click on its content is its own.
    window.click(inside);
    window.wait_for_bun();
    assert!(window.find("div-inside").is_some());
    window.click(elsewhere);
    window.closes("a press on the backdrop to close it");
}

/// A page that hears keys, with a dialog a click opens. A click in it replaces it with a second
/// dialog, which holds a text field.
const MODAL: &str = r#"import { Dialog, h, Input, tool } from "./sdk";

tool({
  name: "modal",
  title: "Modal",
  when: "Never",
  doc: "A dialog over a page that hears keys.",
  state: {},
  memory: () => ({ open: "", heard: "", typed: "" }),
  onKey: ({ memory }, { key, down }) => { if (down) memory.heard = key; },
  view: ({ memory }) => {
    const box = { width: 80, height: 20 };
    return h("div", { style: { padding: 40, gap: 40 } },
      h("div", { key: "open", style: box, onClick: () => (memory.open = "first") }, "Open"),
      () => memory.heard && h("div", { key: `heard-${memory.heard}` }, memory.heard),
      () => memory.typed && h("div", { key: `typed-${memory.typed}` }, memory.typed),
      h(Dialog, { open: () => memory.open === "first", onClose: () => (memory.open = "") }, () =>
        h("div", { key: "first", style: box, onClick: () => (memory.open = "second") }, "Next")),
      h(Dialog, { open: () => memory.open === "second", onClose: () => (memory.open = "") }, () =>
        h("div", { key: "second", style: { gap: 8 } },
          "Second",
          h(Input, { value: "", focus: true, onCommit: (text) => (memory.typed = text) }))),
    );
  },
});
"#;

impl Opened<'_> {
    /// Whether the page heard `key` go down, once Bun answered.
    fn heard_key(&mut self, key: &str) -> bool {
        self.wait_for_bun();
        self.find(&format!("div-heard-{key}")).is_some()
    }
}

#[gpui::test]
fn a_dialog_keeps_the_keys_from_the_page_but_its_text_field_has_them(cx: &mut TestAppContext) {
    let mut window = page(cx, "modal", MODAL);
    let open = window.until_found("div-open");
    window.click(open);
    let first = window.until_found("div-first");
    window.key_down("delete", false);
    assert!(!window.heard_key("delete"));
    // A click replaces it with the second, whose field takes the keys.
    window.click(first);
    window.until_found("div-second");
    window.cx.simulate_input("hi");
    window.key_down("enter", false);
    window.until_found("div-typed-hi");
    // Escape gives the field's keys back to the dialog, which still keeps them.
    window.key_down("escape", false);
    window.key_down("delete", false);
    assert!(!window.heard_key("delete"));
    assert!(!window.heard_key("h"));
    // Closed, the keys go back to the page.
    window.key_down("escape", false);
    window.until("the dialog to close", |window| {
        window.find("div-second").is_none()
    });
    window.key_down("delete", false);
    window.until_found("div-heard-delete");
    // A press on the backdrop while its field has the keys gives them back to the page too.
    window.click(open);
    let first = window.until_found("div-first");
    window.click(first);
    window.until_found("div-second");
    window.click(open);
    window.until("the dialog to close", |window| {
        window.find("div-second").is_none()
    });
    window.key_down("a", false);
    window.until_found("div-heard-a");
}

/// A page that hears keys, with a dialog that holds a space a press focuses, and a div whose
/// click opens a menu, which tells what it answered, and closes the dialog 200 ms later.
const DIALOG_PARTS: &str = r#"import { Dialog, h, Space, timeout, tool } from "./sdk";

tool({
  name: "parts",
  title: "Parts",
  when: "Never",
  doc: "A dialog with a space and a menu.",
  state: {},
  memory: () => ({ open: false, heard: "" }),
  onKey: ({ memory }, { key, down }) => { if (down) memory.heard = key; },
  view: ({ memory, openMenu }) => {
    const box = { width: 80, height: 20 };
    return h("div", { style: { padding: 40, gap: 40 } },
      h("div", { key: "open", style: box, onClick: () => (memory.open = true) }, "Open"),
      () => memory.heard && h("div", { key: `heard-${memory.heard}` }, memory.heard),
      h(Dialog, { open: () => memory.open, onClose: () => (memory.open = false) }, () =>
        h("div", { style: { gap: 8 } },
          h(Space, { key: "board", width: 80, height: 40, onPress: () => {} }),
          h("div", {
            key: "menu",
            style: box,
            onClick: () => {
              openMenu([{ label: "One" }]).then((picked) => (memory.heard = `picked-${picked}`));
              timeout(() => { memory.open = false; }, 200);
            },
          }, "Menu"))),
    );
  },
});
"#;

#[gpui::test]
fn a_dialog_keeps_the_keys_from_the_page_wherever_they_are(cx: &mut TestAppContext) {
    let mut window = page(cx, "parts", DIALOG_PARTS);
    let open = window.until_found("div-open");
    window.click(open);
    // A press on a space in it gives the card the keys.
    let board = window.until_found("space-board");
    window.click(board);
    window.key_down("delete", false);
    assert!(!window.heard_key("delete"));
    window.key_down("escape", false);
    window.until("the dialog to close", |window| {
        window.find("space-board").is_none()
    });
    window.key_down("delete", false);
    window.until_found("div-heard-delete");
}

#[gpui::test]
fn a_dialog_that_closes_while_its_menu_is_open_closes_it_and_gives_the_page_its_keys(
    cx: &mut TestAppContext,
) {
    let mut window = page(cx, "parts", DIALOG_PARTS);
    let open = window.until_found("div-open");
    window.click(open);
    let menu = window.until_found("div-menu");
    window.click(menu);
    window.until_found("menu-0");
    window.until("the timer to close the dialog", |window| {
        window.find("div-menu").is_none()
    });
    window.until_found("div-heard-picked-null");
    assert!(window.find("menu-0").is_none());
    window.key_down("a", false);
    window.until_found("div-heard-a");
}

#[gpui::test]
fn a_dialog_that_closes_while_its_menu_is_open_leaves_keys_that_went_elsewhere_there(
    cx: &mut TestAppContext,
) {
    let mut window = page(cx, "parts", DIALOG_PARTS);
    let open = window.until_found("div-open");
    window.click(open);
    let menu = window.until_found("div-menu");
    window.click(menu);
    window.until_found("menu-0");
    window.focus_the_window();
    let elsewhere = window.focused();
    window.until("the timer to close the dialog", |window| {
        window.find("div-menu").is_none()
    });
    window.until_found("div-heard-picked-null");
    assert!(window.focused() == elsewhere);
    window.key_down("a", false);
    assert!(!window.heard_key("a"));
}

/// A dialog whose click replaces it with a second, which opens a menu as it shows.
const MENU_DIALOG: &str = r#"import { Dialog, h, tool } from "./sdk";

tool({
  name: "asks",
  title: "Asks",
  when: "Never",
  doc: "A dialog that opens a menu as it shows.",
  state: {},
  memory: () => ({ open: "", heard: "" }),
  view: ({ memory, openMenu }) => {
    const box = { width: 80, height: 20 };
    return h("div", { style: { padding: 40, gap: 40 } },
      h("div", { key: "open", style: box, onClick: () => (memory.open = "first") }, "Open"),
      () => memory.heard && h("div", { key: `heard-${memory.heard}` }, memory.heard),
      h(Dialog, { open: () => memory.open === "first", onClose: () => (memory.open = "") }, () =>
        h("div", { key: "first", style: box, onClick: () => (memory.open = "second") }, "Next")),
      h(Dialog, { open: () => memory.open === "second", onClose: () => (memory.open = "") }, () => {
        openMenu([{ label: "One" }]).then((picked) => (memory.heard = `picked-${picked}`));
        return h("div", { key: "second" }, "Second");
      }),
    );
  },
});
"#;

#[gpui::test]
fn a_menu_a_dialog_opens_as_it_replaces_another_stays_open(cx: &mut TestAppContext) {
    let mut window = page(cx, "asks", MENU_DIALOG);
    let open = window.until_found("div-open");
    window.click(open);
    let first = window.until_found("div-first");
    window.click(first);
    window.until_found("div-second");
    window.until_found("menu-0");
    window.wait_for_bun();
    assert!(window.find("menu-0").is_some());
    assert!(window.find("div-heard-picked-null").is_none());
}

/// A dialog with a text field that opens with the keys, a select and a space.
const FORM_DIALOG: &str = r#"import { Dialog, h, Input, Select, Space, tool } from "./sdk";

tool({
  name: "form",
  title: "Form",
  when: "Never",
  doc: "A dialog with a text field and controls.",
  state: {},
  memory: () => ({ open: false, kind: "a" }),
  view: ({ memory }) => {
    return h("div", { style: { padding: 40 } },
      h("div", { key: "open", style: { width: 80, height: 20 }, onClick: () => (memory.open = true) }, "Open"),
      h(Dialog, { open: () => memory.open, onClose: () => (memory.open = false) }, () =>
        h("div", { key: "form", style: { gap: 8 } },
          h(Input, { value: "", focus: true }),
          h(Select, {
            key: "kind",
            value: () => memory.kind,
            onChange: (kind) => (memory.kind = kind),
            options: [{ value: "a", label: "A" }, { value: "b", label: "B" }],
          }),
          h(Space, { key: "board", width: 80, height: 40, onPress: () => {} }))),
    );
  },
});
"#;

#[gpui::test]
fn a_dialog_keeps_the_keys_after_a_press_in_it_takes_them_from_its_text_field(
    cx: &mut TestAppContext,
) {
    let mut window = page(cx, "form", FORM_DIALOG);
    for presses in [&["div-kind", "menu-1"][..], &["space-board"]] {
        let open = window.until_found("div-open");
        window.click(open);
        for selector in presses {
            let at = window.until_found(selector);
            window.click(at);
        }
        window.wait_for_bun();
        window.key_down("escape", false);
        window.until(
            &format!("escape to close the dialog after {presses:?}"),
            |window| window.find("div-form").is_none(),
        );
    }
}

/// An interval counts while a `Show` shows its part, and another, which a click of a node starts,
/// while that node shows. A click hides both and, 100 ms later, says whether the count still went
/// up.
const TICKER: &str = r#"import { h, interval, Show, timeout, tool } from "./sdk";

tool({
  name: "ticker",
  title: "Ticker",
  when: "Never",
  doc: "A count that goes up while it shows.",
  state: {},
  memory: () => ({ ticks: 0, started: false, shown: true, result: "" }),
  view: ({ memory }) => {
    const box = { width: 80, height: 20 };
    const start = h("div", {
      key: "start",
      style: box,
      onClick: () => {
        memory.started = true;
        interval(() => { memory.ticks += 1; }, 5);
      },
    }, "Start");
    return h("div", {},
      h(Show, { when: () => memory.shown }, () => {
        interval(() => { memory.ticks += 1; }, 5);
        return "Counting";
      }),
      () => memory.shown && start,
      () => memory.ticks >= 2 && memory.started && h("div", { key: "counted" }, "Counted"),
      h("div", {
        key: "hide",
        style: box,
        onClick: () => {
          memory.shown = false;
          const ticks = memory.ticks;
          timeout(() => { memory.result = memory.ticks === ticks ? "stopped" : "counting"; }, 100);
        },
      }, "Hide"),
      () => memory.result && h("div", { key: memory.result }, memory.result),
    );
  },
});
"#;

#[gpui::test]
fn a_timer_stops_when_the_part_or_the_node_that_made_it_goes(cx: &mut TestAppContext) {
    let mut window = page(cx, "ticker", TICKER);
    let start = window.until_found("div-start");
    window.click(start);
    window.until_found("div-counted");
    let hide = window.until_found("div-hide");
    window.click(hide);
    window.until("the answer", |window| {
        window.find("div-stopped").is_some() || window.find("div-counting").is_some()
    });
    assert!(window.find("div-stopped").is_some());
}

/// Two divs whose release starts a timer that counts it; a press of the second hides it first.
const RELEASES: &str = r#"import { h, timeout, tool } from "./sdk";

tool({
  name: "releases",
  title: "Releases",
  when: "Never",
  doc: "Releases that count a moment later.",
  state: {},
  memory: () => ({ stays: 0, hides: 0, shown: true }),
  view: ({ memory }) => {
    const box = { width: 80, height: 20 };
    const hides = h("div", {
      key: "hides",
      style: box,
      onPress: () => { memory.shown = false; },
      onRelease: () => timeout(() => { memory.hides += 1; }, 5),
    }, "Hides");
    return h("div", {},
      h("div", { key: "stays", style: box, onRelease: () => timeout(() => { memory.stays += 1; }, 5) }, "Stays"),
      () => memory.shown && hides,
      () => h("div", { key: `counted-${memory.stays}-${memory.hides}` }, "Counted"),
    );
  },
});
"#;

#[gpui::test]
fn a_release_runs_with_its_node_and_what_it_starts_goes_with_it(cx: &mut TestAppContext) {
    let mut window = page(cx, "releases", RELEASES);
    let stays = window.until_found("div-stays");
    let hides = window.until_found("div-hides");
    window.click(stays);
    window.until_found("div-counted-1-0");
    // Its node is gone as it hears the release, so the timer it starts never runs.
    window.click(hides);
    window.until("the node to go", |window| {
        window.find("div-hides").is_none()
    });
    window.click(stays);
    window.until("the second count", |window| {
        window.find("div-counted-1-0").is_none()
    });
    assert!(window.find("div-counted-2-0").is_some());
}

/// A `Switch` of two matches that both hold at 5; a click moves the level to 2, then to 0.
const LEVELS: &str = r#"import { h, Match, Switch, tool } from "./sdk";

tool({
  name: "levels",
  title: "Levels",
  when: "Never",
  doc: "A word for a level.",
  state: {},
  memory: () => ({ level: 5 }),
  view: ({ memory }) =>
    h("div", {},
      h("div", { key: "lower", style: { width: 80, height: 20 }, onClick: () => (memory.level = memory.level === 5 ? 2 : 0) }, "Lower"),
      h(Switch, { fallback: () => h("div", { key: "silent" }, "Silent") },
        h(Match, { when: () => memory.level > 3 }, () => h("div", { key: "loud" }, "Loud")),
        h(Match, { when: () => memory.level > 1 }, () => h("div", { key: "on" }, "On")),
      ),
    ),
});
"#;

#[gpui::test]
fn a_switch_shows_its_first_match_that_holds_else_its_fallback(cx: &mut TestAppContext) {
    let mut window = page(cx, "levels", LEVELS);
    window.until_found("div-loud");
    assert!(window.find("div-on").is_none());
    let lower = window.until_found("div-lower");
    window.click(lower);
    window.until_found("div-on");
    assert!(window.find("div-loud").is_none());
    window.click(lower);
    window.until_found("div-silent");
    assert!(window.find("div-on").is_none());
}

/// A block whose drag, at its end, shows `ended` a moment later.
const LATER: &str = r##"import { h, Rect, Space, timeout, tool } from "./sdk";

tool({
  name: "later",
  title: "Later",
  when: "Never",
  doc: "A block that says it was dragged, a moment after.",
  state: {},
  memory: () => ({ ended: false }),
  view: ({ memory }) =>
    h("div", {},
      h(Space, { key: "board", width: 100, height: 100 },
        h(Rect, {
          x: 0, y: 0, width: 100, height: 100, color: "#ffffff",
          drag: { id: "block", onEnd: () => timeout(() => { memory.ended = true; }, 5) },
        }),
      ),
      () => memory.ended && h("div", { key: "ended" }, "Ended"),
    ),
});
"##;

#[gpui::test]
fn the_end_of_a_drag_starts_a_timer(cx: &mut TestAppContext) {
    let mut window = page(cx, "later", LATER);
    let block = window.until_found("space-board");
    window.drag(block, block + point(px(20.), px(0.)));
    window.until_found("div-ended");
}

/// Two divs whose clicks open menus of three items, the second with its first checked, and a
/// row that wraps four boxes in a box taller than their two lines.
const MENUS_AND_WRAP: &str = r#"import { h, tool } from "./sdk";

tool({
  name: "menus",
  title: "Menus",
  when: "Never",
  doc: "Two menus and a row that wraps.",
  state: {},
  view: ({ openMenu }) => {
    const box = { width: 80, height: 20 };
    const items = (checked: boolean) => ["One", "Two", "Three"].map((label, place) => ({ label, checked: checked && place === 0 }));
    return h("div", { style: { padding: 40, gap: 40 } },
      h("div", { key: "first", style: box, onClick: () => openMenu(items(false)) }, "First"),
      h("div", { key: "second", style: box, onClick: () => openMenu(items(true)) }, "Second"),
      h("div", { style: { direction: "row", wrap: true, width: 100, height: 300 } },
        ["a", "b", "c", "d"].map((name) => h("div", { key: name, style: { width: 40, height: 20 } }, name))),
    );
  },
});
"#;

#[gpui::test]
fn a_menu_checks_only_what_its_code_says_whatever_was_picked_before(cx: &mut TestAppContext) {
    let mut window = page(cx, "menus", MENUS_AND_WRAP);
    let (first, second) = (
        window.until_found("div-first"),
        window.until_found("div-second"),
    );
    window.click(first);
    let three = window.until_found("menu-2");
    window.click(three);
    window.until("the menu to close", |window| {
        window.find("menu-2").is_none()
    });
    window.click(second);
    window.until_found("menu-check-0");
    assert!(window.find("menu-check-2").is_none());
}

#[gpui::test]
fn a_row_that_wraps_puts_its_lines_one_after_another(cx: &mut TestAppContext) {
    let mut window = page(cx, "menus", MENUS_AND_WRAP);
    window.until_found("div-c");
    let (first, third) = (
        window.bounds("div-a").unwrap(),
        window.bounds("div-c").unwrap(),
    );
    assert_eq!(third.origin.y, first.bottom());
}

/// A `grow` column in a box 200 high, holding a list 600 high that scrolls.
const GROWN_LIST: &str = r#"import { h, tool } from "./sdk";

tool({
  name: "grown",
  title: "Grown",
  when: "Never",
  doc: "A grown column with a long list.",
  state: {},
  view: () =>
    h("div", { key: "box", style: { height: 200 } },
      h("div", { key: "grown", style: { grow: true } },
        h("div", { style: { scroll: "y" } },
          Array.from({ length: 30 }, (_, row) => h("div", { key: `row-${row}`, style: { height: 20 } }, `${row}`))),
      ),
    ),
});
"#;

#[gpui::test]
fn a_grown_column_takes_its_parents_height_not_its_lists(cx: &mut TestAppContext) {
    let mut window = page(cx, "grown", GROWN_LIST);
    window.until_found("div-row-0");
    let (parent, grown) = (
        window.bounds("div-box").unwrap(),
        window.bounds("div-grown").unwrap(),
    );
    assert_eq!(grown.size.height, parent.size.height);
}
