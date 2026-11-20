//! Fields, icons, shapes and the keys of the views.

use super::*;

const ICONS: &str = r#"import { h, input, tool } from "./sdk";
export const icons = tool({
  name: "icons", title: "Icons", when: "Never", doc: "Icons.", state: {}, sound: () => input,
  view: () => (
    <row gap={2}>
      <icon name="plus" />
      {/* @ts-expect-error: not yet a file */}
      <icon name="sparkle" size={24} color="sound" />
    </row>
  ),
});
"#;

fn icon_problems(state: &State) -> Vec<String> {
    let problems = state.project.problems().into_iter();
    problems
        .filter(|problem| problem.path.starts_with("extensions/ui/icons/"))
        .map(|problem| format!("{}: {}", problem.path, problem.message))
        .collect()
}

#[test]
fn an_icon_without_a_file_is_a_problem_until_the_file_comes_and_the_code_loads() {
    let (_scratch, folder) = with_tools(&[("icons", ICONS)]);
    let mut state = opened(&folder);
    assert_eq!(
        icon_problems(&state),
        [
            "extensions/ui/icons/sparkle.svg: a view draws the icon \"sparkle\", which has no file here"
        ]
    );
    let icons = state.project.root().join("extensions/ui/icons");
    std::fs::copy(icons.join("plus.svg"), icons.join("sparkle.svg")).unwrap();
    state.tools.as_mut().unwrap().code_changed();
    frame(&mut state, 0.1);
    state.settle();
    frame(&mut state, 0.2);
    assert_eq!(icon_problems(&state), Vec::<String>::new());
    let names =
        std::fs::read_to_string(state.project.root().join("extensions/sdk/icons.ts")).unwrap();
    assert!(names.contains("\"sparkle\""), "{names}");
}

const GROWS: &str = r#"import { h, input, live, tool } from "./sdk";
export const grows = tool({
  name: "grows", title: "Grows", when: "Never", doc: "Grows.", state: {}, sound: () => input,
  view: () => (
    <box w={200} h={40}>
      <shapes key="grows" w={live.elapsed.times(60).min(180)} h={40} units={[4, 1]} lines={[{ color: "text", width: 1, points: [0, 1, 2, 0, 2, 0, 4, 1], fill: "text/10" }]} />
    </box>
  ),
});
"#;

#[test]
fn a_shape_with_a_live_width_grows_every_frame_and_runs_no_view() {
    let (_scratch, folder) = with_tools(&[("grows", GROWS)]);
    let mut window = laid_out(opened(&folder));
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    drawn_frame(&mut window, 0.5);
    let early = width(&window, "t00/chain/grows:grows");
    drawn_frame(&mut window, 1.5);
    let later = width(&window, "t00/chain/grows:grows");
    assert!(early > 0.0 && later > early, "{early} {later}");
    assert_eq!(window.state.tools.as_ref().unwrap().view_runs(), runs);
}

/// A name a press renames in place, a field that takes the keys as it appears.
const RENAME: &str = r#"import { h, input, number, text, tool } from "./sdk";
export const rename = tool({
  name: "rename", title: "Rename", when: "Never", doc: "Rename.", state: { name: text({ default: "Bass" }), left: number({ min: 0, max: 99, default: 0 }) }, sound: () => input,
  memory: () => ({ renaming: false }),
  view: (self, { memory, update }) => (
    <row gap={2}>
      <row key="track" label="Track" w={120} h={24} onPress={() => (memory.renaming = true)}>
        {memory.renaming
          ? <field key="rename" label="Name" value={self.state.name} focus w={120} onCommit={(name) => {
              memory.renaming = false;
              if (name === null) update("Left", (draft) => void (draft.state.left += 1));
              else update("Rename", (draft) => void (draft.state.name = name));
            }} />
          : <text>{self.state.name}</text>}
      </row>
      <box key="other" label="Other" w={24} h={24} onPress={() => {}} />
    </row>
  ),
});
"#;

fn renamer() -> (tempfile::TempDir, rune::Runtime<State>) {
    let (scratch, folder) = with_tools(&[("rename", RENAME)]);
    (scratch, laid_out(opened(&folder)))
}

fn renamed(state: &State) -> Value {
    let id = InstanceId::new("song/t00/chain/rename").unwrap();
    serde_json::from_str(&state.project.state_json(&id).unwrap()).unwrap()
}

fn name(state: &State) -> Value {
    renamed(state)["name"].clone()
}

fn type_in(window: &mut rune::Runtime<State>, text: &str) {
    for letter in text.chars() {
        key(window, rune::Key::Text(letter.to_string()));
    }
}

const TRACK: &str = "t00/chain/rename:track";
const FIELD: &str = "t00/chain/rename:rename";

#[test]
fn a_field_that_appears_after_a_press_takes_the_keys_with_its_text_chosen_and_enter_keeps_it() {
    let (_scratch, mut window) = renamer();
    click(&mut window, TRACK);
    redraw(&mut window);
    assert_eq!(focused(&window).as_deref(), Some(FIELD));
    type_in(&mut window, "Kick");
    key(&mut window, rune::Key::Enter);
    assert_eq!(name(&window.state), "Kick");
    redraw(&mut window);
    assert!(window.find(FIELD).is_none());
    assert_eq!(focused(&window).as_deref(), Some(TRACK));
}

#[test]
fn a_field_left_by_a_press_elsewhere_or_by_tab_keeps_what_it_says() {
    let (_scratch, mut window) = renamer();
    click(&mut window, TRACK);
    redraw(&mut window);
    type_in(&mut window, "Kick");
    click(&mut window, "t00/chain/rename:other");
    redraw(&mut window);
    assert_eq!(name(&window.state), "Kick");
    click(&mut window, TRACK);
    redraw(&mut window);
    type_in(&mut window, "Snare");
    key(&mut window, rune::Key::Tab);
    redraw(&mut window);
    assert_eq!(name(&window.state), "Snare");
}

#[test]
fn escape_in_a_field_says_null_changes_nothing_and_gives_the_keys_back() {
    let (_scratch, mut window) = renamer();
    click(&mut window, TRACK);
    redraw(&mut window);
    type_in(&mut window, "Kick");
    key(&mut window, rune::Key::Escape);
    redraw(&mut window);
    assert_eq!(name(&window.state), Value::Null);
    assert!(window.find(FIELD).is_none());
    assert_eq!(focused(&window).as_deref(), Some(TRACK));
    // Null once, and no text as the field went.
    assert_eq!(renamed(&window.state)["left"], 1);
}

#[test]
fn keys_typed_in_a_field_are_typing_never_a_shortcut() {
    let (_scratch, mut window) = renamer();
    click(&mut window, TRACK);
    redraw(&mut window);
    type_in(&mut window, "A");
    key(&mut window, rune::Key::Enter);
    assert_eq!(name(&window.state), "A");
    click(&mut window, TRACK);
    redraw(&mut window);
    type_in(&mut window, "B C");
    assert!(!window.state.playing);
    window.key(rune::KeyEvent {
        key: rune::Key::Text("z".into()),
        mods: rune::Mods {
            cmd: true,
            ..rune::Mods::default()
        },
    });
    redraw(&mut window);
    // Undo took back typing in the field, not the rename to A.
    assert_eq!(name(&window.state), "A");
    assert_eq!(focused(&window).as_deref(), Some(FIELD));
    key(&mut window, rune::Key::Escape);
    click(&mut window, TRACK);
    redraw(&mut window);
    type_in(&mut window, "B C");
    key(&mut window, rune::Key::Enter);
    assert_eq!(name(&window.state), "B C");
    assert!(!window.state.playing);
}

/// A list to pick a wave from, with a search line.
const WAVES: &str = r#"import { h, input, text, tool } from "./sdk";
const WAVES = ["Saw", "Square", "Sine"];
export const waves = tool({
  name: "waves", title: "Waves", when: "Never", doc: "Waves.", state: { wave: text({ default: "" }) }, sound: () => input,
  view: (self, { update }) => (
    <box key="wave" label="Wave" w={80} h={24} fill="fill">
      <col overlay={{ open: "press" }} role="listbox" group w={120} fill="raised">
        <field key="search" label="Search waves" value="" focus w={120} onCommit={() => {}} />
        {WAVES.map((wave) => <row key={wave.toLowerCase()} label={wave} role="option" w={120} h={24} onPress={() => update("Wave", (draft) => void (draft.state.wave = wave))} />)}
      </col>
    </box>
  ),
});
"#;

fn wave(state: &State) -> Value {
    let id = InstanceId::new("song/t00/chain/waves").unwrap();
    serde_json::from_str::<Value>(&state.project.state_json(&id).unwrap()).unwrap()["wave"].clone()
}

#[test]
fn a_search_line_in_a_list_shows_the_items_it_names_without_running_the_view() {
    let (_scratch, folder) = with_tools(&[("waves", WAVES)]);
    let mut window = laid_out(opened(&folder));
    let at = |name: &str| format!("t00/chain/waves:{name}");
    click(&mut window, &at("wave"));
    redraw(&mut window);
    assert_eq!(focused(&window), Some(at("search")));
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    // Rune's own frames alone, as the window draws while the composer types.
    for letter in ["S", "Q"] {
        window.key(rune::KeyEvent {
            key: rune::Key::Text(letter.into()),
            mods: rune::Mods::default(),
        });
        window.frame(1.0 / 60.0);
    }
    assert!(window.find(&at("square")).is_some());
    assert!(window.find(&at("saw")).is_none() && window.find(&at("sine")).is_none());
    assert_eq!(window.state.tools.as_ref().unwrap().view_runs(), runs);
    key(&mut window, rune::Key::Down);
    assert_eq!(focused(&window), Some(at("square")));
    assert!(window.find(&at("saw")).is_none());
    key(&mut window, rune::Key::Enter);
    assert_eq!(wave(&window.state), "Square");
    assert!(window.state.open.is_empty());

    click(&mut window, &at("wave"));
    redraw(&mut window);
    assert!(window.find(&at("saw")).is_some());
    type_in(&mut window, "zz");
    assert!(window.find(&at("wave-nothing")).is_some());
    for _ in 0..2 {
        key(&mut window, rune::Key::Backspace);
    }
    type_in(&mut window, "si");
    key(&mut window, rune::Key::Enter);
    assert_eq!(wave(&window.state), "Sine");
    assert!(window.state.open.is_empty());
}
