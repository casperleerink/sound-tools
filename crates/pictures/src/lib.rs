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
    /// A list of [`Inputs::waves`] at `at`, 0 its first value and 1 its last, between them.
    Wave {
        table: Wave,
        at: usize,
    },
    /// A colour of a frame of [`Inputs::videos`] at `x`, `y`.
    Video {
        table: VideoTable,
        channel: Channel,
        x: usize,
        y: usize,
    },
    /// A colour of the last frame at `x`, `y`.
    Last {
        channel: Channel,
        x: usize,
        y: usize,
    },
    /// White noise from -1 to 1, new on every frame.
    Noise,
    /// Noise that changes smoothly with `x`, `y` and `z`, from -1 to 1: one bump per unit.
    SmoothNoise {
        x: usize,
        y: usize,
        z: usize,
    },
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
#[serde(deny_unknown_fields)]
pub struct Wave {
    wave: usize,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoTable {
    video: usize,
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
    /// The nodes whose mean over the frame it gives, see [`Rendered::means`].
    measures: Vec<usize>,
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
    /// A frame of `width` by `height` from bytes of red, green, blue and alpha.
    pub fn from_rgba(width: usize, height: usize, rgba: &[u8]) -> Self {
        let pixels = (rgba.chunks_exact(4))
            .map(|pixel| {
                [0, 1, 2].map(|channel| f32::from(pixel.get(channel).copied().unwrap_or(0)) / 255.0)
            })
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }

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
    /// Checks the graph; `lives`, `waves` and `videos` are how many live values, lists and
    /// videos it may read.
    pub fn new(
        nodes: Vec<Node>,
        (output, measures): (Rgb, Vec<usize>),
        across: Option<u32>,
        (lives, waves, videos): (usize, usize, usize),
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
            if let Node::Video { table, .. } = node
                && table.video >= videos
            {
                return Err(format!(
                    "a picture reads video {}, which it does not have",
                    table.video
                ));
            }
            if let Node::Wave { table, .. } = node
                && table.wave >= waves
            {
                return Err(format!(
                    "a picture reads list {}, which it does not have",
                    table.wave
                ));
            }
        }
        let Rgb { red, green, blue } = output;
        let output = [red, green, blue];
        if output.iter().any(|node| *node >= nodes.len()) {
            return Err("a picture's colour is a node it does not have".to_string());
        }
        if measures.iter().any(|node| *node >= nodes.len()) {
            return Err("a picture measures a node it does not have".to_string());
        }
        Ok(Self {
            nodes,
            output,
            measures,
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
            Self::Last { x, y, .. } | Self::Video { x, y, .. } => [Some(x), Some(y), None],
            Self::SmoothNoise { x, y, z } => [Some(x), Some(y), Some(z)],
            Self::Wave { at, .. } => [Some(at), None, None],
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

/// What a picture made of a frame.
pub struct Rendered {
    pub frame: Frame,
    /// The mean over every pixel of each node it measures, in order.
    pub means: Vec<f32>,
}

/// Fewer pixels than this are worked out on one thread: starting more costs more than it gives.
const PIXELS_PER_THREAD: usize = 16_384;

/// The frame of `size` pixels the graph of `program` makes, row by row: each node over a whole
/// row at a time, as the machine of a sound runs a span of frames. Rows read nothing of each
/// other, so bands of them run on threads of their own.
pub fn render(program: &Program, inputs: &Inputs<'_>, (width, height): (usize, usize)) -> Rendered {
    let mut pixels = vec![[0.0_f32; 3]; width * height];
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let threads = threads
        .min((width * height / PIXELS_PER_THREAD).max(1))
        .min(height.max(1));
    let rows_each = height.div_ceil(threads).max(1);
    let size = (width, height);
    let sums = match threads {
        1 => render_rows(program, inputs, size, 0, &mut pixels),
        _ => std::thread::scope(|scope| {
            let bands: Vec<_> = (pixels.chunks_mut(rows_each * width).enumerate())
                .map(|(band, pixels)| {
                    scope
                        .spawn(move || render_rows(program, inputs, size, band * rows_each, pixels))
                })
                .collect();
            let mut sums = vec![0.0_f64; program.measures.len()];
            for band in bands {
                // A band that panicked leaves its rows black.
                for (sum, part) in sums.iter_mut().zip(band.join().unwrap_or_default()) {
                    *sum += part;
                }
            }
            sums
        }),
    };
    let count = (width * height).max(1) as f64;
    Rendered {
        frame: Frame {
            width,
            height,
            pixels,
        },
        means: sums.iter().map(|sum| (sum / count) as f32).collect(),
    }
}

/// The rows of `pixels`, from `first` on, and the sum over them of each node it measures.
fn render_rows(
    program: &Program,
    inputs: &Inputs<'_>,
    (width, height): (usize, usize),
    first: usize,
    pixels: &mut [[f32; 3]],
) -> Vec<f64> {
    let nodes = &program.nodes;
    let mut sums = vec![0.0_f64; program.measures.len()];
    let mut registers = vec![0.0_f32; nodes.len() * width];
    for (row, pixels) in (first..).zip(pixels.chunks_mut(width.max(1))) {
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
                    inputs,
                },
            );
        }
        let row_of = |node: &usize| {
            registers
                .get(node * width..(node + 1) * width)
                .unwrap_or(&[])
        };
        for (sum, node) in sums.iter_mut().zip(&program.measures) {
            *sum += row_of(node)
                .iter()
                .map(|value| f64::from(finite(*value)))
                .sum::<f64>();
        }
        for (channel, node) in program.output.iter().enumerate() {
            for (pixel, value) in pixels.iter_mut().zip(row_of(node)) {
                if let Some(colour) = pixel.get_mut(channel) {
                    *colour = finite(*value).clamp(0.0, 1.0);
                }
            }
        }
    }
    sums
}

/// What a picture reads from outside, once a frame.
pub struct Inputs<'a> {
    /// The values of its live values.
    pub lives: &'a [f32],
    /// The lists it reads.
    pub waves: &'a [Vec<f32>],
    /// The frames of the videos it reads, as they are now.
    pub videos: &'a [&'a Frame],
    /// What it made the frame before.
    pub last: &'a Frame,
    /// Counts the frames, for noise that changes on each.
    pub frame: u32,
}

/// What a node reads of where it runs.
#[derive(Clone, Copy)]
struct Place<'a> {
    row: usize,
    y: f32,
    width: usize,
    height: usize,
    inputs: &'a Inputs<'a>,
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
        Node::Live { index } => {
            out.fill(place.inputs.lives.get(index).copied().unwrap_or(0.0));
        }
        Node::Wave { table, at } => {
            let values = place
                .inputs
                .waves
                .get(table.wave)
                .map_or(&[][..], Vec::as_slice);
            for (out, at) in out.iter_mut().zip(read(at)) {
                *out = along(values, *at);
            }
        }
        Node::Video {
            table,
            channel,
            x,
            y,
        } => {
            let frame = place.inputs.videos.get(table.video);
            for ((out, x), y) in out.iter_mut().zip(read(x)).zip(read(y)) {
                *out = frame.map_or(0.0, |frame| frame.at(*x, *y, channel as usize));
            }
        }
        Node::Last { channel, x, y } => {
            for ((out, x), y) in out.iter_mut().zip(read(x)).zip(read(y)) {
                *out = place.inputs.last.at(*x, *y, channel as usize);
            }
        }
        Node::Noise => {
            for (column, out) in out.iter_mut().enumerate() {
                let pixel = (place.row * place.width + column) as u32;
                *out = hashed(pixel, place.inputs.frame);
            }
        }
        Node::SmoothNoise { x, y, z } => {
            for (out, ((x, y), z)) in out.iter_mut().zip(read(x).iter().zip(read(y)).zip(read(z))) {
                *out = smooth_noise(*x, *y, *z);
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

/// `values` at `at`, 0 the first and 1 the last, between two of them; past an end, the end.
fn along(values: &[f32], at: f32) -> f32 {
    let Some(last) = values.len().checked_sub(1) else {
        return 0.0;
    };
    let place = (finite(at) * last as f32).clamp(0.0, last as f32);
    let before = place.floor() as usize;
    let (from, to) = (
        values.get(before).copied().unwrap_or(0.0),
        values.get(before + 1).copied().unwrap_or(0.0),
    );
    match before == last {
        true => from,
        false => from + (to - from) * (place - before as f32),
    }
}

/// Value noise: a random level at each whole `x`, `y`, `z`, and between them a smooth blend of
/// the eight around, so it has no edges.
fn smooth_noise(x: f32, y: f32, z: f32) -> f32 {
    let [x, y, z] = [x, y, z].map(finite);
    let (cell_x, cell_y, cell_z) = (x.floor(), y.floor(), z.floor());
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let (along_x, along_y, along_z) = (ease(x - cell_x), ease(y - cell_y), ease(z - cell_z));
    let corner = |dx: f32, dy: f32, dz: f32| {
        let at = |value: f32| (value as i64) as u32;
        let key = at(cell_x + dx).wrapping_mul(0x8DA6_B343)
            ^ at(cell_y + dy).wrapping_mul(0xD816_3841)
            ^ at(cell_z + dz).wrapping_mul(0xCB1A_B31F);
        hashed(key, 0)
    };
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let face = |dz: f32| {
        let near = mix(corner(0.0, 0.0, dz), corner(1.0, 0.0, dz), along_x);
        let far = mix(corner(0.0, 1.0, dz), corner(1.0, 1.0, dz), along_x);
        mix(near, far, along_y)
    };
    mix(face(0.0), face(1.0), along_z)
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
        Program::new(nodes, (output, vec![0]), None, (lives, 1, 0)).unwrap()
    }

    #[test]
    fn a_picture_runs_its_graph_once_per_pixel() {
        // Red is x, green is y, blue is a live value: a gradient over a level.
        let program = program(serde_json::json!({
            "nodes": [{"op": "x"}, {"op": "y"}, {"op": "live", "index": 0}],
            "lives": 1,
            "output": {"red": 0, "green": 1, "blue": 2},
        }));
        let last = Frame::default();
        let inputs = Inputs {
            lives: &[0.25],
            waves: &[],
            videos: &[],
            last: &last,
            frame: 0,
        };
        let rendered = render(&program, &inputs, (4, 2));
        let frame = rendered.frame;
        assert_eq!(frame.pixels[0], [0.125, 0.25, 0.25]);
        assert_eq!(frame.pixels[7], [0.875, 0.75, 0.25]);
        // It measures node 0, x: its mean over the frame is the middle.
        assert_eq!(rendered.means, [0.5]);
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
            let inputs = Inputs {
                lives: &[],
                waves: &[],
                videos: &[],
                last: &last,
                frame,
            };
            last = render(&program, &inputs, (3, 3)).frame;
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
    fn a_picture_reads_a_list_between_its_values() {
        // Red is the list read across: 0 at the left, 1 halfway, back to 0 at the right.
        let program = program(serde_json::json!({
            "nodes": [{"op": "x"}, {"op": "wave", "table": {"wave": 0}, "at": 0}],
            "lives": 0,
            "output": {"red": 1, "green": 1, "blue": 1},
        }));
        let last = Frame::default();
        let waves = [vec![0.0, 1.0, 0.0]];
        let inputs = Inputs {
            lives: &[],
            waves: &waves,
            videos: &[],
            last: &last,
            frame: 0,
        };
        let frame = render(&program, &inputs, (4, 1)).frame;
        let reds: Vec<f32> = frame.pixels.iter().map(|pixel| pixel[0]).collect();
        assert_eq!(reds, [0.25, 0.75, 0.75, 0.25]);
    }

    #[test]
    fn smooth_noise_moves_little_between_near_places_and_stays_in_range() {
        let steps = (0..2000).map(|step| step as f32 * 0.01);
        let values: Vec<f32> = steps.map(|at| smooth_noise(at, at * 0.7, 3.0)).collect();
        assert!(values.iter().all(|value| (-1.0..=1.0).contains(value)));
        let largest_step = (values.windows(2))
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max);
        assert!(largest_step < 0.1, "{largest_step}");
        let spread = values
            .iter()
            .fold(0.0_f32, |most, value| most.max(value.abs()));
        assert!(spread > 0.5, "{spread}");
    }

    #[test]
    fn a_picture_on_many_threads_is_the_picture_on_one() {
        // Large enough for bands on threads of their own; noise that differs on every row.
        let program = program(serde_json::json!({
            "nodes": [{"op": "x"}, {"op": "y"}, {"op": "noise"}, {"op": "add", "a": 1, "b": 2}],
            "lives": 0,
            "output": {"red": 0, "green": 3, "blue": 2},
        }));
        let last = Frame::default();
        let inputs = Inputs {
            lives: &[],
            waves: &[],
            videos: &[],
            last: &last,
            frame: 7,
        };
        let size = (300, 301);
        let whole = render(&program, &inputs, size);
        let mut pixels = vec![[0.0_f32; 3]; size.0 * size.1];
        let sums = render_rows(&program, &inputs, size, 0, &mut pixels);
        assert_eq!(whole.frame.pixels, pixels);
        let mean = (sums[0] / (size.0 * size.1) as f64) as f32;
        assert!((whole.means[0] - mean).abs() < 1e-6);
    }

    #[test]
    fn a_picture_that_reads_ahead_does_not_load() {
        let nodes = serde_json::from_value(serde_json::json!([{"op": "sin", "x": 0}])).unwrap();
        let output = Rgb {
            red: 0,
            green: 0,
            blue: 0,
        };
        assert!(Program::new(nodes, (output, Vec::new()), None, (0, 0, 0)).is_err());
    }
}
