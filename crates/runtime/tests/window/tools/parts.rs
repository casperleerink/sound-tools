//! Parts of a view that come and go: a popover and a dialog its code opens and closes, a timer
//! that goes with the part that made it, and the one branch of a `Switch` that shows.

use gpui::TestAppContext;

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

/// A `Popover` under the div that holds its trigger, and a `Dialog`: each opens on a press of
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
        h("div", { key: "popover", style: box, onPress: () => setPopover(!popover()) }, "Popover"),
        h(Popover, { open: popover, onClose: () => setPopover(false) }, () => h("div", { key: "inside" }, "Inside")),
      ),
      h("div", { key: "dialog", style: box, onPress: () => setDialog(true) }, "Dialog"),
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
    // A press of its trigger is the trigger's own, which closes it and does not open it again.
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
    // A press on its content is its own.
    window.click(inside);
    window.wait_for_bun();
    assert!(window.find("div-inside").is_some());
    window.click(elsewhere);
    window.closes("a press on the backdrop to close it");
}

/// An interval counts while a `Show` shows its part. A press hides it and, 100 ms later, says
/// whether the count still went up.
const TICKER: &str = r#"import { h, interval, Show, timeout, tool } from "./sdk";

tool({
  name: "ticker",
  title: "Ticker",
  when: "Never",
  doc: "A count that goes up while it shows.",
  state: {},
  memory: () => ({ ticks: 0, shown: true, result: "" }),
  view: ({ memory }) =>
    h("div", {},
      h(Show, { when: () => memory.shown }, () => {
        interval(() => { memory.ticks += 1; }, 5);
        return "Counting";
      }),
      () => memory.ticks >= 2 && h("div", { key: "counted" }, "Counted"),
      h("div", {
        key: "hide",
        style: { width: 80, height: 20 },
        onPress: () => {
          memory.shown = false;
          const ticks = memory.ticks;
          timeout(() => { memory.result = memory.ticks === ticks ? "stopped" : "counting"; }, 100);
        },
      }, "Hide"),
      () => memory.result && h("div", { key: memory.result }, memory.result),
    ),
});
"#;

#[gpui::test]
fn a_timer_stops_when_the_part_that_made_it_goes(cx: &mut TestAppContext) {
    let mut window = page(cx, "ticker", TICKER);
    window.until_found("div-counted");
    let hide = window.until_found("div-hide");
    window.click(hide);
    window.until("the answer", |window| {
        window.find("div-stopped").is_some() || window.find("div-counting").is_some()
    });
    assert!(window.find("div-stopped").is_some());
}

/// A `Switch` of two matches that both hold at 5; a press moves the level to 2, then to 0.
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
      h("div", { key: "lower", style: { width: 80, height: 20 }, onPress: () => (memory.level = memory.level === 5 ? 2 : 0) }, "Lower"),
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
