//! The part of `sdk.ts` the runtime writes: the record of each built-in tool as a TypeScript
//! type, made from the JSON Schema of its Rust type, the `Tools` that names them, and the
//! `devices` a sound plays through. A doc comment of a field is its TSDoc, with its range and
//! default from its `Parameter`, so the SDK never says other than the Rust check.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use sound_core::{ParameterInfo, Scale, ToolSchema};

use crate::sequencer;

/// The deepest a type nests, so a schema that names itself still ends.
const DEEPEST: usize = 12;
/// A default longer than this, such as a list of bands, is left out of a doc.
const LONGEST_DEFAULT: usize = 48;

/// The runtime's part of `sdk.ts` for the tools of `schemas` and the `sequencer`.
pub(crate) fn sdk_records(schemas: &[ToolSchema]) -> String {
    let sequencer = sequencer::schema();
    let mut tools: Vec<&ToolSchema> = schemas.iter().chain([&sequencer]).collect();
    tools.sort_by_key(|schema| schema.tool);
    let mut text = String::from(
        "
// ---------------------------------------------------------------------------------------------
// Built-in tools: their records, from their Rust types, and the devices a sound plays through.
",
    );
    for schema in &tools {
        let numbers: BTreeMap<String, &(ParameterInfo, f32)> = (schema.numbers.iter())
            .map(|number| (normalized(&number.0.field), number))
            .collect();
        let writer = Writer {
            defs: schema.schema.get("$defs").and_then(Value::as_object),
            numbers,
        };
        let body = writer.object_or_type(&schema.schema, "", 0);
        text.push_str(&format!(
            "\n/** The `state` of a record of the built-in `{}`. */\nexport type {} = {body};\n",
            schema.tool,
            type_name(schema.tool),
        ));
    }
    text.push_str(
        "\n/**\n * Every built-in tool by its name: what its records hold, the numbers a cable moves (`number`),\n * and the live controls (`live`) and triggers (`trigger`) a parent plays.\n */\nexport interface Tools {\n",
    );
    for schema in &tools {
        let names = |names: Vec<String>| match names.is_empty() {
            true => "never".to_string(),
            false => names.join(" | "),
        };
        let number = names(
            schema
                .numbers
                .iter()
                .map(|(info, _)| quoted(&info.field))
                .collect(),
        );
        let live = names(match schema.tool == sequencer::NAME {
            true => vec![quoted(sequencer::LAUNCH)],
            false => Vec::new(),
        });
        text.push_str(&format!(
            "  {}: {{ state: {}; number: {number}; live: {live}; trigger: never }};\n",
            property_name(schema.tool),
            type_name(schema.tool),
        ));
    }
    text.push_str("}\n");
    text.push_str(&sdk_devices(&tools));
    text
}

/// `devices` as the SDK offers them: a knob for each number a signal moves, in its units.
fn sdk_devices(tools: &[&ToolSchema]) -> String {
    let mut text = String::from(
        "
/**
 * The built-in devices a sound plays through: `devices.reverb(input, { mix: 0.3 })`. Each of
 * `fields` takes a signal or a number; the rest of its record is written as it is. Not in an
 * instrument, and not inside a `feedback`.
 */
export const devices = {
",
    );
    for schema in tools.iter().filter(|schema| schema.in_code) {
        let tool = schema.tool;
        text.push_str(&format!(
            "  {}: device({}, {{\n",
            property_name(tool),
            quoted(tool)
        ));
        let properties = schema.schema.get("properties").and_then(Value::as_object);
        for (number, default) in &schema.numbers {
            let field = &*number.field;
            // A number inside a list is written as the rest of the record is.
            if !field
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                continue;
            }
            let description = (properties.and_then(|properties| properties.get(field)))
                .and_then(|property| property.get("description"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let doc = [plain(description), range_doc(number, Some(*default))].join(" ");
            let unit = unit_of(field)
                .filter(|(unit, _)| ["hz", "ms", "db"].contains(unit))
                .map_or(String::new(), |(unit, _)| format!(", unit: \"{unit}\""));
            let (min, max) = (number.range.min, number.range.max);
            text.push_str(&format!(
                "{}    {field}: knob({{ min: {min}, max: {max}, default: {default}{unit} }}),\n",
                tsdoc(doc.trim(), "    "),
            ));
        }
        text.push_str("  }),\n");
    }
    text.push_str("};\n");
    text
}

/// Writes the TypeScript of a schema.
struct Writer<'a> {
    defs: Option<&'a Map<String, Value>>,
    /// The numbers of the record by their path, with `[]` for any item of a list.
    numbers: BTreeMap<String, &'a (ParameterInfo, f32)>,
}

impl Writer<'_> {
    /// The schema behind a `$ref`, or the schema itself.
    fn resolved<'a>(&'a self, schema: &'a Value) -> &'a Value {
        let reference = schema.get("$ref").and_then(Value::as_str);
        let named = reference.and_then(|reference| reference.strip_prefix("#/$defs/"));
        match named.and_then(|name| self.defs?.get(name)) {
            Some(def) => def,
            None => schema,
        }
    }

    /// The type of `schema` at `path` in the record, indented for `depth`.
    fn object_or_type(&self, schema: &Value, path: &str, depth: usize) -> String {
        if depth > DEEPEST {
            return "unknown".to_string();
        }
        let schema = self.resolved(schema);
        let one_of = |key| schema.get(key).and_then(Value::as_array);
        if let Some(variants) = one_of("oneOf").or_else(|| one_of("anyOf")) {
            let variants: Vec<String> = (variants.iter())
                .map(|variant| self.object_or_type(variant, path, depth))
                .collect();
            return dedup(variants).join(" | ");
        }
        if let Some(all) = one_of("allOf").filter(|all| all.len() == 1) {
            return self.object_or_type(&all[0], path, depth);
        }
        if let Some(value) = schema.get("const") {
            return value.to_string();
        }
        if let Some(values) = one_of("enum") {
            let values: Vec<String> = values.iter().map(Value::to_string).collect();
            return values.join(" | ");
        }
        let types: Vec<&str> = match schema.get("type") {
            Some(Value::String(kind)) => vec![kind.as_str()],
            Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if types.is_empty() {
            return "unknown".to_string();
        }
        let types: Vec<String> = (types.into_iter())
            .map(|kind| match kind {
                "object" => self.object(schema, path, depth),
                "array" => self.array(schema, path, depth),
                "integer" | "number" => "number".to_string(),
                "string" => "string".to_string(),
                "boolean" => "boolean".to_string(),
                "null" => "null".to_string(),
                _ => "unknown".to_string(),
            })
            .collect();
        types.join(" | ")
    }

    fn array(&self, schema: &Value, path: &str, depth: usize) -> String {
        let item_path = format!("{path}[]");
        if let Some(items) = schema.get("prefixItems").and_then(Value::as_array) {
            let items: Vec<String> = (items.iter())
                .map(|item| self.object_or_type(item, &item_path, depth))
                .collect();
            return format!("[{}]", items.join(", "));
        }
        match schema.get("items") {
            Some(items) if items.is_object() => {
                let item = self.object_or_type(items, &item_path, depth);
                match item.contains(' ') && !item.starts_with('{') {
                    true => format!("Array<{item}>"),
                    false => format!("{item}[]"),
                }
            }
            _ => "unknown[]".to_string(),
        }
    }

    fn object(&self, schema: &Value, path: &str, depth: usize) -> String {
        let properties = schema.get("properties").and_then(Value::as_object);
        let Some(properties) = properties.filter(|properties| !properties.is_empty()) else {
            return match schema.get("additionalProperties") {
                Some(values) if values.is_object() => {
                    let values = self.object_or_type(values, &format!("{path}{{}}"), depth);
                    format!("Record<string, {values}>")
                }
                _ => "Record<string, never>".to_string(),
            };
        };
        let required: Vec<&str> = (schema.get("required").and_then(Value::as_array))
            .map(|required| required.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let indent = "  ".repeat(depth + 1);
        let mut text = String::from("{\n");
        for (name, property) in properties {
            let field = match path.is_empty() {
                true => name.clone(),
                false => format!("{path}.{name}"),
            };
            let kind = self.object_or_type(property, &field, depth + 1);
            let doc = self.doc(property, &field);
            let optional = if required.contains(&name.as_str()) {
                ""
            } else {
                "?"
            };
            text.push_str(&format!(
                "{}{indent}{}{optional}: {kind};\n",
                tsdoc(&doc, &indent),
                property_name(name),
            ));
        }
        text.push_str(&"  ".repeat(depth));
        text.push('}');
        text
    }

    /// What the TSDoc of a field says: its doc comment, its options, its range and default.
    fn doc(&self, property: &Value, path: &str) -> String {
        let resolved = self.resolved(property);
        // The doc of a type is for its Rust readers past its first paragraph.
        let type_doc = description(resolved)
            .map(|doc| doc.split("\n\n").next().unwrap_or_default().to_string());
        let mut lines: Vec<String> = (description(property).or(type_doc).into_iter())
            .map(|doc| plain(&doc))
            .collect();
        let options = (resolved.get("oneOf").and_then(Value::as_array))
            .into_iter()
            .flatten();
        for option in options {
            if let (Some(value), Some(said)) = (option.get("const"), description(option)) {
                lines.push(format!("- `{value}`: {}", plain(&said)));
            }
        }
        // Each item of a list has its own default.
        let in_list = path.contains("[]") || path.contains("{}");
        if let Some((number, default)) = self.numbers.get(path) {
            lines.push(range_doc(number, (!in_list).then_some(*default)));
        } else if let Some(default) = property.get("default").map(short_value)
            && default.len() <= LONGEST_DEFAULT
        {
            lines.push(format!("Default `{default}`."));
        }
        lines.join("\n")
    }
}

fn description(schema: &Value) -> Option<String> {
    schema
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// A Rust doc comment as plain text on one line for a TSDoc comment: a link to an item is its
/// name.
fn plain(doc: &str) -> String {
    doc.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("[`Self::", "`")
        .replace("[`", "`")
        .replace("`]", "`")
        .replace("*/", "*\\/")
}

/// The range, the unit and the default of a number, as its TSDoc says them.
fn range_doc(number: &ParameterInfo, default: Option<f32>) -> String {
    let unit = unit_of(&number.field).map_or("", |(_, word)| word);
    let scale = match number.range.scale {
        Scale::Logarithmic => ", on a log scale",
        Scale::Linear | Scale::Fader => "",
    };
    let (min, max) = (number.range.min, number.range.max);
    let default = default.map_or(String::new(), |default| format!(" Default `{default}`."));
    format!("From {min} to {max}{unit}{scale}.{default}")
}

/// The unit a field name ends with, and how a doc writes it after a number.
fn unit_of(field: &str) -> Option<(&'static str, &'static str)> {
    let units = [
        ("hz", " Hz"),
        ("ms", " ms"),
        ("db", " dB"),
        ("octaves", " octaves"),
        ("semitones", " semitones"),
        ("cents", " cents"),
        ("seconds", " s"),
    ];
    let last = field.rsplit(['_', '.']).next()?;
    units.into_iter().find(|(unit, _)| *unit == last)
}

/// A JSON value as short as its type: an `f32` default without the digits of an `f64`.
fn short_value(value: &Value) -> String {
    match value {
        Value::Number(number) if number.is_f64() => {
            // The defaults are `f32`s, read as `f64`s.
            number
                .as_f64()
                .map_or(number.to_string(), |float| (float as f32).to_string())
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(short_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(fields) => {
            let fields: Vec<String> = (fields.iter())
                .map(|(name, value)| format!("{name:?}: {}", short_value(value)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        _ => value.to_string(),
    }
}

/// A path of a number as the writer walks it: `bands[0].gain_db` is `bands[].gain_db`, and
/// `pads.36.pan`, of a pad keyed by its note, is `pads{}.pan`.
fn normalized(path: &str) -> String {
    let mut normal = String::new();
    for (index, part) in path.split('.').enumerate() {
        if part.chars().all(|c| c.is_ascii_digit()) {
            normal.push_str("{}");
            continue;
        }
        if index > 0 {
            normal.push('.');
        }
        match part.split_once('[') {
            Some((name, _)) => {
                normal.push_str(name);
                normal.push_str("[]");
            }
            None => normal.push_str(part),
        }
    }
    normal
}

/// The type of the `state` of `tool`: `instrument.synth` is `InstrumentSynthState`.
fn type_name(tool: &str) -> String {
    let words = tool.split(|c: char| !c.is_ascii_alphanumeric());
    let mut name: String = words
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or(String::new(), |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect();
    name.push_str("State");
    name
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

fn property_name(name: &str) -> String {
    let plain = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    match plain {
        true => name.to_string(),
        false => quoted(name),
    }
}

/// `doc` as a TSDoc comment at `indent`, or nothing for no doc.
fn tsdoc(doc: &str, indent: &str) -> String {
    match doc.lines().count() {
        0 => String::new(),
        1 => format!("{indent}/** {doc} */\n"),
        _ => {
            let lines: String = doc
                .lines()
                .map(|line| format!("{indent} * {line}\n"))
                .collect();
            format!("{indent}/**\n{lines}{indent} */\n")
        }
    }
}

fn dedup(mut items: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    items.retain(|item| {
        let new = !seen.contains(item);
        seen.push(item.clone());
        new
    });
    items
}
