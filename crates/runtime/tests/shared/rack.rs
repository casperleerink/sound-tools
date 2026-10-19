//! A host for the built-in devices, written with the SDK as any tool of a project: a `rack` plays
//! its timelines and what reaches its notes on its child `instrument`, then through the children
//! its `order` names, and shows the card of each. Each of its `automation` moves a number of a
//! child with a `lane` child, as a track of the DAW template does. The tests of a device's card
//! and sound put the device in one.

use serde_json::json;

pub(crate) const RACK: &str = r#"import { DeviceCard, For, h, list, text, tool, type Connection, type Declaration } from "./sdk";

tool({
  name: "rack",
  title: "Rack",
  when: "Never: the tests' host of built-in devices",
  doc: "Plays its timelines and what reaches its notes on `instrument`, then through the children `order` names, and shows the card of each. Each of `automation` moves the number `target` of a child, such as `filter.cutoff_hz`, with the lane child `lane`.",
  state: { order: list(text()), automation: list({ lane: text(), target: text() }) },
  behaviour: ({ state, children }): Declaration => {
    const has = (name: string) => children.some((child) => child.name === name);
    const connections: Connection[] = children
      .filter((child) => child.tool === "timeline")
      .map((clip) => ({ from: { child: clip.name, notes: "notes" }, to: { child: "instrument", notes: "notes" } }));
    const chain = [...(has("instrument") ? ["instrument"] : []), ...state.order.filter(has)];
    if (has("instrument")) connections.push({ from: { notes: "notes" }, to: { child: "instrument", notes: "notes" } });
    else if (chain.length > 0) connections.push({ from: { port: "audio" }, to: { child: chain[0]! } });
    chain.slice(1).forEach((name, index) => connections.push({ from: { child: chain[index]! }, to: { child: name } }));
    if (chain.length > 0) connections.push({ from: { child: chain[chain.length - 1]! }, to: { port: "audio" } });
    for (const { lane, target } of state.automation.filter(({ lane }) => has(lane))) {
      const dot = target.lastIndexOf(".");
      connections.push({ from: { child: lane, number: "value" }, to: { child: target.slice(0, dot), number: target.slice(dot + 1) } });
    }
    return { connections };
  },
  view: ({ state, children }) => {
    const shown = () => [...(children.some((child) => child.name === "instrument") ? ["instrument"] : []), ...state.order];
    return (
      <div style={{ gap: 8, padding: 8, align: "start" }}>
        <For each={shown}>
          {(name) => (
            <div key={`card-${name}`}>
              <DeviceCard path={name} />
            </div>
          )}
        </For>
      </div>
    );
  },
});
"#;

/// The files of a project that plays a `rack` of `children`, each a name and its record, into
/// the device. Its `order` is every child but `instrument`, the timelines and the lanes, as
/// given.
pub(crate) fn rack_files(children: &[(&str, &str)]) -> Vec<(String, String)> {
    let project = json!({
        "format": 1,
        "extensions": runtime::bundled_extensions().unwrap(),
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [{ "from": { "instance": "rack", "port": "audio" }, "to": { "device_output": 0 } }],
        "main": "rack",
    });
    let order: Vec<&str> = (children.iter())
        .filter(|(name, record)| *name != "instrument" && !plays_no_sound(record))
        .map(|(name, _)| *name)
        .collect();
    let mut files = vec![
        ("project.json".to_string(), project.to_string()),
        ("extensions/rack.tsx".to_string(), RACK.to_string()),
        (
            "state/rack/instance.json".to_string(),
            json!({ "tool": "rack", "state": { "order": order } }).to_string(),
        ),
    ];
    for (name, record) in children {
        files.push((format!("state/rack/{name}.json"), record.to_string()));
    }
    files
}

/// Whether a record is of a timeline or a lane, which moves another child and has no card.
pub(crate) fn plays_no_sound(record: &str) -> bool {
    let record: serde_json::Value = serde_json::from_str(record).unwrap();
    matches!(record["tool"].as_str(), Some("timeline" | "lane"))
}

/// The record of a built-in tool.
pub(crate) fn record(tool: &str, state: &str) -> String {
    format!(r#"{{"tool": "{tool}", "state": {state}}}"#)
}

/// A timeline of (start, length, pitch) notes at velocity 100, from tick 0 for `length` ticks.
pub(crate) fn notes(length: u64, notes: &[(u64, u64, u8)]) -> String {
    let notes: Vec<_> = (notes.iter())
        .map(|(start, length, pitch)| json!({ "start": start, "length": length, "pitch": pitch, "velocity": 100 }))
        .collect();
    json!({ "tool": "timeline", "state": { "length": length, "notes": notes } }).to_string()
}
