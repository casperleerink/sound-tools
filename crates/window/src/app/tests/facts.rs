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
    let keys: Vec<String> = (controls::collect(&state).controls.into_iter())
        .map(|control| control.key)
        .collect();
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
            let shown =
                (keys.iter()).any(|shown| *shown == key || shown.starts_with(&format!("{key}-")));
            assert!(shown, "{tool} has no control of {path}");
        }
        if tool == "eq" {
            assert!(paths.contains(&"bands.3.gain_db".to_string()), "{paths:?}");
        }
    }
}
