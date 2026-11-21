//! What code asks and tells the window, and the keys of tools: on the offline engine with Bun.

use rune::{FileDialogKind, Key, KeyEvent, Mods};

use super::*;
use crate::app::MAIN_MODIFIER;
use crate::app::frame::poll;
use crate::app::keys::{on_focus, on_key_up};

/// A new project of the DAW with `files` written into it, its window showing `main`.
fn page(files: &[(&str, &str)], main: &str) -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    for (path, contents) in files {
        let path = folder.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let mut state = State::open(Options {
        folder,
        headless: true,
        play: None,
        main: Some(InstanceId::new(main).unwrap()),
        act: None,
    })
    .unwrap();
    frame(&mut state, 0.0);
    (scratch, laid_out(state))
}

fn record_of(state: &State, id: &str) -> Value {
    let id = InstanceId::new(id).unwrap();
    serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap()
}

/// What the actions of `id` wrote down.
fn did(state: &State, id: &str) -> String {
    record_of(state, id)["did"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn problems(state: &State) -> Vec<String> {
    let problems = state.project().problems();
    problems
        .iter()
        .map(|problem| format!("{}: {}", problem.path, problem.message))
        .collect()
}

/// A key as the keymap names it, `shift-up` or `cmd-c`, as the system sends it.
pub(super) fn event(chord: &str) -> KeyEvent {
    let mut mods = Mods::default();
    let mut name = chord;
    loop {
        let held = [("cmd-", 0), ("ctrl-", 1), ("alt-", 2), ("shift-", 3)];
        let Some((rest, which)) =
            (held.iter()).find_map(|(prefix, which)| Some((name.strip_prefix(prefix)?, *which)))
        else {
            break;
        };
        match which {
            0 => (mods.cmd, mods.logo, mods.ctrl) = (true, MAIN_MODIFIER.logo, MAIN_MODIFIER.ctrl),
            1 => mods.ctrl = true,
            2 => mods.alt = true,
            _ => mods.shift = true,
        }
        name = rest;
    }
    let key = match name {
        "space" => Key::Text(" ".into()),
        "up" => Key::Up,
        "enter" => Key::Enter,
        "escape" => Key::Escape,
        "f10" => Key::Function(10),
        letter if mods.cmd => Key::Command(letter.chars().next().unwrap()),
        letter => Key::Text(letter.into()),
    };
    KeyEvent { key, mods }
}

/// Each key of `keys`, down and up, and the frame that answers it.
fn keys(window: &mut rune::Runtime<State>, keys: &str) {
    for chord in keys.split(' ') {
        window.key(event(chord));
        window.key_up(event(chord));
        redraw(window);
    }
}

/// The element `key` takes the keys.
fn focus(window: &mut rune::Runtime<State>, key: &str) {
    let wanted = key.to_string();
    window.update(move |_, tx| tx.focus(&wanted, false));
    redraw(window);
    assert_eq!(focused(window).as_deref(), Some(key));
}

/// The frame of the app, with its `Tx`: it opens the file dialogs code asked for.
fn polled(window: &mut rune::Runtime<State>) {
    let seconds = window.state.last + 1.0 / 60.0;
    window.update(|state, tx| {
        poll(state, seconds, tx);
    });
    window.frame(1.0 / 60.0);
}

/// A page that writes down in its record what its actions did. `d` does `chosen` once a press
/// chose something, else `rest`. Its label `go` says the key of `go`.
const PROBE: &str = r#"import { h, input, text, tool } from "./sdk";

type Doing = { update: (label: string, change: (part: { state: { did: string } }) => void) => void };
const did = (what: string) => ({ update }: Doing) => update(what, (part) => void (part.state.did = `${part.state.did} ${what}`.trim()));

export const probe = tool({
  name: "probe", title: "Probe", when: "Never", doc: "Writes down what its actions did.",
  state: { did: text() }, memory: () => ({ chosen: false }), sound: () => input,
  actions: {
    go: { title: "Go", keys: ["g"], run: did("go") },
    chosen: { title: "Do the chosen", keys: ["d"], run: (tool) => (tool.memory.chosen ? did("chosen")(tool) : false) },
    rest: { title: "Do the rest", keys: ["d"], run: did("rest") },
    up: { title: "Up", keys: ["up"], run: did("up") },
    octave: { title: "Up an octave", keys: ["shift-up"], run: did("octave") },
    mark: { title: "Mark", keys: ["m"], everywhere: true, run: did("mark") },
    fail: { title: "Fail", keys: ["f"], run: () => { throw new Error("nothing to fail on"); } },
    after: { title: "After the failure", keys: ["f"], run: did("after") },
  },
  view: (_, { memory, shortcut }) => (
    <col>
      <box key="choose" w={120} h={20} label="Choose" onPress={() => void (memory.chosen = true)} />
      <box key="go" w={120} h={20} label={`Go on ${shortcut("go") ?? "no key"}`} onPress={() => {}} />
      <field key="field" label="Name" value="" onCommit={() => {}} />
    </col>
  ),
});
"#;

fn probe() -> (tempfile::TempDir, rune::Runtime<State>) {
    let files = [
        ("extensions/probe.tsx", PROBE),
        ("state/probe.json", r#"{"tool": "probe", "state": {}}"#),
    ];
    page(&files, "probe")
}

#[test]
fn a_key_runs_the_actions_it_does_in_order_until_one_does_it() {
    let (_scratch, mut window) = probe();
    keys(&mut window, "g");
    assert_eq!(did(&window.state, "probe"), "go");
    // Nothing is chosen: the first action of `d` passes it on.
    keys(&mut window, "d");
    assert_eq!(did(&window.state, "probe"), "go rest");
    click(&mut window, "choose");
    keys(&mut window, "d");
    assert_eq!(did(&window.state, "probe"), "go rest chosen");
    // Shift is a key of its own, and a key no action has does nothing.
    keys(&mut window, "shift-up up shift-g");
    assert_eq!(did(&window.state, "probe"), "go rest chosen octave up");
    assert_eq!(problems(&window.state), Vec::<String>::new());
    // An action that fails is a problem of its tool, and the key ends there.
    keys(&mut window, "f");
    let problems = problems(&window.state);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("its action fail failed: Error: nothing to fail on")),
        "{problems:?}"
    );
    assert_eq!(did(&window.state, "probe"), "go rest chosen octave up");
}

#[test]
fn every_key_counts_when_the_keys_come_faster_than_bun_answers() {
    let (_scratch, mut window) = probe();
    for _ in 0..5 {
        window.key(event("g"));
    }
    redraw(&mut window);
    assert_eq!(did(&window.state, "probe"), "go go go go go");
    // The five are five steps of undo, as five presses are.
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(did(&window.state, "probe"), "go go go go");
}

#[test]
fn the_composers_key_moves_an_action_and_its_shortcut_says_the_key_it_has() {
    let (_scratch, mut window) = probe();
    let shows = |window: &rune::Runtime<State>, label: &str| {
        labels(&window.state).contains(&label.to_string())
    };
    assert!(shows(&window, "Go on G"), "{:?}", labels(&window.state));
    let keymap = &mut window.state.keys.keymap;
    keymap
        .set_keys("probe.go", vec!["cmd-shift-j".into(), "g".into()])
        .unwrap();
    redraw(&mut window);
    // As this system draws the key: the main one is Ctrl on Windows and Linux.
    let drawn = match cfg!(target_os = "macos") {
        true => "Go on ⇧⌘J",
        false => "Go on Ctrl+Shift+J",
    };
    assert!(shows(&window, drawn), "{:?}", labels(&window.state));
    let keymap = &mut window.state.keys.keymap;
    keymap.set_keys("probe.go", vec!["j".into()]).unwrap();
    keys(&mut window, "g j");
    assert_eq!(did(&window.state, "probe"), "go");
    let keymap = &mut window.state.keys.keymap;
    keymap.set_keys("probe.go", Vec::new()).unwrap();
    redraw(&mut window);
    assert!(
        shows(&window, "Go on no key"),
        "{:?}",
        labels(&window.state)
    );
    keys(&mut window, "g j");
    assert_eq!(did(&window.state, "probe"), "go");
}

/// The composer may give a tool's action a key of the app: the tool's comes first, but undo
/// stays undo.
#[test]
fn a_tool_s_key_comes_before_the_app_s_but_never_before_undo() {
    let (_scratch, mut window) = probe();
    let keymap = &mut window.state.keys.keymap;
    keymap.set_keys("probe.go", vec!["cmd-k".into()]).unwrap();
    keys(&mut window, "cmd-k cmd-k");
    assert_eq!(did(&window.state, "probe"), "go go");
    assert!(!window.state.session.transport().keys);
    let keymap = &mut window.state.keys.keymap;
    keymap.set_keys("probe.go", vec!["cmd-z".into()]).unwrap();
    keys(&mut window, "cmd-z");
    assert_eq!(did(&window.state, "probe"), "go");
}

#[test]
fn an_action_that_runs_everywhere_runs_outside_its_page_but_not_in_a_text_field() {
    let (_scratch, mut window) = probe();
    // The transport has the keys: only what runs everywhere runs.
    focus(&mut window, "play");
    keys(&mut window, "g m");
    assert_eq!(did(&window.state, "probe"), "mark");
    focus(&mut window, "field");
    keys(&mut window, "m g");
    assert_eq!(did(&window.state, "probe"), "mark");
}

#[test]
fn an_agent_runs_a_tool_action_by_name_and_reads_the_actions_and_their_keys() {
    let (_scratch, mut window) = probe();
    act(&mut window.state, json!({ "action": "probe.go" }));
    assert_eq!(did(&window.state, "probe"), "go");
    let wrong = try_act(&mut window.state, json!({ "action": "probe.went" })).unwrap_err();
    assert!(wrong.to_string().contains("it has: go, chosen"), "{wrong}");
    let report = window.state.report();
    let go = (report["actions"].as_array().unwrap().iter())
        .find(|action| action["action"] == "probe.go")
        .cloned();
    assert_eq!(
        go,
        Some(json!({ "action": "probe.go", "title": "Go", "keys": ["g"] }))
    );
}

/// A rig whose page shows the card of its pad. Both have an action on `p`, the rig one on `g`.
/// A press selects the pad and a span, or a child it does not have.
const RIG: &str = r#"import { h, input, text, tool } from "./sdk";

type Doing = { update: (label: string, change: (part: { state: { did: string } }) => void) => void };
const did = (what: string) => ({ update }: Doing) => update(what, (part) => void (part.state.did = `${part.state.did} ${what}`.trim()));

export const pad = tool({
  name: "pad", title: "Pad", when: "Never", doc: "A card with a button and an action.",
  state: { did: text() }, sound: () => input,
  actions: { hit: { title: "Hit", keys: ["p"], run: did("hit") } },
  view: () => <box key="pad" w={120} h={20} label="Pad" onPress={() => {}} />,
});

export const rig = tool({
  name: "rig", title: "Rig", when: "Never", doc: "Shows the card of its pad.",
  state: { did: text() }, sound: () => input,
  behaviour: () => ({ connections: [] }),
  actions: {
    go: { title: "Go", keys: ["g"], run: did("go") },
    pass: { title: "Pass", keys: ["p"], run: did("pass") },
  },
  view: (_, { select, addOffer, catalog }) => (
    <col>
      <child path="pad" />
      <box key="select" w={120} h={20} label="Select the pad" onPress={() => select("pad", { span: { from: 0, to: 3840 } })} />
      <box key="stray" w={120} h={20} label="Select a stranger" onPress={() => select("stranger")} />
      <box key="eq" w={120} h={20} label={`Add ${catalog.find((offer) => offer.key === "eq")?.name} of ${catalog.find((offer) => offer.key === "eq")?.group}`} onPress={() => addOffer("eq", "fx", "Add an EQ")} />
      <box key="nothing" w={120} h={20} label="Add nothing" onPress={() => addOffer("nothing", "fx", "Add nothing")} />
    </col>
  ),
});
"#;

fn rig() -> (tempfile::TempDir, rune::Runtime<State>) {
    let files = [
        ("extensions/rig.tsx", RIG),
        ("state/rig/instance.json", r#"{"tool": "rig", "state": {}}"#),
        ("state/rig/pad.json", r#"{"tool": "pad", "state": {}}"#),
    ];
    page(&files, "rig")
}

/// A page whose action on `m`, and a button a memo kept from its first run, write down the
/// seconds of its region: a fact no record holds.
const MEASURE: &str = r#"import { h, input, memo, text, tool, type Context } from "./sdk";
type Measuring = Pick<Context<{ did: string }>, "children" | "update">;
const measured = (ctx: Measuring) => {
  const region = ctx.children.find((child) => child.tool === "region") as { facts?: { seconds?: number } } | undefined;
  ctx.update("Measure", (part) => void (part.state.did = `${part.state.did} ${region?.facts?.seconds}`.trim()));
};
export const measure = tool({
  name: "measure", title: "Measure", when: "Never", doc: "Says how long its region is.",
  state: { did: text() }, sound: () => input,
  behaviour: () => ({ connections: [] }),
  actions: { measure: { title: "Measure", keys: ["m"], run: measured } },
  view: (_, ctx) => memo("kept", [], () => <box key="measure" w={120} h={20} label={`Measure ${ctx.children.length}`} onPress={() => measured(ctx)} />),
});
"#;

/// The facts of children are the same in a key action and a kept handler as in a view: there,
/// and the newest, though no record changed since the handler's run read them.
#[test]
fn a_key_action_and_a_kept_handler_read_the_newest_facts_of_a_child() {
    let files = [
        ("extensions/measure.tsx", MEASURE),
        (
            "state/measure/instance.json",
            r#"{"tool": "measure", "state": {}}"#,
        ),
        (
            "state/measure/take.json",
            r#"{"tool": "region", "state": {"asset": "take.wav"}}"#,
        ),
    ];
    let (_scratch, mut window) = page(&files, "measure");
    let audio = window.state.project().root().join("assets/audio");
    std::fs::create_dir_all(&audio).unwrap();
    wav(&audio.join("take.wav"));
    let arrived = [audio.join("take.wav")];
    (window.state.session)
        .background(|project| project.apply_outside_changes(&arrived))
        .unwrap();
    for _ in 0..120 {
        redraw(&mut window);
        window.state.settle();
    }
    act(
        &mut window.state,
        json!({ "key": "measure", "action": "press" }),
    );
    keys(&mut window, "m");
    // 4800 samples at 48 kHz, each time.
    let did = did(&window.state, "measure");
    let seconds: Vec<f64> = did.split(' ').map(|each| each.parse().unwrap()).collect();
    assert!(
        seconds.len() == 2 && seconds.iter().all(|each| (each - 0.1).abs() < 1e-6),
        "{did}"
    );
    assert_eq!(problems(&window.state), Vec::<String>::new());
}

#[test]
fn a_key_a_card_has_no_action_for_reaches_the_actions_of_the_page_around_it() {
    let (_scratch, mut window) = rig();
    focus(&mut window, "pad:pad");
    keys(&mut window, "p g");
    assert_eq!(did(&window.state, "rig"), "go");
    assert_eq!(did(&window.state, "rig/pad"), "hit");
    assert_eq!(problems(&window.state), Vec::<String>::new());
}

#[test]
fn code_selects_a_child_and_a_span_and_a_child_it_does_not_own_is_a_problem() {
    let (_scratch, mut window) = rig();
    act(
        &mut window.state,
        json!({ "label": "Select the pad", "action": "press" }),
    );
    let report = window.state.report();
    assert_eq!(
        report["selected"],
        json!({ "instance": "rig/pad", "span": [0, 3840] })
    );
    act(
        &mut window.state,
        json!({ "label": "Select a stranger", "action": "press" }),
    );
    let problems = problems(&window.state);
    assert!(
        problems.iter().any(|problem| problem.contains("rig.tsx")
            && problem.contains("it selects \"stranger\", which rig does not own")),
        "{problems:?}"
    );
    // What was selected stays.
    assert_eq!(window.state.report()["selected"]["instance"], "rig/pad");
}

#[test]
fn an_offer_of_the_catalog_joins_as_one_step_and_one_not_offered_is_a_problem() {
    let (_scratch, mut window) = rig();
    act(
        &mut window.state,
        json!({ "label": "Add EQ of Tone", "action": "press" }),
    );
    let fx = InstanceId::new("rig/fx").unwrap();
    assert_eq!(window.state.project().tool_of(&fx), Some("eq"));
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(window.state.project().tool_of(&fx), None);
    act(
        &mut window.state,
        json!({ "label": "Add nothing", "action": "press" }),
    );
    let problems = problems(&window.state);
    assert!(
        problems.iter().any(|problem| problem.contains(
            "its addOffer \"Add nothing\" was refused: nothing is offered as \"nothing\""
        )),
        "{problems:?}"
    );
}

/// A press that asks a question and keeps the answer in its memory, which only a view that runs
/// again after the answer shows; and one that asks for a file.
const ASKER: &str = r#"import { h, input, text, tool } from "./sdk";
export const asker = tool({
  name: "asker", title: "Asker", when: "Never", doc: "Asks.", state: { file: text() },
  memory: () => ({ said: "nothing" }),
  sound: () => input,
  view: (self, { memory, confirm, openFile, update }) => (
    <col>
      <box key="ask" w={120} h={20} label={`Said ${memory.said}`} onPress={async () => {
        const yes = await confirm({ message: "Delete the track?", detail: "Its clips go with it.", confirm: "Delete" });
        memory.said = yes ? "yes" : "no";
      }} />
      <box key="open" w={120} h={20} label={`File ${self.state.file || "none"}`} onPress={async () => {
        const file = await openFile({ title: "Choose a sound" });
        update("Choose a sound", (part) => void (part.state.file = file ?? "none"));
      }} />
    </col>
  ),
});
"#;

fn asker() -> (tempfile::TempDir, rune::Runtime<State>) {
    let (scratch, folder) = with_tools(&[("asker", ASKER)]);
    (scratch, laid_out(opened(&folder)))
}

#[test]
fn a_question_of_code_is_the_windows_dialog_and_its_answer_runs_the_view_again() {
    let (_scratch, mut window) = asker();
    act(
        &mut window.state,
        json!({ "label": "Said nothing", "action": "press" }),
    );
    let report = window.state.report();
    assert_eq!(
        report["dialog"],
        json!({ "message": "Delete the track?", "detail": "Its clips go with it.", "confirm": "Delete" })
    );
    // Cancel first, the yes button last; an agent presses them by label.
    let buttons: Vec<String> = labels(&window.state)
        .into_iter()
        .filter(|label| label == "Cancel" || label == "Delete")
        .collect();
    assert_eq!(buttons, ["Cancel", "Delete"]);
    act(
        &mut window.state,
        json!({ "label": "Delete", "action": "press" }),
    );
    assert!(
        labels(&window.state).contains(&"Said yes".to_string()),
        "{:?}",
        labels(&window.state)
    );
    assert_eq!(window.state.report()["dialog"], Value::Null);
    // Escape is Cancel, and Enter the yes button.
    act(
        &mut window.state,
        json!({ "label": "Said yes", "action": "press" }),
    );
    redraw(&mut window);
    keys(&mut window, "escape");
    assert!(
        labels(&window.state).contains(&"Said no".to_string()),
        "{:?}",
        labels(&window.state)
    );
    act(
        &mut window.state,
        json!({ "label": "Said no", "action": "press" }),
    );
    redraw(&mut window);
    keys(&mut window, "enter");
    assert!(
        labels(&window.state).contains(&"Said yes".to_string()),
        "{:?}",
        labels(&window.state)
    );
}

/// The answer to a question of code that started again meanwhile reaches no question of the
/// new code, also one with the same number: the composer is told to do it again.
#[test]
fn an_answer_for_code_that_started_again_reaches_no_other_question() {
    let (_scratch, mut window) = asker();
    act(
        &mut window.state,
        json!({ "label": "Said nothing", "action": "press" }),
    );
    let loads = window.state.tools.as_ref().unwrap().loads();
    window.state.tools.as_mut().unwrap().code_changed();
    while window.state.tools.as_ref().unwrap().loads() == loads {
        polled(&mut window);
    }
    window.state.settle();
    let asker = InstanceId::new("song/t00/chain/asker").unwrap();
    (window.state.tools.as_mut().unwrap())
        .act(&asker, "ask", "press", None)
        .unwrap();
    window.state.settle();
    assert_eq!(window.state.waiting.asked.len(), 2);
    // The first dialog is the question of the code before.
    act(
        &mut window.state,
        json!({ "label": "Delete", "action": "press" }),
    );
    window.state.settle();
    assert!(
        labels(&window.state).contains(&"Said nothing".to_string()),
        "{:?}",
        labels(&window.state)
    );
    assert_eq!(
        window.state.session.notice(),
        Some(sound_typescript::window::ASKED_BEFORE)
    );
    act(
        &mut window.state,
        json!({ "label": "Delete", "action": "press" }),
    );
    assert!(labels(&window.state).contains(&"Said yes".to_string()));
}

/// A short WAV file of silence, 16-bit mono at 48 kHz.
pub(super) fn wav(path: &Path) {
    let samples = 4800_u32;
    let data = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(48_000_u32.to_le_bytes());
    bytes.extend(96_000_u32.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0_u8, data as usize));
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_file_code_asks_for_is_the_systems_dialog_and_lands_in_the_assets() {
    let (scratch, mut window) = asker();
    act(
        &mut window.state,
        json!({ "label": "File none", "action": "press" }),
    );
    polled(&mut window);
    let (kind, options) = window.file_dialog().expect("a file dialog");
    assert_eq!(
        (kind, options.title.as_str()),
        (FileDialogKind::Open, "Choose a sound")
    );
    let chosen = scratch.path().join("kick.wav");
    wav(&chosen);
    window.resolve_file_dialog(Some(chosen)).unwrap();
    polled(&mut window);
    redraw(&mut window);
    let file = record_of(&window.state, "song/t00/chain/asker")["file"].clone();
    let file = file.as_str().unwrap_or_default().to_string();
    assert!(file.starts_with("kick"), "{file}");
    assert!(
        window
            .state
            .project()
            .root()
            .join("assets/audio")
            .join(&file)
            .is_file()
    );
    // None chosen: code hears none.
    act(
        &mut window.state,
        json!({ "key": "t00/chain/asker:open", "action": "press" }),
    );
    polled(&mut window);
    window.resolve_file_dialog(None).unwrap();
    polled(&mut window);
    redraw(&mut window);
    assert_eq!(
        record_of(&window.state, "song/t00/chain/asker")["file"],
        "none"
    );
}

/// A press that changes the tempo and its record as one edit, one that the project refuses, and
/// the clock's switches; its labels show the tempo and the click as code reads them.
const CLOCKED: &str = r#"import { h, input, number, tool } from "./sdk";
export const clocked = tool({
  name: "clocked", title: "Clocked", when: "Never", doc: "Moves the clock.", state: { times: number({ min: 0, max: 10, integer: true, default: 0 }) },
  sound: () => input,
  view: (self, { clock, update }) => (
    <col>
      <box key="faster" w={120} h={20} label={`Tempo ${clock.tempo_map.tempo_changes[0].bpm}`} onPress={() => {
        clock.update("Faster", (song) => void (song.tempo_map.tempo_changes[0].bpm = 90));
        update("Faster", (part) => void (part.state.times = self.state.times + 1));
      }} />
      <box key="wrong" w={120} h={20} label="Wrong tempo" onPress={() => clock.update("Wrong", (song) => void (song.tempo_map.tempo_changes[0].bpm = -5))} />
      <box key="click" w={120} h={20} label={`Click ${clock.click ? "on" : "off"}`} onPress={() => clock.setClick(!clock.click)} />
      <box key="switches" w={120} h={20} label="Switches" onPress={() => {
        clock.setComputerKeys(true);
        clock.setWriteAutomation(true);
        clock.setRepeat({ from: 0, to: 3840 });
      }} />
      <box key="no-repeat" w={120} h={20} label="No repeat" onPress={() => clock.setRepeat(null)} />
    </col>
  ),
});
"#;

#[test]
fn code_edits_the_tempo_as_one_step_with_its_record_and_hears_the_song_as_it_is() {
    let (_scratch, folder) = with_tools(&[("clocked", CLOCKED)]);
    let mut state = opened(&folder);
    act(
        &mut state,
        json!({ "label": "Tempo 120", "action": "press" }),
    );
    let bpm = |state: &State| {
        state
            .project()
            .project_file()
            .tempo_map
            .change_at(Ticks(0))
            .bpm
            .bpm()
    };
    assert_eq!(bpm(&state), 90.0);
    assert_eq!(record_of(&state, "song/t00/chain/clocked")["times"], 1);
    act(&mut state, json!({ "action": "undo" }));
    assert_eq!(bpm(&state), 120.0);
    assert_eq!(
        record_of(&state, "song/t00/chain/clocked").get("times"),
        None
    );
    // The view reads the song as it is after the undo, not its own edit.
    frame(&mut state, 0.1);
    state.settle();
    assert!(
        labels(&state).contains(&"Tempo 120".to_string()),
        "{:?}",
        labels(&state)
    );
    act(
        &mut state,
        json!({ "label": "Wrong tempo", "action": "press" }),
    );
    assert_eq!(bpm(&state), 120.0);
    let problems = problems(&state);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("clocked.tsx")
                && problem.contains("its clock.update \"Wrong\" was refused")),
        "{problems:?}"
    );
}

#[test]
fn the_switches_of_the_clock_follow_code_and_code_hears_them_but_never_the_playhead_alone() {
    let (_scratch, folder) = with_tools(&[("clocked", CLOCKED)]);
    let mut state = opened(&folder);
    act(
        &mut state,
        json!({ "label": "Click off", "action": "press" }),
    );
    frame(&mut state, 0.1);
    state.settle();
    assert!(
        labels(&state).contains(&"Click on".to_string()),
        "{:?}",
        labels(&state)
    );
    act(
        &mut state,
        json!({ "label": "Switches", "action": "press" }),
    );
    let report = state.report();
    assert_eq!(
        [
            &report["click"],
            &report["computer_keys"],
            &report["write_automation"],
            &report["repeat"]
        ],
        [&json!(true), &json!(true), &json!(true), &json!([0, 3840])]
    );
    act(
        &mut state,
        json!({ "label": "No repeat", "action": "press" }),
    );
    assert_eq!(state.report()["repeat"], Value::Null);
    // The transport's switches are the clock's: code's show in `--tree`, the composer's reach code.
    let checked = |state: &State, label: &str| {
        let controls = state.report()["controls"].clone();
        let found = (controls.as_array().unwrap().iter())
            .find(|control| control["label"] == label)
            .cloned();
        found.unwrap_or_else(|| panic!("no {label} in {controls}"))["checked"].clone()
    };
    assert_eq!(checked(&state, "Computer keys"), json!(true));
    assert_eq!(checked(&state, "Click"), json!(true));
    act(&mut state, json!({ "label": "Click", "action": "toggle" }));
    frame(&mut state, 0.15);
    state.settle();
    assert!(
        labels(&state).contains(&"Click off".to_string()),
        "{:?}",
        labels(&state)
    );
    assert_eq!(
        state.session.transport().looped.range,
        Ticks(0)..Ticks(3840)
    );
    assert!(problems(&state).is_empty(), "{:?}", problems(&state));
    // Playing moves the playhead: the views that read the click do not run for it.
    let runs = state.tools.as_ref().unwrap().view_runs();
    state.session.background(|project| project.engine().play());
    frame(&mut state, 0.2);
    state.settle();
    let started = state.tools.as_ref().unwrap().view_runs();
    for step in 3..=20 {
        frame(&mut state, f64::from(step) * 0.1);
        state.settle();
    }
    assert!(state.playhead.0 > 0);
    assert_eq!(state.tools.as_ref().unwrap().view_runs(), started);
    assert!(started >= runs);
}

/// A page whose `onKey` writes down each key going down and up.
const HOLDER: &str = r#"import { h, input, text, tool } from "./sdk";
export const holder = tool({
  name: "holder", title: "Holder", when: "Never", doc: "Holds keys.", state: { did: text() },
  sound: () => input,
  keys: ["a", "b", "cmd-c"],
  onKey: ({ update }, key) => update("Key", (part) => void (part.state.did = `${part.state.did} ${key.key}${key.down ? "↓" : "↑"}`.trim())),
  view: () => <box key="here" w={120} h={20} label="Here" onPress={() => {}} />,
});
"#;

#[test]
fn a_key_a_tool_took_comes_up_to_it_and_cmd_or_losing_the_window_lets_go_of_it() {
    let files = [
        ("extensions/holder.tsx", HOLDER),
        ("state/holder.json", r#"{"tool": "holder", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "holder");
    window.key(event("a"));
    redraw(&mut window);
    // A repeat of a held key is nothing new.
    window.repeat_key(event("a"));
    redraw(&mut window);
    window.key_up(event("a"));
    redraw(&mut window);
    assert_eq!(did(&window.state, "holder"), "a↓ a↑");
    // A key it does not take is the window's: `d` is nothing here.
    keys(&mut window, "d");
    assert_eq!(did(&window.state, "holder"), "a↓ a↑");
    // Cmd going down lets go on macOS, which sends no key up under it; elsewhere the up comes.
    // A key with cmd only goes down.
    window.key(event("a"));
    redraw(&mut window);
    window.key(event("cmd-c"));
    redraw(&mut window);
    let mac = cfg!(target_os = "macos");
    if !mac {
        window.key_up(event("a"));
        redraw(&mut window);
    }
    let let_go = match mac {
        true => "a↓ a↑ a↓ a↑ cmd-c↓",
        false => "a↓ a↑ a↓ cmd-c↓ a↑",
    };
    assert_eq!(did(&window.state, "holder"), let_go);
    window.key(event("b"));
    redraw(&mut window);
    window.update(|state, tx| on_focus(state, false, tx));
    redraw(&mut window);
    assert_eq!(did(&window.state, "holder"), format!("{let_go} b↓ b↑"));
    // Its up went already: the system's late one is nobody's.
    window.update(|state, tx| {
        assert!(!on_key_up(state, &event("b"), tx));
    });
}

/// The template's synth with a card of its own, of `body`.
fn with_synth_card(body: &str) -> (tempfile::TempDir, State) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let cards = folder.join("extensions/ui/blocks/cards");
    std::fs::create_dir_all(&cards).unwrap();
    let card = format!(
        "import {{ h, type Card }} from \"../../../sdk\";\nexport const card: Card = () => {body};\n"
    );
    std::fs::write(cards.join("instrument.synth.tsx"), card).unwrap();
    let state = opened(&folder);
    (scratch, state)
}

#[test]
fn a_problem_of_a_card_of_a_tool_names_its_file() {
    let (_scratch, mut state) = with_synth_card(r#"{ throw new Error("no room") }"#);
    frame(&mut state, 0.1);
    state.settle();
    let problems = problems(&state);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("cards/instrument.synth.tsx")
                && problem.contains("no room")),
        "{problems:?}"
    );
    assert!(
        !problems
            .iter()
            .any(|problem| problem.contains("generic-card.tsx")),
        "{problems:?}"
    );
}

#[test]
fn a_card_of_a_tool_that_hangs_is_left_out_next_start_and_the_generic_card_draws() {
    let (_scratch, mut state) = with_synth_card(r#"{ while (true) {} }"#);
    for step in 1..=8 {
        frame(&mut state, f64::from(step) * 0.5);
        state.settle();
    }
    let problems = problems(&state);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("cards/instrument.synth.tsx")
                && problem.contains("show the generic card")),
        "{problems:?}"
    );
    // The generic card draws the synth now, and the page goes on.
    let synth = InstanceId::new("song/t00/synth").unwrap();
    assert!(matches!(state.drawn(&synth), Some(Drawn::View { .. })));
    control(&controls::collect(&state).controls, "Mute Track 2");
}

/// A press that exports the notes of its timeline child, and keeps whether they were written.
const EXPORTER: &str = r#"import { h, input, tool } from "./sdk";
export const exporter = tool({
  name: "exporter", title: "Exporter", when: "Never", doc: "Exports its clip.", state: {},
  memory: () => ({ written: "nothing" }),
  sound: () => input,
  behaviour: () => ({ connections: [] }),
  view: (_, { memory, exportNotes }) => (
    <box key="export" w={120} h={20} label={`Written ${memory.written}`} onPress={async () => {
      memory.written = String(await exportNotes("clip", { name: "Bass" }));
    }} />
  ),
});
"#;

#[test]
fn notes_code_exports_are_written_where_the_composer_says() {
    let clip = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../templates/daw/state/song/t00/bass.json"
    ))
    .unwrap();
    let files = [
        ("extensions/exporter.tsx", EXPORTER),
        (
            "state/exporter/instance.json",
            r#"{"tool": "exporter", "state": {}}"#,
        ),
        ("state/exporter/clip.json", clip.as_str()),
    ];
    let (scratch, mut window) = page(&files, "exporter");
    act(
        &mut window.state,
        json!({ "label": "Written nothing", "action": "press" }),
    );
    polled(&mut window);
    let (kind, options) = window.file_dialog().expect("a save dialog");
    assert_eq!(
        (kind, options.file_name.as_deref()),
        (FileDialogKind::Save, Some("Bass.mid"))
    );
    let file = scratch.path().join("Bass.mid");
    window.resolve_file_dialog(Some(file.clone())).unwrap();
    polled(&mut window);
    redraw(&mut window);
    assert!(std::fs::read(&file).unwrap().starts_with(b"MThd"));
    assert!(
        labels(&window.state).contains(&"Written true".to_string()),
        "{:?}",
        labels(&window.state)
    );
}

/// Counts what its code hears, notes and keys, in its memory; a press on its page keeps the
/// count in its record.
const EAR: &str = r#"import { h, note, number, sine, tool } from "./sdk";
export const ear = tool({
  name: "ear", title: "Ear", when: "Never", doc: "Counts the notes and keys it hears.",
  voices: 4, sound: () => sine(note.freq).times(note.gate).times(0.1),
  state: { heard: number({ min: 0, max: 100, default: 0 }) },
  memory: () => ({ heard: 0 }),
  keys: ["b"],
  onNotes: ({ memory }, event) => { if (event.type === "noteOn") memory.heard += 1; },
  onKey: ({ memory }, key) => { if (key.down) memory.heard += 1; },
  view: (_, { memory, update }) => <box key="keep" w={120} h={20} label="Keep" onPress={() => update("Keep the count", (part) => void (part.state.heard = memory.heard))} />,
});
"#;

/// The page draws again after its code hears a note or a key, and a press on it still runs.
#[test]
fn a_page_still_takes_presses_after_its_code_hears_a_note_and_a_key() {
    let files = [
        ("extensions/ear.tsx", EAR),
        ("state/ear.json", r#"{"tool": "ear", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "ear");
    let heard = |window: &rune::Runtime<State>| record_of(&window.state, "ear")["heard"].clone();
    (window.state.session).select(Some(InstanceId::new("ear").unwrap()), None);
    keys(&mut window, "cmd-k a");
    for _ in 0..5 {
        redraw(&mut window);
    }
    act(
        &mut window.state,
        json!({ "label": "Keep", "action": "press" }),
    );
    redraw(&mut window);
    assert_eq!(heard(&window), 1);
    keys(&mut window, "b");
    act(
        &mut window.state,
        json!({ "label": "Keep", "action": "press" }),
    );
    redraw(&mut window);
    assert_eq!(heard(&window), 2);
}

/// The agent adds the tool the window shows, and its page opens: the focus stays where the
/// composer had it.
#[test]
fn a_page_that_opens_leaves_the_focus_where_it_is() {
    let (_scratch, mut window) = page(&[("extensions/ear.tsx", EAR)], "ear");
    assert!(window.find("keep").is_none());
    focus(&mut window, "play");
    let root = window.state.project().root().to_path_buf();
    let record = root.join("state/ear.json");
    std::fs::write(&record, r#"{"tool": "ear", "state": {}}"#).unwrap();
    (window.state.session).background(|project| project.apply_outside_changes(&[record]).unwrap());
    for _ in 0..5 {
        polled(&mut window);
        redraw(&mut window);
    }
    assert!(window.find("keep").is_some(), "the page did not open");
    assert_eq!(focused(&window).as_deref(), Some("play"));
}

/// A page that fills the room it gets.
const ROOM: &str = r#"import { h, tool } from "./sdk";
export const room = tool({
  name: "room", title: "Room", when: "Never", doc: "Fills its room.", state: {},
  view: () => <box key="room" label="Room" w="full" h={40} />,
});
"#;

/// A resize of the window reaches the page, with no view run.
#[test]
fn a_page_follows_its_room_when_the_window_resizes() {
    let files = [
        ("extensions/room.tsx", ROOM),
        ("state/room.json", r#"{"tool": "room", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "room");
    let width = |window: &rune::Runtime<State>| window.presented("room").unwrap().0[2];
    let (before, runs) = (
        width(&window),
        window.state.tools.as_ref().unwrap().view_runs(),
    );
    window.set_viewport((900.0, 700.0), 1.0);
    for _ in 0..60 {
        redraw(&mut window);
    }
    assert!(
        width(&window) < before - 400.0,
        "{before} {}",
        width(&window)
    );
    assert_eq!(window.state.tools.as_ref().unwrap().view_runs(), runs);
}

/// Two devices of one tool have the same keys inside their views: each element keeps its own
/// focus and its own press, by the instance its key is scoped to.
#[test]
fn two_devices_with_the_same_inner_keys_keep_a_focus_and_a_press_each() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    std::fs::write(folder.join("extensions/steps.tsx"), super::STEPS).unwrap();
    let chain = folder.join("state/song/t00/chain");
    let order = json!({ "tool": "chain", "state": { "order": ["a", "b"] } });
    std::fs::write(chain.join("instance.json"), order.to_string()).unwrap();
    for name in ["a", "b"] {
        std::fs::write(
            chain.join(format!("{name}.json")),
            r#"{"tool": "steps", "state": {}}"#,
        )
        .unwrap();
    }
    let mut window = laid_out(opened(&folder));
    let on = |window: &rune::Runtime<State>, name: &str| {
        record_of(&window.state, &format!("song/t00/chain/{name}"))["on"][2].clone()
    };
    act(
        &mut window.state,
        json!({ "key": "t00/chain/b:step-2", "action": "toggle" }),
    );
    redraw(&mut window);
    assert_eq!(
        (on(&window, "a"), on(&window, "b")),
        (Value::Null, json!(true))
    );
    focus(&mut window, "t00/chain/a:step-2");
    // Moved onto by the keys, as Tab does: Space turns a step only then.
    window.update(|_, tx| tx.focus("t00/chain/b:step-2", true));
    redraw(&mut window);
    keys(&mut window, "space");
    assert_eq!(
        (on(&window, "a"), on(&window, "b")),
        (Value::Null, json!(false))
    );
}

/// A kit of two rows of steps, whose presses turn them in its record.
const KIT: &str = r#"import { boolean, h, list, tool } from "./sdk";
import { StepGrid } from "./ui/blocks/steps";
export const kit = tool({
  name: "kit", title: "Kit", when: "Never", doc: "Rows of steps.", state: { on: list(boolean({ default: false }), { max: 8 }) },
  view: (self, { update }) => {
    const on = (r: number, i: number) => self.state.on[r * 4 + i] ?? false;
    const rows = ["Kick", "Snare"].map((name, r) => ({ name, steps: [0, 1, 2, 3].map((i) => ({ on: on(r, i), velocity: 0.5 })) }));
    return <StepGrid id="grid" label="Steps" rows={rows} onToggle={(r, i) => update("Turn a step", (part) => void (part.state.on = Array.from({ length: 8 }, (_, j) => (j === r * 4 + i ? !on(r, i) : (self.state.on[j] ?? false)))))} onVelocity={() => {}} />;
  },
});
"#;

/// The arrows move along the steps of a grid, and space or enter turns the one they are on.
#[test]
fn the_arrows_move_along_steps_and_space_or_enter_turns_the_one_they_are_on() {
    let files = [
        ("extensions/kit.tsx", KIT),
        ("state/kit.json", r#"{"tool": "kit", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "kit");
    focus(&mut window, "grid-0-0");
    window.key(KeyEvent {
        key: Key::Right,
        mods: Mods::default(),
    });
    redraw(&mut window);
    window.key(KeyEvent {
        key: Key::Down,
        mods: Mods::default(),
    });
    redraw(&mut window);
    assert_eq!(focused(&window).as_deref(), Some("grid-1-1"));
    keys(&mut window, "space");
    assert_eq!(record_of(&window.state, "kit")["on"][5], true);
    keys(&mut window, "enter");
    assert_eq!(record_of(&window.state, "kit")["on"][5], false);
}

/// Counts the left and the right presses of its box, and its drags.
const PRESSES: &str = r#"import { h, number, tool } from "./sdk";
export const presses = tool({
  name: "presses", title: "Presses", when: "Never", doc: "Counts presses.",
  state: { count: number({ min: 0, max: 100, default: 0 }), rights: number({ min: 0, max: 100, default: 0 }), dragged: number({ min: 0, max: 100, default: 0 }) },
  view: (self, { update }) => (
    <col>
      <box key="tap" label="Tap" w={120} h={40} onPress={(p) => update("Count", (part) => void (p.right ? (part.state.rights = self.state.rights + 1) : (part.state.count = self.state.count + 1)))} />
      <box key="drag" label="Drag" w={120} h={40} drag={{ y: true }} onDragEnd={() => update("Drag", (part) => void (part.state.dragged = self.state.dragged + 1))} />
    </col>
  ),
});
"#;

/// The other buttons neither click a press the left holds nor end it, and a move with the
/// right held keeps it: the left's release is its one press; the middle is not heard while
/// the left holds a drag. A right press, and Shift-F10 on it, is its right press.
#[test]
fn the_other_buttons_neither_click_nor_end_a_press_the_left_holds() {
    use rune::PointerButton::{Middle, Secondary};
    let files = [
        ("extensions/presses.tsx", PRESSES),
        ("state/presses.json", r#"{"tool": "presses", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "presses");
    let count = |window: &rune::Runtime<State>, field: &str| {
        record_of(&window.state, "presses")[field].clone()
    };
    let (x, y) = window.find("tap").unwrap();
    window.pointer_down(x, y);
    window.button_down(x, y, Secondary);
    window.pointer_move(x + 1.0, y);
    window.button_up(x, y, Secondary);
    redraw(&mut window);
    assert_eq!(count(&window, "count"), Value::Null);
    window.pointer_up(x + 1.0, y);
    redraw(&mut window);
    assert_eq!(count(&window, "count"), 1);
    // A right press alone is no left press.
    let rights = count(&window, "rights").as_u64().unwrap_or(0);
    window.button_down(x, y, Secondary);
    window.button_up(x, y, Secondary);
    redraw(&mut window);
    assert_eq!(count(&window, "count"), 1);
    assert_eq!(count(&window, "rights"), rights + 1);
    focus(&mut window, "tap");
    keys(&mut window, "shift-f10");
    assert_eq!(count(&window, "rights"), rights + 2);

    let (x, y) = window.find("drag").unwrap();
    window.pointer_down(x, y);
    window.pointer_move(x, y - 20.0);
    window.button_down(x, y - 20.0, Middle);
    window.button_up(x, y - 20.0, Middle);
    window.pointer_up(x, y - 20.0);
    redraw(&mut window);
    assert_eq!(
        (count(&window, "count"), count(&window, "dragged")),
        (json!(1), json!(1))
    );
}

/// A key a tool took comes up to it after the focus left its view: the release follows the
/// key, so none stays held. One the system never sends is let go with the window's focus.
#[test]
fn a_key_a_tool_took_comes_up_to_it_after_the_focus_left_its_view() {
    let files = [
        ("extensions/holder.tsx", HOLDER),
        ("state/holder.json", r#"{"tool": "holder", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "holder");
    focus(&mut window, "here");
    window.key(event("b"));
    redraw(&mut window);
    assert_eq!(did(&window.state, "holder"), "b↓");
    focus(&mut window, "play");
    window.key_up(event("b"));
    redraw(&mut window);
    assert_eq!(did(&window.state, "holder"), "b↓ b↑");
}

/// Two elements of one key in a view: a press of one would reach the other.
const TWINS: &str = r#"import { h, tool } from "./sdk";
export const twins = tool({
  name: "twins", title: "Twins", when: "Never", doc: "Two boxes of one key.", state: {},
  view: () => <col><box key="same" label="First" w={20} h={20} onPress={() => {}} /><box key="same" label="Second" w={20} h={20} onPress={() => {}} /></col>,
});
"#;

/// A key used twice in a view is a problem that names the key and says the fix.
#[test]
fn a_key_used_twice_in_a_view_is_a_problem_that_says_the_fix() {
    let files = [
        ("extensions/twins.tsx", TWINS),
        ("state/twins.json", r#"{"tool": "twins", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "twins");
    redraw(&mut window);
    window.state.settle();
    let problems = problems(&window.state);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("twins.tsx") && problem.contains("\"same\"")),
        "{problems:?}"
    );
}

/// Lists that scroll, one a name of its record, each of rows of its name, with a title the code
/// may change.
fn lists(title: &str) -> String {
    format!(
        r#"import {{ h, list, text, tool }} from "./sdk";
export const lists = tool({{
  name: "lists", title: "Lists", when: "Never", doc: "Lists that scroll.", state: {{ names: list(text(), {{ max: 4 }}) }},
  view: (self) => <row>{{self.state.names.map((name) => (
    <col key={{name}} label="{title} {{name}}" w={{100}} h={{100}} scroll="y">{{Array.from({{ length: 30 }}, (_, i) => <box key={{`${{name}}-${{i}}`}} label={{`${{name}} ${{i}}`}} w={{100}} h={{20}} />)}}</col>
  ))}}</row>,
}});
"#
    )
}

/// The thumb of a list's scroll bar drags: the list scrolls as far as the thumb goes, in its
/// whole length to the bar's.
#[test]
fn a_scroll_bars_thumb_drags_the_list() {
    let code = lists("List");
    let files = [
        ("extensions/lists.tsx", code.as_str()),
        (
            "state/lists.json",
            r#"{"tool": "lists", "state": {"names": ["a"]}}"#,
        ),
    ];
    let (_scratch, mut window) = page(&files, "lists");
    let top = |window: &rune::Runtime<State>| window.presented("a-0").unwrap().0[1];
    // Shown once it scrolls a little.
    let [x, y, w, _] = window.presented("a").unwrap().0;
    window.wheel(x + 50.0, y + 50.0, -1.0);
    for _ in 0..30 {
        window.frame(1.0 / 60.0);
    }
    let before = top(&window);
    window.pointer_down(x + w - 4.0, y + 6.0);
    window.frame(1.0 / 60.0);
    for step in 1..=10 {
        window.pointer_move(x + w - 4.0, y + 6.0 + 4.0 * step as f32);
        window.frame(1.0 / 60.0);
    }
    window.pointer_up(x + w - 4.0, y + 46.0);
    for _ in 0..10 {
        window.frame(1.0 / 60.0);
    }
    // 40 points of a bar 100 long down a list 600 long.
    let moved = before - top(&window);
    assert!((moved - 240.0).abs() < 12.0, "scrolled {moved}");
}

/// A list keeps its scroll by its key: when the code is saved again, and when a list is put in
/// before it.
#[test]
fn a_list_keeps_its_scroll_by_its_key_over_a_save_and_a_list_put_in_first() {
    let code = lists("List");
    let files = [
        ("extensions/lists.tsx", code.as_str()),
        (
            "state/lists.json",
            r#"{"tool": "lists", "state": {"names": ["a", "b"]}}"#,
        ),
    ];
    let (_scratch, mut window) = page(&files, "lists");
    let top = |window: &rune::Runtime<State>, key: &str| window.presented(key).unwrap().0[1];
    let (x, y) = window.find("b").unwrap();
    let (a, b) = (top(&window, "a-0"), top(&window, "b-0"));
    window.wheel(x, y, -200.0);
    for _ in 0..60 {
        window.frame(1.0 / 60.0);
    }
    let scrolled = top(&window, "b-0");
    assert!(scrolled < b - 100.0, "{b} {scrolled}");
    assert_eq!(top(&window, "a-0"), a);

    let root = window.state.project().root().to_path_buf();
    let loads = window.state.tools.as_ref().unwrap().loads();
    std::fs::write(root.join("extensions/lists.tsx"), lists("Scrolled")).unwrap();
    window.state.tools.as_mut().unwrap().code_changed();
    for _ in 0..10 {
        polled(&mut window);
        redraw(&mut window);
    }
    assert!(
        window.state.tools.as_ref().unwrap().loads() > loads,
        "the new code"
    );
    assert_eq!((top(&window, "a-0"), top(&window, "b-0")), (a, scrolled));

    let record = root.join("state/lists.json");
    std::fs::write(
        &record,
        r#"{"tool": "lists", "state": {"names": ["c", "a", "b"]}}"#,
    )
    .unwrap();
    (window.state.session).background(|project| project.apply_outside_changes(&[record]).unwrap());
    for _ in 0..10 {
        polled(&mut window);
        redraw(&mut window);
    }
    assert_eq!((top(&window, "a-0"), top(&window, "b-0")), (a, scrolled));
}

/// A menu of two shapes that checks the one its record names; picking `Square` changes nothing.
const SHAPES: &str = r#"import { h, text, tool } from "./sdk";
export const shapes = tool({
  name: "shapes", title: "Shapes", when: "Never", doc: "A menu of shapes.", state: { shape: text({ default: "saw" }) },
  view: (self, { update }) => (
    <box key="shape" label="Shape" role="button" w={120} h={20} onPress={() => {}}>
      <col overlay={{ open: "press" }} role="menu">
        <box key="saw" label="Saw" role="menuitem" value={self.state.shape === "saw"} w={100} h={20} onPress={() => update("Saw", (part) => void (part.state.shape = "saw"))} />
        <box key="square" label="Square" role="menuitem" value={self.state.shape === "square"} w={100} h={20} onPress={() => {}} />
      </col>
    </box>
  ),
});
"#;

/// A menu checks only what its code says, whatever was picked before.
#[test]
fn a_menu_checks_only_what_its_code_says_whatever_was_picked_before() {
    let files = [
        ("extensions/shapes.tsx", SHAPES),
        ("state/shapes.json", r#"{"tool": "shapes", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "shapes");
    let checked = |window: &rune::Runtime<State>| {
        let controls = controls::collect(&window.state).controls;
        ["Saw", "Square"].map(|label| control(&controls, label).checked)
    };
    for _ in 0..2 {
        act(
            &mut window.state,
            json!({ "label": "Shape", "action": "press" }),
        );
        redraw(&mut window);
        act(
            &mut window.state,
            json!({ "label": "Square", "action": "press" }),
        );
        redraw(&mut window);
        act(
            &mut window.state,
            json!({ "label": "Shape", "action": "press" }),
        );
        redraw(&mut window);
        assert_eq!(checked(&window), [Some(true), Some(false)]);
        act(
            &mut window.state,
            json!({ "label": "Shape", "action": "press" }),
        );
        redraw(&mut window);
    }
}

/// The opacity of the marker of the step `key` that plays: the inert box inside it.
fn ring(window: &rune::Runtime<State>, key: &str) -> f32 {
    let elements = window.elements();
    let step = (elements.iter())
        .find(|element| element.key.as_deref() == Some(key))
        .unwrap();
    let path = format!("{}/", step.path);
    let marker = (elements.iter()).find(|element| element.inert && element.path.starts_with(&path));
    marker.unwrap().opacity
}

/// The step the playhead is in shows that it plays, and only while the piece plays.
#[test]
fn the_step_the_playhead_is_in_shows_it_plays_only_while_the_piece_plays() {
    let files = [
        ("extensions/kit.tsx", KIT),
        ("state/kit.json", r#"{"tool": "kit", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "kit");
    assert_eq!(
        (ring(&window, "grid-0-0"), ring(&window, "grid-0-1")),
        (0.0, 0.0)
    );
    // Into the second step: a step is a sixteenth, 240 ticks, an eighth of a second at 120 bpm.
    (window.state.session).background(|project| {
        project.engine().seek(sound_core::Ticks(300));
        project.engine().play();
    });
    for _ in 0..3 {
        polled(&mut window);
    }
    assert_eq!(
        (ring(&window, "grid-0-0"), ring(&window, "grid-0-1")),
        (0.0, 1.0)
    );
    (window.state.session).background(|project| project.engine().stop());
    for _ in 0..3 {
        polled(&mut window);
    }
    assert_eq!(ring(&window, "grid-0-1"), 0.0);
}

/// A text cut to fit shows whole while the pointer is on it; one that fits shows nothing more.
#[test]
fn a_cut_text_shows_whole_on_hover_and_a_text_that_fits_does_not() {
    const CUT: &str = r#"import { h, tool } from "./sdk";
export const cut = tool({
  name: "cut", title: "Cut", when: "Never", doc: "A text too long for its box.", state: {},
  view: () => <col><text key="long" w={40} truncate>A name far too long for forty points</text><text key="short" w={200} truncate>Short</text></col>,
});
"#;
    let files = [
        ("extensions/cut.tsx", CUT),
        ("state/cut.json", r#"{"tool": "cut", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "cut");
    let tip =
        |window: &rune::Runtime<State>| window.help_tooltip().map(|(text, _)| text.to_string());
    let (x, y) = window.find("long").unwrap();
    window.pointer_move(x, y);
    for _ in 0..60 {
        redraw(&mut window);
    }
    assert_eq!(
        tip(&window).as_deref(),
        Some("A name far too long for forty points")
    );
    let (x, y) = window.find("short").unwrap();
    window.pointer_move(x, y);
    for _ in 0..60 {
        redraw(&mut window);
    }
    assert_eq!(tip(&window), None);
}

/// A project of the DAW template, as an older release made it: `song.tsx` the project never
/// changed and `track.ts` it did, each as the older template had it.
fn older_daw(scratch: &tempfile::TempDir) -> PathBuf {
    let folder = scratch.path().join("song");
    let kept = folder.join(runtime::templates::KEPT);
    for (file, ours) in [
        ("song.tsx", "// older\n"),
        ("track.ts", "// older, then ours\n"),
    ] {
        let path = format!("extensions/{file}");
        let older = std::fs::read_to_string(folder.join(&path)).unwrap() + "// older\n";
        std::fs::write(kept.join(&path), &older).unwrap();
        let theirs = std::fs::read_to_string(folder.join(&path)).unwrap() + ours;
        std::fs::write(folder.join(&path), theirs).unwrap();
    }
    folder
}

/// A newer template: what the project never changed becomes the template's, what it changed
/// stays and waits, and "Merge with the agent" puts the merge into the agent's composer with
/// the old template beside the new.
#[test]
fn a_newer_template_updates_untouched_code_and_the_agent_merges_the_rest() {
    let scratch = new_project();
    let folder = older_daw(&scratch);
    let song = std::fs::read_to_string(folder.join("extensions/song.tsx")).unwrap();
    let track = std::fs::read_to_string(folder.join("extensions/track.ts")).unwrap();
    let mut state = opened_in_place(&folder);
    crate::sidebar::scene(&mut state, "agent-empty");
    let mut window = laid_out(state);
    redraw(&mut window);
    let daw = runtime::templates::named("DAW").unwrap();
    let newest = |path: &str| {
        (daw.files.iter())
            .find(|(each, _)| *each == path)
            .unwrap()
            .1
    };
    assert_ne!(song, newest("extensions/song.tsx"));
    let read = |path: &str| std::fs::read_to_string(folder.join(path)).unwrap();
    assert_eq!(read("extensions/song.tsx"), newest("extensions/song.tsx"));
    assert_eq!(read("extensions/track.ts"), track);
    let words = "A newer DAW template changes a file this project changed too.";
    let labels = window
        .elements()
        .into_iter()
        .filter_map(|element| element.label);
    assert!(labels.into_iter().any(|label| label == words), "no notice");

    act(
        &mut window.state,
        json!({ "label": "Merge with the agent", "action": "press" }),
    );
    redraw(&mut window);
    let composer = (chrome_controls(&window.state).into_iter())
        .find(|control| control.key == "agent-composer")
        .and_then(|control| control.value)
        .unwrap();
    let composer = composer.as_str().unwrap();
    assert!(composer.contains("- extensions/track.ts"), "{composer}");
    assert!(window.state.panel.open && window.state.panel.tab == Tab::Agent);
    let previous = folder.join(".template/previous/extensions/track.ts");
    assert!(
        std::fs::read_to_string(previous)
            .unwrap()
            .ends_with("// older\n")
    );
    assert_eq!(read("extensions/track.ts"), track);
    assert!(window.state.template_update.is_none());
}

/// Dismissed, the notice goes and the project's code stays; it comes again on the next open.
#[test]
fn a_dismissed_template_notice_leaves_the_code_as_it_is() {
    let scratch = new_project();
    let folder = older_daw(&scratch);
    let track = std::fs::read_to_string(folder.join("extensions/track.ts")).unwrap();
    let mut state = opened_in_place(&folder);
    act(
        &mut state,
        json!({ "key": "notice-template-dismiss", "action": "press" }),
    );
    assert!(state.template_update.is_none());
    drop(state);
    assert_eq!(
        std::fs::read_to_string(folder.join("extensions/track.ts")).unwrap(),
        track
    );
    assert!(opened_in_place(&folder).template_update.is_some());
}

/// A page of the GPUI app: its view imports parts the SDK no longer has.
const OLD_PAGE: &str = r#"import { h, number, Page, Knob, sine, tool } from "./sdk";
export const old = tool({
  name: "old", title: "Old", when: "Never", doc: "A tone.",
  state: { hz: number({ min: 20, max: 2000, default: 220 }) },
  sound: ({ hz }) => sine(hz).times(0.2),
  view: () => <Page><Knob field="hz" /></Page>,
});
"#;

/// Code of an older Sound Tools loads without its views: the tool plays and gets a card, and
/// the page says why in plain words.
#[test]
fn a_page_of_an_older_sound_tools_plays_and_says_its_view_is_gone() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = scratch.path().join("old");
    runtime::templates::write(&folder, runtime::templates::named("Bare").unwrap()).unwrap();
    std::fs::write(folder.join("extensions/old.tsx"), OLD_PAGE).unwrap();
    std::fs::create_dir_all(folder.join("state")).unwrap();
    std::fs::write(
        folder.join("state/old.json"),
        r#"{"tool": "old", "state": {}}"#,
    )
    .unwrap();
    let project = std::fs::read_to_string(folder.join("project.json")).unwrap();
    let project = project.replace(
        r#""connections": []"#,
        r#""main": "old", "connections": []"#,
    );
    std::fs::write(folder.join("project.json"), project).unwrap();
    let state = opened(&folder);
    let old = InstanceId::new("old").unwrap();
    assert_eq!(state.project().tool_of(&old), Some("old"));
    assert!(
        state.tools.as_ref().unwrap().knows("old"),
        "its sound loads"
    );
    let problems = state.project().problems();
    assert!(
        problems.iter().any(|problem| problem
            .message
            .ends_with(sound_typescript::window::OLD_VIEWS)),
        "{problems:?}"
    );
    let mut window = laid_out(state);
    redraw(&mut window);
    assert!(window.find("old-views").is_some());
    // The plain card: a knob of its field.
    control(&controls::collect(&window.state).controls, "Old Hz");
}

/// A transport of its own: its presses play, pause, stop and jump the clock, and its label
/// reads whether it plays.
const DECK: &str = r#"import { h, tool } from "./sdk";
export const deck = tool({
  name: "deck", title: "Deck", when: "Never", doc: "Moves the clock.", state: {},
  view: (_, { clock }) => (
    <col>
      <box key="play" label={clock.playing ? "Playing" : "Stopped"} w={120} h={20} onPress={() => (clock.playing ? clock.pause() : clock.play())} />
      <box key="stop" label="Halt" w={120} h={20} onPress={() => clock.stop()} />
      <box key="jump" label="Bar 3" w={120} h={20} onPress={() => clock.jump(7680)} />
      <box key="later" label="Later" w={120} h={20} onPress={() => clock.jump(0, { atTick: 1920 })} />
    </col>
  ),
});
"#;

/// Code plays, pauses, stops and jumps the clock, and what it shows follows.
#[test]
fn a_transport_of_code_moves_the_clock_and_its_view_reads_it() {
    let files = [
        ("extensions/deck.tsx", DECK),
        ("state/deck.json", r#"{"tool": "deck", "state": {}}"#),
    ];
    let (_scratch, mut window) = page(&files, "deck");
    let tick = |window: &rune::Runtime<State>| window.state.session.status().playhead_tick.0;
    act(
        &mut window.state,
        json!({ "label": "Stopped", "action": "press" }),
    );
    for _ in 0..30 {
        polled(&mut window);
    }
    assert!(window.state.session.status().playing);
    assert!(labels(&window.state).contains(&"Playing".to_string()));
    act(
        &mut window.state,
        json!({ "label": "Playing", "action": "press" }),
    );
    polled(&mut window);
    let paused = tick(&window);
    assert!(!window.state.session.status().playing && paused > 0);
    act(
        &mut window.state,
        json!({ "label": "Bar 3", "action": "press" }),
    );
    polled(&mut window);
    assert_eq!(tick(&window), 7680);
    act(
        &mut window.state,
        json!({ "label": "Halt", "action": "press" }),
    );
    polled(&mut window);
    assert!(!window.state.session.status().playing);
    // A jump at a tick waits for the playhead to get there.
    act(
        &mut window.state,
        json!({ "label": "Stopped", "action": "press" }),
    );
    polled(&mut window);
    act(
        &mut window.state,
        json!({ "label": "Later", "action": "press" }),
    );
    let mut ticks = vec![tick(&window)];
    while ticks.len() < 120 && ticks.windows(2).all(|pair| pair[0] <= pair[1]) {
        polled(&mut window);
        ticks.push(tick(&window));
    }
    let back = ticks.windows(2).position(|pair| pair[1] < pair[0]);
    let jumped = back.map(|at| (ticks[at], ticks[at + 1]));
    assert!(
        jumped.is_some_and(|(from, to)| from <= 1920 && to < 200),
        "{ticks:?}"
    );
}

/// A view that reads only the repeat of the clock, and counts its runs.
const REPEATS: &str = r#"import { h, input, tool } from "./sdk";
let runs = 0;
export const repeats = tool({
  name: "repeats", title: "Repeats", when: "Never", doc: "Shows the repeat.", state: {}, sound: () => input,
  view: (_, { clock }) => (
    <box key="repeat" w={120} h={20} label={`Run ${++runs}, repeat ${clock.repeat ? "on" : "off"}`} onPress={() => clock.setRepeat({ from: 0, to: 3840 })} />
  ),
});
"#;

#[test]
fn a_view_that_reads_the_repeat_runs_again_when_it_changes_not_when_the_piece_plays() {
    let (_scratch, folder) = with_tools(&[("repeats", REPEATS)]);
    let mut state = opened(&folder);
    let mut seconds = 0.0;
    let mut frames = |state: &mut State, count: usize| {
        for _ in 0..count {
            seconds += 0.1;
            frame(state, seconds);
            state.settle();
        }
    };
    frames(&mut state, 2);
    let shown = |state: &State| {
        (labels(state).into_iter())
            .find(|label| label.starts_with("Run "))
            .unwrap()
    };
    let before = shown(&state);
    (state.session).background(|project| project.engine().play());
    frames(&mut state, 3);
    (state.session).background(|project| project.engine().stop());
    frames(&mut state, 3);
    assert_eq!(shown(&state), before);
    act(&mut state, json!({ "label": before, "action": "press" }));
    frames(&mut state, 2);
    assert!(shown(&state).ends_with("repeat on"), "{}", shown(&state));
}
