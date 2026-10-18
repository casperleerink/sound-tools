//! What the docs say about the fields of a record, read from the JSON Schema of its type: a
//! doc comment is the meaning of a field, a [`ranges`] transform gives a number its range, and
//! `#[serde(default)]` its default. `sdk.ts` writes it as TSDoc and the agent docs as tables,
//! so the two never say different things.

use schemars::Schema;
use serde_json::{Map, Value};

use crate::parameter::{Parameter, Scale};

/// A default longer than this, such as a list of bands, is left out of a doc.
const LONGEST_DEFAULT: usize = 48;

/// For `#[schemars(transform = ranges(&PARAMETERS))]` on the type the parameters read: gives the
/// field of each its range and scale, so the docs made from the schema say them.
pub fn ranges<S>(parameters: &[&Parameter<S>]) -> impl FnMut(&mut Schema) {
    move |schema: &mut Schema| {
        let properties = schema.get_mut("properties").and_then(Value::as_object_mut);
        let Some(properties) = properties else {
            return;
        };
        for parameter in parameters {
            let field = properties.get_mut(parameter.field);
            let Some(field) = field.and_then(Value::as_object_mut) else {
                continue;
            };
            for (key, end) in [("minimum", parameter.min), ("maximum", parameter.max)] {
                if end.is_finite() {
                    field.insert(key.into(), f64::from(end).into());
                }
            }
            let scale = match parameter.scale {
                Scale::Linear => "linear",
                Scale::Logarithmic => "logarithmic",
                Scale::Fader => "fader",
            };
            field.insert("x-scale".into(), scale.into());
        }
    }
}

/// What a doc says about one field.
pub struct FieldDoc {
    /// Its doc comment on one line, else that of its type, and what its default changes of the
    /// defaults of its type.
    pub meaning: String,
    pub range: Option<FieldRange>,
    pub default: Option<String>,
    /// The values of a choice whose values have doc comments, each with what it means.
    pub options: Vec<(String, String)>,
}

/// The range of a number, from its [`Parameter`].
pub struct FieldRange {
    pub min: String,
    pub max: String,
    /// How a doc writes the unit after a number, such as ` Hz`, or nothing.
    pub unit: &'static str,
    pub logarithmic: bool,
}

impl FieldRange {
    /// `20 to 20000 Hz`.
    pub fn text(&self) -> String {
        format!("{} to {}{}", self.min, self.max, self.unit)
    }
}

/// The JSON Schema of a record, with the named types it refers to.
pub struct RecordSchema<'a> {
    schema: &'a Value,
    defs: Option<&'a Map<String, Value>>,
}

impl<'a> RecordSchema<'a> {
    pub fn new(schema: &'a Value) -> Self {
        let defs = schema.get("$defs").and_then(Value::as_object);
        Self { schema, defs }
    }

    /// The schema behind a `$ref`, or the schema itself.
    pub fn resolved(&self, schema: &'a Value) -> &'a Value {
        let reference = schema.get("$ref").and_then(Value::as_str);
        let named = reference.and_then(|reference| reference.strip_prefix("#/$defs/"));
        match named.and_then(|name| self.defs?.get(name)) {
            Some(def) => def,
            None => schema,
        }
    }

    /// The named type `schema` refers to, when it is an object with fields.
    pub fn object_name(&self, schema: &'a Value) -> Option<&'a str> {
        let name = (schema.get("$ref").and_then(Value::as_str))?.strip_prefix("#/$defs/")?;
        let def = self.defs?.get(name)?;
        let properties = def.get("properties").and_then(Value::as_object)?;
        (!properties.is_empty()).then_some(name)
    }

    /// The named type of `name`.
    pub fn def(&self, name: &str) -> Option<&'a Value> {
        self.defs?.get(name)
    }

    /// The schema behind a `$ref`, and behind an `Option` of it.
    fn unwrapped(&self, schema: &'a Value) -> &'a Value {
        let schema = self.resolved(schema);
        let variants = schema.get("anyOf").and_then(Value::as_array);
        let mut some = variants
            .into_iter()
            .flatten()
            .filter(|variant| !is_null(variant));
        match (some.next(), some.next()) {
            (Some(one), None) => self.resolved(one),
            _ => schema,
        }
    }

    /// What a doc says about the field `name`, whose schema is `property` and whose unit its
    /// name ends with.
    pub fn field(&self, name: &str, property: &'a Value) -> FieldDoc {
        let resolved = self.unwrapped(property);
        let object = self.object_name(property).is_some();
        let mut meaning: Vec<String> = (description(property).map(|doc| plain(&doc)))
            .or_else(|| type_doc(resolved))
            .into_iter()
            .collect();
        if let Some(most) = property.get("maxItems") {
            meaning.push(format!("At most {most}."));
        }
        let mut default = None;
        if object {
            meaning.extend(changed_defaults(property, resolved));
        } else if let Some(value) = property.get("default").map(short_value)
            && value.len() <= LONGEST_DEFAULT
        {
            default = Some(value);
        }
        let range = (property.get("x-scale")).map(|scale| FieldRange {
            min: (property.get("minimum")).map_or("-inf".into(), short_value),
            max: (property.get("maximum")).map_or("inf".into(), short_value),
            unit: unit_of(name).map_or("", |(_, word)| word),
            logarithmic: scale == "logarithmic",
        });
        let options = (resolved.get("oneOf").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(|option| Some((option.get("const")?.to_string(), description(option)?)))
            .map(|(value, said)| (value, plain(&said)))
            .collect();
        FieldDoc {
            meaning: meaning.join(" "),
            range,
            default,
            options,
        }
    }

    /// The schema of the field at `path` in the record: `filter_1.cutoff_hz`, `bands[0].q`, or
    /// `pads.36.pan` for a pad keyed by its note. A list or an object keyed by name is passed
    /// through to its items.
    pub fn property(&self, path: &str) -> Option<&'a Value> {
        let mut schema = self.schema;
        for part in path.split('.') {
            let name = part.split_once('[').map_or(part, |(name, _)| name);
            schema = self.items(schema);
            if name.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            schema = (self.resolved(schema).get("properties"))?.get(name)?;
        }
        Some(schema)
    }

    /// The schema of an item of a list or of an object keyed by name, else the schema itself.
    fn items(&self, schema: &'a Value) -> &'a Value {
        let schema = self.resolved(schema);
        let items = (schema.get("items")).or_else(|| schema.get("additionalProperties"));
        match items.filter(|items| items.is_object()) {
            Some(items) => self.resolved(items),
            None => schema,
        }
    }

    /// The fields of the object at `path`, the record itself for an empty one, as a markdown
    /// table: each with its meaning, the values it takes and its default. A choice whose values
    /// have docs lists them under the table. Fields or values that say the same share a line.
    pub fn table(&self, path: &str) -> Option<String> {
        let property = match path.is_empty() {
            true => self.schema,
            false => self.property(path)?,
        };
        let object = self.items(property);
        let properties = object.get("properties").and_then(Value::as_object)?;
        // The items of a list with a default of its own, such as the bands of an EQ, each take
        // their default from it: one value when all are the same, else each in order. A field
        // an item must have, as in a route of a matrix, has none.
        let item_defaults = (property.get("default").and_then(Value::as_array))
            .filter(|defaults| defaults.iter().all(Value::is_object));
        let required: Vec<&str> = (object.get("required").and_then(Value::as_array))
            .map(|required| required.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        let mut rows = Vec::new();
        let mut options = String::new();
        let mut all_have_defaults = true;
        for (name, property) in properties {
            let field = self.field(name, property);
            let default = match item_defaults {
                _ if required.contains(&name.as_str()) => None,
                Some(defaults) => {
                    let mut values: Vec<String> = (defaults.iter())
                        .filter_map(|item| item.get(name).map(|value| code(&short_value(value))))
                        .collect();
                    if values.iter().all(|value| Some(value) == values.first()) {
                        values.truncate(1);
                    }
                    (!values.is_empty()).then(|| values.join(", "))
                }
                None => field.default.as_deref().map(code),
            };
            let has_default = default.is_some() || property.get("default").is_some();
            let values = match &field.range {
                Some(range) => range.text(),
                None => self.values(property),
            };
            if !field.options.is_empty() {
                options.push_str(&format!("\n`{name}` is one of:\n\n"));
                let named = (field.options.into_iter()).map(|(value, said)| (code(&value), said));
                for (values, said) in merged(named) {
                    options.push_str(&format!("- {}: {said}\n", values.join(", ")));
                }
            }
            all_have_defaults &= has_default;
            rows.push((code(name), (field.meaning, values, default)));
        }

        let rows = merged(rows);
        let defaults = rows.iter().any(|(_, (.., default))| default.is_some());
        let mut text = match defaults {
            true => "| Field | Meaning | Values | Default |\n| --- | --- | --- | --- |\n",
            false => "| Field | Meaning | Values |\n| --- | --- | --- |\n",
        }
        .to_string();
        for (names, (meaning, values, default)) in &rows {
            let mut cells = vec![names.join(", "), cell(meaning), cell(values)];
            if defaults {
                cells.push(cell(default.as_deref().unwrap_or_default()));
            }
            text.push_str(&format!("| {} |\n", cells.join(" | ")));
        }
        text.push_str(&options);
        if path.is_empty() && all_have_defaults {
            text.push_str(
                "\nA field you leave out takes its default, so `\"state\": {}` is the record at \
                 its defaults.\n",
            );
        }
        Some(text.trim_end().to_string())
    }

    /// The values a field without a range takes, as a table says them.
    fn values(&self, property: &'a Value) -> String {
        let schema = self.unwrapped(property);
        let consts: Vec<String> = match (schema.get("oneOf"), schema.get("enum")) {
            (Some(Value::Array(options)), _) => (options.iter())
                .filter_map(|option| option.get("const"))
                .map(|value| code(&value.to_string()))
                .collect(),
            (_, Some(Value::Array(values))) => (values.iter())
                .filter(|value| !value.is_null())
                .map(|value| code(&value.to_string()))
                .collect(),
            _ => Vec::new(),
        };
        if let Some((last, first)) = consts.split_last() {
            return match first.is_empty() {
                true => last.clone(),
                false => format!("{} or {last}", first.join(", ")),
            };
        }
        let kinds: Vec<&str> = match schema.get("type") {
            Some(Value::String(kind)) => vec![kind.as_str()],
            Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        let words: Vec<&str> = (kinds.into_iter())
            .filter_map(|kind| match kind {
                "boolean" => Some("`true` or `false`"),
                "number" => Some("a number"),
                "integer" => Some("a whole number"),
                "string" => Some("text"),
                "array" => Some("a list"),
                "object" => Some("an object"),
                _ => None,
            })
            .collect();
        words.join(" or ")
    }
}

/// Items that say the same thing as one, named together in the order they first come.
fn merged<T: PartialEq>(items: impl IntoIterator<Item = (String, T)>) -> Vec<(Vec<String>, T)> {
    let mut merged: Vec<(Vec<String>, T)> = Vec::new();
    for (name, said) in items {
        match merged.iter_mut().find(|(_, other)| *other == said) {
            Some((names, _)) => names.push(name),
            None => merged.push((vec![name], said)),
        }
    }
    merged
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
pub fn type_doc(schema: &Value) -> Option<String> {
    let doc = description(schema)?;
    Some(plain(doc.split("\n\n").next().unwrap_or_default()))
}

fn description(schema: &Value) -> Option<String> {
    schema
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// A Rust doc comment as plain text on one line: a link to an item is its name.
fn plain(doc: &str) -> String {
    doc.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("[`Self::", "`")
        .replace("[`", "`")
        .replace("`]", "`")
}

/// The unit a field name ends with, and how a doc writes it after a number.
pub fn unit_of(field: &str) -> Option<(&'static str, &'static str)> {
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

fn is_null(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("null")
}

fn code(text: &str) -> String {
    format!("`{text}`")
}

/// Text in a cell of a markdown table.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}
