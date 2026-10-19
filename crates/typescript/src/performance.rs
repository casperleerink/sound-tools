//! What a page of many shapes costs on the side of Bun and of its live values: a measurement,
//! not a check. Run with
//! `cargo nextest run -p sound-typescript --cargo-profile release-fast --run-ignored only performance --no-capture`.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sound_core::InstanceId;

use crate::bun::{Bun, Clock, Drawn, Event, PageSize, Request, Transport, new_opening, record_key};
use crate::feeds::Feeds;
use crate::live::{DragNumbers, Sources};
use crate::tree::{Node, Num};
use crate::{bun_program, host_script, write_sdk};

const SIZES: [usize; 3] = [100, 1_000, 2_000];
const ROUNDS: usize = 30;

/// A page of `count` notes, all in a group the drag `move` moves, as the selected notes of a
/// piano roll are, the first one at the watch `v` so every draw is a new tree; and a canvas
/// whose drag moves the first note through the code, as a drag did before live values.
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
  page: ({ state, watches, memory, now }) => {
    const notes = Array.from({ length: state.count }, (_, index) => ({ start: (index % 64) * 240, pitch: 36 + (index % 48) }));
    if (state.canvas === 1) {
      const shapes: Shape[] = notes.map((note, index) => ({
        kind: "rect", x: (index === 0 ? memory.x + (watches.v ?? 0) : note.start) / 10, y: note.pitch * 4, width: 20, height: 4, color: "#a9b1ff",
      }));
      return h(Canvas, { width: 800, height: 600, shapes, onDrag: (x) => { memory.x = x * 8000; } });
    }
    const move = now.drag("move");
    const drag: Drag = { id: "move", snap: { x: 240, y: 1 }, onEnd: () => {} };
    return h(Space, { key: "roll", grow: true, unit: { x: 0.1, y: 4 }, extent: { x: [0, 38400], y: [0, 128] }, zoom: { x: [0.1, 10] } },
      h(Group, { x: move.dx, y: move.dy },
        notes.map((note, index) => h(Rect, {
          key: String(index),
          x: index === 0 ? (watches.v ?? 0) : note.start,
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

fn clock() -> Clock {
    Clock {
        time: 0.0,
        transport: Transport {
            playing: false,
            tick: 0,
            bar: 1,
            beat: 1,
            bpm: 120.0,
        },
    }
}

fn print(what: &str, mut times: Vec<Duration>) {
    times.sort();
    let mean = times.iter().sum::<Duration>() / times.len().max(1) as u32;
    let median = times.get(times.len() / 2).copied().unwrap_or_default();
    let worst = times.last().copied().unwrap_or_default();
    println!("{what}: mean {mean:?}, median {median:?}, worst {worst:?}");
}

/// The next tree Bun sends, read into nodes, how long it took, and its version.
fn next_tree(bun: &Bun, sent: Instant) -> (Duration, Node, u64) {
    let events = bun.events();
    loop {
        match smol::block_on(events.recv()) {
            Ok(Event::Tree {
                tree: Ok((version, Drawn(tree))),
                ..
            }) => return (sent.elapsed(), tree.unwrap(), version),
            Ok(Event::Tree {
                tree: Err(error), ..
            }) => panic!("{error}"),
            Ok(Event::Unchanged { .. }) => panic!("the same tree again: nothing to measure"),
            Ok(Event::Ended { why, .. }) => panic!("Bun ended: {why}"),
            Ok(_) => {}
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_bun_and_live_values_for_many_shapes() {
    let Some(program) = bun_program() else {
        return eprintln!("skipped: Bun is not installed");
    };
    let folder = tempfile::tempdir().unwrap();
    write_sdk(folder.path(), &[]).unwrap();
    std::fs::write(folder.path().join("grid.ts"), GRID).unwrap();
    let host = host_script().unwrap();
    let (bun, _) = Bun::start(
        &program,
        &host,
        (folder.path(), new_opening()),
        &["grid.ts"],
    )
    .unwrap_or_else(|failure| panic!("{}", failure.why));
    let id = InstanceId::new("grid").unwrap();
    let page = Some(PageSize {
        width: 1400,
        height: 800,
    });
    for count in SIZES {
        for canvas in [0, 1] {
            let state = json!({ "count": count, "canvas": canvas }).to_string();
            let key = record_key("grid", &state);
            assert!(bun.tell([(&id, Some(("grid", key, state.as_str())))]));
            let mut trips = Vec::new();
            let mut bytes = 0;
            let mut tree = None;
            for round in 0..ROUNDS {
                let watches = BTreeMap::from([("v".to_string(), round as f32)]);
                let card = 1;
                let heard = crate::heard_tree_bytes();
                let sent = Instant::now();
                bun.send(&Request::Render {
                    card,
                    instance: &id,
                    watches: &watches,
                    page,
                    clock: clock(),
                });
                let (trip, drawn, version) = next_tree(&bun, sent);
                bytes = crate::heard_tree_bytes() - heard;
                trips.push(trip);
                tree = Some((drawn, version));
            }
            let kind = if canvas == 1 { "canvas" } else { "space" };
            println!("{count} shapes, {kind}: a tree is {bytes} bytes");
            print(
                &format!("{count} shapes, {kind}: render asked to tree read into nodes"),
                trips,
            );
            if canvas == 1 {
                // What one drag step cost before live values: Bun runs `onDrag` and draws again.
                let (tree, mut version) = tree.unwrap();
                let Node::Space(space) = tree else {
                    panic!("a canvas is a space");
                };
                let handler = space.drag.and_then(|drag| drag.on_move).unwrap();
                let mut steps = Vec::new();
                for step in 1..=ROUNDS {
                    let sent = Instant::now();
                    bun.send(&Request::Event {
                        card: 1,
                        version,
                        handler,
                        payload: json!({ "startX": 0, "startY": 0, "dx": step, "dy": 0, "x": step, "y": 0 }),
                        time: 0.0,
                    });
                    let (trip, _, drawn) = next_tree(&bun, sent);
                    version = drawn;
                    steps.push(trip);
                }
                print(
                    &format!(
                        "{count} shapes, canvas: one drag step through Bun, move to tree here"
                    ),
                    steps,
                );
            }
        }
    }
    live_values();
}

/// What working out the live values of a frame costs: one per number of every shape.
fn live_values() {
    let moved: Num = serde_json::from_value(json!({
        "op": "plus",
        "a": { "op": "snap", "a": { "op": "drag", "id": "move", "part": "dx" }, "b": 240 },
        "b": 960,
    }))
    .unwrap();
    let watches = BTreeMap::new();
    let spaces = HashMap::new();
    let numbers = DragNumbers {
        dx: 300.0,
        active: true,
        ..DragNumbers::default()
    };
    let sources = Sources {
        tick: 0.0,
        watches: &watches,
        state: &Value::Null,
        info: None,
        played: None,
        drag: Some(("move", numbers)),
        spaces: &spaces,
        feeds: &Feeds::default(),
        scale: (1.0, 1.0),
    };
    for count in SIZES {
        // Two bound numbers a shape, as a moved note's x and y.
        let bindings = vec![moved.clone(); count * 2];
        let mut frames = Vec::new();
        for _ in 0..1_000 {
            let started = Instant::now();
            let total: f64 = bindings.iter().map(|binding| sources.number(binding)).sum();
            frames.push(started.elapsed());
            assert!(total > 0.0);
        }
        print(&format!("{} live values a frame", count * 2), frames);
    }
}
