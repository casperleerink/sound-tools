//! A view's tree as `sdk/view.ts` sends it, drawn with Rune. What moves every frame is bound
//! here to a [`Live`] value by name; a press or the end of a drag goes to the tool's code.

use std::rc::Rc;

use rune::{El, LineCap, Phase, ScrollSpec, Shape, canvas, col, el, memo, row, text};
use serde::Deserialize;
use serde_json::{Value, json};
use sound_core::InstanceId;
use sound_typescript::next::Drawn;

use crate::app::State;

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Node {
    Row(BoxNode),
    Col(BoxNode),
    #[serde(rename = "box")]
    Plain(BoxNode),
    Text(TextNode),
    Shapes(ShapesNode),
    Child {
        path: String,
    },
    Memo {
        key: String,
    },
}

#[derive(Deserialize, Clone, Copy)]
#[serde(untagged)]
enum Size {
    Points(f32),
    Full(Full),
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Full {
    Full,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoxNode {
    key: Option<String>,
    label: Option<String>,
    role: Option<String>,
    checked: Option<bool>,
    value: Option<Value>,
    #[serde(default)]
    press: bool,
    drag: Option<DragSpec>,
    w: Option<Size>,
    h: Option<Size>,
    grow: Option<f32>,
    pad: Option<u32>,
    px: Option<u32>,
    py: Option<u32>,
    gap: Option<u32>,
    align: Option<String>,
    justify: Option<String>,
    at: Option<[f32; 2]>,
    inset: Option<[Option<f32>; 4]>,
    #[serde(default)]
    clip: bool,
    #[serde(default)]
    scroll: bool,
    #[serde(default)]
    inert: bool,
    fill: Option<String>,
    border: Option<String>,
    border_sides: Option<[f32; 4]>,
    radius: Option<String>,
    shadow: Option<String>,
    opacity: Option<f32>,
    x: Option<LiveX>,
    level: Option<String>,
    #[serde(default)]
    children: Vec<Node>,
}

#[derive(Deserialize, Clone)]
pub(crate) struct LiveX {
    pub(crate) live: String,
    #[serde(default = "one")]
    pub(crate) scale: f32,
    #[serde(default)]
    pub(crate) offset: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Deserialize, Clone, Copy)]
struct DragSpec {
    axis: Axis,
    min: f32,
    max: f32,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Axis {
    X,
    Y,
}

#[derive(Deserialize)]
struct TextNode {
    key: Option<String>,
    at: Option<[f32; 2]>,
    style: Option<String>,
    color: Option<String>,
    w: Option<f32>,
    #[serde(default)]
    truncate: bool,
    live: Option<String>,
    text: String,
}

#[derive(Deserialize)]
struct ShapesNode {
    key: Option<String>,
    at: Option<[f32; 2]>,
    w: f32,
    h: f32,
    units: Option<[f32; 2]>,
    #[serde(default)]
    lines: Vec<Lines>,
    #[serde(default)]
    rects: Vec<Rects>,
}

#[derive(Deserialize)]
struct Lines {
    color: String,
    width: f32,
    points: Vec<f32>,
}

#[derive(Deserialize)]
struct Rects {
    color: String,
    rects: Vec<f32>,
}

/// The view of one instance being drawn: where its keys and live names point.
struct Scope<'a> {
    state: &'a State,
    instance: &'a InstanceId,
    /// What its element keys start with in the window: nothing for the main view, else its
    /// path from there and `:`.
    prefix: &'a str,
    memos: &'a std::collections::HashMap<String, (u64, Rc<Value>)>,
}

impl Scope<'_> {
    fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    fn live(&self, name: &str) -> String {
        crate::lives::live_name(self.instance, name)
    }
}

/// The view `drawn` of `instance`, with its keys after `prefix`.
pub(crate) fn view(state: &State, instance: &InstanceId, prefix: &str, drawn: &Drawn) -> El {
    match drawn {
        Drawn::View { root, memos } => {
            let scope = Scope {
                state,
                instance,
                prefix,
                memos,
            };
            match Node::deserialize(&**root) {
                Ok(node) => build(&scope, &node),
                Err(error) => failed(state, &format!("its view sent what is no view: {error}")),
            }
        }
        Drawn::Failed(error) => failed(state, error),
        Drawn::None => el(),
    }
}

pub(crate) fn failed(state: &State, message: &str) -> El {
    text(format!("This view does not draw: {message}"))
        .font(state.theme.text("label"))
        .color(state.theme.color("peach"))
        .wrap()
}

fn build(scope: &Scope, node: &Node) -> El {
    match node {
        Node::Row(b) => boxed(scope, row(), b),
        Node::Col(b) => boxed(scope, col(), b),
        Node::Plain(b) => boxed(scope, el(), b),
        Node::Text(t) => label(scope, t),
        Node::Shapes(shapes) => drawing(scope, shapes),
        Node::Child { path } => match scope.instance.child(path) {
            Ok(child) => crate::card::card(scope.state, &child),
            Err(error) => failed(scope.state, &error.to_string()),
        },
        Node::Memo { key } => match scope.memos.get(key) {
            Some((version, node)) => {
                // Its controls are listed by the build that made it; Rune keeps that build.
                let node = node.clone();
                let built = || match Node::deserialize(&*node) {
                    Ok(node) => build(scope, &node),
                    Err(error) => failed(scope.state, &error.to_string()),
                };
                memo(scope.key(key), *version, built)
            }
            None => el(),
        },
    }
}

fn boxed(scope: &Scope, mut e: El, b: &BoxNode) -> El {
    let theme = &scope.state.theme;
    let key = b.key.as_deref().map(|key| scope.key(key));
    if let Some(key) = &key {
        e = e.key(key.clone());
    }
    e = match b.w {
        Some(Size::Points(w)) => e.w(w),
        Some(Size::Full(_)) => e.w_full(),
        None => e,
    };
    e = match b.h {
        Some(Size::Points(h)) => e.h(h),
        Some(Size::Full(_)) => e.h_full(),
        None => e,
    };
    if let Some(grow) = b.grow {
        e = e.grow(grow);
    }
    if let Some(step) = b.pad {
        e = e.padding(theme.space(step));
    }
    if let Some(step) = b.px {
        e = e.px(theme.space(step));
    }
    if let Some(step) = b.py {
        e = e.py(theme.space(step));
    }
    if let Some(step) = b.gap {
        e = e.gap(theme.space(step));
    }
    match b.align.as_deref() {
        Some("center") => e = e.items_center(),
        Some("end") => e = e.items_end(),
        _ => {}
    }
    match b.justify.as_deref() {
        Some("center") => e = e.justify_center(),
        Some("between") => e = e.justify_between(),
        Some("end") => e = e.justify_end(),
        _ => {}
    }
    if let Some([x, y]) = b.at {
        e = e.absolute(x, y);
    }
    if let Some([left, top, right, bottom]) = b.inset {
        e = e.pin(left, top, right, bottom);
    }
    if b.clip {
        e = e.clip_soft(0.0);
    }
    if b.scroll {
        e = e.scroll(ScrollSpec {
            stick_bottom: false,
        });
    }
    if b.inert {
        e = e.inert();
    }
    if let Some(colour) = &b.fill {
        e = e.fill(theme.color(colour));
    }
    if let Some(colour) = &b.border {
        e = match b.border_sides {
            Some(sides) => e.border_sides(sides, theme.color(colour)),
            None => e.border(1.0, theme.color(colour)),
        };
    }
    if let Some(radius) = b.radius.as_deref().and_then(|name| theme.radius(name)) {
        e = e.radius(radius);
    }
    if let Some(shadow) = b.shadow.as_deref().and_then(|name| theme.shadow(name)) {
        e = e.shadow(shadow.y, shadow.blur, theme.color(&shadow.color));
    }
    if let Some(opacity) = b.opacity {
        e = e.opacity(opacity);
    }
    e = semantics(scope, e, b, key.as_deref());
    if let Some(key) = &key {
        let lives = &scope.state.lives;
        if let Some(spec) = &b.x {
            e = e.live_offset(&lives.offset(
                key,
                LiveX {
                    live: scope.live(&spec.live),
                    ..spec.clone()
                },
            ));
        }
        if let (Some(name), Some(Size::Points(w)), Some(Size::Points(h))) = (&b.level, b.w, b.h) {
            let level = lives.level(key, scope.live(name), [w, h]);
            e = e.pin(Some(0.0), Some(0.0), None, None).live_rect(&level);
        }
        if let (Some(spec), Some(element)) = (b.drag, &b.key) {
            let moved = lives.dragged(key);
            let (instance, element) = (scope.instance.clone(), element.clone());
            e = e.live_offset(&moved).on_drag(
                move |s: &mut State, d: &mut rune::Drag, _: &mut rune::Tx| {
                    let along =
                        if spec.axis == Axis::X { d.dx } else { d.dy }.clamp(spec.min, spec.max);
                    let offset = if spec.axis == Axis::X {
                        [along, 0.0]
                    } else {
                        [0.0, along]
                    };
                    match d.phase {
                        Phase::Down => {}
                        Phase::Move => {
                            moved.set(offset);
                        }
                        Phase::Up if d.cancelled() => {
                            moved.set([0.0; 2]);
                        }
                        Phase::Up => {
                            moved.set(offset);
                            s.act_on_view(
                                &instance,
                                &element,
                                "drag",
                                Some(json!({ "dx": offset[0], "dy": offset[1] })),
                            );
                        }
                    }
                },
            );
        }
    }
    e.children(b.children.iter().map(|child| build(scope, child)))
}

/// What the element is for an agent and a screen reader, and what a press does.
fn semantics(scope: &Scope, mut e: El, b: &BoxNode, key: Option<&str>) -> El {
    if let Some(role) = b.role.as_deref().and_then(role) {
        e = e.role(role);
    }
    if let Some(label) = &b.label {
        e = e.label(label.clone());
    }
    if let Some(checked) = b.checked {
        e = e.checked(checked);
    }
    if let Some(value) = &b.value {
        e = e.a11y_value(
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_string),
        );
    }
    if let (true, Some(element)) = (b.press && key.is_some(), &b.key) {
        let (instance, element) = (scope.instance.clone(), element.clone());
        e = e
            .focusable(true)
            .on_press(move |s: &mut State| s.act_on_view(&instance, &element, "press", None));
    }
    e
}

/// A role of `sdk/view.ts` as Rune names it: ARIA's names.
fn role(name: &str) -> Option<&'static str> {
    [
        "button", "switch", "slider", "tab", "meter", "group", "heading",
    ]
    .into_iter()
    .find(|known| *known == name)
}

fn label(scope: &Scope, t: &TextNode) -> El {
    let theme = &scope.state.theme;
    let mut e = text(t.text.clone())
        .font(theme.text(t.style.as_deref().unwrap_or("body")))
        .color(theme.color(t.color.as_deref().unwrap_or("text")));
    if let Some(key) = &t.key {
        e = e.key(scope.key(key));
    }
    if let Some(w) = t.w {
        e = e.w(w);
    }
    if t.truncate {
        e = e.truncate();
    }
    if let Some([x, y]) = t.at {
        e = e.absolute(x, y);
    }
    if t.live.as_deref() == Some("time") {
        e = e.live_text(&scope.state.lives.time);
    }
    e
}

fn drawing(scope: &Scope, d: &ShapesNode) -> El {
    let theme = &scope.state.theme;
    let mut shape = Shape::new();
    for lines in &d.lines {
        let colour = theme.color(&lines.color);
        for segment in lines.points.chunks_exact(4) {
            shape = shape.line(
                [segment[0], segment[1]],
                [segment[2], segment[3]],
                lines.width,
                LineCap::Butt,
                colour,
            );
        }
    }
    for rects in &d.rects {
        let colour = theme.color(&rects.color);
        for rect in rects.rects.chunks_exact(4) {
            let [x, y, w, h] = [rect[0], rect[1], rect[2], rect[3]];
            shape = shape.fill(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], colour);
        }
    }
    if let Some([across, down]) = d.units {
        shape = shape.view_box(across, down);
    }
    let mut e = canvas(shape).size(d.w, d.h);
    if let Some(key) = &d.key {
        e = e.key(scope.key(key));
    }
    if let Some([x, y]) = d.at {
        e = e.absolute(x, y);
    }
    e
}
