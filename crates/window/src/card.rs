//! The card of a device: its own view when its tool has one, which draws the whole card, else a
//! control per field of its record, made from the record's schema: a knob for a number, a switch for a yes or no, and
//! segments for a choice. So every device shows, a built-in one, a plugin and a tool an agent
//! wrote without a view. Its controls sit in whole columns of two cells, the first few shown
//! and the rest behind expand, so a card grows wider, never taller. It looks as the template's
//! `ui/blocks/device-card.tsx` and `ui/knob.tsx` do; Bun has no schema of a built-in record to
//! draw it with them.

use rune::{El, LineCap, Shape, canvas, col, el, row, text};
use serde_json::{Value, json};
use sound_core::{InstanceId, RecordSchema};
use sound_typescript::live::{field_readout, text_of};
use sound_typescript::next::Drawn;

use crate::app::State;
use crate::tree;

/// How many controls a card shows until it is expanded.
const FEW: usize = 4;
/// A cell of a card: a control, its label line and its value line. A choice takes two columns
/// or more.
/// From the edge of a knob's dial to its face, and the width of its arc, as `ui/knob.tsx` has them.
const RIM: f32 = 7.5;
const ARC: f32 = 2.5;

/// The grid of a card from the theme: a column's width, a row's height and a knob's dial.
struct Grid {
    column: f32,
    row: f32,
    knob: f32,
}

impl Grid {
    fn of(theme: &crate::theme::Theme) -> Self {
        Self {
            column: theme.size("cell", 64.0),
            row: theme.size("row", 72.0),
            knob: theme.size("knob", 36.0),
        }
    }
}
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

/// The fields of the record of `id` a card shows: the first few until it is expanded, and
/// whether it has more.
pub(crate) fn shown(state: &State, id: &InstanceId) -> (Vec<Field>, bool) {
    let mut fields = fields(state, id);
    let more = fields.len() > FEW;
    if !state.expanded.contains(id) {
        fields.truncate(FEW);
    }
    (fields, more)
}

/// The fields of the record of `id` a card has a control for.
fn fields(state: &State, id: &InstanceId) -> Vec<Field> {
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
    let prefix = state.prefix_of(id);
    let (body, more) = match state.drawn(id) {
        // A view draws its whole card.
        Some(drawn @ Drawn::View { .. }) => return tree::view(state, id, &prefix, drawn),
        Some(drawn @ Drawn::Failed(_)) => (tree::view(state, id, &prefix, drawn), false),
        _ if state.pending(id) => (el(), false),
        _ => controls(state, id),
    };
    let title = title(state, id);
    let expand = more.then(|| {
        let expanded = state.expanded.contains(id);
        let id = id.clone();
        row()
            .key(format!("{prefix}expand"))
            .role("switch")
            .label(format!("Expand {title}"))
            .checked(expanded)
            .focusable(true)
            .h(theme.size("sm", 24.0))
            .px(theme.space(2))
            .items_center()
            .radius(theme.radius("sm").unwrap_or(4.0))
            .on_press(move |s: &mut State| s.expand(&id))
            .child(
                text(if expanded { "Less" } else { "More" })
                    .font(theme.text("label"))
                    .color(theme.color("text-muted")),
            )
    });
    col()
        .key(format!("{prefix}card"))
        .px(theme.space(4))
        .pb(theme.space(4))
        .fill(theme.color("surface"))
        .border(1.0, theme.color("border-strong"))
        .radius(theme.radius("lg").unwrap_or(8.0))
        .child(
            row()
                .h(theme.size("lg", 32.0))
                .gap(theme.space(2))
                .items_center()
                .justify_between()
                .child(
                    text(title)
                        .font(theme.text("heading"))
                        .color(theme.color("text")),
                )
                .children(expand),
        )
        .child(body)
}

/// A control per field shown, in whole columns of two cells, and whether there are more.
pub(crate) fn controls(state: &State, id: &InstanceId) -> (El, bool) {
    let (fields, more) = shown(state, id);
    let prefix = state.prefix_of(id);
    let mut columns: Vec<(bool, Vec<El>)> = Vec::new();
    for field in fields {
        let wide = matches!(field.kind, Kind::Choice(_));
        match columns.last_mut() {
            Some((same, cells)) if *same == wide && cells.len() < 2 => {
                cells.push(control(state, id, &prefix, field));
            }
            _ => columns.push((wide, vec![control(state, id, &prefix, field)])),
        }
    }
    let gap = state.theme.space(2);
    let grid = Grid::of(&state.theme);
    let columns = columns.into_iter().map(|(wide, cells)| {
        let column = col().h(2.0 * grid.row).children(cells);
        match wide {
            // As wide as its segments, two columns at least.
            true => column.min_w(2.0 * grid.column + gap),
            false => column.w(grid.column),
        }
    });
    (row().children(columns), more)
}

fn control(state: &State, id: &InstanceId, prefix: &str, field: Field) -> El {
    let theme = &state.theme;
    let key = format!("{prefix}{}", field.name);
    let caption = |words: String| {
        text(words)
            .font(theme.text("label"))
            .color(theme.color("text-secondary"))
            .truncate()
    };
    let grid = Grid::of(theme);
    let cell = || col().h(grid.row).items_center();
    match field.kind {
        Kind::Number { min, max, log, .. } => {
            let turned = (state.turning.as_ref()).filter(|(turned, _)| *turned == key);
            let record = field.value.as_f64().unwrap_or(min);
            let value = turned.map_or(record, |(_, value)| *value);
            let at = travelled(value, min, max, log) as f32;
            let (id, name, kind, turning) = (
                id.clone(),
                field.name.clone(),
                field.kind.clone(),
                key.clone(),
            );
            let face = el()
                .absolute(RIM, RIM)
                .size(grid.knob - 2.0 * RIM, grid.knob - 2.0 * RIM)
                .radius(grid.knob / 2.0 - RIM)
                .fill(theme.color("fill"));
            let track = arc(Shape::new(), grid.knob, 1.0, theme.color("fill-strong"));
            let dial =
                canvas(arc(track, grid.knob, at, theme.color("text"))).size(grid.knob, grid.knob);
            let knob = el()
                .size(grid.knob, grid.knob)
                .child(dial)
                .child(face)
                .key(key)
                .role("slider")
                .label(field.label.clone())
                .range_value(value, min, max)
                .focusable(true)
                .on_drag(
                    move |s: &mut State, drag: &mut rune::Drag, _: &mut rune::Tx| {
                        let at = travelled(record, min, max, log) + f64::from(-drag.dy / TRAVEL);
                        let to = kind.fit(value_at(at.clamp(0.0, 1.0), min, max, log));
                        match drag.phase {
                            rune::Phase::Down => {}
                            rune::Phase::Move => {
                                s.turning = Some((turning.clone(), to.as_f64().unwrap_or(record)));
                            }
                            rune::Phase::Up => {
                                s.turning = None;
                                if !drag.cancelled() {
                                    s.set_field(&id, &name, to);
                                }
                            }
                        }
                    },
                );
            cell()
                .child(knob)
                .child(el().h(2.0))
                .child(caption(field.label))
                .child(
                    text(match field_readout(&field.name, min, max) {
                        Some(unit) => text_of(value, Some(unit), None),
                        None => number(value),
                    })
                    .font(theme.text("number"))
                    .color(theme.color("text")),
                )
        }
        Kind::Switch => {
            // A switch carries its name and is lit while on.
            let on = field.value.as_bool().unwrap_or(false);
            let (id, name) = (id.clone(), field.name.clone());
            let switch = row()
                .key(key)
                .role("switch")
                .label(field.label.clone())
                .checked(on)
                .focusable(true)
                .h(theme.size("sm", 24.0))
                .px(theme.space(2))
                .items_center()
                .radius(theme.radius("sm").unwrap_or(4.0))
                .w(grid.column - 8.0)
                .justify_center()
                .border(
                    1.0,
                    theme.color(if on { "#00000000" } else { "border-strong" }),
                )
                .fill(match on {
                    true => theme.tinted("on"),
                    false => theme.color("#00000000"),
                })
                .on_press(move |s: &mut State| s.set_field(&id, &name, json!(!on)))
                .child(
                    text(field.label)
                        .font(theme.text("label"))
                        .color(theme.color(if on { "text" } else { "text-secondary" }))
                        .truncate(),
                );
            cell().child(switch)
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
                    .h(theme.size("sm", 24.0))
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
            cell()
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

/// The arc of a knob `size` points wide from its start to `at`, of 270 degrees in all.
fn arc(shape: Shape, size: f32, at: f32, colour: rune::Color) -> Shape {
    let steps = 32;
    let centre = size / 2.0;
    let point = |t: f32| {
        let angle = (135.0 + t * 270.0_f32).to_radians();
        [
            centre + angle.cos() * (centre - ARC / 2.0),
            centre + angle.sin() * (centre - ARC / 2.0),
        ]
    };
    let points: Vec<[f32; 2]> = (0..=steps)
        .map(|step| point(at * step as f32 / steps as f32))
        .collect();
    if at <= 0.0 {
        return shape;
    }
    shape.stroke_with(points, false, ARC, LineCap::Butt, colour)
}
