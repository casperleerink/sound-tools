//! The part of `sdk.ts` the runtime writes: the record of each built-in tool as a TypeScript
//! type, made from the JSON Schema of its Rust type, and the `Tools` that names them. Each
//! object type is written once, as a named type. What
//! [`RecordSchema::field`] says of a field is its TSDoc, as the agent docs say it too.

use serde_json::Value;
use sound_core::{FieldDoc, RecordSchema, ToolSchema, type_doc};

use crate::built_in_schemas;

/// The runtime's part of `sdk.ts` for the tools of `schemas` and the built-in children.
pub(crate) fn sdk_records(schemas: &[ToolSchema]) -> String {
    let built_in = built_in_schemas();
    let mut tools: Vec<&ToolSchema> = schemas.iter().chain(&built_in).collect();
    tools.sort_by_key(|schema| schema.tool);
    let mut text = String::from(
        "
// ---------------------------------------------------------------------------------------------
// Built-in tools: their records, from their Rust types.
",
    );
    for schema in &tools {
        text.push_str(&Writer::new(schema).types());
    }
    text.push_str(
        "\n/**\n * Every built-in tool by its name: what its records hold, the numbers a cable moves (`number`),\n * the number inputs `send` plays (`input`), its bang inputs (`bang`) and the numbers it sends\n * out (`out`).\n */\nexport interface Tools {\n",
    );
    for schema in &tools {
        let names = |names: Vec<String>| match names.is_empty() {
            true => "never".to_string(),
            false => names.join(" | "),
        };
        let inputs = schema.live.iter().map(|input| quoted(input));
        let number = names(
            (schema.numbers.iter())
                .map(|(info, _)| quoted(&info.field))
                .chain(inputs.clone())
                .collect(),
        );
        let input = names(inputs.collect());
        let out = names(schema.outputs.iter().map(|output| quoted(output)).collect());
        let defaults = names(
            defaults_of(schema)
                .keys()
                .map(|field| quoted(field))
                .collect(),
        );
        text.push_str(&format!(
            "  {}: {{ state: {}; number: {number}; input: {input}; bang: never; out: {out}; defaults: {defaults} }};\n",
            property_name(schema.tool),
            type_name(schema.tool, "State"),
        ));
    }
    text.push_str("}\n");
    let defaults: serde_json::Map<String, Value> = (tools.iter())
        .map(|schema| (schema.tool.to_string(), Value::Object(defaults_of(schema))))
        .collect();
    text.push_str(&format!(
        "\n/** What the record of each built-in tool holds where it leaves a field out: what code that edits one gets. */\nexport const defaultStates: Record<string, Record<string, unknown>> = {};\n",
        Value::Object(defaults)
    ));
    text
}

/// The fields of the record of a tool that have a default, with it.
fn defaults_of(schema: &ToolSchema) -> serde_json::Map<String, Value> {
    let record = RecordSchema::new(&schema.schema);
    let root = record.resolved(&schema.schema);
    let properties = root.get("properties").and_then(Value::as_object);
    (properties.into_iter().flatten())
        .filter_map(|(name, property)| Some((name.clone(), property.get("default")?.clone())))
        .collect()
}

/// Writes the TypeScript of the record of one tool.
struct Writer<'a> {
    schema: &'a ToolSchema,
    record: RecordSchema<'a>,
    /// Each object type of the record, which is a named type.
    objects: Vec<&'a str>,
}

impl<'a> Writer<'a> {
    fn new(schema: &'a ToolSchema) -> Self {
        let mut writer = Self {
            schema,
            record: RecordSchema::new(&schema.schema),
            objects: Vec::new(),
        };
        writer.find_objects(&schema.schema);
        writer
    }

    /// The type of the record, and a named type for each object type in it.
    fn types(&self) -> String {
        let tool = self.schema.tool;
        let mut text = format!(
            "\n/** The `state` of a record of the built-in `{tool}`. */\nexport type {} = {};\n",
            type_name(tool, "State"),
            self.object_or_type(&self.schema.schema, 0),
        );
        for name in &self.objects {
            let Some(def) = self.record.def(name) else {
                continue;
            };
            text.push_str(&format!(
                "\n{}export type {} = {};\n",
                tsdoc(&type_doc(def).unwrap_or_default(), ""),
                type_name(tool, name),
                self.object(def, 0),
            ));
        }
        text
    }

    /// Notes each object type under `schema`, the first time.
    fn find_objects(&mut self, schema: &'a Value) {
        if let Some(name) = self.record.object_name(schema) {
            if self.objects.contains(&name) {
                return;
            }
            self.objects.push(name);
        }
        let schema = self.record.resolved(schema);
        for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
            for variant in schema
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.find_objects(variant);
            }
        }
        for key in ["items", "additionalProperties"] {
            if let Some(items) = schema.get(key).filter(|items| items.is_object()) {
                self.find_objects(items);
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for property in properties
            .into_iter()
            .flatten()
            .map(|(_, property)| property)
        {
            self.find_objects(property);
        }
    }

    /// The type of `schema`, indented for `depth`.
    fn object_or_type(&self, schema: &'a Value, depth: usize) -> String {
        if let Some(name) = self.record.object_name(schema) {
            return type_name(self.schema.tool, name);
        }
        let schema = self.record.resolved(schema);
        let one_of = |key| schema.get(key).and_then(Value::as_array);
        if let Some(variants) = one_of("oneOf").or_else(|| one_of("anyOf")) {
            let variants: Vec<String> = (variants.iter())
                .map(|variant| self.object_or_type(variant, depth))
                .collect();
            return dedup(variants).join(" | ");
        }
        if let Some(all) = one_of("allOf").filter(|all| all.len() == 1) {
            return self.object_or_type(&all[0], depth);
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
                "object" => self.object(schema, depth),
                "array" => self.array(schema, depth),
                "integer" | "number" => "number".to_string(),
                "string" => "string".to_string(),
                "boolean" => "boolean".to_string(),
                "null" => "null".to_string(),
                _ => "unknown".to_string(),
            })
            .collect();
        types.join(" | ")
    }

    fn array(&self, schema: &'a Value, depth: usize) -> String {
        if let Some(items) = schema.get("prefixItems").and_then(Value::as_array) {
            let items: Vec<String> = (items.iter())
                .map(|item| self.object_or_type(item, depth))
                .collect();
            return format!("[{}]", items.join(", "));
        }
        match schema.get("items") {
            Some(items) if items.is_object() => {
                let item = self.object_or_type(items, depth);
                match item.contains(' ') && !item.starts_with('{') {
                    true => format!("Array<{item}>"),
                    false => format!("{item}[]"),
                }
            }
            _ => "unknown[]".to_string(),
        }
    }

    fn object(&self, schema: &'a Value, depth: usize) -> String {
        let properties = schema.get("properties").and_then(Value::as_object);
        let Some(properties) = properties.filter(|properties| !properties.is_empty()) else {
            return match schema.get("additionalProperties") {
                Some(values) if values.is_object() => {
                    let values = self.object_or_type(values, depth);
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
            let kind = self.object_or_type(property, depth + 1);
            let doc = tsdoc_text(&self.record.field(name, property));
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
}

/// What the TSDoc of a field says: its meaning, its range and default, then its options.
fn tsdoc_text(field: &FieldDoc) -> String {
    let mut said = vec![field.meaning.clone()];
    if let Some(range) = &field.range {
        let scale = match range.logarithmic {
            true => ", on a log scale",
            false => "",
        };
        said.push(format!("From {}{scale}.", range.text()));
    }
    if let Some(default) = &field.default {
        said.push(format!("Default `{default}`."));
    }
    said.retain(|said| !said.is_empty());
    let mut lines = vec![said.join(" ")];
    for (value, said) in &field.options {
        lines.push(format!("- `{value}`: {said}"));
    }
    lines.retain(|line| !line.is_empty());
    lines.join("\n")
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
    let doc = doc.replace("*/", "*\\/");
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
