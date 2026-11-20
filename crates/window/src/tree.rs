//! A view's tree as `sdk/view.ts` sends it, drawn with Rune. What moves every frame is bound
//! here to a live value of [`crate::lives`]; a press or the end of a drag goes to the tool's code.

use std::collections::HashMap;
use std::rc::Rc;

use rune::El;
use serde::Deserialize;
use serde_json::Value;
use sound_core::InstanceId;
use sound_typescript::live::{Num, Readout};

use crate::app::State;
use crate::lives::Shows;

mod build;
mod field;
mod gesture;
mod icon;
mod overlay;
mod reach;
mod semantics;
mod shapes;

pub(crate) use build::{bound, code_failed, enterable, view};
pub(crate) use field::{Fields, follow_fields};
pub(crate) use gesture::Gesture;
pub(crate) use icon::{Icons, report_missing};
pub(crate) use overlay::overlays;
pub(crate) use reach::Reach;
pub(crate) use semantics::Press;

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
    Field(FieldNode),
    Icon(icon::IconNode),
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
    hover: Option<String>,
    #[serde(default)]
    hover_group: bool,
    #[serde(default)]
    reveal: bool,
    #[serde(default)]
    group: bool,
    pub(crate) overlay: Option<Overlay>,
    border: Option<String>,
    border_sides: Option<[f32; 4]>,
    radius: Option<String>,
    shadow: Option<String>,
    opacity: Option<Num>,
    #[serde(default)]
    pub(crate) children: Vec<Node>,
}

/// How a box shows over everything beside its parent, see `overlay` in `sdk/view.ts`.
#[derive(Deserialize, Clone, Copy)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct Overlay {
    pub(crate) open: Opens,
    #[serde(default)]
    side: Edge,
    #[serde(default)]
    align: Along,
    #[serde(default)]
    gap: f32,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Opens {
    Press,
    Hover,
    /// A right press opens it: not honoured yet, so it stays shut.
    Menu,
}

#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum Edge {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum Along {
    #[default]
    Start,
    Center,
    End,
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
#[serde(try_from = "DragAxes")]
pub(crate) struct DragSpec {
    axis: Axis,
    min: f32,
    max: f32,
}

/// `drag` of `sdk/view.ts` as this window honours it yet: along `y`, else `x`, in its bounds.
#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
struct DragAxes {
    x: Option<Bound>,
    y: Option<Bound>,
}

/// `true` (anywhere), or the least and most of the move.
#[derive(Deserialize)]
#[serde(untagged)]
enum Bound {
    Within([f32; 2]),
    Free(serde::de::IgnoredAny),
}

impl TryFrom<DragAxes> for DragSpec {
    type Error = &'static str;

    fn try_from(DragAxes { x, y }: DragAxes) -> Result<Self, Self::Error> {
        let (axis, along) = match (y, x) {
            (Some(y), _) => (Axis::Y, y),
            (None, Some(x)) => (Axis::X, x),
            (None, None) => return Err("a drag moves along x or y"),
        };
        let [min, max] = match along {
            Bound::Within(bounds) => bounds,
            Bound::Free(_) => [f32::NEG_INFINITY, f32::INFINITY],
        };
        Ok(Self { axis, min, max })
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
    align: Option<String>,
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
    w: Num,
    h: Num,
    units: Option<[f32; 2]>,
    trim: Option<[Num; 2]>,
    #[serde(default)]
    lines: Vec<Lines>,
    #[serde(default)]
    rects: Vec<Rects>,
}

/// A line of text the composer types, see `FieldProps` in `sdk/view.ts`.
#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct FieldNode {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) value: String,
    style: Option<String>,
    color: Option<String>,
    w: Option<f32>,
    align: Option<String>,
    placeholder: Option<String>,
    #[serde(default)]
    pub(crate) focus: bool,
}

#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct Lines {
    color: String,
    width: f32,
    points: Vec<f32>,
    fill: Option<String>,
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
    /// The overlay a press opened that this is in, if any.
    within: Option<Within>,
    /// Inside a control: an opener here is no stop of Tab, as the control's Enter opens it.
    in_control: bool,
}

/// An open overlay: the key of what opened it, and whether a press inside closes it.
#[derive(Clone)]
pub(crate) struct Within {
    pub(crate) opener: String,
    pub(crate) picks: bool,
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
