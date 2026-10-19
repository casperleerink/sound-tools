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
/// whose drag moves the first one through the code, which makes every shape again.
const GRID: &str = r##"import { For, Group, h, number, Rect, Space, tool, type Drag } from "./sdk";

tool({
  name: "grid",
  title: "Grid",
  when: "A measurement",
  doc: "Many notes.",
  state: { count: number({ min: 0, max: 100000, default: 100 }), canvas: number({ min: 0, max: 1, default: 0 }) },
  memory: () => ({ x: 0 }),
  sound: () => 0,
  view: ({ state, memory, now }) => {
    const notes = () => Array.from({ length: state.count }, (_, index) => ({ start: (index % 64) * 240, pitch: 36 + (index % 48) }));
    const canvas = () => state.canvas === 1;
    const shapes = (): ShapeLiteral[] => notes().map((note, index) => ({
      kind: "rect", x: (index === 0 ? memory.x : note.start) / 10, y: note.pitch * 4, width: 24, height: 4, color: "#a9b1ff",
    }));
    const move = now.drag("move");
    const drag: Drag = { id: "move", snap: { x: 240, y: 1 }, onEnd: () => {} };
    return () => canvas()
      ? h(Canvas, { width: 1200, height: 600, shapes, onDrag: (x) => { memory.x = x * 12000; } })
      : h(Space, { key: "roll", width: 1200, height: 600, unit: { x: 0.1, y: 4 } },
          h(Group, { x: move.dx, y: move.dy },
            h(For<{ start: number; pitch: number }>, { each: notes }, (note) => h(Rect, {
              x: note.start, y: note.pitch, width: 240, height: 1, color: "#a9b1ff", hover: "#e9ebef", drag,
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
    let asked = sound_typescript::sent_updates_and_events();
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
    let (updates, events) = sound_typescript::sent_updates_and_events();
    print(
        &format!("{count} shapes: a frame while every shape moves with the drag"),
        dragging,
    );
    println!(
        "{count} shapes: {} updates and {} events told Bun in {MOVES} moves",
        updates - asked.0,
        events - asked.1
    );
    window.up(note + point(px(MOVES as f32 * 3.), px(MOVES as f32)));
}

/// The old road: every move runs the code, which makes the canvas again.
fn measure_old(cx: &mut TestAppContext, count: usize) {
    let Some(mut window) = open_grid(cx, count, true) else {
        return;
    };
    let canvas = window.origin("space-space-0");
    let at = canvas + point(px(100.), px(100.));
    window.down(at);
    let asked = sound_typescript::sent_updates_and_events();
    let mut latencies = Vec::new();
    for step in 1..=MOVES {
        let heard = sound_typescript::heard_batches();
        let started = Instant::now();
        let to = at + point(px(step as f32 * 3.), px(0.));
        window
            .cx
            .simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        // Until the ops of this move are drawn: Bun answers on a thread of its own.
        while sound_typescript::heard_batches() == heard {
            window.cx.run_until_parked();
            if started.elapsed() > Duration::from_secs(5) {
                let problems = window.problems();
                panic!(
                    "no ops came: {:?} {:?} {problems:?}",
                    asked,
                    sound_typescript::sent_updates_and_events()
                );
            }
        }
        window.cx.run_until_parked();
        latencies.push(started.elapsed());
    }
    let (updates, events) = sound_typescript::sent_updates_and_events();
    print(
        &format!("{count} shapes, canvas: a move until its ops are drawn"),
        latencies,
    );
    println!(
        "{count} shapes, canvas: {} updates and {} events told Bun in {MOVES} moves",
        updates - asked.0,
        events - asked.1
    );
    window.up(at);
}

/// `count` notes of the piano roll of `tooling/agent-eval/examples`: `rows` pitches from 56 on
/// every sixteenth.
fn many_notes(count: usize, rows: usize) -> serde_json::Value {
    let notes: Vec<serde_json::Value> = (0..count)
        .map(|index| {
            serde_json::json!({
                "start": (index / rows) * 240, "length": 200, "pitch": 40 + index % rows, "velocity": 40 + index % 80,
            })
        })
        .collect();
    serde_json::Value::Array(notes)
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_the_piano_roll_of_2000_notes(cx: &mut TestAppContext) {
    measure_roll(cx, 2_000, 16);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_the_piano_roll_of_20000_notes(cx: &mut TestAppContext) {
    measure_roll(cx, 20_000, 32);
}

impl Window<'_> {
    /// Waits for the next batch of ops and its frame: how long from `started`, and what it cost.
    fn next_batch(&mut self, what: &str, started: Instant, batches: u64, bytes: u64) {
        while sound_typescript::heard_batches() == batches {
            self.cx.run_until_parked();
            assert!(started.elapsed() < Duration::from_secs(10), "no ops came");
        }
        let heard = started.elapsed();
        let frame = self.timed(|window| window.cx.update(|window, _| window.refresh()));
        let (bun, apply) = sound_typescript::last_batch();
        println!(
            "{what}: {:?} until drawn; Bun {bun:.2} ms, {} bytes, apply {apply:.2} ms, frame {frame:?}",
            heard + frame,
            sound_typescript::heard_bytes() - bytes,
        );
    }
}

/// The piano roll with `count` notes on `rows` pitches: how long it takes to open, frames as the
/// pointer hovers, as it scrolls, as it zooms and as it drags 512 selected notes, and what a
/// click and a release cost until they are drawn.
fn measure_roll(cx: &mut TestAppContext, count: usize, rows: usize) {
    let bytes = sound_typescript::heard_bytes();
    let opened = Instant::now();
    let Some(mut window) = open_roll(cx, &many_notes(count, rows)) else {
        return;
    };
    let roll = window.space_at("roll");
    let (bun, apply) = sound_typescript::last_batch();
    println!(
        "{count} notes: opened and drawn in {:?}, its first ops are {} bytes; Bun {bun:.2} ms, apply {apply:.2} ms",
        opened.elapsed(),
        sound_typescript::heard_bytes() - bytes
    );
    let idle: Vec<Duration> = (0..MOVES)
        .map(|_| window.timed(|window| window.cx.update(|window, _| window.refresh())))
        .collect();
    print(&format!("{count} notes: an idle frame"), idle);
    let (note, empty) = (window.on_roll(100.0, 50), window.on_roll(100_000.0, 120));
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
        &format!("{count} notes: a frame as the pointer hovers on and off a note"),
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
    print(&format!("{count} notes: a frame as it scrolls"), scrolling);
    let zooming: Vec<Duration> = (0..MOVES)
        .map(|step| {
            let sign = if step < MOVES / 2 { 1. } else { -1. };
            let event = wheel(point(px(0.), px(sign * 8.)), Modifiers::command());
            window.timed(|window| window.cx.simulate_event(event))
        })
        .collect();
    print(&format!("{count} notes: a frame as it zooms"), zooming);

    // A click on one note selects it.
    let pressed = window.on_roll(10.0, 50);
    let (batches, bytes) = (
        sound_typescript::heard_batches(),
        sound_typescript::heard_bytes(),
    );
    let clicked = Instant::now();
    window.down(pressed);
    window.up(pressed);
    window.next_batch(
        &format!("{count} notes: a click that selects a note"),
        clicked,
        batches,
        bytes,
    );
    window.wait_for_bun();

    // A marquee over the first sixteenths: 512 notes.
    let columns = 512 / rows;
    let top = 40 + rows - 1;
    let from = window.on_roll(0.0, top as u8) + point(px(1.), px(-ROW));
    let to = window.on_roll((columns * 240 - 30) as f32, 39);
    window.down(from);
    window
        .cx
        .simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    window.cx.run_until_parked();
    let (batches, bytes) = (
        sound_typescript::heard_batches(),
        sound_typescript::heard_bytes(),
    );
    let released = Instant::now();
    window.up(to);
    window.next_batch(
        &format!("{count} notes: a marquee that selects 512 notes, released"),
        released,
        batches,
        bytes,
    );
    window.wait_for_bun();
    window.down(pressed);
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
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
    let (updates, events) = sound_typescript::sent_updates_and_events();
    print(
        &format!("{count} notes: a frame as 512 selected notes move with the drag"),
        dragging,
    );
    println!(
        "{count} notes: {} updates and {} events told Bun in {MOVES} moves",
        updates - asked.0,
        events - asked.1
    );
    let (batches, bytes) = (
        sound_typescript::heard_batches(),
        sound_typescript::heard_bytes(),
    );
    let released = Instant::now();
    window.up(pressed + point(px(MOVES as f32 * 2.), px(MOVES as f32 * 0.5)));
    window.next_batch(
        &format!("{count} notes: a release that moves 512 notes"),
        released,
        batches,
        bytes,
    );
    window.wait_for_bun();
    println!(
        "{count} notes: the window told Bun {} bytes in all",
        sound_typescript::sent_bytes()
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

/// The mixer of `count` strips as it plays: a frame, and what it asks of Bun.
fn measure_mixer(cx: &mut TestAppContext, count: usize) {
    let Some(mut window) = super::mixer::open_mixer(cx, count) else {
        return;
    };
    window.space_at("fader-master");
    window.until("the master meter to show a level", |window| {
        window.play_and_draw(800);
        sound_typescript::shown_by_live_values("peak:master").is_some_and(|peak| peak > 0.1)
    });
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    let batches = sound_typescript::heard_batches();
    let frames: Vec<Duration> = (0..MOVES * 2)
        .map(|_| {
            // 1/60 s of sound at 48 kHz.
            window.loudest(800);
            window.timed(|window| window.cx.update(|window, _| window.refresh()))
        })
        .collect();
    let (updates, events) = sound_typescript::sent_updates_and_events();
    print(
        &format!("mixer of {count} strips: a frame as it plays"),
        frames,
    );
    println!(
        "mixer of {count} strips: {} updates, {} events and {} batches of ops in {} frames",
        updates - asked.0,
        events - asked.1,
        sound_typescript::heard_batches() - batches,
        MOVES * 2,
    );
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_song_of_12_tracks_of_150_clips(cx: &mut TestAppContext) {
    measure_song(cx, 12, 150);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_song_of_24_tracks_of_600_clips(cx: &mut TestAppContext) {
    measure_song(cx, 24, 600);
}

#[gpui::test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_a_song_of_100_tracks_of_100_clips(cx: &mut TestAppContext) {
    let what = "the scale project as a song, 100 tracks of 100 clips";
    let files = crate::song::scale_files();
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let opened = Instant::now();
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    let timeline = window.space_at("timeline");
    println!("{what}: opened and drawn in {:?}", opened.elapsed());
    window.wait_for_bun();
    let idle: Vec<Duration> = (0..MOVES)
        .map(|_| window.timed(|window| window.cx.update(|window, _| window.refresh())))
        .collect();
    print(&format!("{what}: an idle frame"), idle);
    let wheel = |delta: Point<Pixels>, modifiers: Modifiers| ScrollWheelEvent {
        position: timeline + point(px(300.), px(100.)),
        delta: ScrollDelta::Pixels(delta),
        modifiers,
        touch_phase: TouchPhase::Moved,
    };
    for (name, delta) in [
        ("across", point(px(30.), px(0.))),
        ("down", point(px(0.), px(30.))),
    ] {
        let scrolling: Vec<Duration> = (0..MOVES)
            .map(|step| {
                let sign = if step < MOVES / 2 { -1. } else { 1. };
                let event = wheel(point(delta.x * sign, delta.y * sign), Modifiers::default());
                window.timed(|window| window.cx.simulate_event(event))
            })
            .collect();
        print(&format!("{what}: a frame as it scrolls {name}"), scrolling);
    }
    let zooming: Vec<Duration> = (0..MOVES)
        .map(|step| {
            let sign = if step < MOVES / 2 { 1. } else { -1. };
            let event = wheel(point(px(0.), px(sign * 4.)), Modifiers::command());
            window.timed(|window| window.cx.simulate_event(event))
        })
        .collect();
    print(&format!("{what}: a frame as it zooms"), zooming);
    window.focus_the_window();
    window.cx.simulate_keystrokes("space");
    window.poll();
    window.wait_for_bun();
    let playing: Vec<Duration> = (0..MOVES * 2)
        .map(|_| {
            window.loudest(800);
            window.timed(|window| window.cx.update(|window, _| window.refresh()))
        })
        .collect();
    print(
        &format!("{what}: a frame as it plays, every meter moving"),
        playing,
    );
}

/// The song of `tooling/agent-eval/examples`: how long it takes to open, frames idle, as it
/// scrolls, zooms and plays with every meter, and what a click on a clip, a move of one and a
/// note added in the editor cost until they are drawn.
fn measure_song(cx: &mut TestAppContext, tracks: usize, clips: usize) {
    let what = format!("{tracks} tracks of {clips} clips");
    let bytes = sound_typescript::heard_bytes();
    let opened = Instant::now();
    let Some(mut window) = super::song::open_song(cx, tracks, clips) else {
        return;
    };
    let timeline = window.space_at("timeline");
    let (bun, apply) = sound_typescript::last_batch();
    println!(
        "{what}: opened and drawn in {:?}, its first ops are {} bytes; Bun {bun:.2} ms, apply {apply:.2} ms",
        opened.elapsed(),
        sound_typescript::heard_bytes() - bytes
    );
    window.wait_for_bun();
    let idle: Vec<Duration> = (0..MOVES)
        .map(|_| window.timed(|window| window.cx.update(|window, _| window.refresh())))
        .collect();
    print(&format!("{what}: an idle frame"), idle);
    let wheel = |delta: Point<Pixels>, modifiers: Modifiers| ScrollWheelEvent {
        position: timeline + point(px(300.), px(100.)),
        delta: ScrollDelta::Pixels(delta),
        modifiers,
        touch_phase: TouchPhase::Moved,
    };
    let scrolling: Vec<Duration> = (0..MOVES)
        .map(|step| {
            let sign = if step < MOVES / 2 { -1. } else { 1. };
            let event = wheel(point(px(sign * 30.), px(0.)), Modifiers::default());
            window.timed(|window| window.cx.simulate_event(event))
        })
        .collect();
    print(&format!("{what}: a frame as it scrolls"), scrolling);
    let zooming: Vec<Duration> = (0..MOVES)
        .map(|step| {
            // Never as far as the zoom's ends, so it comes back to where it was.
            let sign = if step < MOVES / 2 { 1. } else { -1. };
            let event = wheel(point(px(0.), px(sign * 4.)), Modifiers::command());
            window.timed(|window| window.cx.simulate_event(event))
        })
        .collect();
    print(&format!("{what}: a frame as it zooms"), zooming);

    // A click on the second clip of the second track selects it.
    let clip = window.on_lane(3 * crate::song::BAR + 960, 1);
    let (batches, bytes) = (
        sound_typescript::heard_batches(),
        sound_typescript::heard_bytes(),
    );
    let clicked = Instant::now();
    window.down(clip);
    window.up(clip);
    window.next_batch(
        &format!("{what}: a click that selects a clip"),
        clicked,
        batches,
        bytes,
    );
    window.wait_for_bun();

    // It moves two bars on.
    window.down(clip);
    let asked = sound_typescript::sent_updates_and_events();
    let dragging: Vec<Duration> = (1..=MOVES)
        .map(|step| {
            let to = clip + point(px(step as f32 * 192. / MOVES as f32), px(0.));
            window.timed(|window| {
                window
                    .cx
                    .simulate_mouse_move(to, MouseButton::Left, Modifiers::default())
            })
        })
        .collect();
    let (updates, events) = sound_typescript::sent_updates_and_events();
    print(
        &format!("{what}: a frame as a clip moves with the drag"),
        dragging,
    );
    println!(
        "{what}: {} updates and {} events told Bun in {MOVES} moves",
        updates - asked.0,
        events - asked.1
    );
    let (batches, bytes) = (
        sound_typescript::heard_batches(),
        sound_typescript::heard_bytes(),
    );
    let released = Instant::now();
    window.up(clip + point(px(192.), px(0.)));
    window.next_batch(
        &format!("{what}: a release that moves a clip"),
        released,
        batches,
        bytes,
    );
    window.wait_for_bun();

    // The first clip of the first track opens below, and a double click adds a note to it.
    // Under more tracks than the window is tall there is no room for it: the tracks do not
    // scroll.
    if tracks <= 12 {
        let first = window.on_lane(crate::song::BAR / 2, 0);
        window.double_click(first);
        let editor = window.space_at("editor");
        window.wait_for_bun();
        let (batches, bytes) = (
            sound_typescript::heard_batches(),
            sound_typescript::heard_bytes(),
        );
        let added = Instant::now();
        window.double_click(editor + point(px(30.), px(4.)));
        window.next_batch(
            &format!("{what}: a note added in the editor"),
            added,
            batches,
            bytes,
        );
        window.wait_for_bun();
    }

    window.focus_the_window();
    window.cx.simulate_keystrokes("space");
    window.poll();
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    let batches = sound_typescript::heard_batches();
    let started = Instant::now();
    let playing: Vec<Duration> = (0..MOVES * 2)
        .map(|_| {
            window.loudest(800);
            window.timed(|window| window.cx.update(|window, _| window.refresh()))
        })
        .collect();
    let seconds = started.elapsed().as_secs_f64();
    let (updates, events) = sound_typescript::sent_updates_and_events();
    print(
        &format!("{what}: a frame as it plays, every meter moving"),
        playing,
    );
    println!(
        "{what}: {} updates, {} events and {} batches of ops in {} frames ({seconds:.2} s)",
        updates - asked.0,
        events - asked.1,
        sound_typescript::heard_batches() - batches,
        MOVES * 2,
    );
}
