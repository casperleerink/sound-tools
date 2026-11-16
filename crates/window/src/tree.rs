//! A view's tree as `sdk/view.ts` sends it, drawn with Rune. What moves every frame is bound
//! here to a live value of [`crate::lives`]; a press or the end of a drag goes to the tool's code.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rune::{El, LineCap, Phase, ScrollSpec, Shape, canvas, col, el, memo, row, text};
use serde::Deserialize;
use serde_json::{Value, json};
use sound_core::InstanceId;
use sound_typescript::live::{Num, Readout};
use sound_typescript::next::Drawn;

use crate::app::State;
use crate::lives::{Lives, Shown, Shows};

/// A node of `sdk/view.ts`. A test reads every field it sends, so the two cannot drift.
#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) enum Node {
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

#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum Size {
    Points(Num),
    Full(Full),
}

impl Size {
    fn points(&self) -> Option<&Num> {
        match self {
            Self::Points(points) => Some(points),
            Self::Full(_) => None,
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Full {
    Full,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct BoxNode {
    pub(crate) key: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) role: Option<String>,
    pub(crate) checked: Option<bool>,
    pub(crate) value: Option<Value>,
    #[serde(default)]
    pub(crate) press: bool,
    /// It has `onEnter`: a press held over it paints it.
    #[serde(default)]
    pub(crate) enter: bool,
    /// The names of its `actions`.
    #[serde(default)]
    pub(crate) actions: Vec<String>,
    pub(crate) drag: Option<DragSpec>,
    w: Option<Size>,
    h: Option<Size>,
    grow: Option<f32>,
    pad: Option<u32>,
    px: Option<u32>,
    py: Option<u32>,
    gap: Option<u32>,
    align: Option<String>,
    justify: Option<String>,
    at: Option<[Num; 2]>,
    inset: Option<[Option<f32>; 4]>,
    max_h: Option<f32>,
    #[serde(default)]
    clip: bool,
    scroll: Option<Scroll>,
    #[serde(default)]
    inert: bool,
    fill: Option<String>,
    border: Option<String>,
    border_sides: Option<[f32; 4]>,
    radius: Option<String>,
    shadow: Option<String>,
    opacity: Option<Num>,
    #[serde(default)]
    pub(crate) children: Vec<Node>,
}

impl BoxNode {
    /// Its width and height in points, live or not.
    fn size(&self) -> (Option<&Num>, Option<&Num>) {
        (
            self.w.as_ref().and_then(Size::points),
            self.h.as_ref().and_then(Size::points),
        )
    }
}

impl TextNode {
    fn shows(&self, value: &Num) -> Shows {
        Shows::Text {
            value: value.clone(),
            unit: self.unit,
            digits: self.digits,
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct DragSpec {
    axis: Axis,
    min: f32,
    max: f32,
}

impl DragSpec {
    /// Where a drag by `dx`, `dy` leaves the element: along its axis, within its bounds in
    /// either order, as a view may give them swapped.
    fn offset(self, dx: f32, dy: f32) -> [f32; 2] {
        let (low, high) = (self.min.min(self.max), self.min.max(self.max));
        match self.axis {
            Axis::X => [dx.clamp(low, high), 0.0],
            Axis::Y => [0.0, dy.clamp(low, high)],
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Scroll {
    X,
    Y,
    Both,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Axis {
    X,
    Y,
}

#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct TextNode {
    key: Option<String>,
    at: Option<[Num; 2]>,
    style: Option<String>,
    color: Option<String>,
    w: Option<f32>,
    #[serde(default)]
    truncate: bool,
    opacity: Option<Num>,
    /// A live value it shows in place of `text`, in `unit` or with `digits`.
    live: Option<Num>,
    unit: Option<Readout>,
    digits: Option<usize>,
    text: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct ShapesNode {
    key: Option<String>,
    at: Option<[Num; 2]>,
    opacity: Option<Num>,
    w: f32,
    h: f32,
    units: Option<[f32; 2]>,
    #[serde(default)]
    lines: Vec<Lines>,
    #[serde(default)]
    rects: Vec<Rects>,
}

#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct Lines {
    color: String,
    width: f32,
    points: Vec<f32>,
}

#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct Rects {
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
    memos: &'a Memos,
}

pub(crate) type Memos = HashMap<String, ((u64, u64), Rc<Value>)>;

impl Scope<'_> {
    fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    /// Places `e` at `at`, sized `w` × `h`, at `opacity`, binding what is live.
    fn placed(
        &self,
        mut e: El,
        at: Option<&[Num; 2]>,
        size: (Option<&Num>, Option<&Num>),
        opacity: Option<&Num>,
    ) -> El {
        let live = live_place(at, size, opacity);
        let moved = (live.iter()).any(|shows| matches!(shows, Shows::Rect(_) | Shows::Offset(_)));
        if let (false, Some([x, y])) = (moved, at) {
            e = e.absolute(x.fixed().unwrap_or(0.0), y.fixed().unwrap_or(0.0));
        }
        if let Some(Num::Fixed(opacity)) = opacity {
            e = e.opacity(*opacity);
        }
        live.into_iter().fold(e, |e, shows| self.bind(e, shows))
    }

    fn bind(&self, e: El, shows: Shows) -> El {
        let lives = &self.state.lives;
        lives.bind(self.state, self.instance, shows).apply(e)
    }
}

/// What an element at `at`, sized `w` × `h`, at `opacity`, binds: where any is live, the window
/// moves it every frame, out of the flow.
fn live_place(
    at: Option<&[Num; 2]>,
    (w, h): (Option<&Num>, Option<&Num>),
    opacity: Option<&Num>,
) -> Vec<Shows> {
    let live = |num: &Num| num.fixed().is_none();
    let mut shows = Vec::new();
    let [x, y] = at.cloned().unwrap_or([Num::Fixed(0.0), Num::Fixed(0.0)]);
    if let (Some(w), Some(h)) = (w, h)
        && (live(w) || live(h))
    {
        shows.push(Shows::Rect([x, y, w.clone(), h.clone()]));
    } else if let Some(at) = at.filter(|at| at.iter().any(live)) {
        shows.push(Shows::Offset(at.clone()));
    }
    shows.extend(
        opacity
            .filter(|opacity| live(opacity))
            .cloned()
            .map(Shows::Opacity),
    );
    shows
}

/// The name of every binding the views shown now make, of their memos too, whether Rune builds
/// them again or not: what [`crate::lives::Lives::keep`] keeps.
pub(crate) fn bound(state: &State) -> HashSet<String> {
    let mut named = HashSet::new();
    for id in &state.shown {
        if let Some(Drawn::View { root, memos }) = state.drawn(id)
            && let Ok(node) = Node::deserialize(&**root)
        {
            walk(&node, memos, &mut |shows| {
                named.insert(Lives::name(id, &shows));
            });
        }
    }
    named
}

/// The keys of the elements of `node` and its memos that a paint enters, see [`BoxNode::enter`].
pub(crate) fn enterable(node: &Node, memos: &Memos, keys: &mut Vec<String>) {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            keys.extend(b.key.clone().filter(|_| b.enter));
            for child in &b.children {
                enterable(child, memos, keys);
            }
        }
        Node::Memo { key } => {
            let inner = memos
                .get(key)
                .and_then(|(_, inner)| Node::deserialize(&**inner).ok());
            if let Some(inner) = inner {
                enterable(&inner, memos, keys);
            }
        }
        Node::Text(_) | Node::Shapes(_) | Node::Child { .. } => {}
    }
}

/// Gives `bind` everything `node` binds, and its memos.
fn walk(node: &Node, memos: &Memos, bind: &mut impl FnMut(Shows)) {
    let live = match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            for child in &b.children {
                walk(child, memos, bind);
            }
            live_place(b.at.as_ref(), b.size(), b.opacity.as_ref())
        }
        Node::Text(t) => {
            if let Some(value) = &t.live {
                bind(t.shows(value));
            }
            live_place(t.at.as_ref(), (None, None), t.opacity.as_ref())
        }
        Node::Shapes(d) => live_place(d.at.as_ref(), (None, None), d.opacity.as_ref()),
        Node::Memo { key } => {
            let inner = memos
                .get(key)
                .and_then(|(_, inner)| Node::deserialize(&**inner).ok());
            if let Some(inner) = inner {
                walk(&inner, memos, bind);
            }
            return;
        }
        Node::Child { .. } => return,
    };
    live.into_iter().for_each(bind);
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
                // Rune keys a memo's root with the memo's key: the root keeps its own.
                let root = node.get("key").and_then(Value::as_str).unwrap_or(key);
                memo(scope.key(root), *version, built)
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
    e = match &b.w {
        Some(Size::Points(Num::Fixed(w))) => e.w(*w),
        Some(Size::Full(_)) => e.w_full(),
        _ => e,
    };
    e = match &b.h {
        Some(Size::Points(Num::Fixed(h))) => e.h(*h),
        Some(Size::Full(_)) => e.h_full(),
        _ => e,
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
    // Before `inset`, which pins it to more edges than `at` does.
    e = scope.placed(e, b.at.as_ref(), b.size(), b.opacity.as_ref());
    if let Some([left, top, right, bottom]) = b.inset {
        e = e.pin(left, top, right, bottom);
    }
    if b.clip {
        e = e.clip_soft(0.0);
    }
    if let Some(h) = b.max_h {
        e = e.max_h(h);
    }
    if let Some(scroll) = b.scroll {
        e = scrolled(e, scroll);
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
    e = semantics(scope, e, b, key.as_deref());
    if let Some(key) = &key {
        let lives = &scope.state.lives;
        if let (Some(spec), Some(element)) = (b.drag, &b.key) {
            let moved = lives.dragged(key);
            let (instance, element) = (scope.instance.clone(), element.clone());
            e = e.live_offset(&moved).on_drag(
                move |s: &mut State, d: &mut rune::Drag, _: &mut rune::Tx| {
                    let offset = spec.offset(d.dx, d.dy);
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
                            s.pointed(
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

/// `e` scrolling along `axes`, its edges fading where more waits.
pub(crate) fn scrolled(e: El, axes: Scroll) -> El {
    let e = match axes {
        Scroll::X => e.scroll_x(),
        Scroll::Y => e.scroll(ScrollSpec {
            stick_bottom: false,
        }),
        Scroll::Both => e.scroll_xy(),
    };
    e.scroll_edge_fade(FADE)
}

/// How far the edge of a scroll fades, in points.
const FADE: f32 = 24.0;

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
    if let (true, Some(element)) = (b.enter && key.is_some(), &b.key) {
        let (instance, element) = (scope.instance.clone(), element.clone());
        e = e.on_drag(
            move |s: &mut State, d: &mut rune::Drag, tx: &mut rune::Tx| match d.phase {
                Phase::Down => s.paint(&instance, &element),
                Phase::Move => {
                    let [x, y] = d.window_point();
                    tx.element_at(x, y, |s: &mut State, picked, _: &mut rune::Tx| {
                        if let Some(picked) = picked {
                            s.paint_over(picked.path());
                        }
                    });
                }
                Phase::Up => s.painted(),
            },
        );
    }
    if let (true, Some(element)) = (b.press && key.is_some(), &b.key) {
        let (instance, element) = (scope.instance.clone(), element.clone());
        e = e
            .focusable(true)
            .on_press(move |s: &mut State| s.pointed(&instance, &element, "press", None));
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
    let lives = &scope.state.lives;
    let live =
        (t.live.as_ref()).map(|value| lives.bind(scope.state, scope.instance, t.shows(value)));
    let words = match &live {
        Some(Shown::Text(_, widest)) => widest.borrow().clone(),
        _ => t.text.clone(),
    };
    let mut e = text(words)
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
    if let Some(live) = live {
        e = live.apply(e);
    }
    scope.placed(e, t.at.as_ref(), (None, None), t.opacity.as_ref())
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
    scope.placed(e, d.at.as_ref(), (None, None), d.opacity.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_with_bounds_swapped_stays_between_them() {
        let spec = DragSpec {
            axis: Axis::Y,
            min: 4.0,
            max: -4.0,
        };
        assert_eq!(spec.offset(0.0, 10.0), [0.0, 4.0]);
        assert_eq!(spec.offset(0.0, -10.0), [0.0, -4.0]);
    }
}
