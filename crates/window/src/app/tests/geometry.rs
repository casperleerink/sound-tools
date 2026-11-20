//! The pointer, the drag and the plane, driven as a composer and an agent drive them.

use serde_json::{Value, json};
use sound_core::InstanceId;

use super::{BOARD, act, drawn_frame, key, laid_out, opened, redraw, travel, with_tools};
use crate::app::State;
use crate::controls;

/// Clips on a plane with a ruler that shares its time, a far clip, a menu and the plane's
/// left as text: every pointer they hear is written down, in its units.
const LANES: &str = r#"import { h, input, live, number, text, tool, type Pointer } from "./sdk";
const said = (p: Pointer) => `${p.x} ${p.y} ${p.dx} ${p.dy} ${p.clicks} ${p.right} ${p.over ?? ""}`;
export const lanes = tool({
  name: "lanes", title: "Lanes", when: "Never", doc: "Clips in time.", sound: () => input,
  state: { heard: text({ default: "" }), picked: number({ min: 0, max: 9, default: 0 }), wide: number({ min: 0, max: 1, default: 0 }) },
  view: (self, { update }) => {
    const hear = (p: Pointer) => update("Hear", (draft) => void (draft.state.heard = said(p)));
    return (
      <col w={400}>
        <box key="ruler" label="Ruler" w={400} h={20} plane={{ x: "song" }}>
          <box key="bar-2" label="Bar 2" place={[3840, 10]} w={4} h={10} fill="text" onPress={hear} />
        </box>
        <box key="song" label="Song" w={400} h={160} plane={{ x: 0.1, y: 1, zoom: [0.25, 8] }} drag={{ x: true, y: true, id: "band" }} onPress={hear} onDragEnd={hear}>
          {[0, 1, 2].map((i) => {
            const d = live.drag(`clip-${i}`);
            return <box key={`clip-${i}`} label={`Clip ${i + 1}`} place={[d.dx.plus(i * 3840), d.dy.plus(i * 40), 1920, 30]} fill="fill"
              drag={{ x: true, y: true, snap: [240, 40] }} onPress={hear} onDragEnd={hear} />;
          })}
          <box key="far" label="Far" place={[96000, 0, 960, 30]} onPress={hear} />
          <col overlay={{ open: "menu" }} role="menu" w={80} fill="raised">
            <box key="pick" label="Pick" w={80} h={24} onPress={() => update("Pick", (draft) => void (draft.state.picked = 1))} />
          </col>
        </box>
        <text key="left" live={live.plane("song").left} digits={0} />
      </col>
    );
  },
});
"#;

const PREFIX: &str = "t00/chain/lanes:";

fn lanes() -> (tempfile::TempDir, rune::Runtime<State>) {
    let (scratch, folder) = with_tools(&[("lanes", LANES)]);
    (scratch, laid_out(opened(&folder)))
}

fn heard(state: &State) -> String {
    let id = InstanceId::new("song/t00/chain/lanes").unwrap();
    let record: Value = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
    record["heard"].as_str().unwrap_or_default().to_string()
}

/// The drawn rect of the element `key` of the lanes: [x, y, w, h] in window points.
fn rect(window: &rune::Runtime<State>, key: &str) -> [f32; 4] {
    let key = format!("{PREFIX}{key}");
    window
        .presented(&key)
        .unwrap_or_else(|| panic!("no {key}"))
        .0
}

fn press_at(window: &mut rune::Runtime<State>, x: f32, y: f32) {
    window.pointer_down(x, y);
    window.frame(0.01);
    window.pointer_up(x, y);
    window.frame(0.01);
}

fn view_runs(window: &rune::Runtime<State>) -> u64 {
    window.state.tools.as_ref().unwrap().view_runs()
}

fn wheel(window: &mut rune::Runtime<State>, at: [f32; 2], by: [f32; 2], mods: rune::Mods) {
    let event = rune::WheelEvent::new(by[0], by[1])
        .at(at[0], at[1])
        .mods(mods);
    window.wheel_event(event);
    window.frame(1.0 / 60.0);
}

fn left(window: &rune::Runtime<State>) -> f32 {
    window
        .state
        .planes
        .view(&format!("{PREFIX}song"))
        .unwrap()
        .left
}

#[test]
fn a_press_hears_where_it_was_in_its_plane_and_a_double_click_says_two() {
    let (_scratch, mut window) = lanes();
    let [x, y, ..] = rect(&window, "clip-0");
    press_at(&mut window, x + 10.0, y + 5.0);
    redraw(&mut window);
    assert_eq!(heard(&window.state), "100 5 0 0 1 false ");
    press_at(&mut window, x + 10.0, y + 5.0);
    redraw(&mut window);
    assert_eq!(heard(&window.state), "100 5 0 0 2 false ");
    // A ruler that shares the song's time hears in the same units.
    let [x, y, w, h] = rect(&window, "bar-2");
    press_at(&mut window, x + w / 2.0, y + h / 2.0);
    redraw(&mut window);
    assert!(
        heard(&window.state).starts_with("3840 "),
        "{}",
        heard(&window.state)
    );
}

/// Drags the element `key` by `by` window points, and lets it go.
fn drag(window: &mut rune::Runtime<State>, key: &str, by: (f32, f32)) {
    let (x, y) = window.find(key).unwrap_or_else(|| panic!("no {key}"));
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    let end = travel(window, (x, y), (by.0 / 8.0, by.1 / 8.0), 8);
    window.pointer_up(end.0, end.1);
    redraw(window);
}

#[test]
fn a_drag_lands_where_the_test_helpers_land_it() {
    let (_scratch, folder) = with_tools(&[("board", BOARD)]);
    let mut window = laid_out(opened(&folder));
    let start = |window: &rune::Runtime<State>| {
        let id = InstanceId::new("song/t00/chain/board").unwrap();
        let record: Value =
            serde_json::from_str(&window.state.project().state_json(&id).unwrap()).unwrap();
        record["start"].clone()
    };
    // 13 points on a plane of a tenth of a point a unit: 130 units, the start lands on 240s.
    drag(&mut window, "t00/chain/board:clip", (13.0, 0.0));
    assert_eq!(start(&window), 720);
    window.physical_input(rune::PhysicalInput::Modifiers(rune::Mods {
        cmd: true,
        ..rune::Mods::default()
    }));
    drag(&mut window, "t00/chain/board:clip", (13.0, 0.0));
    assert_eq!(start(&window), 850);
    window.physical_input(rune::PhysicalInput::Modifiers(rune::Mods::default()));
    drag(&mut window, "t00/chain/board:clip", (-200.0, 0.0));
    assert_eq!(start(&window), 0);
}

#[test]
fn a_drag_moves_on_both_axes_snapped_and_says_what_it_ended_over() {
    let (_scratch, mut window) = lanes();
    let runs = view_runs(&window);
    let (x, y) = window.find(&format!("{PREFIX}clip-0")).unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    let end = travel(&mut window, (x, y), (48.0, 5.375), 8);
    // While it moves, the clip follows on the grid and Bun runs no view.
    let hand = window.state.lives.hand(&format!("{PREFIX}clip-0"));
    assert_eq!(hand.moved, [3840.0, 40.0]);
    assert!(hand.active);
    assert_eq!(view_runs(&window), runs);
    let moved = rect(&window, "clip-0");
    assert_eq!(moved[0] - rect(&window, "clip-1")[0], 0.0, "{moved:?}");
    window.pointer_up(end.0, end.1);
    redraw(&mut window);
    assert_eq!(heard(&window.state), "960 15 3840 40 1 false clip-1");
    // Over nothing placed, it is over the plane.
    let (x, y) = window.find(&format!("{PREFIX}clip-0")).unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    let end = travel(&mut window, (x, y), (0.0, 10.0), 10);
    window.pointer_up(end.0, end.1);
    redraw(&mut window);
    assert!(
        heard(&window.state).ends_with(" song"),
        "{}",
        heard(&window.state)
    );
}

#[test]
fn escape_puts_a_drag_back_and_tells_nobody() {
    let (_scratch, mut window) = lanes();
    let before = rect(&window, "clip-0");
    let (x, y) = window.find(&format!("{PREFIX}clip-0")).unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    travel(&mut window, (x, y), (10.0, 0.0), 10);
    key(&mut window, rune::Key::Escape);
    window.pointer_up(x + 100.0, y);
    redraw(&mut window);
    assert_eq!(heard(&window.state), "");
    assert_eq!(rect(&window, "clip-0"), before);
}

#[test]
fn a_right_press_opens_the_menu_at_the_pointer_after_its_press_and_a_pick_closes_it() {
    let (_scratch, mut window) = lanes();
    let [x, y, ..] = rect(&window, "clip-0");
    window.secondary_down(x + 20.0, y + 10.0);
    window.secondary_up(x + 20.0, y + 10.0);
    redraw(&mut window);
    assert_eq!(heard(&window.state), "200 10 0 0 1 true ");
    let song = format!("{PREFIX}song");
    assert_eq!(window.state.open, std::slice::from_ref(&song));
    redraw(&mut window);
    let [mx, my, ..] = rect(&window, "pick");
    assert!(
        (mx - x - 20.0).abs() < 1.0 && (my - y - 10.0).abs() < 1.0,
        "{mx} {my}"
    );
    let controls = controls::collect(&window.state).controls;
    let opener = controls.iter().find(|control| control.key == song).unwrap();
    assert!(opener.actions.contains(&"menu".to_string()));
    assert_eq!(opener.expanded, Some(true));
    let (px, py) = window.find(&format!("{PREFIX}pick")).unwrap();
    press_at(&mut window, px, py);
    redraw(&mut window);
    assert!(window.state.open.is_empty());
    let id = InstanceId::new("song/t00/chain/lanes").unwrap();
    let record: Value =
        serde_json::from_str(&window.state.project().state_json(&id).unwrap()).unwrap();
    assert_eq!(record["picked"], 1);
    // Escape closes it as it closes any overlay.
    window.secondary_down(x + 20.0, y + 10.0);
    window.secondary_up(x + 20.0, y + 10.0);
    redraw(&mut window);
    assert_eq!(window.state.open, [song]);
    key(&mut window, rune::Key::Escape);
    assert!(window.state.open.is_empty());
}

#[test]
fn an_agent_presses_with_a_pointer_opens_a_menu_and_picks_by_label() {
    let (_scratch, folder) = with_tools(&[("lanes", LANES)]);
    let mut state = opened(&folder);
    act(
        &mut state,
        json!({ "label": "Clip 2", "action": "press", "value": { "x": 4000, "clicks": 2 } }),
    );
    assert_eq!(heard(&state), "4000 0 0 0 2 false ");
    act(
        &mut state,
        json!({ "label": "Clip 2", "action": "press", "value": { "right": true } }),
    );
    assert_eq!(state.open, [format!("{PREFIX}song")]);
    act(&mut state, json!({ "label": "Pick", "action": "press" }));
    assert!(state.open.is_empty());
    act(&mut state, json!({ "label": "Song", "action": "menu" }));
    assert_eq!(state.open, [format!("{PREFIX}song")]);
    // A drag by an agent lands on the grid as the pointer's does.
    act(
        &mut state,
        json!({ "label": "Clip 1", "action": "drag", "value": { "dx": 130, "dy": 25 } }),
    );
    assert_eq!(heard(&state), "0 0 240 40 1 false ");
}

#[test]
fn a_wheel_scrolls_a_plane_and_its_ruler_with_no_view_run_and_no_build() {
    let (_scratch, mut window) = lanes();
    redraw(&mut window);
    let (runs, builds) = (view_runs(&window), window.state.planes.builds.get());
    let clip = rect(&window, "clip-0");
    let bar = rect(&window, "bar-2");
    let shift = rune::Mods {
        shift: true,
        ..rune::Mods::default()
    };
    wheel(
        &mut window,
        [clip[0] + 5.0, clip[1] + 5.0],
        [0.0, -50.0],
        shift,
    );
    assert_eq!(left(&window), 500.0);
    assert_eq!(rect(&window, "clip-0")[0], clip[0] - 50.0);
    assert_eq!(rect(&window, "bar-2")[0], bar[0] - 50.0);
    let ruler = window.state.planes.view(&format!("{PREFIX}ruler")).unwrap();
    assert_eq!(ruler.left, 500.0);
    assert_eq!(
        (view_runs(&window), window.state.planes.builds.get()),
        (runs, builds)
    );
    // What binds to the plane follows it.
    let texts = window.state.lives.texts();
    assert!(texts.iter().any(|(_, text)| text == "500"), "{texts:?}");
    // It stops where what is placed in it ends, a view of room after.
    for _ in 0..40 {
        wheel(
            &mut window,
            [clip[0] + 5.0, clip[1] + 5.0],
            [0.0, -500.0],
            shift,
        );
    }
    assert_eq!(left(&window), 96960.0);
    assert_eq!(view_runs(&window), runs);
}

#[test]
fn a_pinch_zooms_about_the_pointer_moving_what_is_placed_and_builds_again_in_rust_once_it_rests() {
    let (_scratch, mut window) = lanes();
    redraw(&mut window);
    let song = rect(&window, "song");
    let (runs, builds) = (view_runs(&window), window.state.planes.builds.get());
    let at = [song[0] + 200.0, song[1] + 50.0];
    let before = window.state.planes.units(&format!("{PREFIX}song"), at);
    window.magnify_event(rune::MagnifyEvent::new(1.0).at(at[0], at[1]));
    window.frame(1.0 / 60.0);
    let after = window.state.planes.units(&format!("{PREFIX}song"), at);
    assert!((before[0] - after[0]).abs() < 0.01, "{before:?} {after:?}");
    let view = window.state.planes.view(&format!("{PREFIX}song")).unwrap();
    assert_eq!(view.zoom, 2.0);
    assert_eq!(rect(&window, "clip-0")[2], 384.0);
    assert_eq!(
        window.state.planes.builds.get(),
        builds,
        "a zoom frame built"
    );
    // A press finds what is placed where it shows, not where it was built.
    let clip = rect(&window, "clip-0");
    let picked = window.element_at(clip[0] + clip[2] - 10.0, clip[1] + 5.0);
    let key = format!("{PREFIX}clip-0");
    assert_eq!(
        picked.as_ref().and_then(rune::Picked::key),
        Some(key.as_str())
    );
    for _ in 0..12 {
        let seconds = window.state.last + 1.0 / 60.0;
        drawn_frame(&mut window, seconds);
    }
    assert_eq!(rect(&window, "clip-0")[2], 384.0);
    assert_eq!(view_runs(&window), runs);
    assert!(
        window.state.planes.builds.get() > builds,
        "the rested zoom never built"
    );
    // Cmd and the wheel zoom too, within the plane's zoom.
    let cmd = rune::Mods {
        cmd: true,
        ..rune::Mods::default()
    };
    for _ in 0..20 {
        wheel(&mut window, at, [0.0, 400.0], cmd);
    }
    assert_eq!(
        window
            .state
            .planes
            .view(&format!("{PREFIX}song"))
            .unwrap()
            .zoom,
        8.0
    );
}

#[test]
fn what_is_far_is_not_built_but_an_agent_sees_it_and_shows_it() {
    let (_scratch, folder) = with_tools(&[("lanes", LANES)]);
    let mut window = laid_out(opened(&folder));
    redraw(&mut window);
    assert!(window.presented(&format!("{PREFIX}far")).is_none());
    let controls = controls::collect(&window.state).controls;
    let far = controls
        .iter()
        .find(|control| control.label == "Far")
        .unwrap();
    assert_eq!(far.place, Some(vec![96000.0, 0.0, 960.0, 30.0]));
    act(
        &mut window.state,
        json!({ "key": format!("{PREFIX}song"), "action": "view", "value": { "x": [95000, 97000] } }),
    );
    window.invalidate();
    window.frame(1.0 / 60.0);
    redraw(&mut window);
    let view = window.state.planes.view(&format!("{PREFIX}song")).unwrap();
    assert_eq!((view.left, view.zoom), (95000.0, 2.0));
    let [x, ..] = rect(&window, "far");
    assert_eq!(x - rect(&window, "song")[0], 200.0);
    let report = window.state.report();
    let planes = report["planes"].as_array().unwrap();
    let song = planes
        .iter()
        .find(|plane| plane["key"] == format!("{PREFIX}song"))
        .unwrap();
    assert_eq!(song["left"], 95000.0);
    assert_eq!(song["right"], 97000.0);
}

#[test]
fn the_keys_scroll_and_zoom_a_plane_that_has_them() {
    let (_scratch, mut window) = lanes();
    super::tab_to(&mut window, &format!("{PREFIX}song"));
    key(&mut window, rune::Key::End);
    assert_eq!(left(&window), 96960.0);
    key(&mut window, rune::Key::Home);
    assert_eq!(left(&window), 0.0);
    window.key(rune::KeyEvent {
        key: rune::Key::Text("=".into()),
        mods: rune::Mods {
            cmd: true,
            ..rune::Mods::default()
        },
    });
    redraw(&mut window);
    let view = window.state.planes.view(&format!("{PREFIX}song")).unwrap();
    assert_eq!(view.zoom, 1.25);
    key(&mut window, rune::Key::PageDown);
    let view = window.state.planes.view(&format!("{PREFIX}song")).unwrap();
    // What is placed ends at 110, and a view of room after it.
    assert_eq!(view.top, 110.0);
}

/// A frame as the app runs it, its poll with Rune's transaction.
fn polled(window: &mut rune::Runtime<State>) {
    let seconds = window.state.last + 1.0 / 60.0;
    window.update(move |state: &mut State, tx: &mut rune::Tx| {
        crate::app::poll(state, seconds, tx);
    });
    window.frame(1.0 / 60.0);
}

#[test]
fn what_has_the_keys_in_a_plane_is_scrolled_to_and_stays_built() {
    // Shown as the main view, its keys are its own: Rune's focus reads a key as a path.
    let (_scratch, folder) = with_tools(&[("lanes", LANES)]);
    let mut state = State::open(crate::app::Options {
        folder,
        headless: true,
        play: None,
        main: Some(InstanceId::new("song/t00/chain/lanes").unwrap()),
        act: None,
    })
    .unwrap();
    super::frame(&mut state, 0.0);
    let mut window = laid_out(state);
    let left = |window: &rune::Runtime<State>| window.state.planes.view("song").unwrap().left;
    let rect = |window: &rune::Runtime<State>, key: &str| window.presented(key).unwrap().0;
    let shift = rune::Mods {
        shift: true,
        ..rune::Mods::default()
    };
    let song = rect(&window, "song");
    wheel(
        &mut window,
        [song[0] + 5.0, song[1] + 5.0],
        [0.0, -500.0],
        shift,
    );
    assert_eq!(left(&window), 5000.0);
    // Clip 2 shows from its middle on: given the keys, the plane scrolls to it.
    window.update(|_: &mut State, tx: &mut rune::Tx| tx.focus("clip-1", true));
    window.frame(1.0 / 60.0);
    polled(&mut window);
    assert_eq!(super::focused(&window).as_deref(), Some("clip-1"));
    assert_eq!(left(&window), 3840.0);
    // Scrolled far from it, it is still built, so it keeps the keys.
    for _ in 0..6 {
        wheel(
            &mut window,
            [song[0] + 5.0, song[1] + 5.0],
            [0.0, -500.0],
            shift,
        );
    }
    polled(&mut window);
    assert_eq!(left(&window), 33840.0);
    assert!(window.presented("clip-0").is_none());
    assert!(window.presented("clip-1").is_some());
    assert_eq!(super::focused(&window).as_deref(), Some("clip-1"));
}

const MISPLACED: &str = r#"import { h, input, tool } from "./sdk";
export const lost = tool({
  name: "lost", title: "Lost", when: "Never", doc: "Lost.", state: {}, sound: () => input,
  view: () => <col><box key="stray" place={[0, 0, 10, 10]} /></col>,
});
"#;

#[test]
fn a_place_outside_a_plane_is_a_problem_that_names_it() {
    let (_scratch, folder) = with_tools(&[("lost", MISPLACED)]);
    let state = opened(&folder);
    let problems = state.project().problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("stray is placed outside a plane")),
        "{problems:?}"
    );
}

const RANGE: &str = r#"import { h, input, tool } from "./sdk";
export const range = tool({
  name: "range", title: "Range", when: "Never", doc: "A range.", state: {}, sound: () => input,
  view: () => (
    <row w={200} h={100}>
      <box key="bands" label="Bands" grow={1} h="full" plane={{ x: [1000, 3000], y: [-24, 24], zoom: [1, 4] }}>
        <box key="band" label="Band" place={[2000, 0]} w={10} h={10} onPress={() => {}} />
      </box>
    </row>
  ),
});
"#;

#[test]
fn a_range_shows_stretched_to_the_box_the_layout_gives_and_opens_where_it_says_once() {
    let (_scratch, folder) = with_tools(&[("range", RANGE)]);
    let mut window = laid_out(opened(&folder));
    redraw(&mut window);
    let key = "t00/chain/range:bands";
    let view = window.state.planes.view(key).unwrap();
    assert_eq!(
        (view.left, view.right, view.top, view.bottom),
        (1000.0, 3000.0, -24.0, 24.0)
    );
    let bands = window.presented(key).unwrap().0;
    let band = window.presented("t00/chain/range:band").unwrap().0;
    assert_eq!([band[0] - bands[0], band[1] - bands[1]], [95.0, 45.0]);
    window.magnify_event(rune::MagnifyEvent::new(1.0).at(bands[0], bands[1]));
    window.frame(1.0 / 60.0);
    window.state.settle();
    redraw(&mut window);
    assert_eq!(window.state.planes.view(key).unwrap().right, 2000.0);
}

/// A plane that keeps the playhead in view, and buttons whose code shows a range of it.
const FOLLOWS: &str = r#"import { h, input, live, tool } from "./sdk";
export const follows = tool({
  name: "follows", title: "Follows", when: "Never", doc: "Follows.", state: {}, sound: () => input,
  view: (_, { show }) => (
    <col w={400}>
      <box key="song" label="Song" w={400} h={100} plane={{ x: 0.1, y: 1, zoom: [0.25, 8], follow: live.playhead }}>
        <box key="far" label="Far" place={[96000, 0, 960, 30]} />
      </box>
      <box key="fit" label="Fit" w={20} h={20} onPress={() => show("song", { x: [9600, 11520] })} />
      <box key="all" label="All" w={20} h={20} onPress={() => show("song", { x: [0, 1e9] })} />
    </col>
  ),
});
"#;

const FOLLOWING: &str = "t00/chain/follows:song";

fn follows() -> (tempfile::TempDir, rune::Runtime<State>) {
    let (scratch, folder) = with_tools(&[("follows", FOLLOWS)]);
    (scratch, laid_out(opened(&folder)))
}

/// Frames of the window, a 60th of a second apart, until `seconds` after it opened.
fn until(window: &mut rune::Runtime<State>, seconds: f64) {
    while window.state.last < seconds {
        let next = window.state.last + 1.0 / 60.0;
        super::drawn_frame(window, next);
    }
}

fn shown(window: &rune::Runtime<State>) -> (f32, f32) {
    let view = window.state.planes.view(FOLLOWING).unwrap();
    (view.left, view.right)
}

fn playhead(window: &rune::Runtime<State>) -> f32 {
    window.state.playhead.0 as f32
}

/// The old rule of a DAW's follow: the view pages on as the playhead leaves it; a scroll by
/// hand leaves the view where it put it until the piece plays again.
#[test]
fn a_plane_that_follows_the_playhead_pages_on_until_a_hand_moves_it() {
    let (_scratch, mut window) = follows();
    let runs = view_runs(&window);
    let engine = |window: &mut rune::Runtime<State>, play: bool| {
        (window.state.session).background(|project| match play {
            true => project.engine().play(),
            false => project.engine().pause(),
        });
    };
    engine(&mut window, true);
    // 400 points at 0.1 a tick show 4000 ticks, 2 seconds at 120 beats a minute.
    until(&mut window, 2.5);
    let (left, right) = shown(&window);
    assert!(
        left > 3000.0 && (left..right).contains(&playhead(&window)),
        "{left} {right}"
    );
    assert_eq!(view_runs(&window), runs);
    let [x, y, w, h] = window.presented(FOLLOWING).unwrap().0;
    wheel(
        &mut window,
        [x + w / 2.0, y + h / 2.0],
        [-2000.0, 0.0],
        rune::Mods::default(),
    );
    let scrolled = shown(&window).0;
    until(&mut window, 3.5);
    assert_eq!(shown(&window).0, scrolled);
    engine(&mut window, false);
    until(&mut window, 4.0);
    engine(&mut window, true);
    until(&mut window, 4.2);
    let (left, right) = shown(&window);
    assert!(
        (left..right).contains(&playhead(&window)),
        "{left} {right} {}",
        playhead(&window)
    );
}

#[test]
fn code_shows_a_range_of_a_plane_zoomed_as_far_as_it_zooms() {
    let (_scratch, mut window) = follows();
    act(
        &mut window.state,
        json!({ "label": "Fit", "action": "press" }),
    );
    redraw(&mut window);
    let view = window.state.planes.view(FOLLOWING).unwrap();
    assert_eq!((view.left, view.right), (9600.0, 11520.0));
    act(
        &mut window.state,
        json!({ "label": "All", "action": "press" }),
    );
    redraw(&mut window);
    let view = window.state.planes.view(FOLLOWING).unwrap();
    assert_eq!((view.left, view.zoom), (0.0, 0.25));
}

/// A ruler in a memo, as the song draws its bars once for many runs.
const KEPT_PLANE: &str = r#"import { h, input, memo, tool } from "./sdk";
export const kept = tool({
  name: "kept", title: "Kept", when: "Never", doc: "Kept.", state: {}, sound: () => input,
  view: () => memo("ruler", [], () => (
    <box key="ruler" label="Ruler" w={400} h={20} plane={{ x: 0.1, zoom: [0.25, 8] }}>
      <box key="bar-2" label="Bar 2" place={[3840, 0, 10, 10]} />
    </box>
  )),
});
"#;

#[test]
fn a_plane_a_memo_kept_builds_again_as_it_zooms() {
    let (_scratch, folder) = with_tools(&[("kept", KEPT_PLANE)]);
    let mut window = laid_out(opened(&folder));
    redraw(&mut window);
    let ruler = "t00/chain/kept:ruler";
    act(
        &mut window.state,
        json!({ "key": ruler, "action": "view", "value": { "x": [0, 2000] } }),
    );
    window.invalidate();
    window.frame(1.0 / 60.0);
    redraw(&mut window);
    let at = |key: &str| window.presented(key).unwrap().0[0];
    assert_eq!(at("t00/chain/kept:bar-2") - at(ruler), 768.0);
}

/// A lane placed in a song plane: its time is the song's, its value runs up from 0 to 1.
const NESTED: &str = r#"import { h, input, tool } from "./sdk";
export const nested = tool({
  name: "nested", title: "Nested", when: "Never", doc: "A plane in a plane.", state: {}, sound: () => input,
  view: () => (
    <col w={400}>
      <box key="song" label="Song" w={400} h={200} plane={{ x: 0.1, y: 1 }}>
        <box key="lane" label="Lane" place={[1000, 100, 2000, 50]} plane={{ x: "song", y: [1, 0] }}>
          <box key="point" label="Point" place={[2000, 0.75]} w={6} h={6} />
          <box key="high" label="High" place={[1000, 0.5, 500, 0.5]} />
        </box>
      </box>
    </col>
  ),
});
"#;

#[test]
fn a_plane_placed_in_the_plane_whose_time_it_shares_starts_where_it_is_placed_and_a_range_runs_up()
{
    let (_scratch, folder) = with_tools(&[("nested", NESTED)]);
    let mut window = laid_out(opened(&folder));
    redraw(&mut window);
    let at = |window: &rune::Runtime<State>, key: &str| {
        let key = format!("t00/chain/nested:{key}");
        window
            .presented(&key)
            .unwrap_or_else(|| panic!("no {key}"))
            .0
    };
    let lane = at(&window, "lane");
    assert_eq!(lane[2..], [200.0, 50.0]);
    let middle = |rect: [f32; 4]| {
        [
            rect[0] + rect[2] / 2.0 - lane[0],
            rect[1] + rect[3] / 2.0 - lane[1],
        ]
    };
    // Tick 2000 of the song is 100 points into a lane that starts at its tick 1000; 0.75 is a
    // quarter of the way down, and 0.5 to 1 the top half.
    assert_eq!(middle(at(&window, "point")), [100.0, 12.5]);
    assert_eq!(at(&window, "high")[..2], [lane[0], lane[1]]);
    assert_eq!(at(&window, "high")[2..], [50.0, 25.0]);
    // A scroll of the song moves the lane and what is in it once, together.
    let song = at(&window, "song");
    let shift = rune::Mods {
        shift: true,
        ..rune::Mods::default()
    };
    wheel(
        &mut window,
        [song[0] + 10.0, song[1] + 10.0],
        [0.0, -50.0],
        shift,
    );
    redraw(&mut window);
    let moved = at(&window, "lane");
    assert_eq!(moved[0], lane[0] - 50.0);
    let middle = |rect: [f32; 4]| {
        [
            rect[0] + rect[2] / 2.0 - moved[0],
            rect[1] + rect[3] / 2.0 - moved[1],
        ]
    };
    assert_eq!(middle(at(&window, "point")), [100.0, 12.5]);
}

/// A line in a memo through a point that drags.
const LINE: &str = r#"import { h, input, live, memo, tool } from "./sdk";
export const line = tool({
  name: "line", title: "Line", when: "Never", doc: "A line through a point that drags.", state: {}, sound: () => input,
  view: () => {
    const d = live.drag("point");
    return (
      <col w={400}>
        <box key="song" label="Song" w={400} h={200} plane={{ x: 1, y: 1 }}>
          {memo("line", [], () => <shapes place={[0, 0, 400, 200]} lines={[{ color: "text", width: 1, points: [0, 100, d.dx.plus(200), d.dy.plus(100), d.dx.plus(200), d.dy.plus(100), 400, 100] }]} />)}
          {memo("still", [], () => <shapes place={[0, 0, 400, 200]} lines={[{ color: "text", width: 1, points: [0, 150, 400, 150] }]} />)}
          <box key="point" label="Point" place={[d.dx.plus(200), d.dy.plus(100)]} w={8} h={8} drag={{ x: true, y: true }} />
        </box>
      </col>
    );
  },
});
"#;

#[test]
fn a_line_through_a_point_that_drags_is_drawn_again_as_it_moves_and_bun_runs_no_view() {
    let (_scratch, folder) = with_tools(&[("line", LINE)]);
    let mut window = laid_out(opened(&folder));
    redraw(&mut window);
    let runs = view_runs(&window);
    let drawn = window.state.lives.drawings.get();
    let (x, y) = window.find("t00/chain/line:point").unwrap();
    let next = |window: &mut rune::Runtime<State>| {
        let seconds = window.state.last + 1.0 / 60.0;
        super::drawn_frame(window, seconds);
    };
    window.pointer_down(x, y);
    next(&mut window);
    for step in 1..=4 {
        window.pointer_move(x + step as f32 * 5.0, y + step as f32 * 5.0);
        next(&mut window);
    }
    // Built again for each move.
    let again = window.state.lives.drawings.get() - drawn;
    assert!((3..=5).contains(&again), "{again}");
    assert_eq!(view_runs(&window), runs);
}
