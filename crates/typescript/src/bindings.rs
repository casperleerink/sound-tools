//! A card's code in Bun, driven as the window drives it: what a press changes crosses as the
//! props it changes, and no more.

use std::collections::BTreeMap;

use serde_json::json;
use sound_core::InstanceId;

use crate::bun::{Bun, Clock, Event, EventKind, Request, Transport, new_opening, record_key};
use crate::tree::{Change, Op, Props};
use crate::{bun_program, host_script, write_sdk};

const ROW: &str = r#"import { h, pattern, Steps, tool } from "./sdk";

tool({
  name: "row",
  title: "Row",
  when: "A test",
  doc: "A row of steps.",
  state: { steps: pattern({ length: 4, min: 0, max: 1, default: 0 }) },
  sound: () => 0,
  view: () => h(Steps, { path: "steps" }),
});
"#;

fn next_ops(bun: &Bun) -> Vec<Op> {
    loop {
        match smol::block_on(bun.events().recv()) {
            Ok(Event::Ops { ops, .. }) => return ops.unwrap(),
            Ok(Event::Ended { why, .. }) => panic!("Bun ended: {why}"),
            Ok(_) => {}
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
fn a_press_on_a_step_sends_its_colour_and_nothing_else() {
    let Some(program) = bun_program() else {
        return eprintln!("skipped: Bun is not installed");
    };
    let folder = tempfile::tempdir().unwrap();
    write_sdk(folder.path(), &[]).unwrap();
    std::fs::write(folder.path().join("row.ts"), ROW).unwrap();
    let host = host_script().unwrap();
    let (bun, _) = Bun::start(&program, &host, (folder.path(), new_opening()), &["row.ts"])
        .unwrap_or_else(|failure| panic!("{}", failure.why));
    let id = InstanceId::new("row").unwrap();
    let state = json!({ "steps": [0, 0, 0, 0] }).to_string();
    assert!(bun.tell([(
        &id,
        Some(("row", record_key("row", &state), state.as_str()))
    )]));
    let transport = Transport::default();
    let clock = Clock {
        time: 0.0,
        transport,
    };
    let watches = BTreeMap::new();
    bun.send(&Request::Mount {
        card: 1,
        instance: &id,
        watches: &watches,
        size: crate::bun::PageSize {
            width: 168,
            height: 144,
        },
        clock,
    });
    let steps: Vec<u64> = (next_ops(&bun).into_iter())
        .filter_map(|op| match op {
            Op::Create(id, Props::Shape(shape)) if shape.on_press == Some(true) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(steps.len(), 4);
    bun.send(&Request::Event {
        card: 1,
        node: steps[2],
        kind: EventKind::Press,
        payload: json!({ "x": 0, "y": 0, "button": "left", "clicks": 1, "shift": false, "alt": false, "cmd": false, "ctrl": false }),
        time: 0.0,
    });
    let ops = next_ops(&bun);
    let [Op::Set(node, Change::Shape(change))] = ops.as_slice() else {
        panic!("one change of one step: {ops:?}");
    };
    assert_eq!(*node, steps[2]);
    assert!(
        format!("{change:?}").contains("color: Some(Some(Fixed(Color("),
        "{change:?}"
    );
}

const HOLDER: &str = r#"import { h, tool } from "./sdk";

tool({
  name: "holder",
  title: "Holder",
  when: "A test",
  doc: "Moves its timeline on.",
  state: {},
  children: { clip: "timeline" },
  behaviour: () => ({ connections: [] }),
  view: ({ update }) =>
    h("div", {
      onPress: () =>
        update("Move it on", ({ children }) => {
          const clip = children[0];
          if (clip?.tool === "timeline") clip.state.start = clip.state.start + 960;
        }),
    }, "On"),
});
"#;

/// A child's record that leaves its fields out reads them at their defaults in an `update`, and
/// the edit sets only what the change changed.
#[test]
fn an_update_reads_the_defaults_of_a_child_and_sets_only_what_it_changed() {
    let Some(program) = bun_program() else {
        return eprintln!("skipped: Bun is not installed");
    };
    let folder = tempfile::tempdir().unwrap();
    write_sdk(folder.path(), &[]).unwrap();
    std::fs::write(folder.path().join("holder.ts"), HOLDER).unwrap();
    let host = host_script().unwrap();
    let (bun, _) = Bun::start(
        &program,
        &host,
        (folder.path(), new_opening()),
        &["holder.ts"],
    )
    .unwrap_or_else(|failure| panic!("{}", failure.why));
    let (id, clip) = (
        InstanceId::new("holder").unwrap(),
        InstanceId::new("holder/clip").unwrap(),
    );
    let (own, empty) = ("{}".to_string(), "{}".to_string());
    assert!(bun.tell([
        (
            &id,
            Some(("holder", record_key("holder", &own), own.as_str()))
        ),
        (
            &clip,
            Some(("timeline", record_key("timeline", &empty), empty.as_str()))
        ),
    ]));
    let clock = Clock {
        time: 0.0,
        transport: Transport::default(),
    };
    let watches = BTreeMap::new();
    bun.send(&Request::Mount {
        card: 1,
        instance: &id,
        watches: &watches,
        size: crate::bun::PageSize {
            width: 168,
            height: 144,
        },
        clock,
    });
    let button = (next_ops(&bun).into_iter())
        .find_map(|op| match op {
            Op::Create(id, Props::Div(div)) if div.on_press == Some(true) => Some(id),
            _ => None,
        })
        .unwrap();
    bun.send(&Request::Event {
        card: 1,
        node: button,
        kind: EventKind::Press,
        payload: json!({ "x": 0, "y": 0, "button": "left", "clicks": 1, "shift": false, "alt": false, "cmd": false, "ctrl": false }),
        time: 0.0,
    });
    let changes = loop {
        match smol::block_on(bun.events().recv()) {
            Ok(Event::Edit { changes, .. }) => break changes,
            Ok(Event::Ended { why, .. }) => panic!("Bun ended: {why}"),
            Ok(Event::Failed { message, .. }) => panic!("{message}"),
            Ok(_) => {}
            Err(error) => panic!("{error}"),
        }
    };
    let [
        crate::bun::RecordEdit::Fields {
            child,
            set,
            removed,
        },
    ] = changes.as_slice()
    else {
        panic!("one change of the clip: {changes:?}");
    };
    assert_eq!(child.as_deref(), Some("clip"));
    assert_eq!(
        serde_json::Value::Object(set.clone()),
        json!({ "start": 960 })
    );
    assert!(removed.is_empty(), "{removed:?}");
}

const TEMPO: &str = r#"import { h, tool } from "./sdk";

tool({
  name: "tempo",
  title: "Tempo",
  when: "A test",
  doc: "Shows the tempo and makes it faster.",
  state: {},
  sound: () => 0,
  view: ({ clock }) =>
    h("div", {
      onPress: () => clock.update("Faster", ({ tempo_map }) => { tempo_map.tempo_changes[0].bpm += 10; }),
    }, () => `${clock.tempo_map.tempo_changes[0]?.bpm} bpm`),
});
"#;

const PLAYHEAD: &str = r#"import { h, tool } from "./sdk";

tool({
  name: "playhead",
  title: "Playhead",
  when: "A test",
  doc: "Follows the playhead, which no binding may.",
  state: {},
  sound: () => 0,
  view: ({ clock }) => h("div", {}, () => String(clock.tick)),
});
"#;

/// Starts Bun with one tool and mounts its view, which reads no record.
fn mounted(file: &str, code: &str, name: &str) -> Option<Bun> {
    let program = bun_program()?;
    let folder = tempfile::tempdir().unwrap();
    write_sdk(folder.path(), &[]).unwrap();
    std::fs::write(folder.path().join(file), code).unwrap();
    let host = host_script().unwrap();
    let (bun, _) = Bun::start(&program, &host, (folder.path(), new_opening()), &[file])
        .unwrap_or_else(|failure| panic!("{}", failure.why));
    // Bun reads its files at the start; the folder may go.
    drop(folder);
    let id = InstanceId::new(name).unwrap();
    let state = "{}".to_string();
    assert!(bun.tell([(&id, Some((name, record_key(name, &state), state.as_str())))]));
    let clock = Clock {
        time: 0.0,
        transport: Transport::default(),
    };
    bun.send(&Request::Mount {
        card: 1,
        instance: &id,
        watches: &BTreeMap::new(),
        size: crate::bun::PageSize {
            width: 168,
            height: 144,
        },
        clock,
    });
    Some(bun)
}

/// The tempo map of the window reaches a binding that reads it, and an edit of it in code goes
/// out as one song edit.
#[test]
fn a_binding_follows_the_tempo_map_and_code_edits_it() {
    let Some(bun) = mounted("tempo.ts", TEMPO, "tempo") else {
        return eprintln!("skipped: Bun is not installed");
    };
    let button = (next_ops(&bun).into_iter())
        .find_map(|op| match op {
            Op::Create(id, Props::Div(div)) if div.on_press == Some(true) => Some(id),
            _ => None,
        })
        .unwrap();
    let tempo_map = sound_core::TempoMap::constant(
        sound_core::TimeSignature::default(),
        sound_core::Tempo::from_bpm(90.0).unwrap(),
    );
    bun.send(&Request::Song {
        tempo_map: &tempo_map,
        sections: &sound_core::Sections::default(),
    });
    let ops = format!("{:?}", next_ops(&bun));
    assert!(ops.contains("90 bpm"), "{ops}");
    bun.send(&Request::Event {
        card: 1,
        node: button,
        kind: EventKind::Press,
        payload: json!({ "x": 0, "y": 0, "button": "left", "clicks": 1, "shift": false, "alt": false, "cmd": false, "ctrl": false }),
        time: 0.0,
    });
    let edited = loop {
        match smol::block_on(bun.events().recv()) {
            Ok(Event::SongEdit { tempo_map, .. }) => break tempo_map,
            Ok(Event::Ended { why, .. }) => panic!("Bun ended: {why}"),
            Ok(Event::Failed { message, .. }) => panic!("{message}"),
            Ok(_) => {}
            Err(error) => panic!("{error}"),
        }
    };
    assert_eq!(edited["tempo_changes"][0]["bpm"], 100.0, "{edited}");
}

/// The playhead moves on every frame: a binding that reads it fails with what to do instead.
#[test]
fn a_binding_that_reads_the_playhead_fails_with_what_to_do() {
    let Some(bun) = mounted("playhead.ts", PLAYHEAD, "playhead") else {
        return eprintln!("skipped: Bun is not installed");
    };
    let error = loop {
        match smol::block_on(bun.events().recv()) {
            Ok(Event::Ops { ops, .. }) => break ops.unwrap_err(),
            Ok(Event::Ended { why, .. }) => panic!("Bun ended: {why}"),
            Ok(_) => {}
            Err(error) => panic!("{error}"),
        }
    };
    assert!(error.contains("show it with now.tick"), "{error}");
}
