//! Sounds built in Rust as the SDK builds them in TypeScript, for the tests: each function adds
//! a node to the graph, and [`compile`] reads and lowers the graph as the runtime does.

#![allow(clippy::unwrap_used, dead_code)]

use std::cell::RefCell;
use std::ops::{Add, Div, Mul, Neg, Sub};

use serde_json::{Value, json};
use sound_signals::{ArraySpec, Code, ControlSpec, Declarations, Graph, ParameterSpec};

#[derive(Default)]
struct Building {
    nodes: Vec<Value>,
    feedbacks: Vec<Option<usize>>,
    buffers: Vec<f32>,
    watches: Vec<Value>,
    numbers: Vec<Value>,
    bangs: Vec<Value>,
    notes: Vec<Value>,
    /// Of the bins of a `spectral`: the node of the sound around it each capture reads.
    captures: Vec<usize>,
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
pub(crate) fn channel() -> Signal {
    node("channel", &[])
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
pub(crate) fn pitch() -> Signal {
    node("pitch", &[])
}
pub(crate) fn onset() -> Signal {
    node("onset", &[])
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
pub(crate) fn delay_up_to(x: impl Into<Signal>, ms: impl Into<Signal>, longest: f32) -> Signal {
    let operands = [
        ("x", x.into()),
        ("ms", ms.into()),
        ("longest", longest.into()),
    ];
    node("delay", &operands)
}
pub(crate) fn lowpass(x: impl Into<Signal>, hz: impl Into<Signal>, q: f32) -> Signal {
    filter("lowpass", x, hz, q)
}
/// `lowpass`, `highpass`, `bandpass`, `notch` or `allpass`.
pub(crate) fn filter(op: &str, x: impl Into<Signal>, hz: impl Into<Signal>, q: f32) -> Signal {
    node(op, &[("x", x.into()), ("hz", hz.into()), ("q", q.into())])
}
/// `peak`, `lowshelf` or `highshelf`.
pub(crate) fn gained_filter(
    op: &str,
    x: impl Into<Signal>,
    hz: f32,
    db: impl Into<Signal>,
    q: f32,
) -> Signal {
    let operands = [
        ("x", x.into()),
        ("hz", hz.into()),
        ("db", db.into()),
        ("q", q.into()),
    ];
    node(op, &operands)
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

/// Sends `value` out as the number `name`.
pub(crate) fn output_number(name: &str, value: impl Into<Signal>) {
    let value = value.into();
    let number = json!({ "name": name, "node": value.0 });
    BUILDING.with_borrow_mut(|building| building.numbers.push(number));
}

/// Sends a bang `name` where `when` goes above 0.
pub(crate) fn output_bang(name: &str, when: impl Into<Signal>) {
    let when = when.into();
    let bang = json!({ "name": name, "node": when.0 });
    BUILDING.with_borrow_mut(|building| building.bangs.push(bang));
}

/// Sends notes `name`: one sounds while `gate` is above 0.
pub(crate) fn output_notes(name: &str, gate: Signal, pitch: Signal, velocity: Signal) {
    let notes = json!({ "name": name, "gate": gate.0, "pitch": pitch.0, "velocity": velocity.0 });
    BUILDING.with_borrow_mut(|building| building.notes.push(notes));
}

/// What a bin of a `spectral` is.
pub(crate) struct Bin {
    pub(crate) magnitude: Signal,
    pub(crate) phase: Signal,
    pub(crate) hz: Signal,
    pub(crate) index: Signal,
}

/// Inside the bins of a `spectral`: `outer`, a signal of the sound around it.
pub(crate) fn capture(outer: Signal) -> Signal {
    let index = BUILDING.with_borrow_mut(|building| {
        building.captures.push(outer.0);
        building.captures.len() - 1
    });
    add(json!({ "op": "capture", "index": index }))
}

/// `x` through a spectrum of `size`, each bin as `bins` makes its magnitude and phase. A
/// signal of the sound around it reaches the bins through `capture`.
pub(crate) fn spectral(
    x: impl Into<Signal>,
    size: usize,
    bins: impl FnOnce(Bin) -> (Signal, Signal),
) -> Signal {
    let x = x.into();
    let around = BUILDING.take();
    let bin = Bin {
        magnitude: node("magnitude", &[]),
        phase: node("phase", &[]),
        hz: node("hz", &[]),
        index: node("index", &[]),
    };
    let (magnitude, phase) = bins(bin);
    let inside = BUILDING.replace(around);
    let feedbacks: Vec<usize> = (inside.feedbacks.iter())
        .map(|value| value.expect("a feedback is never set"))
        .collect();
    add(json!({
        "op": "spectral",
        "x": x.0,
        "size": size,
        "bins": {
            "nodes": inside.nodes,
            "feedbacks": feedbacks,
            "captures": inside.captures,
            "output": { "magnitude": magnitude.0, "phase": phase.0 },
        },
    }))
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
        "numbers": building.numbers,
        "bangs": building.bangs,
        "notes": building.notes,
        "output": output.0,
    });
    let graph: Graph = serde_json::from_value(graph).unwrap();
    graph.compile(&building.declarations).unwrap()
}
