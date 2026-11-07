//! The graph of a canvas as a Metal compute shader. The shader is the graph written out as
//! Metal, one line per node, with the same maths as the CPU, so both make the same canvas. It
//! writes the colours, the bytes the window paints, and each pixel's values to measure, to
//! memory the CPU reads as it is.

use metal::{
    Buffer, CommandQueue, CompileOptions, ComputePipelineState, Device, MTLResourceOptions, MTLSize,
};

#[cfg(test)]
use crate::Frame;
use crate::{Inputs, Node, Program};

/// The Metal of `program`, or why the GPU cannot run it: lists and images stay on the CPU here.
pub fn shader(program: &Program) -> Result<String, String> {
    let mut code = String::from(PRELUDE);
    code.push_str(
        "kernel void canvas(device float4* out [[buffer(0)]], device const float4* last [[buffer(1)]],\n\
         constant float* lives [[buffer(2)]], constant uint4& size [[buffer(3)]],\n\
         device const float4* images [[buffer(4)]], device const uint4* image_at [[buffer(5)]],\n\
         device const float* lists [[buffer(6)]], device const uint2* list_at [[buffer(7)]],\n\
         device float* measured [[buffer(8)]], device uchar4* shown [[buffer(9)]],\n\
         uint2 at [[thread_position_in_grid]]) {\n\
         if (at.x >= size.x || at.y >= size.y) return;\n\
         uint width = size.x, height = size.y, frame = size.z, lastWidth = size.w;\n\
         uint lastHeight = lastWidth == 0 ? 0 : height;\n",
    );
    for (index, node) in program.nodes.iter().enumerate() {
        let r = |node: &usize| format!("r{node}");
        let value = match node {
            Node::Constant { value } => format!("{value:?}"),
            Node::X => "(float(at.x) + 0.5) / float(width)".into(),
            Node::Y => "(float(at.y) + 0.5) / float(height)".into(),
            Node::Aspect => "float(width) / float(height)".into(),
            Node::Live { index } => format!("lives[{index}]"),
            Node::Noise => "hashed(at.y * width + at.x, frame)".into(),
            Node::SmoothNoise { x, y, z } => format!("smooth_noise({}, {}, {})", r(x), r(y), r(z)),
            Node::Last { channel, x, y } => {
                let channel = *channel as u8;
                format!(
                    "sample(last, lastWidth, lastHeight, {}, {}, {channel})",
                    r(x),
                    r(y)
                )
            }
            Node::Wave { table, at } => format!(
                "along(lists + list_at[{0}].x, list_at[{0}].y, {1})",
                table.wave,
                r(at)
            ),
            Node::Image {
                table,
                channel,
                x,
                y,
            } => format!(
                "sample(images + image_at[{0}].x, image_at[{0}].y, image_at[{0}].z, {1}, {2}, {3})",
                table.image,
                r(x),
                r(y),
                *channel as u8
            ),
            Node::Negate { x } => format!("-{}", r(x)),
            Node::Sin { x } => format!("sin({})", r(x)),
            Node::Cos { x } => format!("cos({})", r(x)),
            Node::Tan { x } => format!("tan({})", r(x)),
            Node::Tanh { x } => format!("tanh({})", r(x)),
            Node::Abs { x } => format!("abs({})", r(x)),
            Node::Sqrt { x } => format!("sqrt(max({}, 0.0f))", r(x)),
            Node::Exp { x } => format!("exp({})", r(x)),
            Node::Log { x } => format!("log(max({}, FLT_MIN))", r(x)),
            Node::Floor { x } => format!("floor({})", r(x)),
            Node::Wrap { x } => format!("({0} - floor({0}))", r(x)),
            Node::Add { a, b } => format!("{} + {}", r(a), r(b)),
            Node::Subtract { a, b } => format!("{} - {}", r(a), r(b)),
            Node::Multiply { a, b } => format!("{} * {}", r(a), r(b)),
            Node::Divide { a, b } => format!("({1} == 0.0f ? 0.0f : {0} / {1})", r(a), r(b)),
            Node::Remainder { a, b } => format!("remainder_of({}, {})", r(a), r(b)),
            Node::Less { a, b } => format!("float({} < {})", r(a), r(b)),
            Node::Greater { a, b } => format!("float({} > {})", r(a), r(b)),
            Node::LessOrEqual { a, b } => format!("float({} <= {})", r(a), r(b)),
            Node::GreaterOrEqual { a, b } => format!("float({} >= {})", r(a), r(b)),
            Node::Equal { a, b } => format!("float({} == {})", r(a), r(b)),
            Node::NotEqual { a, b } => format!("float({} != {})", r(a), r(b)),
            Node::Min { a, b } => format!("fmin({}, {})", r(a), r(b)),
            Node::Max { a, b } => format!("fmax({}, {})", r(a), r(b)),
            Node::Pow { a, b } => format!("pow({}, {})", r(a), r(b)),
            Node::Clamp { x, low, high } => {
                format!("fmin(fmax({}, {}), {})", r(x), r(low), r(high))
            }
            Node::Mix { a, b, amount } => {
                format!("({0} + ({1} - {0}) * {2})", r(a), r(b), r(amount))
            }
        };
        code.push_str(&format!("float r{index} = {value};\n"));
    }
    let channel = |node: &Option<usize>| {
        node.map_or("1.0f".to_string(), |node| {
            format!("clamp(finite(r{node}), 0.0f, 1.0f)")
        })
    };
    let [red, green, blue, alpha] = program.output.each_ref().map(channel);
    let count = program.measures.len();
    for (index, node) in program.measures.iter().enumerate() {
        code.push_str(&format!(
            "measured[(at.y * width + at.x) * {count} + {index}] = finite(r{node});\n"
        ));
    }
    code.push_str(&format!(
        "float4 colour = float4({red}, {green}, {blue}, {alpha});\n\
         out[at.y * width + at.x] = colour;\n\
         shown[at.y * width + at.x] = uchar4(round(colour.zyxw * 255.0f));\n}}\n"
    ));
    Ok(code)
}

const PRELUDE: &str = r#"
#include <metal_stdlib>
using namespace metal;

float finite(float value) { return isfinite(value) ? value : 0.0f; }

float hashed(uint pixel, uint frame) {
    uint value = (pixel * 0x9E3779B9u) ^ (frame * 0x85EBCA6Bu);
    value ^= value >> 16; value *= 0x7FEB352Du;
    value ^= value >> 15; value *= 0x846CA68Bu;
    value ^= value >> 16;
    return (float(value) / 4294967296.0f) * 2.0f - 1.0f;
}

float smooth_noise(float x, float y, float z) {
    x = finite(x); y = finite(y); z = finite(z);
    float cx = floor(x), cy = floor(y), cz = floor(z);
    float ax = x - cx, ay = y - cy, az = z - cz;
    ax = ax * ax * (3.0f - 2.0f * ax); ay = ay * ay * (3.0f - 2.0f * ay); az = az * az * (3.0f - 2.0f * az);
    float corners[8];
    for (int index = 0; index < 8; index++) {
        float dx = float(index & 1), dy = float((index >> 1) & 1), dz = float((index >> 2) & 1);
        uint key = (uint(int(cx + dx)) * 0x8DA6B343u) ^ (uint(int(cy + dy)) * 0xD8163841u) ^ (uint(int(cz + dz)) * 0xCB1AB31Fu);
        corners[index] = hashed(key, 0);
    }
    float near0 = mix(corners[0], corners[1], ax), far0 = mix(corners[2], corners[3], ax);
    float near1 = mix(corners[4], corners[5], ax), far1 = mix(corners[6], corners[7], ax);
    return mix(mix(near0, far0, ay), mix(near1, far1, ay), az);
}

float along(device const float* values, uint length, float at) {
    if (length == 0) return 0.0f;
    float last = float(length - 1);
    float place = clamp(finite(at) * last, 0.0f, last);
    uint before = uint(floor(place));
    if (before + 1 >= length) return values[before];
    return values[before] + (values[before + 1] - values[before]) * (place - float(before));
}

float remainder_of(float a, float b) {
    if (b == 0.0f) return 0.0f;
    float magnitude = abs(b);
    float rest = fmod(a, magnitude);
    return rest < 0.0f ? rest + magnitude : rest;
}

float sample(device const float4* frame, uint width, uint height, float x, float y, int channel) {
    if (width == 0 || height == 0) return 0.0f;
    x = clamp(finite(x) * float(width) - 0.5f, 0.0f, float(width - 1));
    y = clamp(finite(y) * float(height) - 0.5f, 0.0f, float(height - 1));
    uint left = uint(floor(x)), top = uint(floor(y));
    uint right = min(left + 1, width - 1), bottom = min(top + 1, height - 1);
    float across = x - float(left), down = y - float(top);
    float a = frame[top * width + left][channel], b = frame[top * width + right][channel];
    float c = frame[bottom * width + left][channel], d = frame[bottom * width + right][channel];
    float upper = a + (b - a) * across, lower = c + (d - c) * across;
    return upper + (lower - upper) * down;
}
"#;

/// A canvas's shader on the GPU, and the memory it writes, which it reads as the last step.
pub struct Gpu {
    device: Device,
    queue: CommandQueue,
    pipeline: ComputePipelineState,
    frames: [Option<(Buffer, (usize, usize))>; 2],
    newest: usize,
    /// The newest step as the window paints it.
    shown: Option<Buffer>,
    /// Each pixel's values of the nodes it measures, and how many there are.
    measured: Option<Buffer>,
    measures: usize,
}

impl Gpu {
    pub fn new(program: &Program) -> Result<Self, String> {
        let source = shader(program)?;
        let device = Device::system_default().ok_or("no GPU")?;
        let options = CompileOptions::new();
        // The same maths as the CPU, to the last bit where it can.
        options.set_fast_math_enabled(false);
        let library = device.new_library_with_source(&source, &options)?;
        let function = library.get_function("canvas", None)?;
        let pipeline = device.new_compute_pipeline_state_with_function(&function)?;
        Ok(Self {
            queue: device.new_command_queue(),
            device,
            pipeline,
            frames: [None, None],
            newest: 0,
            shown: None,
            measured: None,
            measures: program.measures.len(),
        })
    }

    /// The next step at `size`, reading what `inputs` hold besides the last step, which it
    /// keeps itself: the mean of each node it measures.
    pub fn render(&mut self, inputs: &Inputs<'_>, (width, height): (usize, usize)) -> Vec<f32> {
        let next = 1 - self.newest;
        let pixels = width * height;
        let bytes = (pixels * 16).max(16) as u64;
        if self.frames[next]
            .as_ref()
            .is_none_or(|(_, size)| *size != (width, height))
        {
            let buffer = self
                .device
                .new_buffer(bytes, MTLResourceOptions::StorageModeShared);
            self.frames[next] = Some((buffer, (width, height)));
        }
        if self
            .shown
            .as_ref()
            .is_none_or(|buffer| buffer.length() != (pixels * 4).max(16) as u64)
        {
            self.shown = Some(self.device.new_buffer(
                (pixels * 4).max(16) as u64,
                MTLResourceOptions::StorageModeShared,
            ));
        }
        let measures = self.measures;
        let measured_bytes = (pixels * measures * 4).max(16) as u64;
        if self
            .measured
            .as_ref()
            .is_none_or(|buffer| buffer.length() != measured_bytes)
        {
            self.measured = Some(
                self.device
                    .new_buffer(measured_bytes, MTLResourceOptions::StorageModeShared),
            );
        }
        // What it reads besides itself, laid end to end, with where each starts and how long it is.
        let mut images: Vec<[f32; 4]> = Vec::new();
        let mut image_at: Vec<[u32; 4]> = Vec::new();
        for frame in inputs.images {
            image_at.push([
                images.len() as u32,
                frame.width as u32,
                frame.height as u32,
                0,
            ]);
            images.extend_from_slice(&frame.pixels);
        }
        let mut lists: Vec<f32> = Vec::new();
        let mut list_at: Vec<[u32; 2]> = Vec::new();
        for list in inputs.waves {
            list_at.push([lists.len() as u32, list.len() as u32]);
            lists.extend_from_slice(list);
        }
        let shared = |data: &[u8]| {
            let padded = if data.is_empty() { &[0; 16][..] } else { data };
            self.device.new_buffer_with_data(
                padded.as_ptr().cast(),
                padded.len() as u64,
                MTLResourceOptions::StorageModeShared,
            )
        };
        let images = shared(bytes_of(&images));
        let image_at = shared(bytes_of(&image_at));
        let lists = shared(bytes_of(&lists));
        let list_at = shared(bytes_of(&list_at));
        let commands = self.queue.new_command_buffer();
        let encoder = commands.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&self.pipeline);
        let (Some((out, _)), last, Some(measured), Some(shown)) = (
            &self.frames[next],
            &self.frames[self.newest],
            &self.measured,
            &self.shown,
        ) else {
            return Vec::new();
        };
        encoder.set_buffer(0, Some(out), 0);
        let last_size = match last {
            Some((buffer, size)) if *size == (width, height) => {
                encoder.set_buffer(1, Some(buffer), 0);
                width as u32
            }
            _ => {
                encoder.set_buffer(1, Some(out), 0);
                0
            }
        };
        let lives = if inputs.lives.is_empty() {
            &[0.0][..]
        } else {
            inputs.lives
        };
        encoder.set_bytes(
            2,
            std::mem::size_of_val(lives) as u64,
            lives.as_ptr().cast(),
        );
        let size: [u32; 4] = [width as u32, height as u32, inputs.frame, last_size];
        encoder.set_bytes(3, 16, size.as_ptr().cast());
        for (index, buffer) in [
            (4, &images),
            (5, &image_at),
            (6, &lists),
            (7, &list_at),
            (8, measured),
            (9, shown),
        ] {
            encoder.set_buffer(index, Some(buffer), 0);
        }
        encoder.dispatch_threads(
            MTLSize::new(width as u64, height as u64, 1),
            MTLSize::new(16, 16, 1),
        );
        encoder.end_encoding();
        commands.commit();
        commands.wait_until_completed();
        self.newest = next;
        // SAFETY: the buffer is shared memory as long as it says, written by the GPU before
        // `wait_until_completed` returned, and not written again until the next step.
        let values = unsafe {
            std::slice::from_raw_parts(measured.contents().cast::<f32>(), pixels * measures)
        };
        let mut sums = vec![0.0_f64; measures];
        for pixel in values.chunks_exact(measures.max(1)) {
            for (sum, value) in sums.iter_mut().zip(pixel) {
                *sum += f64::from(*value);
            }
        }
        sums.iter()
            .map(|sum| (sum / pixels.max(1) as f64) as f32)
            .collect()
    }

    /// The colours of the newest step, as another canvas reads them.
    pub fn colours(&self) -> &[[f32; 4]] {
        match &self.frames[self.newest] {
            // SAFETY: as in `render`, the newest step's buffer holds a colour for each pixel.
            Some((buffer, (width, height))) => unsafe {
                std::slice::from_raw_parts(buffer.contents().cast(), width * height)
            },
            None => &[],
        }
    }

    /// The newest step as the window paints it: blue, green, red and alpha, a byte each.
    pub fn shown(&self) -> &[u8] {
        match (&self.frames[self.newest], &self.shown) {
            // SAFETY: as in `render`, written with the newest step, four bytes a pixel.
            (Some((_, (width, height))), Some(shown)) => unsafe {
                std::slice::from_raw_parts(shown.contents().cast(), width * height * 4)
            },
            _ => &[],
        }
    }
}

/// The bytes of plain numbers, as the GPU reads them.
fn bytes_of<T: Copy>(values: &[T]) -> &[u8] {
    // SAFETY: plain numbers have no padding, and the bytes live as long as the values.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;

    fn program(
        nodes: serde_json::Value,
        output: serde_json::Value,
        measures: Vec<usize>,
    ) -> Program {
        let nodes = serde_json::from_value(nodes).unwrap();
        let output: Rgb = serde_json::from_value(output).unwrap();
        Program::new(nodes, (output, measures), None, (1, 1, 1)).unwrap()
    }

    #[test]
    fn the_gpu_makes_the_canvas_and_the_measures_the_cpu_makes() {
        // Rings of a live value, smooth and white noise, a trail of the step before, a list read
        // across and a photo read upside down; it measures the rings.
        let program = program(
            serde_json::json!([
                {"op": "x"}, {"op": "y"}, {"op": "live", "index": 0},
                {"op": "constant", "value": 7.0}, {"op": "multiply", "a": 0, "b": 3},
                {"op": "sin", "x": 4}, {"op": "smoothNoise", "x": 4, "y": 1, "z": 2},
                {"op": "noise"}, {"op": "last", "channel": "red", "x": 1, "y": 0},
                {"op": "constant", "value": 0.9}, {"op": "multiply", "a": 8, "b": 9},
                {"op": "max", "a": 5, "b": 10}, {"op": "remainder", "a": 6, "b": 2},
                {"op": "wave", "table": {"wave": 0}, "at": 0},
                {"op": "image", "table": {"image": 0}, "channel": "green", "x": 0, "y": 1},
                {"op": "add", "a": 13, "b": 14},
            ]),
            serde_json::json!({"red": 11, "green": 6, "blue": 15, "alpha": 12}),
            vec![5, 13],
        );
        let waves = [vec![0.0, 1.0, 0.25, 0.5]];
        let photo = Frame {
            width: 3,
            height: 2,
            pixels: (0..6)
                .map(|index| [0.0, index as f32 / 6.0, 0.0, 1.0])
                .collect(),
        };
        let (size, steps) = ((37, 23), 5);
        let mut gpu = Gpu::new(&program).unwrap();
        let mut cpu = crate::Rendered {
            frame: Frame::default(),
            means: Vec::new(),
        };
        let (mut colours, mut means) = (Vec::new(), Vec::new());
        for frame in 0..steps {
            let inputs = Inputs {
                lives: &[0.3],
                waves: &waves,
                images: &[&photo],
                last: &cpu.frame,
                frame,
            };
            means = gpu.render(&inputs, size);
            colours = gpu.colours().to_vec();
            cpu = crate::render(&program, &inputs, size);
        }
        let shown = gpu.shown();
        assert_eq!(shown.len(), cpu.frame.bgra().len());
        assert!((shown.iter().zip(cpu.frame.bgra())).all(|(gpu, cpu)| gpu.abs_diff(cpu) <= 1));
        let most = (cpu.frame.pixels.iter().flatten())
            .zip(colours.iter().flatten())
            .map(|(cpu, gpu)| (cpu - gpu).abs())
            .fold(0.0, f32::max);
        assert!(most < 1e-4, "{most}");
        for (cpu, gpu) in cpu.means.iter().zip(&means) {
            assert!((cpu - gpu).abs() < 1e-4, "{cpu} {gpu}");
        }
        assert_eq!(means.len(), 2);
    }
}
