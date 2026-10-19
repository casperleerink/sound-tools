//! What a frame of a page of many shapes costs in the window: a measurement, not a check. A
//! frame here is the event, the work it causes and the draw into the scene, not the GPU. Run
//! with `cargo nextest run -p runtime --test window --cargo-profile release-fast --run-ignored only performance --no-capture`.

use std::time::{Duration, Instant};

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, TestAppContext, TouchPhase, point, px,
};

use super::piano_roll::{ROW, open_roll};
use super::{ONLY_THE_TOOL, Window, open};

/// `count` notes of a piano roll, all in a group the drag `move` moves, as selected notes are,
/// with a colour of their own under the pointer; or with `canvas`, the same notes on a canvas
/// whose drag moves the first one through the code, as a drag did before live values.
const GRID: &str = r##"import { Canvas, Group, h, number, Rect, Space, tool, type Drag, type Shape } from "./sdk";

tool({
  name: "grid",
  title: "Grid",
  when: "A measurement",
  doc: "Many notes.",
  kind: "source",
  state: { count: number({ min: 0, max: 100000, default: 100 }), canvas: number({ min: 0, max: 1, default: 0 }) },
  memory: () => ({ x: 0 }),
  sound: () => 0,
  page: ({ state, memory, now }) => {
    const notes = Array.from({ length: state.count }, (_, index) => ({ start: (index % 64) * 240, pitch: 36 + (index % 48) }));
    if (state.canvas === 1) {
      const shapes: Shape[] = notes.map((note, index) => ({
        kind: "rect", x: (index === 0 ? memory.x : note.start) / 10, y: note.pitch * 4, width: 24, height: 4, color: "#a9b1ff",
      }));
      return h(Canvas, { width: 1200, height: 600, shapes, onDrag: (x) => { memory.x = x * 12000; } });
    }
    const move = now.drag("move");
    const drag: Drag = { id: "move", snap: { x: 240, y: 1 }, onEnd: () => {} };
    return h(Space, { key: "roll", width: 1200, height: 600, unit: { x: 0.1, y: 4 } },
      h(Group, { x: move.dx, y: move.dy },
        notes.map((note, index) => h(Rect, {
          key: String(index),
          x: note.start,
          y: note.pitch,
          width: 240,
          height: 1,
          color: "#a9b1ff",
          hover: "#e9ebef",
          drag,
        })),
      ),
    );
  },
});
"##;

const SIZES: [usize; 3] = [100, 1_000, 2_000];
const MOVES: usize = 60;

fn print(what: &str, mut times: Vec<Duration>) {
    times.sort();
    let mean = times.iter().sum::<Duration>() / times.len().max(1) as u32;
    let median = times.get(times.len() / 2).copied().unwrap_or_default();
    let worst = times.last().copied().unwrap_or_default();
    println!("{what}: mean {mean:?}, median {median:?}, worst {worst:?}");
}

impl Window<'_> {
    /// One mouse event and the frame it causes.
    fn timed(&mut self, event: impl FnOnce(&mut Self)) -> Duration {
        let started = Instant::now();
        event(self);
        self.cx.run_until_parked();
        started.elapsed()
    }

    fn down(&mut self, position: Point<Pixels>) {
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

    fn up(&mut self, position: Point<Pixels>) {
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    fn origin(&mut self, selector: &'static str) -> Point<Pixels> {
        self.until(selector, |window| window.find(selector).is_some());
        self.cx.debug_bounds(selector).unwrap().origin
    }
}

fn open_grid(cx: &mut TestAppContext, count: usize, canvas: bool) -> Option<Window<'_>> {
    let record = format!(
        r#"{{"tool": "grid", "state": {{"count": {count}, "canvas": {}}}}}"#,
        u8::from(canvas)
    );
    open(
        cx,
        &[
            ONLY_THE_TOOL[0],
            ("state/grid.json", &record),
            ("extensions/grid.ts", GRID),
        ],
    )
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_page_of_many_shapes_100(cx: &mut TestAppContext) {
    measure(cx, SIZES[0]);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_page_of_many_shapes_1000(cx: &mut TestAppContext) {
    measure(cx, SIZES[1]);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_page_of_many_shapes_2000(cx: &mut TestAppContext) {
    measure(cx, SIZES[2]);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_drag_through_bun_100(cx: &mut TestAppContext) {
    measure_old(cx, SIZES[0]);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_drag_through_bun_1000(cx: &mut TestAppContext) {
    measure_old(cx, SIZES[1]);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_drag_through_bun_2000(cx: &mut TestAppContext) {
    measure_old(cx, SIZES[2]);
}

/// The new road: the window moves every note as the pointer goes, and Bun hears the end.
fn measure(cx: &mut TestAppContext, count: usize) {
    let Some(mut window) = open_grid(cx, count, false) else {
        return;
    };
    let roll = window.origin("space-roll");
    // The first note, and a place between notes.
    let (note, empty) = (
        roll + point(px(12.), px(146.)),
        roll + point(px(1190.), px(10.)),
    );
    // Each move goes on or off a note, so each draws the whole page again.
    let hovering: Vec<Duration> = (0..MOVES)
        .map(|step| {
            let to = if step % 2 == 0 { note } else { empty };
            window.timed(|window| {
                window
                    .cx
                    .simulate_mouse_move(to, None, Modifiers::default())
            })
        })
        .collect();
    print(
        &format!("{count} shapes: a frame with no drag, drawn again"),
        hovering,
    );
    let asked = sound_typescript::sent_renders_and_events();
    window.down(note);
    let dragging: Vec<Duration> = (1..=MOVES)
        .map(|step| {
            let to = note + point(px(step as f32 * 3.), px(step as f32));
            window.timed(|window| {
                window
                    .cx
                    .simulate_mouse_move(to, MouseButton::Left, Modifiers::default())
            })
        })
        .collect();
    let (renders, events) = sound_typescript::sent_renders_and_events();
    print(
        &format!("{count} shapes: a frame while every shape moves with the drag"),
        dragging,
    );
    println!(
        "{count} shapes: {} renders and {} events asked of Bun in {MOVES} moves",
        renders - asked.0,
        events - asked.1
    );
    window.up(note + point(px(MOVES as f32 * 3.), px(MOVES as f32)));
}

/// The old road: every move runs the code, which draws a new tree.
fn measure_old(cx: &mut TestAppContext, count: usize) {
    let Some(mut window) = open_grid(cx, count, true) else {
        return;
    };
    let canvas = window.origin("space-space-");
    let at = canvas + point(px(100.), px(100.));
    window.down(at);
    let asked = sound_typescript::sent_renders_and_events();
    let mut latencies = Vec::new();
    for step in 1..=MOVES {
        let heard = sound_typescript::heard_trees();
        let started = Instant::now();
        let to = at + point(px(step as f32 * 3.), px(0.));
        window
            .cx
            .simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        // Until the tree of this move is drawn: Bun answers on a thread of its own.
        while sound_typescript::heard_trees() == heard {
            window.cx.run_until_parked();
            if started.elapsed() > Duration::from_secs(5) {
                let problems = window.problems();
                panic!(
                    "no tree came: {:?} {:?} {problems:?}",
                    asked,
                    sound_typescript::sent_renders_and_events()
                );
            }
        }
        window.cx.run_until_parked();
        latencies.push(started.elapsed());
    }
    let (renders, events) = sound_typescript::sent_renders_and_events();
    print(
        &format!("{count} shapes, canvas: a move until its tree is drawn"),
        latencies,
    );
    println!(
        "{count} shapes, canvas: {} renders and {} events asked of Bun in {MOVES} moves",
        renders - asked.0,
        events - asked.1
    );
    window.up(at);
}

/// 2,000 notes of the piano roll of `tooling/agent-eval/examples`: 16 pitches from 56 on every
/// sixteenth for 125 sixteenths.
fn many_notes() -> serde_json::Value {
    let notes: Vec<serde_json::Value> = (0..2_000)
        .map(|index| {
            serde_json::json!({
                "start": (index / 16) * 240, "length": 200, "pitch": 56 + index % 16, "velocity": 40 + index % 80,
            })
        })
        .collect();
    serde_json::Value::Array(notes)
}

/// The piano roll with 2,000 notes: frames as the pointer hovers, as it scrolls, as it zooms
/// and as it drags 512 selected notes, and how long a release takes to come back as a tree.
#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_the_piano_roll_of_2000_notes(cx: &mut TestAppContext) {
    let heard = sound_typescript::heard_tree_bytes();
    let Some(mut window) = open_roll(cx, &many_notes()) else {
        return;
    };
    let roll = window.space_at("roll");
    println!(
        "piano roll: the first tree is {} bytes",
        sound_typescript::heard_tree_bytes() - heard
    );
    let (note, empty) = (window.on_roll(100.0, 60), window.on_roll(20_000.0, 80));
    let hovering: Vec<Duration> = (0..MOVES)
        .map(|step| {
            let to = if step % 2 == 0 { note } else { empty };
            window.timed(|window| {
                window
                    .cx
                    .simulate_mouse_move(to, None, Modifiers::default())
            })
        })
        .collect();
    print(
        "piano roll: a frame as the pointer hovers on and off a note",
        hovering,
    );
    let wheel = |delta: Point<Pixels>, modifiers: Modifiers| ScrollWheelEvent {
        position: roll + point(px(300.), px(200.)),
        delta: ScrollDelta::Pixels(delta),
        modifiers,
        touch_phase: TouchPhase::Moved,
    };
    let scrolling: Vec<Duration> = (0..MOVES)
        .map(|step| {
            let sign = if step < MOVES / 2 { -1. } else { 1. };
            let event = wheel(point(px(sign * 20.), px(sign * 6.)), Modifiers::default());
            window.timed(|window| window.cx.simulate_event(event))
        })
        .collect();
    print("piano roll: a frame as it scrolls", scrolling);
    let zooming: Vec<Duration> = (0..MOVES)
        .map(|step| {
            let sign = if step < MOVES / 2 { 1. } else { -1. };
            let event = wheel(point(px(0.), px(sign * 8.)), Modifiers::command());
            window.timed(|window| window.cx.simulate_event(event))
        })
        .collect();
    print("piano roll: a frame as it zooms", zooming);

    // A marquee over the first 32 sixteenths: 512 notes.
    let from = window.on_roll(0.0, 72) + point(px(1.), px(-ROW));
    let to = window.on_roll(7_500.0, 55);
    window.down(from);
    window
        .cx
        .simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    window.cx.run_until_parked();
    window.up(to);
    window.wait_for_bun();
    let pressed = window.on_roll(10.0, 60);
    window.down(pressed);
    window.wait_for_bun();
    let asked = sound_typescript::sent_renders_and_events();
    let dragging: Vec<Duration> = (1..=MOVES)
        .map(|step| {
            let to = pressed + point(px(step as f32 * 2.), px(step as f32 * 0.5));
            window.timed(|window| {
                window
                    .cx
                    .simulate_mouse_move(to, MouseButton::Left, Modifiers::default())
            })
        })
        .collect();
    let (renders, events) = sound_typescript::sent_renders_and_events();
    print(
        "piano roll: a frame as 512 selected notes move with the drag",
        dragging,
    );
    println!(
        "piano roll: {} renders and {} events asked of Bun in {MOVES} moves",
        renders - asked.0,
        events - asked.1
    );
    let (trees, bytes) = (
        sound_typescript::heard_trees(),
        sound_typescript::heard_tree_bytes(),
    );
    let released = Instant::now();
    window.up(pressed + point(px(MOVES as f32 * 2.), px(MOVES as f32 * 0.5)));
    while sound_typescript::heard_trees() == trees {
        window.cx.run_until_parked();
        assert!(released.elapsed() < Duration::from_secs(5), "no tree came");
    }
    window.cx.run_until_parked();
    println!(
        "piano roll: release until its new tree is drawn: {:?}, a tree of {} bytes",
        released.elapsed(),
        sound_typescript::heard_tree_bytes() - bytes
    );
}

/// A frame of the mixer as it plays, with `count` strips: the engine plays a frame's worth of
/// sound between frames, which is not timed.
fn measure_mixer(cx: &mut TestAppContext, count: usize) {
    let Some(mut window) = super::mixer::open_mixer(cx, count) else {
        return;
    };
    window.space_at("fader-master");
    window.until("the master meter to show a level", |window| {
        window.play_and_draw(800);
        sound_typescript::shown_by_live_values("peak:master").is_some_and(|peak| peak > 0.1)
    });
    let asked = sound_typescript::sent_renders_and_events();
    let drawn = sound_typescript::live_frames();
    let frames: Vec<Duration> = (0..MOVES * 2)
        .map(|_| {
            // 1/60 s of sound at 48 kHz.
            window.loudest(800);
            window.timed(|window| window.cx.update(|window, _| window.refresh()))
        })
        .collect();
    let (renders, events) = sound_typescript::sent_renders_and_events();
    print(
        &format!("mixer of {count} strips: a frame as it plays"),
        frames,
    );
    println!(
        "mixer of {count} strips: {} renders and {} events asked of Bun in {} frames ({} drawn)",
        renders - asked.0,
        events - asked.1,
        MOVES * 2,
        sound_typescript::live_frames() - drawn,
    );
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_the_mixer_8(cx: &mut TestAppContext) {
    measure_mixer(cx, 8);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_the_mixer_32(cx: &mut TestAppContext) {
    measure_mixer(cx, 32);
}

/// `count` waves of the last 8192 frames of a tone, as lines or as bands.
const SCOPES: &str = r##"import { h, number, Space, tool, Wave, type Declaration } from "./sdk";

tool({
  name: "scopes",
  title: "Scopes",
  when: "A measurement",
  doc: "Many waves of one meter.",
  state: { count: number({ min: 0, max: 64, default: 1 }), band: number({ min: 0, max: 1, default: 0 }) },
  behaviour: (): Declaration => ({ connections: [{ from: { child: "tone" }, to: { meter: "tone" } }] }),
  page: ({ state, now }) =>
    h("div", { style: { gap: 2 } },
      Array.from({ length: state.count }, (_, index) =>
        h(Space, { key: `wave${index}`, width: 600, height: 30 },
          h(Wave, { x: 0, y: 0, width: 600, height: 30, list: now.scope("tone", { frames: 8192 }), color: "#7ee0a0", band: state.band === 1 }),
        ),
      ),
    ),
});
"##;

fn measure_scopes(cx: &mut TestAppContext, count: usize, band: bool) {
    let record = format!(
        r#"{{"tool": "scopes", "state": {{"count": {count}, "band": {}}}}}"#,
        u8::from(band)
    );
    let project = r#"{"format": 1, "extensions": ["tone"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": []}"#;
    let Some(mut window) = open(
        cx,
        &[
            ("project.json", project),
            ("state/scopes/instance.json", &record),
            (
                "state/scopes/tone.json",
                r#"{"tool": "tone", "state": {"frequency_hz": 220, "gain": 0.3}}"#,
            ),
            ("extensions/scopes.ts", SCOPES),
        ],
    ) else {
        return;
    };
    window.space_at("wave0");
    // A whole scope of sound.
    for _ in 0..20 {
        window.loudest(800);
        window.find("space-wave0");
    }
    let frames: Vec<Duration> = (0..MOVES)
        .map(|_| {
            window.loudest(800);
            window.timed(|window| window.cx.update(|window, _| window.refresh()))
        })
        .collect();
    let kind = if band { "bands" } else { "lines" };
    print(
        &format!("{count} waves of 8192 frames as {kind}: a frame"),
        frames,
    );
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_waves(cx: &mut TestAppContext) {
    measure_scopes(cx, 1, false);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_waves_16_lines(cx: &mut TestAppContext) {
    measure_scopes(cx, 16, false);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_waves_16_bands(cx: &mut TestAppContext) {
    measure_scopes(cx, 16, true);
}
