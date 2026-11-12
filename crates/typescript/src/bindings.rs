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
  view: () => h(Steps, { field: "steps" }),
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
fn a_step_turned_sends_how_strong_the_steps_are_and_nothing_else() {
    let program = bun_program().expect("Bun is installed, see tooling/fetch-bun.sh");
    let folder = tempfile::tempdir().unwrap();
    write_sdk(folder.path(), &[]).unwrap();
    std::fs::write(folder.path().join("row.ts"), ROW).unwrap();
    let host = host_script().unwrap();
    let (bun, _) = Bun::start(
        &program,
        &host,
        (folder.path(), new_opening()),
        &["row.ts"],
        false,
    )
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
    let made = next_ops(&bun).into_iter().find_map(|op| match op {
        Op::Create(id, Props::Steps(_)) => Some(id),
        _ => None,
    });
    let steps = made.expect("a row of steps");
    bun.send(&Request::Event {
        card: 1,
        node: steps,
        kind: EventKind::Change,
        payload: json!({ "step": 2 }),
        time: 0.0,
    });
    let ops = next_ops(&bun);
    let [Op::Set(node, Change::Steps(change))] = ops.as_slice() else {
        panic!("one change of the row: {ops:?}");
    };
    assert_eq!(*node, steps);
    let on = "levels: Some(Some([0.0, 0.0, 1.0, 0.0]))";
    let change = format!("{change:?}");
    assert!(
        change.contains(on) && change.matches("Some").count() == 2,
        "{change}"
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
      onClick: () =>
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
    let program = bun_program().expect("Bun is installed, see tooling/fetch-bun.sh");
    let folder = tempfile::tempdir().unwrap();
    write_sdk(folder.path(), &[]).unwrap();
    std::fs::write(folder.path().join("holder.ts"), HOLDER).unwrap();
    let host = host_script().unwrap();
    let (bun, _) = Bun::start(
        &program,
        &host,
        (folder.path(), new_opening()),
        &["holder.ts"],
        false,
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
            Op::Create(id, Props::Div(div)) if div.on_click == Some(true) => Some(id),
            _ => None,
        })
        .unwrap();
    bun.send(&Request::Event {
        card: 1,
        node: button,
        kind: EventKind::Click,
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
      onClick: () => clock.update("Faster", ({ tempo_map }) => { tempo_map.tempo_changes[0].bpm += 10; }),
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
fn mounted(file: &str, code: &str, name: &str) -> Bun {
    let program = bun_program().expect("Bun is installed, see tooling/fetch-bun.sh");
    let folder = tempfile::tempdir().unwrap();
    write_sdk(folder.path(), &[]).unwrap();
    std::fs::write(folder.path().join(file), code).unwrap();
    let host = host_script().unwrap();
    let (bun, _) = Bun::start(
        &program,
        &host,
        (folder.path(), new_opening()),
        &[file],
        false,
    )
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
    bun
}

/// The tempo map of the window reaches a binding that reads it, and an edit of it in code goes
/// out as one song edit.
#[test]
fn a_binding_follows_the_tempo_map_and_code_edits_it() {
    let bun = mounted("tempo.ts", TEMPO, "tempo");
    let button = (next_ops(&bun).into_iter())
        .find_map(|op| match op {
            Op::Create(id, Props::Div(div)) if div.on_click == Some(true) => Some(id),
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
        kind: EventKind::Click,
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
    let bun = mounted("playhead.ts", PLAYHEAD, "playhead");
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

const SHARES: &str = r#"import { number, tool } from "./sdk";

tool({
  name: "shares",
  title: "Shares",
  when: "A test",
  doc: "Numbers from 0 to 1, and one that is not.",
  state: {
    share: number({ min: 0, max: 1, default: 0.5 }),
    seed: number({ min: 0, max: 1, default: 0.5, unit: "plain" }),
    on: number({ min: 0, max: 1, integer: true }),
    turns: number({ min: 0, max: 4 }),
  },
  sound: () => 0,
});
"#;

/// The ops of the view `mounted` mounts, or why it does not draw.
fn first_ops(bun: &Bun) -> Result<Vec<Op>, String> {
    loop {
        match smol::block_on(bun.events().recv()) {
            Ok(Event::Ops { ops, .. }) => return ops,
            Ok(Event::Ended { why, .. }) => panic!("Bun ended: {why}"),
            Ok(_) => {}
            Err(error) => panic!("{error}"),
        }
    }
}

/// The shortest form of a share reads as the cards of the app read theirs: only the knob of
/// `share` says a percent, in the view of a tool that has none.
#[test]
fn a_number_from_0_to_1_reads_as_a_percent_unless_its_field_says_plain() {
    let bun = mounted("shares.ts", SHARES, "shares");
    let ops = format!("{:?}", first_ops(&bun).unwrap());
    // Its readout, and the drag that turns it.
    assert_eq!(ops.matches("Percent").count(), 2, "{ops}");
}

const LOOSE: &str = r#"import { h, Knob, number, Row, tool } from "./sdk";

tool({
  name: "loose",
  title: "Loose",
  when: "A test",
  doc: "A knob with its label in a row.",
  state: { level: number({ min: 0, max: 1 }) },
  sound: () => 0,
  view: () => h(Row, {}, h(Knob, { field: "level" })),
});
"#;

/// A knob that draws its label and value is a cell: in a row by hand it stands off the grid,
/// so its view says where it goes. One that its row names is no cell.
#[test]
fn a_knob_with_its_label_in_a_row_is_told_to_go_in_controls() {
    let bun = mounted("loose.ts", LOOSE, "loose");
    let error = first_ops(&bun).unwrap_err();
    assert!(
        error.contains("put controls in Controls") && error.contains(r#"<Knob field="level" />"#),
        "{error}"
    );
    let named = LOOSE.replace(
        r#"{ field: "level" }"#,
        r#"{ field: "level", text: "value" }"#,
    );
    let bun = mounted("loose.ts", &named, "loose");
    assert!(first_ops(&bun).is_ok());
}

const SCALED: &str = r#"import { Display, h, tool } from "./sdk";

tool({
  name: "scaled",
  title: "Scaled",
  when: "A test",
  doc: "A display with the marks of its scale.",
  state: {},
  sound: () => 0,
  view: () => h(Display, { scale: { x: [0.25, [0.5, "1k"]], y: [0.75] }, zero: 0.5 }),
});
"#;

#[test]
fn a_display_gets_the_marks_of_its_scale() {
    let bun = mounted("scaled.ts", SCALED, "scaled");
    let display = (first_ops(&bun).unwrap().into_iter())
        .find_map(|op| match op {
            Op::Create(_, Props::Display(display)) => Some(display),
            _ => None,
        })
        .expect("a display");
    assert_eq!(
        format!("{:?} {:?}", display.scale, display.zero),
        r#"Some(Scale { x: [Line(0.25), Worded(0.5, "1k")], y: [0.75] }) Some(0.5)"#
    );
}
