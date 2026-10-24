//! What a page of many shapes costs on the side of Bun and of its live values: a measurement,
//! not a check. Run with
//! `cargo nextest run -p sound-typescript --cargo-profile release-fast --run-ignored only performance --no-capture`.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sound_core::InstanceId;

use crate::bun::{Bun, Clock, Event, PageSize, Request, Transport, new_opening, record_key};
use crate::feeds::Feeds;
use crate::live::{DragNumbers, Sources};
use crate::tree::{NodeTable, Num, Op};
use crate::{bun_program, host_script, write_sdk};

const SIZES: [usize; 3] = [100, 1_000, 2_000];
const ROUNDS: usize = 30;

/// A page of `count` notes, all in a group the drag `move` moves, as the selected notes of a
/// piano roll are, the first one at the output `v`, so a round moves one note.
const GRID: &str = r##"import { For, Group, h, number, Rect, Space, tool, type Drag } from "./sdk";

tool({
  name: "grid",
  title: "Grid",
  when: "A measurement",
  doc: "Many notes.",
  state: { count: number({ min: 0, max: 100000, default: 100 }) },
  sound: () => 0,
  view: ({ state, outputs, now }) => {
    const notes = () => Array.from({ length: state.count }, (_, index) => ({ index, start: (index % 64) * 240, pitch: 36 + (index % 48) }));
    const move = now.drag("move");
    const drag: Drag = { id: "move", snap: { x: 240, y: 1 }, onEnd: () => {} };
    return h(Space, { key: "roll", grow: true, unit: { x: 0.1, y: 4 }, extent: { x: [0, 38400], y: [0, 128] }, zoom: { x: [0.1, 10] } },
      h(Group, { x: move.dx, y: move.dy },
        h(For<{ index: number; start: number; pitch: number }>, { each: notes }, (note) => h(Rect, {
          x: note.index === 0 ? () => outputs.v ?? 0 : note.start,
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
        transport: Transport::default(),
    }
}

fn print(what: &str, mut times: Vec<Duration>) {
    times.sort();
    let mean = times.iter().sum::<Duration>() / times.len().max(1) as u32;
    let median = times.get(times.len() / 2).copied().unwrap_or_default();
    let worst = times.last().copied().unwrap_or_default();
    println!("{what}: mean {mean:?}, median {median:?}, worst {worst:?}");
}

/// The next ops Bun sends for a card, read, and how long they took to come.
fn next_ops(bun: &Bun, sent: Instant) -> (Duration, Vec<Op>) {
    let events = bun.events();
    loop {
        match smol::block_on(events.recv()) {
            Ok(Event::Ops { ops: Ok(ops), .. }) => return (sent.elapsed(), ops),
            Ok(Event::Ops {
                ops: Err(error), ..
            }) => panic!("{error}"),
            Ok(Event::Ended { why, .. }) => panic!("Bun ended: {why}"),
            Ok(_) => {}
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed times"]
fn performance_of_bun_and_live_values_for_many_shapes() {
    let program = bun_program().expect("Bun is installed, see tooling/fetch-bun.sh");
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
    let size = PageSize {
        width: 1400,
        height: 800,
    };
    for (card, count) in SIZES.into_iter().enumerate() {
        let card = card as u64;
        let state = json!({ "count": count }).to_string();
        let key = record_key("grid", &state);
        assert!(bun.tell([(&id, Some(("grid", key, state.as_str())))]));
        let heard = crate::heard_bytes();
        let watches = BTreeMap::new();
        let sent = Instant::now();
        bun.send(&Request::Mount {
            card,
            instance: &id,
            watches: &watches,
            size,
            clock: clock(),
        });
        let (trip, ops) = next_ops(&bun, sent);
        let bytes = crate::heard_bytes() - heard;
        let started = Instant::now();
        let mut table = NodeTable::default();
        table.apply(ops).unwrap();
        println!(
            "{count} shapes: made in {trip:?} with {bytes} bytes of ops, applied in {:?}, {} nodes",
            started.elapsed(),
            table.len()
        );
        let mut trips = Vec::new();
        let mut applies = Vec::new();
        for round in 1..=ROUNDS {
            let watches = BTreeMap::from([("v".to_string(), round as f32)]);
            let heard = crate::heard_bytes();
            let sent = Instant::now();
            bun.send(&Request::Watches {
                instance: &id,
                watches: &watches,
            });
            let (trip, ops) = next_ops(&bun, sent);
            let started = Instant::now();
            table.apply(ops).unwrap();
            applies.push(started.elapsed());
            trips.push(trip);
            assert!(crate::heard_bytes() - heard < 200);
        }
        print(
            &format!("{count} shapes: a watch moves one note, told to ops read"),
            trips,
        );
        print(&format!("{count} shapes: its ops applied"), applies);
        bun.send(&Request::Drop { card });
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
        hovered: None,
        feeds: &Feeds::default(),
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
