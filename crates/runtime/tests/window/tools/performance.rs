//! What a frame of a page of many shapes costs in the window: a measurement, not a check. A
//! frame here is the event, the work it causes and the draw into the scene, not the GPU. Run
//! with `cargo nextest run -p runtime --test window --cargo-profile release-fast --run-ignored only performance --no-capture`.

use std::time::{Duration, Instant};

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, point, px,
};

use super::{ONLY_THE_TOOL, Window, open};

/// `count` notes of a piano roll, all bound to the drag `move` as selected notes are, with a
/// colour of their own under the pointer; or with `canvas`, the same notes on a canvas whose
/// drag moves the first one through the code, as a drag did before live values.
const GRID: &str = r##"import { Canvas, h, number, Rect, Space, tool, type Shape } from "./sdk";

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
    return h(Space, { key: "roll", width: 1200, height: 600, unit: { x: 0.1, y: 4 } },
      notes.map((note, index) => h(Rect, {
        key: String(index),
        x: move.dx.plus(note.start),
        y: move.dy.plus(note.pitch),
        width: 240,
        height: 1,
        color: "#a9b1ff",
        hover: "#e9ebef",
        drag: { id: "move", snap: { x: 240, y: 1 }, onEnd: () => {} },
      })),
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
