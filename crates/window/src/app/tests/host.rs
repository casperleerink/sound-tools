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
    assert!(shows(&window, "Go on g"), "{:?}", labels(&window.state));
    let keymap = &mut window.state.keys.keymap;
    keymap
        .set_keys("probe.go", vec!["j".into(), "g".into()])
        .unwrap();
    redraw(&mut window);
    assert!(shows(&window, "Go on j"), "{:?}", labels(&window.state));
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
        "/template/state/song/t00/bass.json"
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
