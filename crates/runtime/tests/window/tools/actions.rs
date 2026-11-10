//! The actions of a tool of the project: a key runs those it does now, in the order the tool has
//! them until one does it, from its page, from a card inside it, or from anywhere in the window.
//! The composer's own key for one moves it, also for the computer keys and for what a menu shows.

use gpui::TestAppContext;
use sound_ui::Shortcuts;

use super::parts::page;
use super::{EXPERIMENT, ONLY_THE_TOOL, Opened, open};

/// A page that writes down in its record what its actions did. `d` does `chosen` once a click
/// chose something, else `rest`. A click on `menu` opens a menu whose row shows the key of `go`.
const PROBE: &str = r#"import { h, Input, text, tool } from "./sdk";

const did = (what: string) => ({ update }: { update: (label: string, change: (part: { state: { did: string } }) => void) => void }) =>
  update(what, ({ state }) => { state.did = `${state.did} ${what}`.trim(); });

tool({
  name: "probe",
  title: "Probe",
  when: "A test",
  doc: "Writes down what its actions did.",
  state: { did: text() },
  memory: () => ({ chosen: false }),
  sound: () => 0,
  actions: {
    go: { title: "Go", keys: ["g"], run: did("go") },
    chosen: { title: "Do the chosen", keys: ["d"], run: (tool) => (tool.memory.chosen ? did("chosen")(tool) : false) },
    rest: { title: "Do the rest", keys: ["d"], run: did("rest") },
    up: { title: "Up", keys: ["up"], run: did("up") },
    octave: { title: "Up an octave", keys: ["shift-up"], run: did("octave") },
    play: { title: "Play", keys: ["space"], everywhere: true, run: did("play") },
    fail: { title: "Fail", keys: ["f"], run: () => { throw new Error("nothing to fail on"); } },
    after: { title: "After the failure", keys: ["f"], run: did("after") },
  },
  view: ({ memory, openMenu }) => {
    const box = { width: 120, height: 20 };
    return h("div", { style: { padding: 40, gap: 40 } },
      h("div", { key: "choose", style: box, onClick: () => (memory.chosen = true) }, "Choose"),
      h("div", { key: "menu", style: box, onClick: () => openMenu([{ label: "Go", shortcut: "probe.go" }]) }, "Menu"),
      h("div", { key: "field", style: box }, h(Input, { value: "" })),
    );
  },
});
"#;

impl Opened<'_> {
    /// What the actions of `instance` wrote down, once Bun answered.
    pub(super) fn did(&mut self, instance: &str) -> String {
        self.wait_for_bun();
        let did = self.record(instance)["did"].as_str().map(str::to_string);
        did.unwrap_or_default()
    }

    /// The composer picks `keys` for the action `id`.
    pub(super) fn set_keys(&mut self, id: &str, keys: &[&str]) {
        let keys = keys.iter().map(ToString::to_string).collect();
        self.cx.update(|_, cx| {
            let shortcuts = cx.global_mut::<Shortcuts>();
            shortcuts.set_keys(id, keys).unwrap();
        });
    }
}

#[gpui::test]
fn a_key_runs_the_actions_it_does_in_order_until_one_does_it(cx: &mut TestAppContext) {
    let mut window = page(cx, "probe", PROBE);
    window.until_found("div-choose");
    window.keys("g");
    window.until("go", |window| window.did("probe") == "go");
    // Nothing is chosen: the first action of `d` passes it on.
    window.keys("d");
    window.until("the rest", |window| window.did("probe") == "go rest");
    let choose = window.until_found("div-choose");
    window.click(choose);
    window.keys("d");
    window.until("the chosen", |window| {
        window.did("probe") == "go rest chosen"
    });
    // Shift is a key of its own, and a key no action has does nothing.
    window.keys("shift-up up shift-g");
    window.until("up", |window| {
        window.did("probe") == "go rest chosen octave up"
    });
    assert_eq!(window.problems(), []);
    // An action that fails is a problem of its tool, and the key ends there.
    window.keys("f");
    window.until("the failure", |window| !window.problems().is_empty());
    let problems = window.problems();
    assert!(
        (problems[0].message).contains("its action fail failed: Error: nothing to fail on"),
        "{problems:?}"
    );
    assert_eq!(window.did("probe"), "go rest chosen octave up");
}

#[gpui::test]
fn the_composers_key_moves_an_action_and_a_menu_shows_the_key_it_has(cx: &mut TestAppContext) {
    let mut window = page(cx, "probe", PROBE);
    let menu = window.until_found("div-menu");
    let shown = |window: &mut Opened<'_>, keys: Option<&str>| {
        window.click(menu);
        window.until_found("menu-0");
        for key in ["g", "j"] {
            let shows = window.find(&format!("kbd-{key}")).is_some();
            assert_eq!(shows, keys == Some(key), "{key}");
        }
        window.key_down("escape", false);
        window.until("the menu closed", |window| window.find("menu-0").is_none());
    };
    shown(&mut window, Some("g"));

    window.set_keys("probe.go", &["j", "g"]);
    shown(&mut window, Some("j"));
    window.set_keys("probe.go", &["j"]);
    window.keys("g j");
    window.until("go on its new key", |window| window.did("probe") == "go");

    window.set_keys("probe.go", &[]);
    shown(&mut window, None);
    window.keys("g j");
    assert_eq!(window.did("probe"), "go");
}

#[gpui::test]
fn an_action_that_runs_everywhere_runs_outside_its_page_but_not_in_a_text_field(
    cx: &mut TestAppContext,
) {
    let mut window = page(cx, "probe", PROBE);
    window.until_found("div-choose");
    window.focus_the_window();
    // `g` runs only where its page has the keys.
    window.keys("g space");
    window.until("play", |window| window.did("probe") == "play");

    let field = window.until_found("div-field");
    window.click(field);
    window.until("the field has the keys", |window| {
        window.cx.update(|window, _| {
            (window.context_stack().iter())
                .any(|context| context.contains(sound_ui::components::text_input::KEY_CONTEXT))
        })
    });
    window.keys("space g");
    assert_eq!(window.did("probe"), "play");
}

/// A rig whose page shows the card of its pad. Both have an action on `p`, the rig one on `g`.
const RIG: &str = r#"import { Button, DeviceCard, h, text, tool } from "./sdk";

const did = (what: string) => ({ update }: { update: (label: string, change: (part: { state: { did: string } }) => void) => void }) =>
  update(what, ({ state }) => { state.did = `${state.did} ${what}`.trim(); });

const pad = tool({
  name: "pad",
  title: "Pad",
  when: "A test",
  doc: "A card with a button and an action.",
  state: { did: text() },
  sound: () => 0,
  actions: { hit: { title: "Hit", keys: ["p"], run: did("hit") } },
  view: () => h(Button, { key: "pad", label: "Pad", onClick: () => {} }),
});

tool({
  name: "rig",
  title: "Rig",
  when: "A test",
  doc: "Shows the card of its pad.",
  state: { did: text() },
  children: { pad },
  behaviour: () => ({ connections: [] }),
  actions: {
    go: { title: "Go", keys: ["g"], run: did("go") },
    pass: { title: "Pass", keys: ["p"], run: did("pass") },
  },
  view: () => h("div", { key: "page", style: { width: 600, height: 400 } }, DeviceCard({ path: "pad" })),
});
"#;

#[gpui::test]
fn a_key_a_card_has_no_action_for_reaches_the_actions_of_the_page_around_it(
    cx: &mut TestAppContext,
) {
    let files = [
        ONLY_THE_TOOL[0],
        ("state/rig/instance.json", r#"{"tool": "rig", "state": {}}"#),
        ("state/rig/pad.json", r#"{"tool": "pad", "state": {}}"#),
        ("extensions/rig.ts", RIG),
    ];
    let mut window = open(cx, &files);
    window.until_found("button-pad");
    // Onto the button of the card.
    window.keys("tab");
    window.keys("p g");
    window.until("the page's action", |window| window.did("rig") == "go");
    assert_eq!(window.did("rig/pad"), "hit");
    assert_eq!(window.problems(), []);
}

/// The tool of `KEYS` with an action on `a` in place of its `onKey`: it counts in its record, and
/// keeps the last note the computer keys played.
const COUNTER: &str = r#"import { adsr, h, note, number, sine, tool } from "./sdk";

tool({
  name: "keys",
  title: "Keys",
  when: "A test",
  doc: "Counts on a.",
  voices: 8,
  state: { last: number({ min: 0, max: 127, default: 0 }), count: number({ min: 0, max: 100, default: 0 }) },
  sound: () => sine(note.freq).times(adsr(note.gate, 1, 1, 1, 1)).times(0.2),
  actions: {
    count: { title: "Count", keys: ["a"], run: ({ update }) => update("Count", ({ state }) => { state.count = (state.count ?? 0) + 1; }) },
  },
  onNotes: ({ update }, event) => {
    if (event.type !== "noteOn") return;
    update("Keep the note", ({ state }) => { state.last = event.pitch; });
  },
  view: () => h("div", {}, "Play a"),
});
"#;

#[gpui::test]
fn the_computer_keys_leave_the_key_an_action_has_now(cx: &mut TestAppContext) {
    let files = [
        EXPERIMENT[0],
        EXPERIMENT[1],
        ("extensions/keys.ts", COUNTER),
    ];
    let mut window = open(cx, &files);
    window.cx.simulate_keystrokes("secondary-k");
    window.settle();
    window.settle();
    window.keys("a");
    window.until("a counted", |window| window.record("keys")["count"] == 1);
    window.wait_for_bun();
    assert_eq!(window.record("keys")["last"], serde_json::Value::Null);

    window.set_keys("keys.count", &["s"]);
    window.keys("a");
    window.until("a played by the computer keys", |window| {
        window.record("keys")["last"] == 60
    });
    window.keys("s");
    window.until("s counted", |window| window.record("keys")["count"] == 2);
    window.wait_for_bun();
    assert_eq!(window.record("keys")["last"], 60);
}
