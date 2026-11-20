//! A view's tree as `sdk/view.ts` sends it, drawn with Rune. What moves every frame is bound
//! here to a live value of [`crate::lives`]; a press or the end of a drag goes to the tool's code.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rune::El;
use serde::Deserialize;
use serde_json::Value;
use sound_core::InstanceId;
use sound_typescript::live::{Num, Readout};
use sound_typescript::next::Drawn;

use crate::app::State;
use crate::lives::Shows;

mod build;
mod drop;
mod field;
mod gesture;
mod icon;
mod overlay;
mod plane;
mod reach;
mod semantics;
mod shapes;

pub(crate) use build::{bound, code_failed, enterable, keyed, view};
pub(crate) use field::{Fields, follow_fields};
pub(crate) use gesture::Gesture;
pub(crate) use icon::{Icons, report_missing};
pub(crate) use overlay::overlays;
pub(crate) use plane::{MISPLACED, Planes};
pub(crate) use reach::Reach;
pub(crate) use semantics::Press;
pub(crate) use shapes::Listed;

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
    /// It has `onDrop`: files from the system.
    #[serde(default)]
    pub(crate) drop: bool,
    pub(crate) drag: Option<DragSpec>,
    pub(crate) plane: Option<PlaneSpec>,
    pub(crate) place: Option<Place>,
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
    /// A right press on its parent or anything inside opens it, at the pointer.
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

impl Node {
    /// Its key in the view, if it has one.
    pub(crate) fn key(&self) -> Option<&str> {
        match self {
            Self::Row(b) | Self::Col(b) | Self::Plain(b) => b.key.as_deref(),
            Self::Text(t) => t.key.as_deref(),
            Self::Shapes(d) => d.key.as_deref(),
            Self::Field(f) => Some(&f.key),
            Self::Icon(_) | Self::Child { .. } | Self::Memo { .. } => None,
        }
    }
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

/// `drag` of `sdk/view.ts`: along `x`, `y` or both, in units of the plane around it.
#[derive(Deserialize, Clone, Debug)]
#[serde(try_from = "DragWire")]
pub(crate) struct DragSpec {
    pub(crate) x: Option<Bound>,
    pub(crate) y: Option<Bound>,
    /// Grid steps: `from` lands on them. 0 does not snap.
    pub(crate) snap: [f32; 2],
    pub(crate) from: Option<[f32; 2]>,
    /// What `live.drag` follows: its key when left out.
    pub(crate) id: Option<String>,
    /// The numbers of the record it plays while it is held, by path.
    pub(crate) plays: Option<HashMap<String, Num>>,
}

#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
struct DragWire {
    x: Option<Bound>,
    y: Option<Bound>,
    snap: Option<[f32; 2]>,
    from: Option<[f32; 2]>,
    id: Option<String>,
    plays: Option<HashMap<String, Num>>,
}

impl TryFrom<DragWire> for DragSpec {
    type Error = &'static str;

    fn try_from(wire: DragWire) -> Result<Self, Self::Error> {
        if wire.x.is_none() && wire.y.is_none() {
            return Err("a drag moves along x or y");
        }
        Ok(Self {
            x: wire.x,
            y: wire.y,
            snap: wire.snap.unwrap_or_default(),
            from: wire.from,
            id: wire.id,
            plays: wire.plays,
        })
    }
}

/// How far a drag may go along an axis: anywhere, or between the least and the most.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(from = "BoundWire")]
pub(crate) enum Bound {
    Free,
    Within(f32, f32),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BoundWire {
    Within([f32; 2]),
    Free(serde::de::IgnoredAny),
}

impl From<BoundWire> for Bound {
    fn from(wire: BoundWire) -> Self {
        match wire {
            // A view may give the bounds swapped.
            BoundWire::Within([a, b]) => Self::Within(a.min(b), a.max(b)),
            BoundWire::Free(_) => Self::Free,
        }
    }
}

/// `plane` of `sdk/view.ts`: units of a box's own, which the window scrolls and zooms.
#[derive(Deserialize, Clone, Debug)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct PlaneSpec {
    pub(crate) x: Option<AxisSpec>,
    pub(crate) y: Option<AxisSpec>,
    /// How far across it zooms, least and most: not at all when left out.
    pub(crate) zoom: Option<[f32; 2]>,
    /// A place across it keeps in view while it moves.
    pub(crate) follow: Option<Num>,
}

/// Points per unit, a range shown stretched to the box, or the key of a plane it moves with.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub(crate) enum AxisSpec {
    Scale(f32),
    Range([f32; 2]),
    Shared(String),
}

/// `place` of `sdk/view.ts`, in units of the plane around it: a mark, centred on its point and
/// sized in points, or a box.
#[derive(Deserialize, Clone, Debug)]
#[serde(try_from = "Vec<Num>")]
pub(crate) enum Place {
    Mark([Num; 2]),
    Box([Num; 4]),
}

impl TryFrom<Vec<Num>> for Place {
    type Error = &'static str;

    fn try_from(numbers: Vec<Num>) -> Result<Self, Self::Error> {
        match <[Num; 4]>::try_from(numbers) {
            Ok(rect) => Ok(Self::Box(rect)),
            Err(numbers) => <[Num; 2]>::try_from(numbers)
                .map(Self::Mark)
                .map_err(|_| "a place is [x, y] or [x, y, w, h]"),
        }
    }
}

impl Place {
    /// Its top left, or the point of a mark.
    pub(crate) fn origin(&self) -> [Num; 2] {
        match self {
            Self::Mark([x, y]) | Self::Box([x, y, _, _]) => [x.clone(), y.clone()],
        }
    }

    pub(crate) fn numbers(&self) -> &[Num] {
        match self {
            Self::Mark(point) => point,
            Self::Box(rect) => rect,
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
    /// Points, or a live value; left out when `place` sizes it.
    #[serde(default = "no_size")]
    w: Num,
    #[serde(default = "no_size")]
    h: Num,
    place: Option<Place>,
    units: Option<[shapes::Span; 2]>,
    trim: Option<[Num; 2]>,
    #[serde(default)]
    lines: Vec<Lines>,
    #[serde(default)]
    rects: Vec<Rects>,
}

fn no_size() -> Num {
    Num::Fixed(0.0)
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
    points: shapes::Points,
    fill: Option<String>,
}

#[derive(Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
pub(crate) struct Rects {
    color: String,
    rects: shapes::Points,
}

/// The view of one instance being drawn: where its keys and live names point.
#[derive(Clone)]
struct Scope<'a> {
    state: &'a State,
    instance: &'a InstanceId,
    /// What its element keys start with in the window: nothing for the main view, else its
    /// path from there and `:`.
    prefix: &'a str,
    /// The overlay a press opened that this is in, if any.
    within: Option<Within>,
    /// Inside a control: an opener here is no stop of Tab, as the control's Enter opens it.
    in_control: bool,
    /// The key in the window of the plane around it.
    plane: Option<String>,
    /// The place of its parent, when that is placed in the plane: its children are placed from it.
    parent: Option<[Num; 2]>,
    /// The place of the placed element nearest around it, or its own: what a drag snaps.
    placed: Option<[Num; 2]>,
    /// The key in the window of what opens the menu around it, or its own.
    menu: Option<String>,
}

/// An open overlay: the key of what opened it, and whether a press inside closes it.
#[derive(Clone)]
pub(crate) struct Within {
    pub(crate) opener: String,
    pub(crate) picks: bool,
}

/// The trees of the views as nodes, each read once from the JSON Bun sent: a build, the
/// controls and the overlays read them many times.
#[derive(Default)]
pub(crate) struct Trees(RefCell<HashMap<(InstanceId, Option<String>), Read>>);

/// A tree as Bun sent it, kept so its address is not another's, and what it reads as.
type Read = (Rc<Value>, Result<Rc<Node>, String>);

impl Trees {
    /// The node of `value`: the tree of the view of `id`, or its memo `memo`.
    fn read(
        &self,
        id: &InstanceId,
        memo: Option<&str>,
        value: &Rc<Value>,
    ) -> Result<Rc<Node>, String> {
        let mut trees = self.0.borrow_mut();
        let entry = trees.entry((id.clone(), memo.map(str::to_string)));
        let (sent, node) = entry.or_insert_with(|| (value.clone(), Err(String::new())));
        if !Rc::ptr_eq(sent, value) || node.as_ref().is_err_and(String::is_empty) {
            *sent = value.clone();
            *node = Node::deserialize(&**value)
                .map(Rc::new)
                .map_err(|error| error.to_string());
        }
        node.clone()
    }

    /// Forgets the trees the views do not show any more.
    pub(crate) fn keep(&self, state: &State) {
        self.0.borrow_mut().retain(|(id, memo), (sent, _)| {
            let Some(Drawn::View { root, memos }) = state.drawn(id) else {
                return false;
            };
            match memo {
                None => Rc::ptr_eq(root, sent),
                Some(key) => memos
                    .get(key)
                    .is_some_and(|(_, inner)| Rc::ptr_eq(inner, sent)),
            }
        });
    }
}

impl State {
    /// The tree of the view of `id`, when it drew one that reads.
    pub(crate) fn root(&self, id: &InstanceId) -> Option<Rc<Node>> {
        self.read_root(id)?.ok()
    }

    /// The tree of the view of `id`, or why it is no view, when it drew one.
    fn read_root(&self, id: &InstanceId) -> Option<Result<Rc<Node>, String>> {
        match self.drawn(id)? {
            Drawn::View { root, .. } => Some(self.trees.read(id, None, root)),
            _ => None,
        }
    }

    /// The memo `key` of the view of `id`, and its version.
    pub(crate) fn memo_of(&self, id: &InstanceId, key: &str) -> Option<((u64, u64), Rc<Node>)> {
        let Some(Drawn::View { memos, .. }) = self.drawn(id) else {
            return None;
        };
        let (version, value) = memos.get(key)?;
        Some((*version, self.trees.read(id, Some(key), value).ok()?))
    }
}

impl<'a> Scope<'a> {
    fn new(state: &'a State, instance: &'a InstanceId, prefix: &'a str) -> Self {
        Self {
            state,
            instance,
            prefix,
            within: None,
            in_control: false,
            plane: None,
            parent: None,
            placed: None,
            menu: None,
        }
    }

    fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    /// What the memo `key` of this view holds.
    fn memo(&self, key: &str) -> Option<Rc<Node>> {
        self.state.memo_of(self.instance, key).map(|(_, node)| node)
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
