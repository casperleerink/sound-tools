//! The nodes a card makes, as the host sends them: ops on a table the window keeps, so a change
//! of a prop crosses as that prop. The types are the contract with `sdk.ts`: a prop this side
//! does not know is an error on the card, not something left out.

use std::collections::{BTreeSet, HashMap, HashSet};

use gpui::{Div, Hsla, prelude::*, px};
use serde::de::value::MapAccessDeserializer;
use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::tools::Unit;

/// A value whose change may set it, unset it with `null`, or leave it out.
fn changed<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}

/// The props of a kind of node, every one optional, and a change of some of them.
macro_rules! props {
    ($(#[$meta:meta])* $name:ident / $change:ident { $($(#[$field_meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        pub(crate) struct $name {
            $($(#[$field_meta])* pub $field: Option<$ty>,)*
        }

        #[derive(Debug, Default, Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        pub(crate) struct $change {
            $(#[serde(default, deserialize_with = "changed")] $field: Option<Option<$ty>>,)*
        }

        impl $name {
            fn apply(&mut self, change: $change) {
                $(if let Some(value) = change.$field {
                    self.$field = value;
                })*
            }
        }
    };
}

props! {
    DivProps / DivChange {
        /// Keeps the state of the element.
        key: String,
        style: Style,
        /// It hears a click.
        on_click: bool,
    }
}

props! {
    TextProps / TextChange { text: String }
}

props! {
    /// The file of a sample field, and a button that chooses one.
    SampleProps / SampleChange { path: String, label: String }
}

props! {
    /// A surface that draws shapes in units of its own, scrolls and zooms, and hears the pointer.
    SpaceProps / SpaceChange {
        key: String,
        width: f32,
        height: f32,
        grow: bool,
        /// Points per unit at zoom 1.
        unit: Axes,
        extent: Ranges,
        zoom: Ranges,
        /// Where it shows from when it first draws.
        scroll: Axes,
        follow: Follow,
        background: Color,
        radius: f32,
        on_press: bool,
        /// Its drag, by its number among the drags of the card.
        drag: u64,
        cursor: Cursor,
    }
}

props! {
    /// A shape of a space. One struct for every kind, as the props a kind does not have are left
    /// out by `sdk.ts`'s types; a prop no kind has is still an error.
    ShapeProps / ShapeChange {
        kind: ShapeKind,
        on_press: bool,
        drag: u64,
        anchor: Axes,
        /// The drags it moves with, and those it grows with, by their ids.
        moves: Vec<String>,
        grows: Vec<String>,
        hover: Color,
        cursor: Cursor,
        opacity: Num,
        x: Num,
        y: Num,
        width: Num,
        height: Num,
        radius: Num,
        color: Paint,
        border: Color,
        border_width: Num,
        from: Reach,
        to: Reach,
        text: Words,
        unit: Unit,
        size: f32,
        align: Align,
        /// The values a wave draws, oldest first, and the values at its bottom and its top.
        list: List,
        low: f32,
        high: f32,
        /// A wave as a band from the lowest to the highest value under each point, else a line.
        band: bool,
    }
}

/// A node of the table.
#[derive(Clone, Debug)]
pub(crate) enum Props {
    Div(DivProps),
    Text(TextProps),
    Sample(SampleProps),
    Space(Box<SpaceProps>),
    Shape(Box<ShapeProps>),
    /// Where a list or a part that changes puts its nodes: draws nothing.
    Marker,
}

#[derive(Debug)]
pub(crate) enum Change {
    Div(DivChange),
    Text(TextChange),
    Sample(SampleChange),
    Space(Box<SpaceChange>),
    Shape(Box<ShapeChange>),
    Marker,
}

/// One change of the nodes of a card, as `host.ts` writes it: `["create", id, family, props]`,
/// `["set", id, family, props]`, `["insert", parent, id, before]`, `["remove", id]`, `["root",
/// id]` and `["drag", id, drag]`.
#[derive(Debug)]
pub(crate) enum Op {
    Create(u64, Props),
    Set(u64, Change),
    Insert {
        parent: u64,
        node: u64,
        before: Option<u64>,
    },
    Remove(u64),
    Root(u64),
    Drag(u64, Drag),
}

impl<'de> Deserialize<'de> for Op {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(OpVisitor)
    }
}

struct OpVisitor;

fn next<'de, T: Deserialize<'de>, A: SeqAccess<'de>>(
    seq: &mut A,
    what: &str,
) -> Result<T, A::Error> {
    seq.next_element()?
        .ok_or_else(|| A::Error::custom(format!("an op without its {what}")))
}

impl<'de> Visitor<'de> for OpVisitor {
    type Value = Op;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an op such as [\"set\", id, family, props]")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Op, A::Error> {
        let kind: String = next(&mut seq, "kind")?;
        let id: u64 = next(&mut seq, "node")?;
        Ok(match kind.as_str() {
            "create" => {
                let family: String = next(&mut seq, "family")?;
                Op::Create(
                    id,
                    match family.as_str() {
                        "div" => Props::Div(next(&mut seq, "props")?),
                        "text" => Props::Text(next(&mut seq, "props")?),
                        "sample" => Props::Sample(next(&mut seq, "props")?),
                        "space" => Props::Space(Box::new(next(&mut seq, "props")?)),
                        "shape" => Props::Shape(Box::new(next(&mut seq, "props")?)),
                        "marker" => {
                            next::<serde::de::IgnoredAny, _>(&mut seq, "props")?;
                            Props::Marker
                        }
                        other => return Err(A::Error::unknown_variant(other, FAMILIES)),
                    },
                )
            }
            "set" => {
                let family: String = next(&mut seq, "family")?;
                Op::Set(
                    id,
                    match family.as_str() {
                        "div" => Change::Div(next(&mut seq, "props")?),
                        "text" => Change::Text(next(&mut seq, "props")?),
                        "sample" => Change::Sample(next(&mut seq, "props")?),
                        "space" => Change::Space(Box::new(next(&mut seq, "props")?)),
                        "shape" => Change::Shape(Box::new(next(&mut seq, "props")?)),
                        "marker" => {
                            next::<serde::de::IgnoredAny, _>(&mut seq, "props")?;
                            Change::Marker
                        }
                        other => return Err(A::Error::unknown_variant(other, FAMILIES)),
                    },
                )
            }
            "insert" => Op::Insert {
                parent: id,
                node: next(&mut seq, "child")?,
                before: next(&mut seq, "sibling")?,
            },
            "remove" => Op::Remove(id),
            "root" => Op::Root(id),
            "drag" => Op::Drag(id, next(&mut seq, "drag")?),
            other => {
                return Err(A::Error::unknown_variant(
                    other,
                    &["create", "set", "insert", "remove", "root", "drag"],
                ));
            }
        })
    }
}

const FAMILIES: &[&str] = &["div", "text", "sample", "space", "shape", "marker"];

/// What of the window the live values of a card move with, without its code, and what the
/// window reads for them on every frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Uses {
    pub watches: bool,
    pub tick: bool,
    /// It moves on every frame by itself: with the clock, the engine's levels or what a lane
    /// or a cable plays, or as it eases.
    pub animates: bool,
    /// The peaks it reads, by name, and whether it reads the device output.
    pub peaks: BTreeSet<String>,
    pub output: bool,
    /// The numbers it reads as they play.
    pub played: BTreeSet<String>,
    /// The scopes it draws.
    pub scopes: BTreeSet<String>,
    /// The fields of instances inside it it reads, as `path.field`.
    pub inside: BTreeSet<String>,
}

/// How many nodes of a table use each thing, so a node that goes takes its use with it.
#[derive(Clone, Debug, Default)]
struct UseCounts {
    watches: usize,
    tick: usize,
    animates: usize,
    output: usize,
    named: HashMap<(Named, String), usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Named {
    Peak,
    Played,
    Scope,
    Inside,
}

impl UseCounts {
    fn count(&mut self, uses: &Uses, add: bool) {
        let step = |count: &mut usize, used: bool| {
            if used {
                *count = if add {
                    *count + 1
                } else {
                    count.saturating_sub(1)
                };
            }
        };
        step(&mut self.watches, uses.watches);
        step(&mut self.tick, uses.tick);
        step(&mut self.animates, uses.animates);
        step(&mut self.output, uses.output);
        let named = [
            (Named::Peak, &uses.peaks),
            (Named::Played, &uses.played),
            (Named::Scope, &uses.scopes),
            (Named::Inside, &uses.inside),
        ];
        for (kind, names) in named {
            for name in names {
                let key = (kind, name.clone());
                let count = self.named.entry(key.clone()).or_default();
                step(count, true);
                if *count == 0 {
                    self.named.remove(&key);
                }
            }
        }
    }

    fn uses(&self) -> Uses {
        let names = |kind: Named| -> BTreeSet<String> {
            (self.named.keys())
                .filter(|(each, _)| *each == kind)
                .map(|(_, name)| name.clone())
                .collect()
        };
        Uses {
            watches: self.watches > 0,
            tick: self.tick > 0,
            animates: self.animates > 0,
            output: self.output > 0,
            peaks: names(Named::Peak),
            played: names(Named::Played),
            scopes: names(Named::Scope),
            inside: names(Named::Inside),
        }
    }
}

/// A node of a card and its children, in order, by their slots.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    /// The id its code knows it by, which an event names.
    pub id: u64,
    pub props: Props,
    pub children: Vec<usize>,
    parent: Option<usize>,
    uses: Uses,
}

/// The nodes of a card as the window keeps them. A node keeps the slot it was made in, so the
/// rows of a list made one after another lie side by side, and a frame walks them in order
/// without looking an id up.
#[derive(Clone, Debug, Default)]
pub(crate) struct NodeTable {
    /// Gone nodes leave their slot empty: a slot is never given again, so a list of children
    /// never names another node.
    slots: Vec<Option<Entry>>,
    ids: HashMap<u64, usize>,
    root: Option<usize>,
    drags: HashMap<u64, Drag>,
    counts: UseCounts,
    /// What the nodes use together, worked out again after a batch that changed it.
    uses: Uses,
}

impl NodeTable {
    pub(crate) fn root(&self) -> Option<usize> {
        self.root
    }

    pub(crate) fn get(&self, slot: usize) -> Option<&Entry> {
        self.slots.get(slot)?.as_ref()
    }

    pub(crate) fn drag(&self, id: Option<u64>) -> Option<&Drag> {
        self.drags.get(&id?)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.ids.len()
    }

    #[cfg(test)]
    fn children_of(&self, id: u64) -> Vec<u64> {
        let entry = self.get(self.ids[&id]).unwrap();
        (entry.children.iter())
            .filter_map(|slot| self.get(*slot).map(|child| child.id))
            .collect()
    }

    #[cfg(test)]
    fn props_of(&self, id: u64) -> &Props {
        &self.get(self.ids[&id]).unwrap().props
    }

    pub(crate) fn uses(&self) -> &Uses {
        &self.uses
    }

    fn slot(&self, id: u64) -> Result<usize, String> {
        self.ids
            .get(&id)
            .copied()
            .ok_or_else(|| format!("no node {id}"))
    }

    fn entry_mut(&mut self, slot: usize) -> Option<&mut Entry> {
        self.slots.get_mut(slot)?.as_mut()
    }

    /// Applies the ops of one batch, or says what is wrong with them.
    pub(crate) fn apply(&mut self, ops: Vec<Op>) -> Result<(), String> {
        let mut emptied = HashSet::new();
        for op in ops {
            match op {
                Op::Create(id, props) => {
                    if let Props::Shape(shape) = &props
                        && shape.kind.is_none()
                    {
                        return Err(format!("the shape {id} has no kind"));
                    }
                    let uses = uses_of(&props);
                    self.counts.count(&uses, true);
                    let entry = Entry {
                        id,
                        props,
                        children: Vec::new(),
                        parent: None,
                        uses,
                    };
                    match self.ids.get(&id) {
                        Some(&slot) => {
                            if let Some(old) = self.slots[slot].replace(entry) {
                                self.counts.count(&old.uses, false);
                            }
                        }
                        None => {
                            self.ids.insert(id, self.slots.len());
                            self.slots.push(Some(entry));
                        }
                    }
                }
                Op::Set(id, change) => {
                    let slot = self.slot(id)?;
                    let Some(entry) = self.entry_mut(slot) else {
                        return Err(format!("no node {id}"));
                    };
                    match (&mut entry.props, change) {
                        (Props::Div(props), Change::Div(change)) => props.apply(change),
                        (Props::Text(props), Change::Text(change)) => props.apply(change),
                        (Props::Sample(props), Change::Sample(change)) => props.apply(change),
                        (Props::Space(props), Change::Space(change)) => props.apply(*change),
                        (Props::Shape(props), Change::Shape(change)) => props.apply(*change),
                        (Props::Marker, Change::Marker) => {}
                        _ => return Err(format!("a change of another kind of node for {id}")),
                    }
                    let after = uses_of(&entry.props);
                    if after != entry.uses {
                        let before = std::mem::replace(&mut entry.uses, after.clone());
                        self.counts.count(&before, false);
                        self.counts.count(&after, true);
                    }
                }
                Op::Insert {
                    parent,
                    node,
                    before,
                } => {
                    let (parent, node) = (self.slot(parent)?, self.slot(node)?);
                    let before = before.and_then(|before| self.ids.get(&before).copied());
                    let Some(entry) = self.entry_mut(node) else {
                        return Err(format!("no node to put in {parent}"));
                    };
                    let old = entry.parent.replace(parent);
                    // A move: out of where it was first.
                    if let Some(old) = old.and_then(|old| self.entry_mut(old))
                        && let Some(at) = old.children.iter().rposition(|child| *child == node)
                    {
                        old.children.remove(at);
                    }
                    if emptied.remove(&parent) {
                        self.tidy(parent);
                    }
                    let Some(siblings) = self.entry_mut(parent) else {
                        return Err(format!("no node to put {node} in"));
                    };
                    let at = before.and_then(|before| {
                        siblings.children.iter().rposition(|child| *child == before)
                    });
                    match at {
                        Some(at) => siblings.children.insert(at, node),
                        None => siblings.children.push(node),
                    }
                }
                Op::Remove(id) => {
                    let Some(&slot) = self.ids.get(&id) else {
                        continue;
                    };
                    let parent = self.get(slot).and_then(|entry| entry.parent);
                    self.remove(slot);
                    // Taken out of its parent once for the whole batch: a list of thousands loses
                    // hundreds of rows at once.
                    if let Some(parent) = parent {
                        emptied.insert(parent);
                    }
                }
                Op::Root(id) => self.root = Some(self.slot(id)?),
                Op::Drag(id, drag) => {
                    self.drags.insert(id, drag);
                }
            }
        }
        for parent in emptied {
            self.tidy(parent);
        }
        self.uses = self.counts.uses();
        Ok(())
    }

    /// Takes the children that are gone out of the list of `parent`.
    fn tidy(&mut self, parent: usize) {
        let Some(entry) = self.get(parent) else {
            return;
        };
        let kept: Vec<usize> = (entry.children.iter().copied())
            .filter(|child| self.get(*child).is_some())
            .collect();
        if let Some(entry) = self.entry_mut(parent) {
            entry.children = kept;
        }
    }

    fn remove(&mut self, slot: usize) {
        let Some(entry) = self.slots.get_mut(slot).and_then(Option::take) else {
            return;
        };
        self.ids.remove(&entry.id);
        self.counts.count(&entry.uses, false);
        for child in entry.children {
            self.remove(child);
        }
    }
}

fn uses_of(props: &Props) -> Uses {
    match props {
        Props::Shape(shape) => shape.uses(),
        _ => Uses::default(),
    }
}

impl ShapeProps {
    fn uses(&self) -> Uses {
        let mut uses = Uses::default();
        self.numbers().for_each(|number| number.add_uses(&mut uses));
        if let Some(Paint::Mix { by, .. }) = &self.color {
            by.add_uses(&mut uses);
        }
        match &self.list {
            Some(List::Scope { name, .. }) => {
                uses.scopes.insert(name.clone());
                uses.animates = true;
            }
            Some(List::History { of, .. }) => {
                of.add_uses(&mut uses);
                uses.animates = true;
            }
            None => {}
        }
        uses
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
        self.on_press == Some(true)
            || self.drag.is_some()
            || self.hover.is_some()
            || self.cursor.is_some()
    }
}

impl SpaceProps {
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

/// The spaces a space shows the scroll and zoom of, on an axis, by their keys.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Follow {
    pub x: Option<String>,
    pub y: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShapeKind {
    Group,
    Rect,
    Circle,
    Line,
    Arc,
    Text,
    Wave,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Align {
    Left,
    Center,
    Right,
}

/// The colour of a shape: as it is, or between two by a live value, 0 the first and 1 the second.
#[derive(Debug, Clone)]
pub(crate) enum Paint {
    Fixed(Color),
    Mix { from: Color, to: Color, by: Num },
}

impl<'de> Deserialize<'de> for Paint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PaintVisitor)
    }
}

struct PaintVisitor;

impl<'de> Visitor<'de> for PaintVisitor {
    type Value = Paint;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a colour, or a mix of two")
    }

    fn visit_str<E: Error>(self, text: &str) -> Result<Paint, E> {
        Color::try_from(text.to_string())
            .map(Paint::Fixed)
            .map_err(E::custom)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Paint, A::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Mix {
            mix: [Color; 2],
            by: Num,
        }
        let Mix {
            mix: [from, to],
            by,
        } = Mix::deserialize(MapAccessDeserializer::new(map))?;
        Ok(Paint::Mix { from, to, by })
    }
}

/// What a wave draws: the last sound of a meter, or the last values of a live value, one a frame.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum List {
    Scope {
        name: String,
        /// 0 left, 1 right; their mean when left out.
        channel: Option<u8>,
        /// How many of the last frames, at most [`sound_core::Scope::FRAMES`].
        frames: Option<u32>,
    },
    History {
        of: Num,
        frames: u32,
        /// The same history wherever it is drawn: its live value, as `sdk.ts` writes it.
        key: String,
    },
}

/// An end of a line, a point, or of an arc, a turn.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Reach {
    Point([Num; 2]),
    Turn(Num),
}

/// The text of a shape: as it is, or a live number as a readout.
#[derive(Clone, Debug, Deserialize)]
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
    fn add_uses(&self, uses: &mut Uses) {
        if let Self::Live(live) = self {
            live.add_uses(uses);
        }
    }
}

/// A live value, as `sdk.ts`'s `Live` sends it.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Live {
    Tick,
    /// Seconds on the clock of the window.
    Time,
    Watch {
        name: String,
    },
    Field {
        name: String,
        fraction: bool,
        /// Of the record of this instance inside it, by its path, instead.
        child: Option<String>,
    },
    Control {
        name: String,
        fraction: bool,
    },
    /// The loudest sample since the last frame: of a meter its behaviour declares, or of peaks
    /// an instance inside it keeps, as `path.name`.
    Peak {
        name: String,
        /// 0 left, 1 right; the louder when left out.
        channel: Option<u8>,
    },
    /// The same, of what the device plays.
    Output {
        channel: Option<u8>,
    },
    /// A number as it plays: what a lane or a cable plays into it, else its record. Of an
    /// instance inside it as `path.name`.
    Played {
        name: String,
        fraction: bool,
    },
    Drag {
        id: String,
        part: DragPart,
    },
    Space {
        key: String,
        part: SpacePart,
    },
    Plus {
        a: Num,
        b: Num,
    },
    Minus {
        a: Num,
        b: Num,
    },
    Times {
        a: Num,
        b: Num,
    },
    Over {
        a: Num,
        b: Num,
    },
    Mod {
        a: Num,
        b: Num,
    },
    Pow {
        a: Num,
        b: Num,
    },
    Min {
        a: Num,
        b: Num,
    },
    Max {
        a: Num,
        b: Num,
    },
    Lt {
        a: Num,
        b: Num,
    },
    Gt {
        a: Num,
        b: Num,
    },
    Le {
        a: Num,
        b: Num,
    },
    Ge {
        a: Num,
        b: Num,
    },
    Eq {
        a: Num,
        b: Num,
    },
    Ne {
        a: Num,
        b: Num,
    },
    Snap {
        a: Num,
        b: Num,
    },
    Negate {
        x: Num,
    },
    Abs {
        x: Num,
    },
    Floor {
        x: Num,
    },
    Round {
        x: Num,
    },
    Sin {
        x: Num,
    },
    Cos {
        x: Num,
    },
    /// An amplitude in decibels, -120 at most down.
    Decibels {
        x: Num,
    },
    /// What moves from frame to frame: `key` is its live value as `sdk.ts` writes it, so the
    /// same one drawn twice is one, as a signal used twice is.
    Ease {
        x: Num,
        ease: Ease,
        key: String,
    },
}

/// How a live value follows another over time.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Ease {
    /// Glides toward it, about two thirds of the way in `ms`.
    Smooth { ms: f32 },
    /// Rises with it at once and falls at most `per_second`.
    Fall { per_second: f32 },
    /// Rises with it at once and stays for `seconds`, then follows it.
    Hold { seconds: f32 },
}

impl Live {
    fn add_uses(&self, uses: &mut Uses) {
        match self {
            Self::Tick => uses.tick = true,
            Self::Watch { .. } => uses.watches = true,
            Self::Time => uses.animates = true,
            Self::Peak { name, .. } => {
                uses.peaks.insert(name.clone());
                uses.animates = true;
            }
            Self::Output { .. } => {
                uses.output = true;
                uses.animates = true;
            }
            Self::Played { name, .. } => {
                uses.played.insert(name.clone());
                if name.contains('.') {
                    uses.inside.insert(name.clone());
                }
                uses.animates = true;
            }
            Self::Field {
                child: Some(child),
                name,
                ..
            } => {
                uses.inside.insert(format!("{child}.{name}"));
            }
            Self::Field { child: None, .. }
            | Self::Control { .. }
            | Self::Drag { .. }
            | Self::Space { .. } => {}
            Self::Plus { a, b }
            | Self::Minus { a, b }
            | Self::Times { a, b }
            | Self::Over { a, b }
            | Self::Mod { a, b }
            | Self::Pow { a, b }
            | Self::Min { a, b }
            | Self::Max { a, b }
            | Self::Lt { a, b }
            | Self::Gt { a, b }
            | Self::Le { a, b }
            | Self::Ge { a, b }
            | Self::Eq { a, b }
            | Self::Ne { a, b }
            | Self::Snap { a, b } => {
                a.add_uses(uses);
                b.add_uses(uses);
            }
            Self::Negate { x }
            | Self::Abs { x }
            | Self::Floor { x }
            | Self::Round { x }
            | Self::Sin { x }
            | Self::Cos { x }
            | Self::Decibels { x } => x.add_uses(uses),
            Self::Ease { x, .. } => {
                x.add_uses(uses);
                uses.animates = true;
            }
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
    /// Its code hears its end, or every move.
    #[serde(default)]
    pub on_end: bool,
    #[serde(default)]
    pub on_move: bool,
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
    /// The instance inside it, by its path, whose `field` it turns, else its own record.
    pub child: Option<String>,
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

#[derive(Clone, Debug, Default, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Direction {
    Row,
    Column,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Place {
    Start,
    Center,
    End,
}

#[derive(Clone, Debug, Deserialize)]
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

    fn ops(json: serde_json::Value) -> Result<Vec<Op>, serde_json::Error> {
        serde_json::from_value(json)
    }

    #[test]
    fn a_prop_the_runtime_does_not_know_is_an_error() {
        let wrong = [
            (
                serde_json::json!([["create", 1, "div", { "style": { "margin": 4 } }]]),
                "margin",
            ),
            (
                serde_json::json!([["set", 1, "space", { "shapes": [] }]]),
                "shapes",
            ),
        ];
        for (json, field) in wrong {
            let error = ops(json).unwrap_err().to_string();
            assert!(
                error.contains(&format!("unknown field `{field}`")),
                "{error}"
            );
        }
    }

    #[test]
    fn ops_set_unset_move_and_remove_nodes_in_place() {
        let mut table = NodeTable::default();
        let made = ops(serde_json::json!([
            ["create", 1, "space", { "key": "roll" }],
            ["create", 2, "shape", { "kind": "rect", "x": 1, "color": "#ffffff", "hover": "#000000" }],
            ["create", 3, "shape", { "kind": "rect", "x": { "op": "tick" } }],
            ["create", 4, "marker", {}],
            ["insert", 1, 4, null], ["insert", 1, 2, 4], ["insert", 1, 3, 4], ["root", 1],
        ]));
        table.apply(made.unwrap()).unwrap();
        assert_eq!(table.children_of(1), [2, 3, 4]);
        assert!(table.uses().tick);
        let changed = ops(serde_json::json!([
            ["set", 2, "shape", { "x": 5, "hover": null }],
            ["insert", 1, 3, 2],
            ["remove", 3],
        ]));
        table.apply(changed.unwrap()).unwrap();
        assert_eq!(table.children_of(1), [2, 4]);
        assert!(!table.uses().tick);
        let Props::Shape(shape) = table.props_of(2) else {
            panic!("a shape");
        };
        assert!(matches!(shape.x, Some(Num::Fixed(5.0))));
        assert!(shape.hover.is_none() && shape.color.is_some());
    }
}
