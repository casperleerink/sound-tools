//! Built-in devices as the children of a tool of the project, and the SDK of their records.

use serde_json::Value;

use crate::support::{Harness, write};

/// `rig` plays its child `hiss`, which it imports from `HISS`, through its child `room`, the
/// built-in reverb. `alone` names no child.
const TOOLS: &str = r#"import { tool, type Declaration } from "./sdk";
import { hiss } from "./wind";

const children = { hiss, room: "reverb" } as const;

tool({
  name: "rig",
  title: "Rig",
  when: "Never",
  doc: "Plays hiss through room.",
  state: {},
  children,
  behaviour: (): Declaration<typeof children> => ({
    connections: [
      { from: { child: "hiss" }, to: { child: "room" } },
      { from: { child: "room" }, to: { port: "audio" } },
    ],
  }),
});

tool({
  name: "alone",
  title: "Alone",
  when: "Never",
  doc: "Has no children.",
  state: {},
  children: {},
  behaviour: () => ({ connections: [] }),
});
"#;

/// In a file after `TOOLS` in name order, so `rig` loads it first.
const HISS: &str = r#"import { noise, tool } from "./sdk";

export const hiss = tool({
  name: "hiss",
  title: "Hiss",
  when: "Never",
  doc: "Noise.",
  state: {},
  sound: () => noise().times(0.25),
});
"#;

/// A project that plays the instance `top` of `TOOLS` with `records`.
fn project(top: &str, records: &[(&str, &str)]) -> Harness {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = format!(
        r#"{{"format": 1, "extensions": ["reverb"],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{{"from": {{"instance": "{top}", "port": "audio"}}, "to": {{"device_output": 0}}}}]}}"#
    );
    write(root, "project.json", &project);
    write(root, "extensions/tools.ts", TOOLS);
    write(root, "extensions/wind.ts", HISS);
    for (path, record) in records {
        write(root, path, record);
    }
    Harness::open(folder)
}

fn record(tool: &str, state: &str) -> String {
    format!(r#"{{"tool": "{tool}", "state": {state}}}"#)
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn a_built_in_device_plays_as_the_child_of_a_parent() {
    let records = [
        ("state/rig/instance.json", record("rig", "{}")),
        ("state/rig/hiss.json", record("hiss", "{}")),
        ("state/rig/room.json", record("reverb", "{}")),
    ];
    let records: Vec<(&str, &str)> = (records.iter())
        .map(|(path, record)| (*path, record.as_str()))
        .collect();
    let mut harness = project("rig", &records);
    assert_eq!(harness.project.problems(), []);
    let played = harness.play(24_000);
    assert!(rms(&played[2 * 4_800..]) > 0.01, "{}", rms(&played));
}

#[test]
fn a_child_of_another_tool_than_its_parent_names_is_a_problem_of_the_parent() {
    let records = [
        ("state/rig/instance.json", record("rig", "{}")),
        ("state/rig/hiss.json", record("hiss", "{}")),
        ("state/rig/room.json", record("hiss", "{}")),
        ("state/rig/more.json", record("hiss", "{}")),
    ];
    let records: Vec<(&str, &str)> = (records.iter())
        .map(|(path, record)| (*path, record.as_str()))
        .collect();
    let harness = project("rig", &records);
    let problems: Vec<String> = (harness.project.problems().into_iter())
        .map(|problem| format!("{}: {}", problem.path, problem.message))
        .collect();
    // Then what the hiss in place of the room cannot take.
    assert_eq!(
        problems[..2],
        [
            "state/rig/instance.json: its child more is none of its `children`: hiss, room",
            "state/rig/instance.json: its child room is a hiss, and its `children` say reverb",
        ]
    );
}

#[test]
fn a_child_of_a_parent_that_names_none_is_a_problem_of_the_parent() {
    let records = [
        ("state/alone/instance.json", record("alone", "{}")),
        ("state/alone/stray.json", record("hiss", "{}")),
    ];
    let records: Vec<(&str, &str)> = (records.iter())
        .map(|(path, record)| (*path, record.as_str()))
        .collect();
    let harness = project("alone", &records);
    let problems: Vec<String> = (harness.project.problems().into_iter())
        .map(|problem| format!("{}: {}", problem.path, problem.message))
        .collect();
    assert_eq!(
        problems,
        ["state/alone/instance.json: its child stray is none of its `children`, which name none"]
    );
}

/// So an agent reads in `sdk.ts` what each field of a built-in record does: every field has a
/// doc comment, its own or its type's.
#[test]
fn every_field_of_a_built_in_record_has_a_doc() {
    let mut undocumented = Vec::new();
    for tool in runtime::tool_schemas().unwrap() {
        let defs = &tool.schema["$defs"];
        let def = |schema: &Value| {
            let name = schema["$ref"].as_str()?.strip_prefix("#/$defs/")?;
            Some(&defs[name])
        };
        // Every object of the record: its own, and each of its types.
        let objects = [&tool.schema]
            .into_iter()
            .chain(defs.as_object().into_iter().flat_map(|defs| defs.values()));
        for object in objects {
            for (name, property) in object["properties"].as_object().into_iter().flatten() {
                // `Option<T>` is `T` or `null`.
                let types = [property]
                    .into_iter()
                    .chain(property["anyOf"].as_array().into_iter().flatten());
                let documented = types
                    .flat_map(|schema| [Some(schema), def(schema)])
                    .flatten()
                    .any(|schema| schema["description"].is_string());
                if !documented {
                    undocumented.push(format!("{}: {name}", tool.tool));
                }
            }
        }
    }
    assert!(undocumented.is_empty(), "{undocumented:#?}");
}

/// So the docs of a built-in record give the range of every number a lane or a signal moves:
/// the type that holds it has a `ranges` transform with the right parameters.
#[test]
fn every_number_a_lane_moves_has_its_range_in_the_docs() {
    let mut missing = Vec::new();
    for tool in runtime::tool_schemas().unwrap() {
        let record = sound_core::RecordSchema::new(&tool.schema);
        for (number, _) in &tool.numbers {
            let field = record.property(&number.field);
            let range = field.and_then(|field| record.field(&number.field, field).range);
            let ends = range.map(|range| (range.min, range.max));
            let expected = (number.range.min.to_string(), number.range.max.to_string());
            if ends != Some(expected) {
                missing.push(format!("{}: {}", tool.tool, number.field));
            }
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

/// Types an agent's tool must get right, checked with `bun check` next to the SDK of the
/// built-in tools, as the doc for agents says to check a tool.
#[test]
fn the_types_of_the_sdk_refuse_what_an_agent_gets_wrong() {
    let bun = sound_typescript::bun_program().expect("Bun is installed, see tooling/fetch-bun.sh");
    let project = tempfile::tempdir().unwrap();
    let extensions = project.path().join("extensions");
    std::fs::create_dir(&extensions).unwrap();
    let schemas = runtime::tool_schemas().unwrap();
    assert!(sound_typescript::Extensions::load(project.path(), &schemas).is_some());
    write(&extensions, "types.ts", include_str!("sdk-types.ts"));
    // The host checks with the SDK it runs next to.
    write(&extensions, "host.ts", sound_typescript::HOST);
    write(&extensions, "node.d.ts", sound_typescript::HOST_TYPES);
    // And the tools of the DAW template, as a new project from it holds them.
    let daw = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../templates/daw/extensions"
    );
    for file in std::fs::read_dir(daw).unwrap() {
        let file = file.unwrap();
        let text = std::fs::read_to_string(file.path()).unwrap();
        write(&extensions, file.file_name().to_str().unwrap(), &text);
    }
    // And the gallery, which uses every part of the view SDK.
    let gallery = include_str!("../sdk-gallery/extensions/gallery.tsx");
    write(&extensions, "gallery.tsx", gallery);
    let checked = sound_core::process::background_command(bun)
        .args(["check", "-p", "extensions"])
        .current_dir(project.path())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&checked.stdout);
    assert!(checked.status.success(), "{said}");
}
