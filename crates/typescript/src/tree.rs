//! The tree a card draws, as the host sends it, and what GPUI draws of it. The types are the
//! contract with `sdk.ts`: a field this side does not know is an error on the card, not
//! something left out.

use gpui::{AnyElement, Div, ElementId, Hsla, SharedString, div, prelude::*, px};
use serde::Deserialize;

use crate::tools::Unit;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Node {
    Div {
        #[serde(default)]
        style: Style,
        /// The index of its click handler in the host.
        #[serde(default, rename = "onClick")]
        on_click: Option<usize>,
        #[serde(default)]
        children: Vec<Node>,
    },
    Text {
        text: String,
    },
    Knob(KnobNode),
    /// A row of steps on a pattern of the record.
    Steps {
        path: String,
        /// What a step that is on holds. The pattern's max when left out.
        #[serde(default)]
        max: Option<f32>,
        /// The watch that says which step plays.
        #[serde(default)]
        playing: Option<String>,
    },
    /// The file of a sample field, and a button that chooses one.
    Sample {
        path: String,
        #[serde(default)]
        label: Option<String>,
    },
    /// A bar from 0 to 1 that shows a watch.
    Meter {
        watch: String,
        #[serde(default)]
        label: Option<String>,
    },
    /// A square that plays two live controls with the pointer.
    Pad {
        x: String,
        y: String,
        #[serde(default)]
        size: Option<f32>,
    },
    Canvas(CanvasNode),
}

/// A surface the code draws shapes on, which hears the pointer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct CanvasNode {
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub background: Option<Color>,
    /// The index of the handler of a press, and of a drag.
    #[serde(default)]
    pub on_press: Option<usize>,
    #[serde(default)]
    pub on_drag: Option<usize>,
}

/// A shape of a canvas, in points from its top left.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Shape {
    Circle {
        x: f32,
        y: f32,
        radius: f32,
        color: Color,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        #[serde(default)]
        radius: Option<f32>,
    },
    Line {
        from: [f32; 2],
        to: [f32; 2],
        color: Color,
        #[serde(default)]
        width: Option<f32>,
    },
}

/// A knob on the field `path` of the record, or on the live control `live`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KnobNode {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub live: Option<String>,
    pub label: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    #[serde(default)]
    pub unit: Option<Unit>,
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

/// What a card does for the nodes that need the view: everything that is played or shows what
/// plays.
pub(crate) trait Controls {
    fn clickable(&mut self, element: Div, id: ElementId, handler: usize) -> AnyElement;
    fn knob(&mut self, knob: &KnobNode) -> AnyElement;
    fn steps(&mut self, path: &str, max: Option<f32>, playing: Option<&str>) -> AnyElement;
    fn meter(&mut self, watch: &str, label: Option<&str>) -> AnyElement;
    fn sample(&mut self, path: &str, label: Option<&str>) -> AnyElement;
    fn pad(&mut self, x: &str, y: &str, size: f32) -> AnyElement;
    fn canvas(&mut self, canvas: &CanvasNode) -> AnyElement;
}

/// `index` tells siblings apart, so every element that needs an id gets its own.
pub(crate) fn draw(node: &Node, index: &[usize], controls: &mut impl Controls) -> AnyElement {
    match node {
        Node::Text { text } => SharedString::from(text.clone()).into_any_element(),
        Node::Knob(knob) => controls.knob(knob),
        Node::Steps { path, max, playing } => controls.steps(path, *max, playing.as_deref()),
        Node::Meter { watch, label } => controls.meter(watch, label.as_deref()),
        Node::Sample { path, label } => controls.sample(path, label.as_deref()),
        Node::Pad { x, y, size } => controls.pad(x, y, size.unwrap_or(120.0)),
        Node::Canvas(canvas) => controls.canvas(canvas),
        Node::Div {
            style,
            on_click,
            children,
        } => {
            let children: Vec<AnyElement> = children
                .iter()
                .enumerate()
                .map(|(position, child)| {
                    let index = [index, &[position]].concat();
                    draw(child, &index, controls)
                })
                .collect();
            let element = style.apply(div()).children(children);
            match on_click {
                Some(handler) => {
                    let path: Vec<String> = index.iter().map(usize::to_string).collect();
                    let id = ElementId::Name(format!("div-{}", path.join("-")).into());
                    controls.clickable(element, id, *handler)
                }
                None => element.into_any_element(),
            }
        }
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
                serde_json::json!({ "type": "steps", "path": "steps", "length": 8 }),
                "length",
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
