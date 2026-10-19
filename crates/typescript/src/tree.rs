//! The tree a card draws, as the host sends it. The types are the contract with `sdk.ts`: a
//! field this side does not know is an error on the card, not something left out.

use gpui::{Div, Hsla, prelude::*, px};
use serde::de::value::MapAccessDeserializer;
use serde::de::{Error, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::tools::Unit;

/// A node of a tree. Its `type` comes first, as `host.ts` writes it, so a large space is read
/// in one pass; serde's tagged enums would read it twice.
#[derive(Debug)]
pub(crate) enum Node {
    Div(DivNode),
    Text(TextNode),
    /// The file of a sample field, and a button that chooses one.
    Sample(SampleNode),
    Space(Box<SpaceNode>),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DivNode {
    /// Keeps the state of the element when the tree is drawn again.
    pub key: Option<String>,
    #[serde(default)]
    pub style: Style,
    /// The index of its click handler in the host.
    #[serde(rename = "onClick")]
    pub on_click: Option<usize>,
    pub children: Vec<Node>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextNode {
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SampleNode {
    pub path: String,
    pub label: Option<String>,
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a node with its `type` first")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let first: Option<String> = map.next_key()?;
        if first.as_deref() != Some("type") {
            return Err(A::Error::custom("a node names its `type` first"));
        }
        let kind: String = map.next_value()?;
        let rest = MapAccessDeserializer::new(map);
        Ok(match kind.as_str() {
            "div" => Node::Div(DivNode::deserialize(rest)?),
            "text" => Node::Text(TextNode::deserialize(rest)?),
            "sample" => Node::Sample(SampleNode::deserialize(rest)?),
            "space" => Node::Space(Box::new(SpaceNode::deserialize(rest)?)),
            other => {
                return Err(A::Error::unknown_variant(
                    other,
                    &["div", "text", "sample", "space"],
                ));
            }
        })
    }
}

/// What of the window a tree moves with, without asking for a new tree.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Uses {
    pub watches: bool,
    pub tick: bool,
}

impl Uses {
    fn and(self, other: Self) -> Self {
        Self {
            watches: self.watches || other.watches,
            tick: self.tick || other.tick,
        }
    }
}

impl Node {
    pub(crate) fn uses(&self) -> Uses {
        match self {
            Self::Div(DivNode { children, .. }) => (children.iter())
                .map(Self::uses)
                .fold(Uses::default(), Uses::and),
            Self::Space(space) => space.uses(),
            Self::Text(_) | Self::Sample(_) => Uses::default(),
        }
    }
}

/// A surface that draws shapes in units of its own, scrolls and zooms, and hears the pointer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SpaceNode {
    pub key: Option<String>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    #[serde(default)]
    pub grow: bool,
    /// Points per unit at zoom 1.
    pub unit: Option<Axes>,
    pub extent: Option<Ranges>,
    pub zoom: Option<Ranges>,
    pub background: Option<Color>,
    pub radius: Option<f32>,
    pub on_press: Option<usize>,
    pub drag: Option<Drag>,
    /// The drags of its shapes, which name theirs by its place here: a drag every note shares
    /// comes once.
    #[serde(default)]
    pub drags: Vec<Drag>,
    pub cursor: Option<Cursor>,
    pub children: Vec<Shape>,
}

impl SpaceNode {
    fn uses(&self) -> Uses {
        self.children
            .iter()
            .fold(Uses::default(), |uses, shape| uses.and(shape.uses()))
    }

    /// Points per unit at zoom 1, across and down.
    pub(crate) fn unit(&self) -> (f64, f64) {
        let unit = self.unit.unwrap_or_default();
        (
            f64::from(unit.x.unwrap_or(1.0)),
            f64::from(unit.y.unwrap_or(1.0)),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Axes {
    pub x: Option<f32>,
    pub y: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ranges {
    pub x: Option<[f32; 2]>,
    pub y: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Cursor {
    Pointer,
    Grab,
    Crosshair,
    EwResize,
    NsResize,
}

impl Cursor {
    pub(crate) fn style(self) -> gpui::CursorStyle {
        match self {
            Self::Pointer => gpui::CursorStyle::PointingHand,
            Self::Grab => gpui::CursorStyle::OpenHand,
            Self::Crosshair => gpui::CursorStyle::Crosshair,
            Self::EwResize => gpui::CursorStyle::ResizeLeftRight,
            Self::NsResize => gpui::CursorStyle::ResizeUpDown,
        }
    }
}

/// A shape of a space. One struct for every kind, as the fields a kind does not have are left
/// out by `sdk.ts`'s types; a field no kind has is still an error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Shape {
    pub kind: ShapeKind,
    pub key: Option<String>,
    pub on_press: Option<usize>,
    /// Its drag, by its place in the space's `drags`.
    pub drag: Option<usize>,
    pub anchor: Option<Axes>,
    pub hover: Option<Color>,
    pub cursor: Option<Cursor>,
    pub opacity: Option<Num>,
    pub x: Option<Num>,
    pub y: Option<Num>,
    pub width: Option<Num>,
    pub height: Option<Num>,
    pub radius: Option<Num>,
    pub color: Option<Color>,
    pub border: Option<Color>,
    pub border_width: Option<Num>,
    pub from: Option<Reach>,
    pub to: Option<Reach>,
    pub text: Option<Words>,
    pub unit: Option<Unit>,
    pub size: Option<f32>,
    pub align: Option<Align>,
    /// The shapes of a group.
    #[serde(default)]
    pub children: Vec<Shape>,
}

impl Shape {
    fn uses(&self) -> Uses {
        let own = (self.numbers()).fold(Uses::default(), |uses, number| uses.and(number.uses()));
        (self.children.iter()).fold(own, |uses, shape| uses.and(shape.uses()))
    }

    fn numbers(&self) -> impl Iterator<Item = &Num> {
        let reaches = [&self.from, &self.to]
            .into_iter()
            .flatten()
            .flat_map(|reach| match reach {
                Reach::Point([x, y]) => vec![x, y],
                Reach::Turn(turn) => vec![turn],
            });
        let words = match &self.text {
            Some(Words::Live(live)) => Some(live),
            _ => None,
        };
        [
            &self.opacity,
            &self.x,
            &self.y,
            &self.width,
            &self.height,
            &self.radius,
            &self.border_width,
        ]
        .into_iter()
        .flatten()
        .chain(reaches)
        .chain(words)
    }

    /// Whether the pointer can do something with it, so it is found under the pointer.
    pub(crate) fn is_pointed(&self) -> bool {
        self.on_press.is_some()
            || self.drag.is_some()
            || self.hover.is_some()
            || self.cursor.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShapeKind {
    Group,
    Rect,
    Circle,
    Line,
    Arc,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Align {
    Left,
    Center,
    Right,
}

/// An end of a line, a point, or of an arc, a turn.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Reach {
    Point([Num; 2]),
    Turn(Num),
}

/// The text of a shape: as it is, or a live number as a readout.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Words {
    Fixed(String),
    Live(Num),
}

/// A number, or a live value the window works out on every frame. Read by hand: serde's
/// untagged enums buffer every number of a tree.
#[derive(Clone, Debug)]
pub(crate) enum Num {
    Fixed(f32),
    Live(Box<Live>),
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NumVisitor)
    }
}

struct NumVisitor;

impl<'de> Visitor<'de> for NumVisitor {
    type Value = Num;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a number or a live value")
    }

    fn visit_f64<E: Error>(self, value: f64) -> Result<Num, E> {
        Ok(Num::Fixed(value as f32))
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Num, E> {
        Ok(Num::Fixed(value as f32))
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Num, E> {
        Ok(Num::Fixed(value as f32))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Num, A::Error> {
        Ok(Num::Live(Box::new(Live::deserialize(
            MapAccessDeserializer::new(map),
        )?)))
    }
}

impl Num {
    fn uses(&self) -> Uses {
        match self {
            Self::Fixed(_) => Uses::default(),
            Self::Live(live) => live.uses(),
        }
    }
}

/// A live value, as `sdk.ts`'s `Live` sends it.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Live {
    Tick,
    Watch { name: String },
    Field { name: String, fraction: bool },
    Control { name: String, fraction: bool },
    Drag { id: String, part: DragPart },
    Space { key: String, part: SpacePart },
    Plus { a: Num, b: Num },
    Minus { a: Num, b: Num },
    Times { a: Num, b: Num },
    Over { a: Num, b: Num },
    Min { a: Num, b: Num },
    Max { a: Num, b: Num },
    Lt { a: Num, b: Num },
    Gt { a: Num, b: Num },
    Le { a: Num, b: Num },
    Ge { a: Num, b: Num },
    Eq { a: Num, b: Num },
    Ne { a: Num, b: Num },
    Snap { a: Num, b: Num },
    Negate { x: Num },
    Abs { x: Num },
    Floor { x: Num },
    Round { x: Num },
    Sin { x: Num },
    Cos { x: Num },
}

impl Live {
    fn uses(&self) -> Uses {
        match self {
            Self::Tick => Uses {
                tick: true,
                ..Uses::default()
            },
            Self::Watch { .. } => Uses {
                watches: true,
                ..Uses::default()
            },
            Self::Field { .. } | Self::Control { .. } | Self::Drag { .. } | Self::Space { .. } => {
                Uses::default()
            }
            Self::Plus { a, b }
            | Self::Minus { a, b }
            | Self::Times { a, b }
            | Self::Over { a, b }
            | Self::Min { a, b }
            | Self::Max { a, b }
            | Self::Lt { a, b }
            | Self::Gt { a, b }
            | Self::Le { a, b }
            | Self::Ge { a, b }
            | Self::Eq { a, b }
            | Self::Ne { a, b }
            | Self::Snap { a, b } => a.uses().and(b.uses()),
            Self::Negate { x }
            | Self::Abs { x }
            | Self::Floor { x }
            | Self::Round { x }
            | Self::Sin { x }
            | Self::Cos { x } => x.uses(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DragPart {
    StartX,
    StartY,
    Dx,
    Dy,
    X,
    Y,
    Active,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SpacePart {
    Left,
    Top,
    Right,
    Bottom,
    PointX,
    PointY,
}

/// The pointer pressing and dragging a shape or a space.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Drag {
    pub id: Option<String>,
    pub snap: Option<Snap>,
    pub axis: Option<Axis>,
    pub x: Option<Turns>,
    pub y: Option<Turns>,
    pub on_end: Option<usize>,
    pub on_move: Option<usize>,
}

/// Whole steps of a drag, in units: of how far it went, or with `grid`, of where the anchor of
/// what it holds lands.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snap {
    pub x: Option<f32>,
    pub y: Option<f32>,
    #[serde(default)]
    pub grid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Axis {
    X,
    Y,
}

/// A number a drag turns as it goes: a field of the record, or a live control.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Turns {
    pub field: Option<String>,
    pub control: Option<String>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub unit: Option<Unit>,
    /// Points of pointer travel for the whole range.
    pub travel: Option<f32>,
    #[serde(default)]
    pub absolute: bool,
    pub default: Option<f32>,
    pub label: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Style {
    direction: Option<Direction>,
    gap: Option<f32>,
    padding: Option<f32>,
    padding_x: Option<f32>,
    padding_y: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    grow: bool,
    align: Option<Place>,
    justify: Option<Justify>,
    background: Option<Color>,
    color: Option<Color>,
    font_size: Option<f32>,
    radius: Option<f32>,
    border_color: Option<Color>,
    border_width: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Direction {
    Row,
    Column,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Place {
    Start,
    Center,
    End,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Justify {
    Start,
    Center,
    End,
    Between,
}

/// `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Color(pub(crate) Hsla);

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let wrong = || format!("{text:?} is not a colour such as \"#ff8800\"");
        let hex = text.strip_prefix('#').ok_or_else(wrong)?;
        let value = u32::from_str_radix(hex, 16).map_err(|_| wrong())?;
        let rgba = match hex.len() {
            6 => (value << 8) | 0xff,
            8 => value,
            _ => return Err(wrong()),
        };
        Ok(Self(gpui::rgba(rgba).into()))
    }
}

impl Style {
    pub(crate) fn apply(&self, element: Div) -> Div {
        let mut element = element.flex();
        element = match self.direction {
            Some(Direction::Row) => element.flex_row(),
            Some(Direction::Column) | None => element.flex_col(),
        };
        if let Some(gap) = self.gap {
            element = element.gap(px(gap));
        }
        if let Some(padding) = self.padding {
            element = element.p(px(padding));
        }
        if let Some(padding) = self.padding_x {
            element = element.px(px(padding));
        }
        if let Some(padding) = self.padding_y {
            element = element.py(px(padding));
        }
        if let Some(width) = self.width {
            element = element.w(px(width));
        }
        if let Some(height) = self.height {
            element = element.h(px(height));
        }
        if self.grow {
            element = element.flex_grow(1.);
        }
        element = match self.align {
            Some(Place::Start) => element.items_start(),
            Some(Place::Center) => element.items_center(),
            Some(Place::End) => element.items_end(),
            None => element,
        };
        element = match self.justify {
            Some(Justify::Start) => element.justify_start(),
            Some(Justify::Center) => element.justify_center(),
            Some(Justify::End) => element.justify_end(),
            Some(Justify::Between) => element.justify_between(),
            None => element,
        };
        if let Some(Color(color)) = self.background {
            element = element.bg(color);
        }
        if let Some(Color(color)) = self.color {
            element = element.text_color(color);
        }
        if let Some(size) = self.font_size {
            element = element.text_size(px(size));
        }
        if let Some(radius) = self.radius {
            element = element.rounded(px(radius));
        }
        if let Some(Color(color)) = self.border_color {
            element = element.border_color(color);
        }
        if let Some(width) = self.border_width {
            element = element.border(px(width));
        }
        element
    }
}

/// Sets the field `name` of a record.
pub(crate) fn set_field(state: &mut serde_json::Value, name: &str, value: serde_json::Value) {
    if let Some(fields) = state.as_object_mut() {
        fields.insert(name.to_string(), value);
    }
}

/// A number as a record writes it: the shortest decimal that reads back as the same `f32`. A
/// knob at 16.7 writes 16.7, not the 16.700000762939453 that a widening to `f64` would.
pub(crate) fn decimal(number: f32) -> serde_json::Value {
    let decimal = number.to_string().parse::<f64>();
    serde_json::Value::from(decimal.unwrap_or(f64::from(number)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_is_written_as_the_decimal_it_shows() {
        assert_eq!(decimal(16.7).to_string(), "16.7");
        assert_eq!(decimal(0.1).to_string(), "0.1");
    }

    #[test]
    fn a_field_the_runtime_does_not_know_is_an_error() {
        let trees = [
            (
                serde_json::json!({ "type": "div", "style": { "margin": 4 }, "children": [] }),
                "margin",
            ),
            (
                serde_json::json!({ "type": "space", "children": [], "shapes": [] }),
                "shapes",
            ),
        ];
        for (tree, field) in trees {
            let error = serde_json::from_value::<Node>(tree)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(&format!("unknown field `{field}`")),
                "{error}"
            );
        }
    }
}
