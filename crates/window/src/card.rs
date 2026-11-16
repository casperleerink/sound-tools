//! The card of a device: its own view when its tool has one, else a control per field of its
//! record, made from the record's schema: a knob for a number, a switch for a yes or no, and
//! segments for a choice. So every device shows, a built-in one, a plugin and a tool an agent
//! wrote without a view.

use rune::{El, LineCap, Shape, canvas, col, el, row, text};
use serde_json::{Value, json};
use sound_core::{InstanceId, RecordSchema};
use sound_typescript::next::Drawn;

use crate::app::State;
use crate::tree;

/// The most controls a card shows. Cards grow wider, never taller.
const MOST: usize = 8;
const KNOB: f32 = 32.0;
/// How far a drag moves a knob from its start to its end, in points.
const TRAVEL: f32 = 120.0;

#[derive(Clone)]
pub(crate) enum Kind {
    /// `log`: its knob travels in octaves, as a frequency's. `integer`: whole numbers only.
    Number {
        min: f64,
        max: f64,
        log: bool,
        integer: bool,
    },
    Switch,
    Choice(Vec<Value>),
}

/// A field of a record a card shows a control for.
#[derive(Clone)]
pub(crate) struct Field {
    pub(crate) name: String,
    /// What the composer reads, such as "Cutoff".
    pub(crate) label: String,
    pub(crate) kind: Kind,
    pub(crate) value: Value,
}

/// What a card of `id` is called: its tool's title, else its tool's name in words.
pub(crate) fn title(state: &State, id: &InstanceId) -> String {
    let tool = state.project.tool_of(id).unwrap_or_default();
    let schema = state.schema(tool);
    let titled = schema
        .as_ref()
        .and_then(|schema| schema.get("x-title")?.as_str().map(str::to_string));
    titled.unwrap_or_else(|| words(tool.rsplit('.').next().unwrap_or(tool)))
}

/// The fields of the record of `id` a card shows, at most [`MOST`].
pub(crate) fn fields(state: &State, id: &InstanceId) -> Vec<Field> {
    let Some(tool) = state.project.tool_of(id) else {
        return Vec::new();
    };
    let Some(schema) = state.schema(tool) else {
        return Vec::new();
    };
    let record: Value = state
        .project
        .state_json(id)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let reader = RecordSchema::new(&schema);
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    for (name, property) in properties {
        let property = reader.resolved(property);
        let given = record
            .get(name)
            .filter(|value| !value.is_null())
            .or_else(|| property.get("default"))
            .cloned();
        let label = property
            .get("title")
            .and_then(Value::as_str)
            .map_or_else(|| words(name), str::to_string);
        let number = |key| property.get(key).and_then(Value::as_f64);
        let options: Vec<Value> = match (property.get("enum"), property.get("oneOf")) {
            (Some(Value::Array(options)), _) => options.clone(),
            (None, Some(Value::Array(variants))) => variants
                .iter()
                .filter_map(|variant| variant.get("const").cloned())
                .collect(),
            _ => Vec::new(),
        };
        let kind = match (
            property.get("type").and_then(Value::as_str),
            number("minimum"),
            number("maximum"),
        ) {
            _ if !options.is_empty() => Kind::Choice(options),
            (Some(kind @ ("number" | "integer")), Some(min), Some(max)) => {
                let log = property.get("x-scale").and_then(Value::as_str) == Some("logarithmic")
                    && min > 0.0;
                let integer = kind == "integer";
                Kind::Number {
                    min,
                    max,
                    log,
                    integer,
                }
            }
            (Some("boolean"), ..) => Kind::Switch,
            _ => continue,
        };
        let value = given.unwrap_or_else(|| match &kind {
            Kind::Number { min, .. } => json!(min),
            Kind::Switch => json!(false),
            Kind::Choice(options) => options.first().cloned().unwrap_or_default(),
        });
        fields.push(Field {
            name: name.clone(),
            label,
            kind,
            value,
        });
        if fields.len() == MOST {
            break;
        }
    }
    fields
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

impl Kind {
    /// `value` as a field of this kind holds it: within its range, and whole for an integer.
    pub(crate) fn fit(&self, value: f64) -> Value {
        match *self {
            Kind::Number {
                min, max, integer, ..
            } => {
                let value = value.clamp(min.min(max), max.max(min));
                match integer {
                    true => json!(value.round() as i64),
                    false => json!(value),
                }
            }
            _ => json!(value),
        }
    }
}

/// A number as a card shows it: at most two decimals.
pub(crate) fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The card of the device `id`.
pub(crate) fn card(state: &State, id: &InstanceId) -> El {
    let theme = &state.theme;
    let body = match state.drawn(id) {
        Some(drawn @ (Drawn::View { .. } | Drawn::Failed(_))) => {
            tree::view(state, id, &state.prefix_of(id), drawn)
        }
        _ if state.pending(id) => el(),
        _ => controls(state, id),
    };
    col()
        .key(format!("{}card", state.prefix_of(id)))
        .padding(theme.space(3))
        .gap(theme.space(2))
        .fill(theme.color("surface"))
        .border(1.0, theme.color("border-strong"))
        .radius(theme.radius("lg").unwrap_or(8.0))
        .child(
            text(title(state, id))
                .font(theme.text("heading"))
                .color(theme.color("text")),
        )
        .child(body)
}

/// A control per field.
pub(crate) fn controls(state: &State, id: &InstanceId) -> El {
    let prefix = state.prefix_of(id);
    row().gap(state.theme.space(4)).children(
        fields(state, id)
            .into_iter()
            .map(|field| control(state, id, &prefix, field)),
    )
}

fn control(state: &State, id: &InstanceId, prefix: &str, field: Field) -> El {
    let theme = &state.theme;
    let key = format!("{prefix}{}", field.name);
    let caption = |words: String| {
        text(words)
            .font(theme.text("label"))
            .color(theme.color("text-secondary"))
    };
    match field.kind {
        Kind::Number { min, max, log, .. } => {
            let value = field.value.as_f64().unwrap_or(min);
            let at = travelled(value, min, max, log) as f32;
            let (id, name, kind) = (id.clone(), field.name.clone(), field.kind.clone());
            let knob = canvas(arc(
                arc(Shape::new(), 1.0, theme.color("fill-strong")),
                at,
                theme.color("text"),
            ))
            .size(KNOB, KNOB)
            .key(key)
            .role("slider")
            .label(field.label.clone())
            .range_value(value, min, max)
            .focusable(true)
            .on_drag(
                move |s: &mut State, drag: &mut rune::Drag, _: &mut rune::Tx| {
                    if drag.phase == rune::Phase::Up && !drag.cancelled() {
                        let at = travelled(value, min, max, log) + f64::from(-drag.dy / TRAVEL);
                        s.set_field(
                            &id,
                            &name,
                            kind.fit(value_at(at.clamp(0.0, 1.0), min, max, log)),
                        );
                    }
                },
            );
            col()
                .gap(theme.space(1))
                .items_center()
                .child(knob)
                .child(caption(field.label))
                .child(
                    text(number(value))
                        .font(theme.text("number"))
                        .color(theme.color("text-muted")),
                )
        }
        Kind::Switch => {
            let on = field.value.as_bool().unwrap_or(false);
            let (id, name) = (id.clone(), field.name.clone());
            let switch = el()
                .key(key)
                .role("switch")
                .label(field.label.clone())
                .checked(on)
                .focusable(true)
                .size(20.0, 20.0)
                .radius(theme.radius("sm").unwrap_or(4.0))
                .border(1.0, theme.color(if on { "green" } else { "border-strong" }))
                .fill(theme.color(if on { "green/25" } else { "fill" }))
                .on_press(move |s: &mut State| s.set_field(&id, &name, json!(!on)));
            col()
                .gap(theme.space(1))
                .items_center()
                .child(switch)
                .child(caption(field.label))
        }
        Kind::Choice(options) => {
            let segments = options.into_iter().map(|option| {
                let chosen = option == field.value;
                let shown = option.as_str().map_or_else(|| option.to_string(), words);
                let (id, name) = (id.clone(), field.name.clone());
                row()
                    .key(format!(
                        "{key}={}",
                        option
                            .as_str()
                            .map_or_else(|| option.to_string(), str::to_string)
                    ))
                    .role("button")
                    .label(format!("{} {shown}", field.label))
                    .checked(chosen)
                    .focusable(true)
                    .h(24.0)
                    .px(theme.space(2))
                    .items_center()
                    .radius(theme.radius("sm").unwrap_or(4.0))
                    .fill(theme.color(if chosen { "selected" } else { "fill" }))
                    .on_press(move |s: &mut State| s.set_field(&id, &name, option.clone()))
                    .child(
                        text(shown)
                            .font(theme.text("label"))
                            .color(theme.color(if chosen { "text" } else { "text-muted" })),
                    )
            });
            col()
                .gap(theme.space(1))
                .child(row().gap(2.0).children(segments))
                .child(caption(field.label))
        }
    }
}

/// Where `value` is on the travel of a knob, from 0 to 1.
fn travelled(value: f64, min: f64, max: f64, log: bool) -> f64 {
    let at = match log {
        true => (value.max(min) / min).ln() / (max / min).ln(),
        false => (value - min) / (max - min),
    };
    if at.is_finite() {
        at.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn value_at(at: f64, min: f64, max: f64, log: bool) -> f64 {
    match log {
        true => min * (max / min).powf(at),
        false => min + at * (max - min),
    }
}

/// The arc of a knob from its start to `at`, of 270 degrees in all.
fn arc(shape: Shape, at: f32, colour: rune::Color) -> Shape {
    let steps = 32;
    let centre = KNOB / 2.0;
    let point = |t: f32| {
        let angle = (135.0 + t * 270.0_f32).to_radians();
        [
            centre + angle.cos() * (centre - 3.0),
            centre + angle.sin() * (centre - 3.0),
        ]
    };
    let points: Vec<[f32; 2]> = (0..=steps)
        .map(|step| point(at * step as f32 / steps as f32))
        .collect();
    if at <= 0.0 {
        return shape;
    }
    shape.stroke_with(points, false, 3.0, LineCap::Round, colour)
}
