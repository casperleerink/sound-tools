//! The nodes a card makes, as the host sends them: ops on a table the window keeps, so a change
//! of a prop crosses as that prop. The types are the contract with `sdk.ts`: a prop this side
//! does not know is an error on the card, not something left out.

use std::collections::{BTreeSet, HashMap, HashSet};

use gpui::{Div, Hsla, prelude::*, px};
use serde::de::value::MapAccessDeserializer;
use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::tools::{ToolInfo, Unit};

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
        on_press: bool,
        /// The spaces whose scroll its children move with, by their keys.
        follow: Follow,
        /// What a drag of it carries to a node that hears a drop.
        carry: String,
        /// It hears a drop: files from the system, or what a drag of a node carries.
        on_drop: bool,
        /// It shows in the window's title bar instead, when the tool's view is the window's.
        title_bar: bool,
    }
}

props! {
    /// A text field. It shows `value` until it is typed in, and tells its text on enter or when
    /// it loses the keys.
    InputProps / InputChange {
        key: String,
        value: String,
        placeholder: String,
        width: f32,
        /// It takes the keys when it is made.
        focus: bool,
        on_commit: bool,
        /// Escape tells the view.
        on_cancel: bool,
    }
}

props! {
    /// The card of an instance below the tool's, by its path, as a rack shows it.
    CardProps / CardChange {
        key: String,
        instance: String,
        /// Whether its power icon is on. The icons show only with their handlers.
        on: bool,
        on_power: bool,
        on_close: bool,
        /// Whether its title is a menu: a chevron after it, and a press tells the view.
        on_title: bool,
        /// The selects of what keys a device with a sidechain input, behind its expand.
        sidechain: Vec<CardSelect>,
        /// A pick in one of them tells the view.
        on_sidechain: bool,
    }
}

/// A select of a card: what it shows, the label under it, and what its list holds.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CardSelect {
    pub label: String,
    pub value: String,
    pub options: Vec<String>,
}

props! {
    TextProps / TextChange { text: String }
}

props! {
    /// A button of the app: a label, an icon or both, in one of the app's looks.
    ButtonProps / ButtonChange {
        key: String,
        label: String,
        icon: String,
        variant: ButtonLook,
        /// The accent of a `solid`, `subtle` or `ghost` look.
        color: Color,
        size: ButtonSized,
        /// A pill, round at its ends.
        rounded: bool,
        disabled: bool,
        /// Quieter than its look, 0 to 1.
        opacity: f32,
        on_press: bool,
    }
}

props! {
    /// A stereo meter of the app: the peaks of `of` as `now.peak` names them, of the channels of
    /// the audio input `input` names, or of what the device plays.
    MeterProps / MeterChange {
        key: String,
        of: String,
        input: Vec<usize>,
        horizontal: bool,
        length: f32,
    }
}

props! {
    /// A switch of the app: a letter, an icon with a count, or a dot, coloured while `on`.
    ToggleProps / ToggleChange {
        key: String,
        label: String,
        icon: String,
        count: f32,
        dot: bool,
        on: bool,
        color: Color,
        disabled: bool,
        on_press: bool,
    }
}

props! {
    /// An icon of the app, by its name.
    IconProps / IconChange { name: String, size: f32, color: Color }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ButtonLook {
    Primary,
    Subtle,
    Ghost,
    Solid,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ButtonSized {
    Xs,
    Sm,
    Md,
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
        fit: Fit,
        follow: Follow,
        background: Color,
        radius: f32,
        on_press: bool,
        /// Its drag, by its number among the drags of the card.
        drag: u64,
        cursor: Cursor,
        on_drop: bool,
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
        /// The drags it moves with, those it grows with and those it shrinks with, by their ids.
        moves: Vec<String>,
        grows: Vec<String>,
        shrinks: Vec<String>,
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
        unit: Readout,
        size: f32,
        align: Align,
        /// The values a wave draws, oldest first, and the values at its bottom and its top; the
        /// take whose notes `Notes` draws.
        list: List,
        /// A group's box from its place, outside which its shapes do not show.
        clip: Clip,
        low: f32,
        high: f32,
        /// A wave as a band from the lowest to the highest value under each point, else a line.
        band: bool,
        /// The corners of a path, in order.
        points: Vec<[Num; 2]>,
        /// What a wave's values are multiplied by along it: `[x, factor]` from its left, straight
        /// between them.
        level: Vec<[Num; 2]>,
    }
}

/// A node of the table.
#[derive(Clone, Debug)]
pub(crate) enum Props {
    Div(DivProps),
    Text(TextProps),
    Space(Box<SpaceProps>),
    Shape(Box<ShapeProps>),
    Input(InputProps),
    Card(CardProps),
    Button(ButtonProps),
    Icon(IconProps),
    Meter(MeterProps),
    Toggle(ToggleProps),
    /// Where a list or a part that changes puts its nodes: draws nothing.
    Marker,
}

#[derive(Debug)]
pub(crate) enum Change {
    Div(DivChange),
    Text(TextChange),
    Space(Box<SpaceChange>),
    Shape(Box<ShapeChange>),
    Input(InputChange),
    Card(CardChange),
    Button(ButtonChange),
    Icon(IconChange),
    Meter(MeterChange),
    Toggle(ToggleChange),
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
                        "space" => Props::Space(Box::new(next(&mut seq, "props")?)),
                        "shape" => Props::Shape(Box::new(next(&mut seq, "props")?)),
                        "input" => Props::Input(next(&mut seq, "props")?),
                        "card" => Props::Card(next(&mut seq, "props")?),
                        "button" => Props::Button(next(&mut seq, "props")?),
                        "icon" => Props::Icon(next(&mut seq, "props")?),
                        "meter" => Props::Meter(next(&mut seq, "props")?),
                        "toggle" => Props::Toggle(next(&mut seq, "props")?),
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
                        "space" => Change::Space(Box::new(next(&mut seq, "props")?)),
                        "shape" => Change::Shape(Box::new(next(&mut seq, "props")?)),
                        "input" => Change::Input(next(&mut seq, "props")?),
                        "card" => Change::Card(next(&mut seq, "props")?),
                        "button" => Change::Button(next(&mut seq, "props")?),
                        "icon" => Change::Icon(next(&mut seq, "props")?),
                        "meter" => Change::Meter(next(&mut seq, "props")?),
                        "toggle" => Change::Toggle(next(&mut seq, "props")?),
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

const FAMILIES: &[&str] = &[
    "div", "text", "space", "shape", "input", "card", "button", "icon", "meter", "toggle", "marker",
];

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
    /// It reads the level of the audio input.
    pub input: bool,
    /// The numbers it reads as they play.
    pub played: BTreeSet<String>,
    /// The scopes it draws.
    pub scopes: BTreeSet<String>,
    /// The fields of instances inside it it reads, as `path.field`.
    pub inside: BTreeSet<String>,
    /// The audio files whose waveforms it draws.
    pub files: BTreeSet<String>,
    /// The recorders whose takes it draws as they grow.
    pub takes: BTreeSet<String>,
}

/// How many nodes of a table use each thing, so a node that goes takes its use with it.
#[derive(Clone, Debug, Default)]
struct UseCounts {
    watches: usize,
    tick: usize,
    animates: usize,
    output: usize,
    input: usize,
    named: HashMap<(Named, String), usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Named {
    Peak,
    Played,
    Scope,
    Inside,
    File,
    Take,
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
        step(&mut self.input, uses.input);
        let named = [
            (Named::Peak, &uses.peaks),
            (Named::Played, &uses.played),
            (Named::Scope, &uses.scopes),
            (Named::Inside, &uses.inside),
            (Named::File, &uses.files),
            (Named::Take, &uses.takes),
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
            input: self.input > 0,
            peaks: names(Named::Peak),
            played: names(Named::Played),
            scopes: names(Named::Scope),
            inside: names(Named::Inside),
            files: names(Named::File),
            takes: names(Named::Take),
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
    /// Goes up with every batch, so what a frame keeps of the nodes knows when to look again.
    version: u64,
}

impl NodeTable {
    pub(crate) fn root(&self) -> Option<usize> {
        self.root
    }

    pub(crate) fn get(&self, slot: usize) -> Option<&Entry> {
        self.slots.get(slot)?.as_ref()
    }

    /// Every node there is now, in the order they were made.
    pub(crate) fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.slots.iter().flatten()
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

    pub(crate) fn version(&self) -> u64 {
        self.version
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
        self.version += 1;
        let mut emptied = HashSet::new();
        for op in ops {
            match op {
                Op::Create(id, props) => {
                    if let Props::Shape(shape) = &props
                        && shape.kind.is_none()
                    {
                        return Err(format!("the shape {id} has no kind"));
                    }
                    if self.ids.contains_key(&id) {
                        return Err(format!("the node {id} is made twice"));
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
                    self.ids.insert(id, self.slots.len());
                    self.slots.push(Some(entry));
                }
                Op::Set(id, change) => {
                    let slot = self.slot(id)?;
                    let Some(entry) = self.entry_mut(slot) else {
                        return Err(format!("no node {id}"));
                    };
                    match (&mut entry.props, change) {
                        (Props::Div(props), Change::Div(change)) => props.apply(change),
                        (Props::Text(props), Change::Text(change)) => props.apply(change),
                        (Props::Space(props), Change::Space(change)) => props.apply(*change),
                        (Props::Input(props), Change::Input(change)) => props.apply(change),
                        (Props::Button(props), Change::Button(change)) => props.apply(change),
                        (Props::Icon(props), Change::Icon(change)) => props.apply(change),
                        (Props::Meter(props), Change::Meter(change)) => props.apply(change),
                        (Props::Toggle(props), Change::Toggle(change)) => props.apply(change),
                        (Props::Card(props), Change::Card(change)) => props.apply(change),
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
                    let mut above = Some(parent);
                    while let Some(slot) = above {
                        if slot == node {
                            return Err(
                                "a node goes into itself or into a node inside it".to_string()
                            );
                        }
                        above = self.get(slot).and_then(|entry| entry.parent);
                    }
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
        Props::Meter(MeterProps { of, input, .. }) => {
            let mut uses = Uses {
                animates: true,
                ..Uses::default()
            };
            match (of, input) {
                (_, Some(_)) => uses.input = true,
                (Some(name), None) => _ = uses.peaks.insert(name.clone()),
                (None, None) => uses.output = true,
            }
            uses
        }
        _ => Uses::default(),
    }
}

impl ShapeProps {
    /// Whether where it is never moves: no live number places or sizes it and no drag moves
    /// or grows it. A live colour, opacity or readout does not move it. A wave's list is worked
    /// out on every frame, so a history keeps counting while it does not show.
    pub(crate) fn stays(&self) -> bool {
        let reaches = [&self.from, &self.to]
            .into_iter()
            .flatten()
            .all(|reach| match reach {
                Reach::Point([x, y]) => x.fixed().is_some() && y.fixed().is_some(),
                Reach::Turn(turn) => turn.fixed().is_some(),
            });
        let numbers = [
            &self.x,
            &self.y,
            &self.width,
            &self.height,
            &self.radius,
            &self.border_width,
        ]
        .into_iter()
        .flatten()
        .chain(self.points.iter().flatten().flatten())
        .all(|number| number.fixed().is_some());
        let followed = self.moves.is_none() && self.grows.is_none() && self.shrinks.is_none();
        followed && self.list.is_none() && reaches && numbers
    }

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
            Some(List::File { asset, .. }) => {
                uses.files.insert(asset.clone());
            }
            Some(List::Take { name }) => {
                uses.takes.insert(name.clone());
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
        let clip = (self.clip.iter()).flat_map(|clip| [&clip.width, &clip.height]);
        let corners = [&self.points, &self.level]
            .into_iter()
            .flatten()
            .flatten()
            .flatten();
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
        .chain(clip)
        .chain(corners)
    }

    /// For a group: whether where it puts its shapes never moves, so they may stay too.
    pub(crate) fn group_stays(&self) -> bool {
        let fixed = |number: &Option<Num>| {
            number
                .as_ref()
                .is_none_or(|number| number.fixed().is_some())
        };
        let clip = (self.clip.as_ref())
            .is_none_or(|clip| clip.width.fixed().is_some() && clip.height.fixed().is_some());
        fixed(&self.x) && fixed(&self.y) && clip && self.moves.is_none() && self.grows.is_none()
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

/// A range of units a space fits across its width once it is laid out: `x` from `before`
/// points right of its left edge to `after` points left of its right one, as far as it zooms.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fit {
    pub x: [f32; 2],
    #[serde(default)]
    pub before: f32,
    #[serde(default)]
    pub after: f32,
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
    Notes,
    Path,
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
    /// The waveform of an audio file under `assets/audio/`, from `from` to `to` seconds of it:
    /// the peak of each stretch, as `+peak` then `-peak`, for a band from -1 to 1.
    File {
        asset: String,
        from: Option<f64>,
        to: Option<f64>,
    },
    /// The waveform of the take a `recorder` at the path `name` inside records, as it grows,
    /// in the form of [`List::File`].
    Take { name: String },
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
    /// Parts of one line, each in a colour and weight of its own.
    Parts(Vec<Part>),
    Live(Num),
}

/// A part of a line of text: in the label's colour and weight unless it names its own.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Part {
    pub text: String,
    pub color: Option<Color>,
    pub weight: Option<Weight>,
    /// A rounded box around it, of this colour, filled a little.
    pub border: Option<Color>,
}

/// The size of a group's box, in the units of its space.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Clip {
    pub width: Num,
    pub height: Num,
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
    /// Its number, when it is one and not a live value.
    pub(crate) fn fixed(&self) -> Option<f32> {
        match self {
            Self::Fixed(number) => Some(*number),
            Self::Live(_) => None,
        }
    }

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
    /// The tempo at the playhead.
    Bpm,
    /// Seconds since the view opened.
    Elapsed,
    /// A number by its name: a field of the record, a number input or a number output of the
    /// sound; of the instance at the path `child` inside it, its field or its output.
    Value {
        name: String,
        child: Option<String>,
        /// A field as it plays: what a lane or a cable plays into it, else its record.
        played: bool,
        fraction: bool,
    },
    /// The loudest sample since the last frame: of a `meter` at a path inside, or of peaks
    /// an instance inside it keeps, as `path.name`.
    Peak {
        name: String,
        /// 0 left, 1 right; the louder when left out.
        channel: Option<u8>,
    },
    /// The same, of what the device plays.
    DevicePeak {
        channel: Option<u8>,
    },
    /// The loudest sample of a channel of the audio input at the last poll, while it is open.
    Input {
        channel: usize,
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
            Self::Tick | Self::Bpm => uses.tick = true,
            Self::Elapsed => uses.animates = true,
            Self::Peak { name, .. } => {
                uses.peaks.insert(name.clone());
                uses.animates = true;
            }
            Self::DevicePeak { .. } => {
                uses.output = true;
                uses.animates = true;
            }
            Self::Input { .. } => {
                uses.input = true;
                uses.animates = true;
            }
            Self::Value {
                name,
                child,
                played,
                ..
            } => {
                let named = match child {
                    Some(child) => format!("{child}.{name}"),
                    None => name.clone(),
                };
                if child.is_some() {
                    uses.inside.insert(named.clone());
                }
                if *played {
                    uses.played.insert(named);
                    uses.animates = true;
                }
                // It may be an output, which moves with the watches.
                uses.watches = true;
            }
            Self::Drag { .. } | Self::Space { .. } => {}
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
    /// Its code hears its end.
    #[serde(default)]
    pub on_end: bool,
    /// Its code hears a press that let go where it was, as a click.
    #[serde(default)]
    pub on_click: bool,
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

/// How text shows a live number: in a unit, or a tick as a place in the piece, as minutes and
/// seconds, or as the time signature there.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Readout {
    Hz,
    Ms,
    Db,
    Percent,
    /// A pan from -1 to 1: `C`, `25L`, `100R`.
    Pan,
    Position,
    Time,
    Signature,
    /// The beats of a bar, and the note value of a beat, of the time signature there.
    Beats,
    Note,
}

/// A number a drag turns as it goes: a field of the record, or a live control.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Turns {
    /// What it turns, as `now.value` names it: a field, of an instance inside it as
    /// `path.field`, or a number input.
    pub value: String,
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

/// What a drag turns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Turned<'a> {
    /// A field of the record, or of the instance at the path `child` inside it.
    Field {
        child: Option<&'a str>,
        name: &'a str,
    },
    Input(&'a str),
}

impl Turned<'_> {
    /// The input it is, as an owned name, or `None` for a field.
    pub(crate) fn to_owned_input(self) -> Option<String> {
        match self {
            Self::Input(name) => Some(name.to_string()),
            Self::Field { .. } => None,
        }
    }
}

impl Turns {
    /// What it turns, for the tool `info`: a number input of that name, else a field.
    pub(crate) fn target(&self, info: Option<&ToolInfo>) -> Turned<'_> {
        if let Some((child, name)) = self.value.rsplit_once('.') {
            return Turned::Field {
                child: Some(child),
                name,
            };
        }
        match info.and_then(|info| info.input(&self.value)) {
            Some(_) => Turned::Input(&self.value),
            None => Turned::Field {
                child: None,
                name: &self.value,
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Style {
    direction: Option<Direction>,
    gap: Option<f32>,
    padding: Option<f32>,
    padding_x: Option<f32>,
    padding_y: Option<f32>,
    padding_left: Option<f32>,
    padding_right: Option<f32>,
    padding_top: Option<f32>,
    padding_bottom: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    grow: bool,
    align: Option<Place>,
    justify: Option<Justify>,
    background: Option<Color>,
    color: Option<Color>,
    font_size: Option<f32>,
    /// How far apart the lines of its text are, in points.
    line_height: Option<f32>,
    radius: Option<f32>,
    border_color: Option<Color>,
    border_width: Option<f32>,
    /// Its children scroll along this axis where they do not fit its size.
    scroll: Option<ScrollAxis>,
    /// `tabular`: digits of one width, so a number that changes does not move what follows.
    font: Option<FontKind>,
    weight: Option<Weight>,
    /// Its background while the pointer is over it.
    hover: Option<Color>,
    /// It takes no more room than it needs, nor less: it does not shrink in a row too narrow.
    keep: bool,
    /// A line of text cut with an ellipsis where it does not fit.
    truncate: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FontKind {
    Tabular,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Weight {
    Normal,
    Medium,
    Semibold,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScrollAxis {
    X,
    Y,
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

/// `#rrggbb`, `#rrggbbaa`, or a colour of the app's theme by name, such as `gray-700`, with an
/// opacity in percent after a slash: `alpha/10`, `#a9b1ff/70`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Color(pub(crate) Hsla);

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let wrong = || {
            format!("{text:?} is not a colour such as \"#ff8800\", \"gray-700\" or \"alpha/10\"")
        };
        let (name, percent) = match text.split_once('/') {
            Some((name, percent)) => (name, Some(percent)),
            None => (text.as_str(), None),
        };
        let mut color: Hsla = match name.strip_prefix('#') {
            Some(hex) => {
                let value = u32::from_str_radix(hex, 16).map_err(|_| wrong())?;
                let rgba = match hex.len() {
                    6 => (value << 8) | 0xff,
                    8 => value,
                    _ => return Err(wrong()),
                };
                gpui::rgba(rgba).into()
            }
            None => (sound_ui::theme::Theme::dark().named(name)).ok_or_else(wrong)?,
        };
        if let Some(percent) = percent {
            let percent: f32 = percent.parse().map_err(|_| wrong())?;
            color.a = (percent / 100.0).clamp(0.0, 1.0);
        }
        Ok(Self(color))
    }
}

impl Style {
    /// How it lines its children up, and nothing of its size or looks: the inside of a div
    /// whose children scroll.
    pub(crate) fn lines_up(&self, element: Div) -> Div {
        let lines = Style {
            direction: self.direction.clone(),
            gap: self.gap,
            align: self.align.clone(),
            justify: self.justify.clone(),
            ..Style::default()
        };
        lines.apply(element)
    }

    /// The axis its children scroll along, if any.
    pub(crate) fn scroll(&self) -> Option<ScrollAxis> {
        self.scroll
    }

    /// Its size and how it grows, for the box its children scroll in.
    pub(crate) fn sized(&self, mut element: Div) -> Div {
        element = element.flex();
        if let Some(width) = self.width {
            element = element.w(px(width));
        }
        if let Some(height) = self.height {
            element = element.h(px(height));
        }
        if self.grow {
            element = element.flex_grow(1.);
        }
        element
    }

    /// The height its style fixes, if any.
    pub(crate) fn fixed_height(&self) -> Option<f32> {
        self.height
    }

    /// Whether it stacks its children down, and the room between them.
    pub(crate) fn stacks_down(&self) -> Option<f32> {
        match self.direction {
            Some(Direction::Row) => None,
            Some(Direction::Column) | None => Some(self.gap.unwrap_or(0.0)),
        }
    }

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
        if let Some(padding) = self.padding_left {
            element = element.pl(px(padding));
        }
        if let Some(padding) = self.padding_right {
            element = element.pr(px(padding));
        }
        if let Some(padding) = self.padding_top {
            element = element.pt(px(padding));
        }
        if let Some(padding) = self.padding_bottom {
            element = element.pb(px(padding));
        }
        match self.scroll {
            // As large as its children along the axis, and as its box across it.
            Some(ScrollAxis::X) => element = element.flex_none().h_full(),
            Some(ScrollAxis::Y) => element = element.flex_none().w_full(),
            None => {
                if let Some(width) = self.width {
                    element = element.w(px(width));
                }
                if let Some(height) = self.height {
                    element = element.h(px(height));
                }
                if self.grow {
                    element = element.flex_grow(1.);
                }
            }
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
        if let Some(height) = self.line_height {
            element = element.line_height(px(height));
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
        if let Some(FontKind::Tabular) = self.font {
            element = element.font(sound_ui::typography::tabular());
        }
        element = match self.weight {
            Some(Weight::Normal) => element.font_weight(gpui::FontWeight::NORMAL),
            Some(Weight::Medium) => element.font_weight(gpui::FontWeight::MEDIUM),
            Some(Weight::Semibold) => element.font_weight(gpui::FontWeight::SEMIBOLD),
            None => element,
        };
        if let Some(Color(color)) = self.hover {
            element = element.hover(move |style| style.bg(color));
        }
        if self.keep {
            element = element.flex_none();
        }
        if self.truncate {
            element = element
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis();
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
    fn a_colour_of_the_theme_or_of_a_track_takes_an_opacity() {
        let alpha = |text: &str| Color::try_from(text.to_string()).map(|color| color.0.a);
        assert_eq!(alpha("alpha/10"), Ok(0.1));
        assert_eq!(alpha("#a9b1ff/70"), Ok(0.7));
        assert!(alpha("#a9b1ff/x").is_err());
    }

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

    #[test]
    fn a_node_made_twice_or_put_inside_itself_is_an_error() {
        let mut table = NodeTable::default();
        let made = ops(serde_json::json!([
            ["create", 1, "div", {}],
            ["create", 2, "div", {}],
            ["insert", 1, 2, null],
        ]));
        table.apply(made.unwrap()).unwrap();
        let twice = ops(serde_json::json!([["create", 1, "div", {}]]));
        assert!(
            table
                .apply(twice.unwrap())
                .unwrap_err()
                .contains("made twice")
        );
        let inside = ops(serde_json::json!([["insert", 2, 1, null]]));
        assert!(
            table
                .apply(inside.unwrap())
                .unwrap_err()
                .contains("into itself")
        );
        assert_eq!(table.children_of(1), [2]);
    }
}
