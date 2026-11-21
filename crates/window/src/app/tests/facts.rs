//! Facts: what the window knows of an instance that no record holds, as its views read them.

use std::collections::BTreeSet;

use super::*;

/// The path of each value of `record` the generic card shows a control for, inside `at`: its
/// numbers, switches and choices, also those of groups and of each item of a list or a map.
fn values(field: &Value, record: &Value, at: &str) -> Vec<String> {
    let (inside, at) = match field["name"].as_str() {
        Some("") | None => (record, at.to_string()),
        Some(name) if at.is_empty() => (&record[name], name.to_string()),
        Some(name) => (&record[name], format!("{at}.{name}")),
    };
    let item = |key: String, item: &Value| values(&field["item"], item, &format!("{at}.{key}"));
    match field["kind"].as_str() {
        Some("group") => (field["fields"].as_array().into_iter().flatten())
            .flat_map(|field| values(field, inside, &at))
            .collect(),
        Some("list") => (inside.as_array().into_iter().flatten().enumerate())
            .flat_map(|(index, each)| item(index.to_string(), each))
            .collect(),
        Some("map") => (inside.as_object().into_iter().flatten())
            .flat_map(|(key, each)| item(key.clone(), each))
            .collect(),
        _ => vec![at],
    }
}

#[test]
fn the_generic_card_of_every_built_in_device_has_a_control_for_each_value_of_its_record() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    without_cards(&folder);
    let chain = folder.join("state/song/t00/chain");
    let tools: Vec<String> = (sound_typescript::devices::built_in().into_iter())
        .map(|offer| offer.key)
        .collect();
    let names: Vec<String> = (0..tools.len()).map(|at| format!("d{at:02}")).collect();
    for (name, tool) in names.iter().zip(&tools) {
        let record = json!({ "tool": tool, "state": {} }).to_string();
        std::fs::write(chain.join(format!("{name}.json")), record).unwrap();
    }
    let chained = json!({ "tool": "chain", "state": { "order": names } });
    std::fs::write(chain.join("instance.json"), chained.to_string()).unwrap();
    let mut state = opened(&folder);
    let ids: Vec<InstanceId> = (names.iter())
        .map(|name| InstanceId::new(&format!("song/t00/chain/{name}")).unwrap())
        .collect();
    for name in &names {
        let more = format!("t00/chain/{name}:card-more");
        if controls::collect(&state)
            .controls
            .iter()
            .any(|control| control.key == more)
        {
            act(&mut state, json!({ "key": more, "action": "press" }));
        }
    }
    let facts = state.facts_now(&ids.iter().cloned().collect::<BTreeSet<_>>());
    state.settle();
    let keys = |state: &State| -> Vec<String> {
        (controls::collect(state).controls.into_iter())
            .map(|control| control.key)
            .collect()
    };
    for (id, tool) in ids.iter().zip(&tools) {
        let record: Value = serde_json::from_str(&state.project().state_json(id).unwrap()).unwrap();
        let fields = facts[id].to_value()["fields"].clone();
        let paths: Vec<String> = (fields.as_array().into_iter().flatten())
            .flat_map(|field| values(field, &record, ""))
            .collect();
        let prefix = format!("{}:", id.as_str().trim_start_matches("song/"));
        for path in &paths {
            // A knob's or switch's key is its path; a choice side by side has one an option.
            let key = format!("{prefix}{path}");
            let shown = |keys: &[String]| {
                (keys.iter()).any(|shown| *shown == key || shown.starts_with(&format!("{key}-")))
            };
            if !shown(&keys(&state)) {
                // A value of an item of a list or a map shows once the card's chooser picks it.
                let parts: Vec<&str> = path.split('.').collect();
                let chooser = |n: usize| format!("{prefix}{}", parts[..n].join("."));
                let item = |n: usize| format!("{}-{}", chooser(n), parts[n]);
                let at = (1..parts.len())
                    .find(|&n| {
                        let keys = keys(&state);
                        keys.contains(&chooser(n)) || keys.contains(&item(n))
                    })
                    .unwrap_or_else(|| panic!("{tool} has no control of {path}"));
                let item = item(at);
                if !keys(&state).contains(&item) {
                    act(&mut state, json!({ "key": chooser(at), "action": "press" }));
                    state.settle();
                }
                act(&mut state, json!({ "key": item, "action": "press" }));
                state.settle();
            }
            assert!(shown(&keys(&state)), "{tool} has no control of {path}");
        }
        if tool == "eq" {
            assert!(paths.contains(&"bands.3.gain_db".to_string()), "{paths:?}");
        }
    }
}

/// A tool whose view shows the file of a region that is a grandchild: its facts.
const PEEK: &str = r#"import { h, tool } from "./sdk";
export const peek = tool({
  name: "peek", title: "Peek", when: "Never", doc: "Peeks.", state: {},
  behaviour: () => ({ connections: [{ from: { port: "audio" }, to: { port: "audio" } }] }),
  view: (self) => {
    const region = self.children[0]?.children[0];
    const file = region?.tool === "region" ? region.facts.file.state : "none";
    return <box key="file" label={file} onPress={() => {}} />;
  },
});
"#;

#[test]
fn a_view_runs_again_when_a_fact_of_a_grandchild_changes_and_not_while_none_does() {
    let (_scratch, folder) = with_tools(&[("peek", PEEK)]);
    let chain = folder.join("state/song/t00/chain");
    std::fs::remove_file(chain.join("peek.json")).unwrap();
    std::fs::create_dir_all(chain.join("peek/box")).unwrap();
    let files = [
        ("peek/instance.json", json!({ "tool": "peek", "state": {} })),
        (
            "peek/box/instance.json",
            json!({ "tool": "chain", "state": {} }),
        ),
        (
            "peek/box/late.json",
            json!({ "tool": "region", "state": { "asset": "late.wav" } }),
        ),
    ];
    for (name, record) in files {
        std::fs::write(chain.join(name), record.to_string()).unwrap();
    }
    let mut state = opened(&folder);
    let file = |state: &State| match state.drawn(&InstanceId::new("song/t00/chain/peek").unwrap()) {
        Some(Drawn::Failed(why)) => why.clone(),
        _ => super::labels(state).join(" "),
    };
    assert!(file(&state).contains("missing"), "{}", file(&state));
    let runs = state.tools.as_ref().unwrap().view_runs();
    for step in 1..=60 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    assert_eq!(state.tools.as_ref().unwrap().view_runs(), runs);
    let audio = state.project().root().join("assets/audio");
    std::fs::create_dir_all(&audio).unwrap();
    super::host::wav(&audio.join("late.wav"));
    // As the watcher of a window tells it.
    let arrived = [audio.join("late.wav")];
    (state.session)
        .background(|project| project.apply_outside_changes(&arrived))
        .unwrap();
    for step in 61..=180 {
        frame(&mut state, f64::from(step) / 60.0);
        state.settle();
    }
    assert!(file(&state).contains("plays"), "{}", file(&state));
    assert!(state.tools.as_ref().unwrap().view_runs() > runs);
}

/// The input of a test: its levels, and a take of notes that began at 960.
struct Recorded(Vec<f32>, runtime::input::TakeNotes);

impl crate::feeds::Recording for Recorded {
    fn levels(&self) -> &[f32] {
        &self.0
    }
    fn notes(&self, _: &InstanceId) -> Option<&runtime::input::TakeNotes> {
        Some(&self.1)
    }
    fn began(&self, _: &InstanceId) -> Option<(sound_core::Ticks, f64)> {
        Some((self.1.start, 2.0))
    }
    /// The seconds after the take's start in the file each edge is at, to see where they fall.
    fn peaks(&self, _: &InstanceId, edges: &[f64]) -> Option<Vec<f32>> {
        Some(
            edges
                .iter()
                .map(|seconds| (*seconds - 2.0) as f32)
                .collect(),
        )
    }
}

#[test]
fn a_recorder_shows_its_input_its_take_and_when_it_began() {
    use runtime::input::{TakeNote, TakeNotes};
    use sound_core::Ticks;
    use sound_core::feeds::{Feed, FeedRequest, Readings, read};
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let rec = json!({ "tool": "recorder", "state": { "channels": [1] } });
    std::fs::write(folder.join("state/song/t00/rec.json"), rec.to_string()).unwrap();
    let mut state = opened(&folder);
    state.playhead = Ticks(1920);
    let id = InstanceId::new("song/t00/rec").unwrap();
    let held = TakeNote {
        pitch: 62,
        start: Ticks(1200),
        end: None,
    };
    let ended = TakeNote {
        pitch: 60,
        start: Ticks(960),
        end: Some(Ticks(1440)),
    };
    let take = TakeNotes {
        recorder: id.clone(),
        start: Ticks(960),
        notes: vec![ended, held],
    };
    let recorded = Recorded(vec![0.25, 0.5], take);
    let sources = state.recorded(&recorded);
    let project = state.project();
    let feed = |name: &str, points: usize| {
        let request = FeedRequest {
            tick: Ticks(1920),
            points,
            range: None,
        };
        read(project, &Readings::new(), &sources, &id, name, request)
    };
    assert_eq!(feed("input", 0), Some(Feed::Number(0.5)));
    // A held note lasts up to now.
    let notes = [0.0, 60.0, 480.0, 1.0, 240.0, 62.0, 720.0, 1.0];
    assert_eq!(feed("notes", 0), Some(Feed::List(notes.to_vec())));
    // The take began at the second 2 of its file; a quarter note at 120 bpm is half a second.
    // Two columns of the 960 ticks so far, each as tall as the second its left edge is after
    // the take began, about the middle.
    let band = [0.0, 0.5, 480.0, 0.0, 480.0, 0.375, 480.0, 0.25];
    assert_eq!(feed("take", 2), Some(Feed::List(band.to_vec())));
    let facts = sound_core::facts::of(project, &sources, &id, None).unwrap();
    assert_eq!(facts.to_value()["taking"], json!({ "from": 960 }));
}
