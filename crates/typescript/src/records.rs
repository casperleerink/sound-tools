//! The part of `sdk.ts` the runtime writes: the record of each built-in tool as a TypeScript
//! type, made from the JSON Schema of its Rust type, the `Tools` that names them, and the
//! `devices` a sound plays through. Each object type is written once, as a named type. A doc
//! comment of a field is its TSDoc, with its range and default from its `Parameter`, so the
//! SDK never says other than the Rust check.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use sound_core::{ParameterInfo, Scale, ToolSchema};

use crate::sequencer;

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
        text.push_str(&Writer::new(schema).types());
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
            (schema.numbers.iter())
                .map(|(info, _)| quoted(&info.field))
                .collect(),
        );
        let live = names(schema.live.iter().map(|live| quoted(live)).collect());
        text.push_str(&format!(
            "  {}: {{ state: {}; number: {number}; live: {live}; trigger: never }};\n",
            property_name(schema.tool),
            type_name(schema.tool, "State"),
        ));
    }
    text.push_str("}\n");
    text.push_str(&sdk_devices(&tools));
    text
}

/// `devices` as the SDK offers them: a knob for each number a signal moves, in its units. The
/// docs of their settings are those of the record.
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
        for (number, default) in &schema.numbers {
            let field = &*number.field;
            // A number inside a list is written as the rest of the record is.
            if !field
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                continue;
            }
            let unit = unit_of(field)
                .filter(|(unit, _)| ["hz", "ms", "db"].contains(unit))
                .map_or(String::new(), |(unit, _)| format!(", unit: \"{unit}\""));
            let (min, max) = (number.range.min, number.range.max);
            text.push_str(&format!(
                "    {field}: knob({{ min: {min}, max: {max}, default: {default}{unit} }}),\n",
            ));
        }
        text.push_str("  }),\n");
    }
    text.push_str("};\n");
    text
}

/// Writes the TypeScript of the record of one tool.
struct Writer<'a> {
    schema: &'a ToolSchema,
    defs: Option<&'a Map<String, Value>>,
    /// The numbers of the record by their path, with `[]` for any item of a list.
    numbers: BTreeMap<String, &'a (ParameterInfo, f32)>,
    /// Each object type of the record, which is a named type, with the first path it is at.
    objects: Vec<(&'a str, String)>,
}

impl<'a> Writer<'a> {
    fn new(schema: &'a ToolSchema) -> Self {
        let mut writer = Self {
            schema,
            defs: schema.schema.get("$defs").and_then(Value::as_object),
            numbers: (schema.numbers.iter())
                .map(|number| (normalized(&number.0.field), number))
                .collect(),
            objects: Vec::new(),
        };
        writer.find_objects(&schema.schema, "");
        writer
    }

    /// The type of the record, and a named type for each object type in it.
    fn types(&self) -> String {
        let tool = self.schema.tool;
        let mut text = format!(
            "\n/** The `state` of a record of the built-in `{tool}`. */\nexport type {} = {};\n",
            type_name(tool, "State"),
            self.object_or_type(&self.schema.schema, "", 0),
        );
        for (name, path) in &self.objects {
            let Some(def) = self.defs.and_then(|defs| defs.get(*name)) else {
                continue;
            };
            text.push_str(&format!(
                "\n{}export type {} = {};\n",
                tsdoc(&type_doc(def).unwrap_or_default(), ""),
                type_name(tool, name),
                self.object(def, path, 0),
            ));
        }
        text
    }

    /// Notes each object type under `schema` at `path`, the first time.
    fn find_objects(&mut self, schema: &'a Value, path: &str) {
        if let Some(name) = self.object_name(schema) {
            if self.objects.iter().any(|(object, _)| *object == name) {
                return;
            }
            self.objects.push((name, path.to_string()));
        }
        let schema = self.resolved(schema);
        for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
            for variant in schema
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let path = match key {
                    "prefixItems" => format!("{path}[]"),
                    _ => path.to_string(),
                };
                self.find_objects(variant, &path);
            }
        }
        if let Some(items) = schema.get("items").filter(|items| items.is_object()) {
            self.find_objects(items, &format!("{path}[]"));
        }
        if let Some(values) =
            (schema.get("additionalProperties")).filter(|values| values.is_object())
        {
            self.find_objects(values, &format!("{path}{{}}"));
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (name, property) in properties.into_iter().flatten() {
            self.find_objects(property, &field_path(path, name));
        }
    }

    /// The schema behind a `$ref`, or the schema itself.
    fn resolved(&self, schema: &'a Value) -> &'a Value {
        let reference = schema.get("$ref").and_then(Value::as_str);
        let named = reference.and_then(|reference| reference.strip_prefix("#/$defs/"));
        match named.and_then(|name| self.defs?.get(name)) {
            Some(def) => def,
            None => schema,
        }
    }

    /// The name of the object type `schema` refers to, which is written once as a named type.
    fn object_name(&self, schema: &'a Value) -> Option<&'a str> {
        let name = (schema.get("$ref").and_then(Value::as_str))?.strip_prefix("#/$defs/")?;
        let def = self.defs?.get(name)?;
        let properties = def.get("properties").and_then(Value::as_object)?;
        (!properties.is_empty()).then_some(name)
    }

    /// The type of `schema` at `path` in the record, indented for `depth`.
    fn object_or_type(&self, schema: &'a Value, path: &str, depth: usize) -> String {
        if let Some(name) = self.object_name(schema) {
            return type_name(self.schema.tool, name);
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

    fn array(&self, schema: &'a Value, path: &str, depth: usize) -> String {
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

    fn object(&self, schema: &'a Value, path: &str, depth: usize) -> String {
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
            let field = field_path(path, name);
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

    /// What the TSDoc of a field says: its doc comment, its range and default, then its
    /// options. A field of an object type says only where its defaults are not the type's.
    fn doc(&self, property: &'a Value, path: &str) -> String {
        let resolved = self.resolved(property);
        let object = self.object_name(property).is_some();
        let own = description(property).map(|doc| plain(&doc));
        let mut said: Vec<String> = match object {
            true => own.into_iter().collect(),
            false => own.or_else(|| type_doc(resolved)).into_iter().collect(),
        };
        if let Some(most) = property.get("maxItems") {
            said.push(format!("At most {most}."));
        }
        // Each item of a list has its own default.
        let in_list = path.contains("[]") || path.contains("{}");
        if let Some((number, default)) = self.numbers.get(path) {
            said.push(range_doc(number, (!in_list).then_some(*default)));
        } else if object {
            said.extend(changed_defaults(property, resolved));
        } else if let Some(default) = property.get("default").map(short_value)
            && default.len() <= LONGEST_DEFAULT
        {
            said.push(format!("Default `{default}`."));
        }
        let mut lines = vec![said.join(" ")];
        let options = (resolved.get("oneOf").and_then(Value::as_array))
            .into_iter()
            .flatten();
        for option in options {
            if let (Some(value), Some(said)) = (option.get("const"), description(option)) {
                lines.push(format!("- `{value}`: {}", plain(&said)));
            }
        }
        lines.retain(|line| !line.is_empty());
        lines.join("\n")
    }
}

/// Where the default of a field of an object type differs from the type's own: `osc_2` of
/// the wavetable is detuned.
fn changed_defaults(property: &Value, object: &Value) -> Option<String> {
    let defaults = property.get("default").and_then(Value::as_object)?;
    let changed: Vec<String> = (defaults.iter())
        .filter(|(name, value)| {
            let own = object.pointer(&format!("/properties/{name}/default"));
            own.is_some_and(|own| own != *value)
        })
        .map(|(name, value)| format!("`{name}` is `{}`", short_value(value)))
        .collect();
    (!changed.is_empty()).then(|| format!("Here {} by default.", changed.join(" and ")))
}

/// The first paragraph of the doc of a type: the rest is for its Rust readers.
fn type_doc(schema: &Value) -> Option<String> {
    let doc = description(schema)?;
    Some(plain(doc.split("\n\n").next().unwrap_or_default()))
}

fn field_path(path: &str, name: &str) -> String {
    match path.is_empty() {
        true => name.to_string(),
        false => format!("{path}.{name}"),
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

/// A type of the record of `tool`: `instrument.synth` and `State` is `InstrumentSynthState`.
fn type_name(tool: &str, suffix: &str) -> String {
    let words = tool.split(|c: char| !c.is_ascii_alphanumeric());
    let mut name: String = words
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or(String::new(), |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect();
    name.push_str(suffix);
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
