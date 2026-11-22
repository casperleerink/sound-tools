//! Facts: what Rust knows of an instance that is no record and rarely changes, and shapes its
//! view. Every instance has its [`Card`], from the schema of its record; a tool adds its own
//! with [`crate::ToolRegistration::facts`], such as whether a file is missing. A view runs again
//! when they change, so the caller compares them each frame, see [`Facts`].

use std::any::Any;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};

use super::Project;
use super::assets::Assets;
use super::feeds::Sources;
use super::fields::RecordSchema;
use super::instance::{InstanceId, State};
use crate::clock::Clock;

/// What every instance's facts hold: its title, and the fields of its record a card can show.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Card {
    pub title: String,
    pub fields: Vec<FieldInfo>,
}

/// One field of a record, as a card shows it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct FieldInfo {
    /// Its key in the record, or in the object or list around it. Empty for the item of a list.
    pub name: String,
    pub label: String,
    #[serde(flatten)]
    pub kind: FieldKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FieldKind {
    Number {
        min: f64,
        max: f64,
        scale: FieldScale,
        /// It holds whole numbers.
        integer: bool,
        /// An automation lane can move it.
        lane: bool,
        unit: Option<String>,
        default: f64,
    },
    Switch {
        default: bool,
    },
    Choice {
        options: Vec<Choice>,
        default: Value,
    },
    /// An object of fields, such as an oscillator of the wavetable.
    Group {
        fields: Vec<FieldInfo>,
    },
    /// A list of `item`s, such as the bands of the EQ, at most `max_items`.
    List {
        item: Box<FieldInfo>,
        #[serde(rename = "maxItems")]
        max_items: Option<u64>,
        /// A row of steps, such as a sequence's: a card shows them as steps, and no lane moves one.
        pattern: bool,
        /// The items a record that leaves it out holds.
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<Vec<Value>>,
    },
    /// An object of `item`s by key, such as the pads of the drum pad.
    Map {
        item: Box<FieldInfo>,
    },
}

/// How a number spreads over the travel of its knob.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FieldScale {
    Linear,
    #[serde(rename = "log")]
    Logarithmic,
    Fader,
}

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Choice {
    pub value: Value,
    pub text: String,
}

/// The facts of an instance: its card, shared while its tool's schema stays, and what its tool
/// adds. Equal facts compare equal cheaply.
#[derive(Clone, Debug)]
pub struct Facts {
    card: Arc<Card>,
    extra: Map<String, Value>,
}

impl PartialEq for Facts {
    fn eq(&self, other: &Self) -> bool {
        (Arc::ptr_eq(&self.card, &other.card) || self.card == other.card)
            && self.extra == other.extra
    }
}

impl Facts {
    pub fn card(&self) -> &Card {
        &self.card
    }

    /// `{ title, fields, ...what the tool adds }`, as a view gets them.
    pub fn to_value(&self) -> Value {
        let mut value = Map::new();
        value.insert("title".into(), self.card.title.clone().into());
        let fields = serde_json::to_value(&self.card.fields).unwrap_or_default();
        value.insert("fields".into(), fields);
        value.extend(self.extra.clone());
        Value::Object(value)
    }
}

/// What a facts function reads besides the record.
pub struct FactsContext<'a> {
    project: &'a Project,
    instance: &'a InstanceId,
    sources: &'a dyn Sources,
}

impl FactsContext<'_> {
    pub fn instance(&self) -> &InstanceId {
        self.instance
    }

    pub fn assets(&self) -> &Assets {
        self.project.assets()
    }

    /// The watch the instance keeps as `name`.
    pub fn watched(&self, name: &str) -> Option<f32> {
        self.project.watched(self.instance, name)
    }

    pub fn clock(&self) -> &Clock {
        self.project.clock()
    }

    pub fn sources(&self) -> &dyn Sources {
        self.sources
    }
}

pub(crate) struct RegisteredFacts {
    read: Box<dyn Fn(&dyn Any, &FactsContext<'_>) -> Option<Map<String, Value>>>,
    /// The JSON Schema of what it adds.
    schema: Value,
}

/// See [`crate::ToolRegistration::facts`].
pub(crate) fn erase<S: State, F: Serialize + JsonSchema>(
    facts: impl Fn(&S, &FactsContext<'_>) -> F + 'static,
) -> RegisteredFacts {
    RegisteredFacts {
        read: Box::new(move |state, context| {
            let read = |state: &S| match serde_json::to_value(facts(state, context)) {
                Ok(Value::Object(map)) => map,
                _ => Map::new(),
            };
            match state.downcast_ref::<S>() {
                Some(state) => Some(read(state)),
                None => {
                    let json = state.downcast_ref::<super::instance::JsonState>()?;
                    Some(read(&serde_json::from_value(json.value.clone()).ok()?))
                }
            }
        }),
        schema: schemars::schema_for!(F).to_value(),
    }
}

/// The facts of `instance`. Its card comes from `schema` when given, as for a tool of the
/// project, whose schema its code gives; else from the schema its tool registered. `None` for an
/// instance that is not there.
pub fn of(
    project: &Project,
    sources: &dyn Sources,
    instance: &InstanceId,
    schema: Option<&Value>,
) -> Option<Facts> {
    let record = project.instances.get(instance)?;
    let registry = &project.registry;
    let shown = registry.shown(record.tool);
    let card = match schema {
        // A lane moves every number of a tool of the project but a whole one.
        Some(schema) => Arc::new(with_lanes(card(record.tool, schema), &|_, integer| {
            !integer
        })),
        None => {
            let registered = registry.definition(record.tool)?.schema.as_ref();
            let make = || {
                Arc::new(registered.map_or_else(
                    || card(record.tool, &Value::Null),
                    |registered| {
                        let lanes = |path: &str, _| {
                            (registered.numbers.iter()).any(|(number, _)| fits(path, &number.field))
                        };
                        with_lanes(card(record.tool, &registered.schema), &lanes)
                    },
                ))
            };
            match shown {
                Some(shown) => shown.card.get_or_init(make).clone(),
                None => make(),
            }
        }
    };
    let context = FactsContext {
        project,
        instance,
        sources,
    };
    let extra = (shown.and_then(|shown| shown.facts.as_ref()))
        .and_then(|facts| (facts.read)(record.state.as_any(), &context))
        .unwrap_or_default();
    Some(Facts { card, extra })
}

/// The JSON Schema of the facts of each tool that adds some, for the types of the code of a
/// project. The tool `*` is what every instance has.
pub fn describe(registry: &super::Registry) -> Vec<(&'static str, Value)> {
    let every = ("*", schemars::schema_for!(Card).to_value());
    let tools = (registry.shown.iter()).filter_map(|(tool, shown)| {
        let facts = shown.facts.as_ref()?;
        Some((*tool, facts.schema.clone()))
    });
    std::iter::once(every).chain(tools).collect()
}

/// The card of a record of `tool` whose JSON Schema is `schema`.
pub fn card(tool: &str, schema: &Value) -> Card {
    let title = (schema.get("x-title").and_then(Value::as_str)).map_or_else(
        || words(tool.rsplit('.').next().unwrap_or(tool)),
        str::to_string,
    );
    let reader = RecordSchema::new(schema);
    Card {
        title,
        fields: fields(&reader, reader.resolved(schema), 0),
    }
}

/// `card` with `lane` set on each number that `moves`, by its path with `*` for an item of a
/// list or a map, and whether it is whole.
fn with_lanes(mut card: Card, moves: &dyn Fn(&str, bool) -> bool) -> Card {
    mark_lanes(&mut card.fields, "", moves);
    card
}

fn mark_lanes(fields: &mut [FieldInfo], prefix: &str, moves: &dyn Fn(&str, bool) -> bool) {
    for field in fields {
        let path = match field.name.is_empty() {
            true => format!("{prefix}*"),
            false => format!("{prefix}{}", field.name),
        };
        match &mut field.kind {
            FieldKind::Number { integer, lane, .. } => *lane = moves(&path, *integer),
            FieldKind::Group { fields } => mark_lanes(fields, &format!("{path}."), moves),
            FieldKind::List { pattern: true, .. } => {}
            FieldKind::List { item, .. } | FieldKind::Map { item } => {
                mark_lanes(
                    std::slice::from_mut(item.as_mut()),
                    &format!("{path}."),
                    moves,
                );
            }
            FieldKind::Switch { .. } | FieldKind::Choice { .. } => {}
        }
    }
}

/// Whether `pattern`, a path with `*` for any one name, names `path`, such as `bands.*.gain_db`
/// and `bands.2.gain_db` or `bands[2].gain_db`.
fn fits(pattern: &str, path: &str) -> bool {
    let path = path.replace('[', ".").replace(']', "");
    let (mut patterns, mut names) = (pattern.split('.'), path.split('.'));
    loop {
        match (patterns.next(), names.next()) {
            (None, None) => return true,
            (Some(want), Some(name)) if want == "*" || want == name => {}
            _ => return false,
        }
    }
}

/// Deeper than any record of a built-in tool, and short of a schema that refers to itself.
const DEEPEST: usize = 6;

fn fields(reader: &RecordSchema<'_>, object: &Value, depth: usize) -> Vec<FieldInfo> {
    let properties = object.get("properties").and_then(Value::as_object);
    (properties.into_iter().flatten())
        .filter_map(|(name, property)| field(reader, name, property, depth))
        .collect()
}

/// A field `name` whose schema is `property`, as a card shows it. `None` for one a card cannot
/// show, such as a text or a number without a range.
fn field(
    reader: &RecordSchema<'_>,
    name: &str,
    property: &Value,
    depth: usize,
) -> Option<FieldInfo> {
    if depth > DEEPEST {
        return None;
    }
    let property = nullable(reader, property);
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
        property.get("type").and_then(kind_of),
        number("minimum"),
        number("maximum"),
    ) {
        _ if !options.is_empty() => {
            let first = options.first().cloned().unwrap_or_default();
            let options = (options.into_iter())
                .map(|value| {
                    let text = value.as_str().map_or_else(|| value.to_string(), words);
                    Choice { value, text }
                })
                .collect();
            FieldKind::Choice {
                options,
                default: default.unwrap_or(first),
            }
        }
        (Some(kind @ ("number" | "integer")), Some(min), Some(max)) => {
            let scale = match property.get("x-scale").and_then(Value::as_str) {
                Some("logarithmic") if min > 0.0 => FieldScale::Logarithmic,
                Some("fader") => FieldScale::Fader,
                _ => FieldScale::Linear,
            };
            // A tool of the project declares its unit, `null` for a plain number; a built-in
            // one's is read from its name.
            let unit = match property.get("x-unit") {
                Some(declared) => declared.as_str().map(str::to_string),
                None => unit_of_name(name, min, max).map(str::to_string),
            };
            FieldKind::Number {
                min,
                max,
                scale,
                integer: kind == "integer",
                // Set by `with_lanes`, which knows the tool.
                lane: false,
                unit,
                default: default.as_ref().and_then(Value::as_f64).unwrap_or(min),
            }
        }
        (Some("boolean"), ..) => FieldKind::Switch {
            default: default.as_ref().and_then(Value::as_bool).unwrap_or(false),
        },
        (Some("object"), ..) if property.get("properties").is_some() => {
            let fields = fields(reader, property, depth + 1);
            (!fields.is_empty()).then_some(FieldKind::Group { fields })?
        }
        (Some("object"), ..) => {
            let item = property.get("additionalProperties")?;
            let item = field(reader, "", item, depth + 1)?;
            FieldKind::Map {
                item: Box::new(item),
            }
        }
        (Some("array"), ..) => {
            let item = field(reader, "", property.get("items")?, depth + 1)?;
            FieldKind::List {
                item: Box::new(item),
                max_items: property.get("maxItems").and_then(Value::as_u64),
                pattern: property.get("x-pattern") == Some(&Value::Bool(true)),
                default: default.as_ref().and_then(Value::as_array).cloned(),
            }
        }
        _ => return None,
    };
    Some(FieldInfo {
        name: name.to_string(),
        label,
        kind,
    })
}

/// The type of a field: a number a record may leave out is `["number", "null"]`.
fn kind_of(kind: &Value) -> Option<&str> {
    match kind {
        Value::Array(kinds) => {
            (kinds.iter().filter_map(Value::as_str)).find(|kind| *kind != "null")
        }
        kind => kind.as_str(),
    }
}

/// The schema behind a `$ref`, and behind an `Option` of it: a card shows a field that may be
/// left out as the field.
fn nullable<'a>(reader: &RecordSchema<'a>, schema: &'a Value) -> &'a Value {
    let schema = reader.resolved(schema);
    let variants = schema.get("anyOf").and_then(Value::as_array);
    let mut some = (variants.into_iter().flatten())
        .filter(|variant| variant.get("type").and_then(Value::as_str) != Some("null"));
    match (some.next(), some.next()) {
        (Some(only), None) => reader.resolved(only),
        _ => schema,
    }
}

/// The unit of a number of a built-in tool, read from its name: `cutoff_hz` is in Hz, a pan,
/// or a share of 0 to 1 as a percent. As `sdk/fields.ts` names it.
pub fn unit_of_name(field: &str, min: f64, max: f64) -> Option<&'static str> {
    const UNITS: [(&str, &str); 5] = [
        ("_hz", "hz"),
        ("_db", "db"),
        ("_ms", "ms"),
        ("_seconds", "seconds"),
        ("_percent", "percent"),
    ];
    let unit = UNITS.iter().find(|(end, _)| field.ends_with(end));
    match unit {
        Some((_, unit)) => Some(unit),
        None if field.rsplit('.').next() == Some("pan") => Some("pan"),
        None if min == 0.0 && max == 1.0 => Some("percent"),
        None => None,
    }
}

/// `cutoff_hz` as `Cutoff`: a field's name in words, without its unit.
pub fn words(name: &str) -> String {
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
