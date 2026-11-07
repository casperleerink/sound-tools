//! Pictures: a graph of signals run once per pixel, a whole row at a time, as the machine of a
//! sound runs a span of frames. What it reads from outside are numbers worked out once a frame
//! (`lives`), and the frame before (`last`).

use serde::Deserialize;

/// The most pixels across.
pub const MAX_ACROSS: u32 = 480;
const DEFAULT_ACROSS: u32 = 160;

/// A node of the graph of a picture; each `usize` is the index of a node before it.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Node {
    Constant {
        value: f32,
    },
    /// Where the pixel is, 0 at the left and the top, 1 at the right and the bottom.
    X,
    Y,
    /// How many times wider than tall it is.
    Aspect,
    /// The live value of this place in [`Graph::lives`].
    Live {
        index: usize,
    },
    /// A colour of the last frame at `x`, `y`.
    Last {
        channel: Channel,
        x: usize,
        y: usize,
    },
    /// White noise from -1 to 1, new on every frame.
    Noise,
    Negate {
        x: usize,
    },
    Sin {
        x: usize,
    },
    Cos {
        x: usize,
    },
    Tan {
        x: usize,
    },
    Tanh {
        x: usize,
    },
    Abs {
        x: usize,
    },
    Sqrt {
        x: usize,
    },
    Exp {
        x: usize,
    },
    Log {
        x: usize,
    },
    Floor {
        x: usize,
    },
    Wrap {
        x: usize,
    },
    Add {
        a: usize,
        b: usize,
    },
    Subtract {
        a: usize,
        b: usize,
    },
    Multiply {
        a: usize,
        b: usize,
    },
    Divide {
        a: usize,
        b: usize,
    },
    Remainder {
        a: usize,
        b: usize,
    },
    Less {
        a: usize,
        b: usize,
    },
    Greater {
        a: usize,
        b: usize,
    },
    LessOrEqual {
        a: usize,
        b: usize,
    },
    GreaterOrEqual {
        a: usize,
        b: usize,
    },
    Equal {
        a: usize,
        b: usize,
    },
    NotEqual {
        a: usize,
        b: usize,
    },
    Min {
        a: usize,
        b: usize,
    },
    Max {
        a: usize,
        b: usize,
    },
    Pow {
        a: usize,
        b: usize,
    },
    Clamp {
        x: usize,
        low: usize,
        high: usize,
    },
    Mix {
        a: usize,
        b: usize,
        amount: usize,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    Red,
    Green,
    Blue,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rgb {
    red: usize,
    green: usize,
    blue: usize,
}

/// A graph of a picture, checked.
#[derive(Debug)]
pub struct Program {
    nodes: Vec<Node>,
    output: [usize; 3],
    across: u32,
}

/// The colours of a frame, row by row from the top, 0 to 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[f32; 3]>,
}

impl Frame {
    /// Its pixels as blue, green, red and alpha, a byte each: as GPUI paints an image.
    pub fn bgra(&self) -> Vec<u8> {
        let byte = |value: f32| (value * 255.0).round() as u8;
        (self.pixels.iter())
            .flat_map(|[red, green, blue]| [byte(*blue), byte(*green), byte(*red), 255])
            .collect()
    }

    /// The colour at `x`, `y` from 0 to 1, between the pixels around it; past an edge, the edge.
    fn at(&self, x: f32, y: f32, channel: usize) -> f32 {
        if self.pixels.is_empty() {
            return 0.0;
        }
        let x = (finite(x) * self.width as f32 - 0.5).clamp(0.0, (self.width - 1) as f32);
        let y = (finite(y) * self.height as f32 - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (left, top) = (x.floor() as usize, y.floor() as usize);
        let (right, bottom) = (
            (left + 1).min(self.width - 1),
            (top + 1).min(self.height - 1),
        );
        let (across, down) = (x - left as f32, y - top as f32);
        let value = |column: usize, row: usize| {
            (self.pixels.get(row * self.width + column))
                .and_then(|pixel| pixel.get(channel))
                .copied()
                .unwrap_or(0.0)
        };
        let upper = value(left, top) + (value(right, top) - value(left, top)) * across;
        let lower = value(left, bottom) + (value(right, bottom) - value(left, bottom)) * across;
        upper + (lower - upper) * down
    }
}

impl Program {
    /// Checks the graph; `lives` is how many live values it may read.
    pub fn new(
        nodes: Vec<Node>,
        output: Rgb,
        across: Option<u32>,
        lives: usize,
    ) -> Result<Self, String> {
        for (index, node) in nodes.iter().enumerate() {
            if node.reads().into_iter().flatten().any(|read| read >= index) {
                return Err(format!("node {index} of a picture reads a node after it"));
            }
            if let Node::Live { index: live } = node
                && *live >= lives
            {
                return Err(format!(
                    "a picture reads live value {live}, which it does not have"
                ));
            }
        }
        let Rgb { red, green, blue } = output;
        let output = [red, green, blue];
        if output.iter().any(|node| *node >= nodes.len()) {
            return Err("a picture's colour is a node it does not have".to_string());
        }
        Ok(Self {
            nodes,
            output,
            across: across.unwrap_or(DEFAULT_ACROSS).clamp(1, MAX_ACROSS),
        })
    }

    /// How many pixels across it is worked out.
    pub fn across(&self) -> u32 {
        self.across
    }
}

impl Node {
    fn reads(self) -> [Option<usize>; 3] {
        match self {
            Self::Constant { .. }
            | Self::X
            | Self::Y
            | Self::Aspect
            | Self::Live { .. }
            | Self::Noise => [None; 3],
            Self::Last { x, y, .. } => [Some(x), Some(y), None],
            Self::Negate { x }
            | Self::Sin { x }
            | Self::Cos { x }
            | Self::Tan { x }
            | Self::Tanh { x }
            | Self::Abs { x }
            | Self::Sqrt { x }
            | Self::Exp { x }
            | Self::Log { x }
            | Self::Floor { x }
            | Self::Wrap { x } => [Some(x), None, None],
            Self::Add { a, b }
            | Self::Subtract { a, b }
            | Self::Multiply { a, b }
            | Self::Divide { a, b }
            | Self::Remainder { a, b }
            | Self::Less { a, b }
            | Self::Greater { a, b }
            | Self::LessOrEqual { a, b }
            | Self::GreaterOrEqual { a, b }
            | Self::Equal { a, b }
            | Self::NotEqual { a, b }
            | Self::Min { a, b }
            | Self::Max { a, b }
            | Self::Pow { a, b } => [Some(a), Some(b), None],
            Self::Clamp { x, low, high } => [Some(x), Some(low), Some(high)],
            Self::Mix { a, b, amount } => [Some(a), Some(b), Some(amount)],
        }
    }
}

/// The frame of `size` pixels the graph of `program` makes, row by row: each node over a whole
/// row at a time, as the machine of a sound runs a span of frames.
pub fn render(
    program: &Program,
    lives: &[f32],
    (width, height): (usize, usize),
    last: &Frame,
    frame: u32,
) -> Frame {
    let nodes = &program.nodes;
    let mut registers = vec![0.0_f32; nodes.len() * width];
    let mut pixels = vec![[0.0_f32; 3]; width * height];
    for row in 0..height {
        let y = (row as f32 + 0.5) / height as f32;
        for (index, node) in nodes.iter().enumerate() {
            let (before, after) = registers.split_at_mut(index * width);
            let Some(out) = after.get_mut(..width) else {
                continue;
            };
            let read = |node: usize| before.get(node * width..(node + 1) * width).unwrap_or(&[]);
            run(
                *node,
                out,
                read,
                Place {
                    row,
                    y,
                    width,
                    height,
                    lives,
                    last,
                    frame,
                },
            );
        }
        let pixels = pixels
            .get_mut(row * width..(row + 1) * width)
            .unwrap_or_default();
        for (channel, node) in program.output.iter().enumerate() {
            let values = registers
                .get(node * width..(node + 1) * width)
                .unwrap_or(&[]);
            for (pixel, value) in pixels.iter_mut().zip(values) {
                if let Some(colour) = pixel.get_mut(channel) {
                    *colour = finite(*value).clamp(0.0, 1.0);
                }
            }
        }
    }
    Frame {
        width,
        height,
        pixels,
    }
}

/// What a node reads of where it runs.
#[derive(Clone, Copy)]
struct Place<'a> {
    row: usize,
    y: f32,
    width: usize,
    height: usize,
    lives: &'a [f32],
    last: &'a Frame,
    frame: u32,
}

fn run<'a>(node: Node, out: &mut [f32], read: impl Fn(usize) -> &'a [f32], place: Place<'_>) {
    let unary = |out: &mut [f32], x: usize, function: fn(f32) -> f32| {
        for (out, x) in out.iter_mut().zip(read(x)) {
            *out = function(*x);
        }
    };
    let binary = |out: &mut [f32], a: usize, b: usize, function: fn(f32, f32) -> f32| {
        for ((out, a), b) in out.iter_mut().zip(read(a)).zip(read(b)) {
            *out = function(*a, *b);
        }
    };
    match node {
        Node::Constant { value } => out.fill(value),
        Node::X => {
            for (column, out) in out.iter_mut().enumerate() {
                *out = (column as f32 + 0.5) / place.width as f32;
            }
        }
        Node::Y => out.fill(place.y),
        Node::Aspect => out.fill(place.width as f32 / place.height as f32),
        Node::Live { index } => out.fill(place.lives.get(index).copied().unwrap_or(0.0)),
        Node::Last { channel, x, y } => {
            for ((out, x), y) in out.iter_mut().zip(read(x)).zip(read(y)) {
                *out = place.last.at(*x, *y, channel as usize);
            }
        }
        Node::Noise => {
            for (column, out) in out.iter_mut().enumerate() {
                let pixel = (place.row * place.width + column) as u32;
                *out = hashed(pixel, place.frame);
            }
        }
        Node::Negate { x } => unary(out, x, |x| -x),
        Node::Sin { x } => unary(out, x, f32::sin),
        Node::Cos { x } => unary(out, x, f32::cos),
        Node::Tan { x } => unary(out, x, f32::tan),
        Node::Tanh { x } => unary(out, x, f32::tanh),
        Node::Abs { x } => unary(out, x, f32::abs),
        Node::Sqrt { x } => unary(out, x, |x| x.max(0.0).sqrt()),
        Node::Exp { x } => unary(out, x, f32::exp),
        Node::Log { x } => unary(out, x, |x| x.max(f32::MIN_POSITIVE).ln()),
        Node::Floor { x } => unary(out, x, f32::floor),
        Node::Wrap { x } => unary(out, x, |x| x - x.floor()),
        Node::Add { a, b } => binary(out, a, b, |a, b| a + b),
        Node::Subtract { a, b } => binary(out, a, b, |a, b| a - b),
        Node::Multiply { a, b } => binary(out, a, b, |a, b| a * b),
        Node::Divide { a, b } => binary(out, a, b, |a, b| if b == 0.0 { 0.0 } else { a / b }),
        Node::Remainder { a, b } => {
            binary(
                out,
                a,
                b,
                |a, b| if b == 0.0 { 0.0 } else { a.rem_euclid(b) },
            );
        }
        Node::Less { a, b } => binary(out, a, b, |a, b| truth(a < b)),
        Node::Greater { a, b } => binary(out, a, b, |a, b| truth(a > b)),
        Node::LessOrEqual { a, b } => binary(out, a, b, |a, b| truth(a <= b)),
        Node::GreaterOrEqual { a, b } => binary(out, a, b, |a, b| truth(a >= b)),
        Node::Equal { a, b } => binary(out, a, b, |a, b| truth(a == b)),
        Node::NotEqual { a, b } => binary(out, a, b, |a, b| truth(a != b)),
        Node::Min { a, b } => binary(out, a, b, f32::min),
        Node::Max { a, b } => binary(out, a, b, f32::max),
        Node::Pow { a, b } => binary(out, a, b, f32::powf),
        Node::Clamp { x, low, high } => {
            let values = read(x).iter().zip(read(low)).zip(read(high));
            for (out, ((x, low), high)) in out.iter_mut().zip(values) {
                *out = x.max(*low).min(*high);
            }
        }
        Node::Mix { a, b, amount } => {
            let values = read(a).iter().zip(read(b)).zip(read(amount));
            for (out, ((a, b), amount)) in out.iter_mut().zip(values) {
                *out = a + (b - a) * amount;
            }
        }
    }
}

/// -1 to 1, the same for the same pixel and frame.
fn hashed(pixel: u32, frame: u32) -> f32 {
    let mut value = pixel.wrapping_mul(0x9E37_79B9) ^ frame.wrapping_mul(0x85EB_CA6B);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^= value >> 16;
    (value as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn truth(holds: bool) -> f32 {
    f32::from(u8::from(holds))
}

pub fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Graph {
        nodes: Vec<Node>,
        lives: usize,
        output: Rgb,
    }

    fn program(json: serde_json::Value) -> Program {
        let Graph {
            nodes,
            lives,
            output,
        } = serde_json::from_value(json).unwrap();
        Program::new(nodes, output, None, lives).unwrap()
    }

    #[test]
    fn a_picture_runs_its_graph_once_per_pixel() {
        // Red is x, green is y, blue is a live value: a gradient over a level.
        let program = program(serde_json::json!({
            "nodes": [{"op": "x"}, {"op": "y"}, {"op": "live", "index": 0}],
            "lives": 1,
            "output": {"red": 0, "green": 1, "blue": 2},
        }));
        let frame = render(&program, &[0.25], (4, 2), &Frame::default(), 0);
        assert_eq!(frame.pixels[0], [0.125, 0.25, 0.25]);
        assert_eq!(frame.pixels[7], [0.875, 0.75, 0.25]);
    }

    #[test]
    fn a_picture_reads_its_last_frame_between_its_pixels() {
        // Each frame adds a little to what was there, then it stays at 1.
        let program = program(serde_json::json!({
            "nodes": [
                {"op": "x"}, {"op": "y"}, {"op": "last", "channel": "red", "x": 0, "y": 1},
                {"op": "constant", "value": 0.4}, {"op": "add", "a": 2, "b": 3},
            ],
            "lives": 0,
            "output": {"red": 4, "green": 4, "blue": 4},
        }));
        let mut last = Frame::default();
        let mut reds = Vec::new();
        for frame in 0..3 {
            last = render(&program, &[], (3, 3), &last, frame);
            reds.push(last.pixels[4][0]);
        }
        assert_eq!(reds, [0.4, 0.8, 1.0]);
        // Halfway between a pixel of 0 and one of 1 reads a half.
        let edge = Frame {
            width: 2,
            height: 1,
            pixels: vec![[0.0; 3], [1.0; 3]],
        };
        assert_eq!(edge.at(0.5, 0.5, 0), 0.5);
    }

    #[test]
    fn a_picture_that_reads_ahead_does_not_load() {
        let nodes = serde_json::from_value(serde_json::json!([{"op": "sin", "x": 0}])).unwrap();
        let output = Rgb {
            red: 0,
            green: 0,
            blue: 0,
        };
        assert!(Program::new(nodes, output, None, 0).is_err());
    }
}
