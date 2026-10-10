//! Sounds built in Rust as the SDK builds them in TypeScript, for the tests: each function adds
//! a node to the graph, and [`compile`] reads and lowers the graph as the runtime does.

#![allow(clippy::unwrap_used, dead_code)]

use std::cell::RefCell;
use std::ops::{Add, Div, Mul, Neg, Sub};

use serde_json::{Value, json};
use sound_hum::{ArraySpec, Code, ControlSpec, Declarations, Graph, ParameterSpec};

#[derive(Default)]
struct Building {
    nodes: Vec<Value>,
    feedbacks: Vec<Option<usize>>,
    buffers: Vec<f32>,
    watches: Vec<Value>,
    declarations: Declarations,
}

thread_local! {
    static BUILDING: RefCell<Building> = RefCell::default();
}

/// A node of the sound being built. A number is one too, where it is used.
#[derive(Clone, Copy)]
pub(crate) struct Signal(usize);

fn add(node: Value) -> Signal {
    BUILDING.with_borrow_mut(|building| {
        building.nodes.push(node);
        Signal(building.nodes.len() - 1)
    })
}

fn node(op: &str, operands: &[(&str, Signal)]) -> Signal {
    let mut node = json!({ "op": op });
    for (name, operand) in operands {
        node[name] = json!(operand.0);
    }
    add(node)
}

impl From<f32> for Signal {
    fn from(value: f32) -> Self {
        add(json!({ "op": "constant", "value": value }))
    }
}

macro_rules! operator {
    ($trait:ident, $method:ident, $op:literal) => {
        impl<T: Into<Signal>> $trait<T> for Signal {
            type Output = Signal;
            fn $method(self, other: T) -> Signal {
                node($op, &[("a", self), ("b", other.into())])
            }
        }
        impl $trait<Signal> for f32 {
            type Output = Signal;
            fn $method(self, other: Signal) -> Signal {
                node($op, &[("a", self.into()), ("b", other)])
            }
        }
    };
}
operator!(Add, add, "add");
operator!(Sub, sub, "subtract");
operator!(Mul, mul, "multiply");
operator!(Div, div, "divide");

impl Neg for Signal {
    type Output = Signal;
    fn neg(self) -> Signal {
        node("negate", &[("x", self)])
    }
}

impl Signal {
    pub(crate) fn lt(self, other: impl Into<Signal>) -> Signal {
        node("less", &[("a", self), ("b", other.into())])
    }
}

pub(crate) fn input() -> Signal {
    node("input", &[])
}
pub(crate) fn input_left() -> Signal {
    node("inputLeft", &[])
}
pub(crate) fn input_right() -> Signal {
    node("inputRight", &[])
}
pub(crate) fn beat() -> Signal {
    node("beat", &[])
}
pub(crate) fn freq() -> Signal {
    node("frequency", &[])
}
pub(crate) fn gate() -> Signal {
    node("gate", &[])
}
pub(crate) fn velocity() -> Signal {
    node("velocity", &[])
}
pub(crate) fn noise() -> Signal {
    node("noise", &[])
}

fn one(op: &str, x: impl Into<Signal>) -> Signal {
    node(op, &[("x", x.into())])
}
pub(crate) fn sin(x: impl Into<Signal>) -> Signal {
    one("sin", x)
}
pub(crate) fn wrap(x: impl Into<Signal>) -> Signal {
    one("wrap", x)
}
pub(crate) fn saturate(x: impl Into<Signal>) -> Signal {
    one("saturate", x)
}
pub(crate) fn rise(x: impl Into<Signal>) -> Signal {
    one("rise", x)
}
pub(crate) fn phasor(hz: impl Into<Signal>) -> Signal {
    node("phasor", &[("hz", hz.into())])
}
pub(crate) fn hold(x: impl Into<Signal>, when: impl Into<Signal>) -> Signal {
    node("hold", &[("x", x.into()), ("when", when.into())])
}
pub(crate) fn mix(a: impl Into<Signal>, b: impl Into<Signal>, amount: impl Into<Signal>) -> Signal {
    node(
        "mix",
        &[("a", a.into()), ("b", b.into()), ("amount", amount.into())],
    )
}
pub(crate) fn delay(x: impl Into<Signal>, ms: impl Into<Signal>) -> Signal {
    node("delay", &[("x", x.into()), ("ms", ms.into())])
}
pub(crate) fn lowpass(x: impl Into<Signal>, hz: impl Into<Signal>, q: f32) -> Signal {
    node(
        "lowpass",
        &[("x", x.into()), ("hz", hz.into()), ("q", q.into())],
    )
}
pub(crate) fn adsr(
    gate: impl Into<Signal>,
    attack: f32,
    decay: f32,
    sustain: impl Into<Signal>,
    release: f32,
) -> Signal {
    let operands = [
        ("gate", gate.into()),
        ("attack", attack.into()),
        ("decay", decay.into()),
        ("sustain", sustain.into()),
        ("release", release.into()),
    ];
    node("adsr", &operands)
}

fn spec(name: &str, default: f32, [min, max]: [f32; 2]) -> ParameterSpec {
    let name = name.to_string();
    ParameterSpec {
        name,
        default,
        min,
        max,
    }
}

fn declare(declare: impl FnOnce(&mut Declarations)) {
    BUILDING.with_borrow_mut(|building| declare(&mut building.declarations));
}

fn named(name: &str) -> Signal {
    add(json!({ "op": "param", "name": name }))
}

/// A knob or a toggle.
pub(crate) fn param(name: &str, default: f32, range: [f32; 2]) -> Signal {
    declare(|declarations| (declarations.parameters).push(spec(name, default, range)));
    named(name)
}

pub(crate) fn live(name: &str, default: f32, range: [f32; 2]) -> Signal {
    let live = ControlSpec::Live(spec(name, default, range));
    declare(|declarations| declarations.controls.push(live));
    named(name)
}

pub(crate) fn trigger(name: &str) -> Signal {
    let trigger = ControlSpec::Trigger(name.to_string());
    declare(|declarations| declarations.controls.push(trigger));
    named(name)
}

/// A table: a pattern of `length` values from 0 to 1, or a buffer.
#[derive(Clone)]
pub(crate) struct Table(Value);

pub(crate) fn pattern(name: &str, length: usize) -> Table {
    let array = ArraySpec {
        name: name.to_string(),
        length: Some(length),
        default: 0.0,
        min: 0.0,
        max: 1.0,
    };
    declare(|declarations| declarations.arrays.push(array));
    Table(json!({ "list": name }))
}

pub(crate) fn buffer(seconds: f32) -> Table {
    BUILDING.with_borrow_mut(|building| {
        building.buffers.push(seconds);
        Table(json!({ "buffer": building.buffers.len() - 1 }))
    })
}

impl Table {
    pub(crate) fn at(&self, index: impl Into<Signal>) -> Signal {
        let index = index.into();
        add(json!({ "op": "at", "table": self.0, "index": index.0 }))
    }

    pub(crate) fn write(&self, index: impl Into<Signal>, value: impl Into<Signal>) {
        let (index, value) = (index.into(), value.into());
        let buffer = &self.0["buffer"];
        add(json!({ "op": "write", "buffer": buffer, "index": index.0, "value": value.0 }));
    }
}

pub(crate) fn lookup(table: &Table, phase: impl Into<Signal>) -> Signal {
    let phase = phase.into();
    add(json!({ "op": "lookup", "table": table.0, "phase": phase.0 }))
}

/// A value that feeds back, by its slot.
#[derive(Clone, Copy)]
pub(crate) struct Feedback(usize);

pub(crate) fn feedback() -> Feedback {
    BUILDING.with_borrow_mut(|building| {
        building.feedbacks.push(None);
        Feedback(building.feedbacks.len() - 1)
    })
}

impl Feedback {
    /// What it was set to one frame before.
    pub(crate) fn read(self) -> Signal {
        add(json!({ "op": "feedback", "slot": self.0 }))
    }

    pub(crate) fn set(self, value: impl Into<Signal>) {
        let value = value.into();
        BUILDING.with_borrow_mut(|building| building.feedbacks[self.0] = Some(value.0));
    }
}

pub(crate) fn watch(name: &str, value: impl Into<Signal>) {
    let value = value.into();
    let watch = json!({ "name": name, "node": value.0 });
    BUILDING.with_borrow_mut(|building| building.watches.push(watch));
}

/// What leaves a sound: one signal, or a pair for a sound in stereo.
pub(crate) struct Output(Value);

impl From<Signal> for Output {
    fn from(signal: Signal) -> Self {
        Self(json!({ "mono": signal.0 }))
    }
}

impl From<f32> for Output {
    fn from(value: f32) -> Self {
        Signal::from(value).into()
    }
}

impl From<(Signal, Signal)> for Output {
    fn from((left, right): (Signal, Signal)) -> Self {
        Self(json!({ "left": left.0, "right": right.0 }))
    }
}

/// The code of the sound `build` makes, read from JSON and lowered as the runtime does.
pub(crate) fn compile<O: Into<Output>>(build: impl FnOnce() -> O) -> Code {
    BUILDING.set(Building::default());
    let output = build().into();
    let building = BUILDING.take();
    let feedbacks: Vec<usize> = (building.feedbacks.iter())
        .map(|value| value.expect("a feedback is never set"))
        .collect();
    let graph = json!({
        "nodes": building.nodes,
        "feedbacks": feedbacks,
        "buffers": building.buffers,
        "watches": building.watches,
        "output": output.0,
    });
    let graph: Graph = serde_json::from_value(graph).unwrap();
    graph.compile(&building.declarations).unwrap()
}
