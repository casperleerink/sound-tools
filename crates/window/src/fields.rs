//! What the card of a device without a view of its own shows, from the JSON Schema of its
//! record: its title, and a control per field, a number with its range and unit, a switch, or
//! a choice of its options. Bun draws it with the project's `ui/blocks/generic-card.tsx`; values
//! come from the record there.

use serde_json::{Value, json};
use sound_core::{InstanceId, RecordSchema};

use crate::app::State;

/// The card of `id`: `{ title, fields }`, as `Facts` of `sdk/view.ts` types it. None for
/// an instance whose tool has no schema.
pub(crate) fn card_of(state: &State, id: &InstanceId) -> Option<Value> {
    let tool = state.project().tool_of(id)?;
    let schema = state.schema(tool)?;
    let title = (schema.get("x-title").and_then(Value::as_str)).map_or_else(
        || words(tool.rsplit('.').next().unwrap_or(tool)),
        str::to_string,
    );
    let reader = RecordSchema::new(&schema);
    let properties = (reader.resolved(&schema).get("properties")).and_then(Value::as_object)?;
    let fields: Vec<Value> = (properties.iter())
        .filter_map(|(name, property)| field(name, reader.resolved(property)))
        .collect();
    Some(json!({ "title": title, "fields": fields }))
}

/// A field `name` of a record, whose schema is `property`, as its card shows it.
fn field(name: &str, property: &Value) -> Option<Value> {
    let label =
        (property.get("title").and_then(Value::as_str)).map_or_else(|| words(name), str::to_string);
    let number = |key| property.get(key).and_then(Value::as_f64);
    let options: Vec<Value> = match (property.get("enum"), property.get("oneOf")) {
        (Some(Value::Array(options)), _) => options.clone(),
        (None, Some(Value::Array(variants))) => (variants.iter())
            .filter_map(|variant| variant.get("const").cloned())
            .collect(),
        _ => Vec::new(),
    };
    let default = property.get("default").cloned();
    let kind = match (
        property.get("type").and_then(Value::as_str),
        number("minimum"),
        number("maximum"),
    ) {
        _ if !options.is_empty() => {
            let options: Vec<Value> = (options.iter())
                .map(|option| {
                    let text = option.as_str().map_or_else(|| option.to_string(), words);
                    json!({ "value": option, "text": text })
                })
                .collect();
            let first = options.first().map(|option| option["value"].clone());
            json!({ "kind": "choice", "options": options, "default": default.or(first) })
        }
        (Some(kind @ ("number" | "integer")), Some(min), Some(max)) => {
            let log =
                property.get("x-scale").and_then(Value::as_str) == Some("logarithmic") && min > 0.0;
            // A tool of the project declares its unit; a built-in one's is read from its name.
            let unit = match property.get("x-unit") {
                Some(declared) => declared.clone(),
                None => json!(sound_typescript::live::field_unit(name, min, max)),
            };
            json!({
                "kind": "number", "min": min, "max": max, "log": log,
                "integer": kind == "integer", "unit": unit, "default": default.unwrap_or(json!(min)),
            })
        }
        (Some("boolean"), ..) => {
            json!({ "kind": "switch", "default": default.unwrap_or(json!(false)) })
        }
        _ => return None,
    };
    let mut field = json!({ "name": name, "label": label });
    field.as_object_mut()?.extend(kind.as_object()?.clone());
    Some(field)
}

/// `cutoff_hz` as `Cutoff`: a field's name in words, without its unit.
fn words(name: &str) -> String {
    const UNITS: [&str; 8] = [
        "hz",
        "db",
        "ms",
        "s",
        "seconds",
        "pct",
        "semitones",
        "cents",
    ];
    let mut parts: Vec<&str> = name
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() > 1 && parts.last().is_some_and(|last| UNITS.contains(last)) {
        parts.pop();
    }
    let joined = parts.join(" ");
    let mut letters = joined.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}
