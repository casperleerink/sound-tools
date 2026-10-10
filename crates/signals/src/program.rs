//! The order a [`Machine`](crate::Machine) runs the operations of [`Code`] in over a span of
//! frames, worked out when the machine is made.
//!
//! Most operations run over the whole span, one after the other. Those that hear each other
//! within a frame run together in a loop, frame by frame: around a feedback, whose read hears
//! what its source was in the frame before, and through a buffer, which a write changes for the
//! reads after it. A feedback read in no loop with its source runs after it over the span, one
//! frame behind.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::code::{Code, Operation, Register, Table};

#[derive(Clone, Debug)]
pub(crate) enum Step {
    /// One operation over the whole span.
    Block(Register),
    /// Operations in their order, frame by frame.
    Loop(Box<[(Register, Operation)]>),
}

/// Whether `operation` is the same in every frame for as long as a machine lives. It is worked
/// out when the machine is made, and is no step.
pub(crate) fn is_fixed(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::Constant(_) | Operation::SampleRate | Operation::Length(Table::Buffer(_))
    )
}

/// The steps that run `code` over a span. Each operation comes after what it reads, and
/// otherwise as close to its own place as it can, so the order stays near the one the graph
/// was written in.
pub(crate) fn schedule(code: &Code) -> Box<[Step]> {
    let edges = edges(code);
    let (component_of, members) = components(&edges);
    // How many edges into each component are from components not run yet.
    let mut waiting = vec![0_usize; members.len()];
    for (from, targets) in edges.iter().enumerate() {
        for to in targets {
            let (Some(from), Some(to)) = (component_of.get(from), component_of.get(*to)) else {
                continue;
            };
            if let Some(count) = waiting.get_mut(*to)
                && from != to
            {
                *count += 1;
            }
        }
    }
    // Kahn's algorithm, the ready component with the first operation first. A component is
    // known by its first operation.
    let mut ready: BinaryHeap<Reverse<usize>> = (members.iter().zip(&waiting))
        .filter(|(_, waiting)| **waiting == 0)
        .filter_map(|(nodes, _)| nodes.first().copied().map(Reverse))
        .collect();
    let mut steps = Vec::new();
    while let Some(Reverse(first)) = ready.pop() {
        let Some(&component) = component_of.get(first) else {
            continue;
        };
        let nodes = members.get(component).map_or(&[][..], Vec::as_slice);
        let looped =
            nodes.len() > 1 || (edges.get(first)).is_some_and(|targets| targets.contains(&first));
        let operation = code.operations.get(first).copied();
        if looped {
            steps.push(Step::Loop(
                (nodes.iter())
                    .filter_map(|node| Some((*node as Register, *code.operations.get(*node)?)))
                    .collect(),
            ));
        } else if operation.is_some_and(|operation| !is_fixed(operation)) {
            steps.push(Step::Block(first as Register));
        }
        for to in nodes.iter().filter_map(|node| edges.get(*node)).flatten() {
            let Some(&next) = component_of.get(*to) else {
                continue;
            };
            if next == component {
                continue;
            }
            if let Some(count) = waiting.get_mut(next) {
                *count -= 1;
                if *count == 0
                    && let Some(first) = members.get(next).and_then(|nodes| nodes.first())
                {
                    ready.push(Reverse(*first));
                }
            }
        }
    }
    steps.into_boxed_slice()
}

/// For each operation, those that run after it: what reads it, the reads of a feedback it is
/// the source of, and, in a ring, the next operation on its buffer.
fn edges(code: &Code) -> Vec<Vec<usize>> {
    let count = code.operations.len();
    let mut edges = vec![Vec::new(); count];
    let mut add = |from: usize, to: usize| {
        if to < count
            && let Some(targets) = edges.get_mut(from)
        {
            targets.push(to);
        }
    };
    // The operations on each buffer, which take turns in every frame.
    let mut on_buffers: Vec<Vec<usize>> = vec![Vec::new(); code.slots.buffers.len()];
    for (index, operation) in code.operations.iter().enumerate() {
        for operand in operands(*operation) {
            add(usize::from(operand), index);
        }
        if let Operation::History(slot) = operation
            && let Some(source) = code.feedbacks.get(usize::from(*slot))
        {
            add(usize::from(*source), index);
        }
        if let Some(buffer) = buffer_of(*operation)
            && let Some(operations) = on_buffers.get_mut(usize::from(buffer))
        {
            operations.push(index);
        }
    }
    // A ring through all of them puts them in one loop.
    for operations in on_buffers.iter().filter(|operations| operations.len() > 1) {
        let next = operations.iter().cycle().skip(1);
        for (from, to) in operations.iter().zip(next) {
            add(*from, *to);
        }
    }
    edges
}

/// The registers `operation` reads in the same frame.
fn operands(operation: Operation) -> Vec<Register> {
    match operation {
        Operation::Unary(_, x)
        | Operation::Phasor { hz: x, .. }
        | Operation::Rise { input: x, .. }
        | Operation::Change { input: x, .. }
        | Operation::Read { index: x, .. }
        | Operation::Lookup { phase: x, .. } => vec![x],
        Operation::Binary(_, a, b)
        | Operation::Delay {
            input: a, ms: b, ..
        }
        | Operation::Smooth {
            input: a, ms: b, ..
        }
        | Operation::Hold {
            input: a, when: b, ..
        }
        | Operation::Write {
            index: a, value: b, ..
        } => vec![a, b],
        Operation::Clamp(a, b, c)
        | Operation::Mix(a, b, c)
        | Operation::Filter {
            input: a,
            hz: b,
            q: c,
            ..
        } => vec![a, b, c],
        Operation::Envelope {
            gate,
            attack,
            decay,
            sustain,
            release,
            ..
        } => vec![gate, attack, decay, sustain, release],
        Operation::Constant(_)
        | Operation::Input
        | Operation::InputLeft
        | Operation::InputRight
        | Operation::Channel
        | Operation::SampleRate
        | Operation::Beat
        | Operation::Bpm
        | Operation::Playing
        | Operation::Frequency
        | Operation::Pitch
        | Operation::Gate
        | Operation::Velocity
        | Operation::Onset
        | Operation::Parameter(_)
        | Operation::Live(_)
        | Operation::Trigger(_)
        | Operation::History(_)
        | Operation::Noise { .. }
        | Operation::Length(_) => Vec::new(),
    }
}

/// The buffer `operation` reads or writes in each frame. Its length is the same in every one.
fn buffer_of(operation: Operation) -> Option<u16> {
    match operation {
        Operation::Read {
            table: Table::Buffer(buffer),
            ..
        }
        | Operation::Lookup {
            table: Table::Buffer(buffer),
            ..
        }
        | Operation::Write { buffer, .. } => Some(buffer),
        _ => None,
    }
}

/// The strongly connected components of a graph, by Tarjan's algorithm: the component of each
/// node, and the nodes of each component in order. Without recursion, so a long chain of
/// operations does not overflow the stack.
fn components(edges: &[Vec<usize>]) -> (Vec<usize>, Vec<Vec<usize>>) {
    let mut tarjan = Tarjan {
        seen: vec![NONE; edges.len()],
        low: vec![NONE; edges.len()],
        component_of: vec![NONE; edges.len()],
        stack: Vec::new(),
        members: Vec::new(),
        seen_count: 0,
    };
    for root in 0..edges.len() {
        if tarjan.seen.get(root) == Some(&NONE) {
            tarjan.search(edges, root);
        }
    }
    (tarjan.component_of, tarjan.members)
}

const NONE: usize = usize::MAX;

struct Tarjan {
    /// When each node was first seen.
    seen: Vec<usize>,
    /// The first seen node on the stack each node reaches.
    low: Vec<usize>,
    /// [`NONE`] for a node in no component yet: one seen is then on the stack.
    component_of: Vec<usize>,
    stack: Vec<usize>,
    members: Vec<Vec<usize>>,
    seen_count: usize,
}

impl Tarjan {
    fn search(&mut self, edges: &[Vec<usize>], root: usize) {
        // Each node being visited, and the next of its edges to follow.
        let mut visiting = vec![(root, 0)];
        self.visit(root);
        while let Some((node, next)) = visiting.last_mut() {
            let node = *node;
            if let Some(&to) = edges.get(node).and_then(|targets| targets.get(*next)) {
                *next += 1;
                if self.seen.get(to) == Some(&NONE) {
                    self.visit(to);
                    visiting.push((to, 0));
                } else if self.component_of.get(to) == Some(&NONE) {
                    let reached = self.seen.get(to).copied().unwrap_or(NONE);
                    self.lower(node, reached);
                }
                continue;
            }
            visiting.pop();
            let low = self.low.get(node).copied().unwrap_or(NONE);
            if let Some((parent, _)) = visiting.last() {
                self.lower(*parent, low);
            }
            if Some(&low) == self.seen.get(node) {
                let mut nodes = Vec::new();
                while let Some(member) = self.stack.pop() {
                    if let Some(component) = self.component_of.get_mut(member) {
                        *component = self.members.len();
                    }
                    nodes.push(member);
                    if member == node {
                        break;
                    }
                }
                nodes.sort_unstable();
                self.members.push(nodes);
            }
        }
    }

    fn visit(&mut self, node: usize) {
        if let (Some(seen), Some(low)) = (self.seen.get_mut(node), self.low.get_mut(node)) {
            (*seen, *low) = (self.seen_count, self.seen_count);
        }
        self.seen_count += 1;
        self.stack.push(node);
    }

    fn lower(&mut self, node: usize, to: usize) {
        if let Some(low) = self.low.get_mut(node) {
            *low = (*low).min(to);
        }
    }
}
