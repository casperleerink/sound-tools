//! A spike: the graph of a picture as a Metal compute shader, to measure what the GPU gives. The
//! shader is the graph written out as Metal, one line per node, with the same maths as the CPU,
//! so both make the same picture. It writes the colours to memory the CPU reads as it is.

use metal::{
    Buffer, CommandQueue, CompileOptions, ComputePipelineState, Device, MTLResourceOptions, MTLSize,
};

use crate::{Channel, Frame, Node, Program};

/// The Metal of `program`, or why the GPU cannot run it: lists and images stay on the CPU here.
pub fn shader(program: &Program) -> Result<String, String> {
    if !program.measures.is_empty() {
        return Err("a picture that measures runs on the CPU in this spike".into());
    }
    let mut code = String::from(PRELUDE);
    code.push_str(
        "kernel void picture(device float4* out [[buffer(0)]], device const float4* last [[buffer(1)]],\n\
         constant float* lives [[buffer(2)]], constant uint4& size [[buffer(3)]], uint2 at [[thread_position_in_grid]]) {\n\
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
                let channel = match channel {
                    Channel::Red => 0,
                    Channel::Green => 1,
                    Channel::Blue => 2,
                    Channel::Alpha => 3,
                };
                format!(
                    "sample(last, lastWidth, lastHeight, {}, {}, {channel})",
                    r(x),
                    r(y)
                )
            }
            Node::Wave { .. } | Node::Image { .. } => {
                return Err("lists and images are read on the CPU in this spike".into());
            }
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
    code.push_str(&format!(
        "out[at.y * width + at.x] = float4({red}, {green}, {blue}, {alpha});\n}}\n"
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

/// A picture's shader on the GPU, and the memory it writes, which it reads as the last step.
pub struct Gpu {
    device: Device,
    queue: CommandQueue,
    pipeline: ComputePipelineState,
    frames: [Option<(Buffer, (usize, usize))>; 2],
    newest: usize,
    frame: u32,
}

impl Gpu {
    pub fn new(program: &Program) -> Result<Self, String> {
        let source = shader(program)?;
        let device = Device::system_default().ok_or("no GPU")?;
        let options = CompileOptions::new();
        // The same maths as the CPU, to the last bit where it can.
        options.set_fast_math_enabled(false);
        let library = device.new_library_with_source(&source, &options)?;
        let function = library.get_function("picture", None)?;
        let pipeline = device.new_compute_pipeline_state_with_function(&function)?;
        Ok(Self {
            queue: device.new_command_queue(),
            device,
            pipeline,
            frames: [None, None],
            newest: 0,
            frame: 0,
        })
    }

    /// The next step at `size`, with `lives`: its colours, as the GPU left them.
    pub fn render(&mut self, lives: &[f32], (width, height): (usize, usize)) -> &[[f32; 4]] {
        let next = 1 - self.newest;
        let bytes = (width * height * 16).max(16) as u64;
        if self.frames[next]
            .as_ref()
            .is_none_or(|(_, size)| *size != (width, height))
        {
            let buffer = self
                .device
                .new_buffer(bytes, MTLResourceOptions::StorageModeShared);
            self.frames[next] = Some((buffer, (width, height)));
        }
        let commands = self.queue.new_command_buffer();
        let encoder = commands.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&self.pipeline);
        let (Some((out, _)), last) = (&self.frames[next], &self.frames[self.newest]) else {
            return &[];
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
        let lives = if lives.is_empty() { &[0.0][..] } else { lives };
        encoder.set_bytes(
            2,
            std::mem::size_of_val(lives) as u64,
            lives.as_ptr().cast(),
        );
        let size: [u32; 4] = [width as u32, height as u32, self.frame, last_size];
        encoder.set_bytes(3, 16, size.as_ptr().cast());
        let side = 16;
        encoder.dispatch_threads(
            MTLSize::new(width as u64, height as u64, 1),
            MTLSize::new(side, side, 1),
        );
        encoder.end_encoding();
        commands.commit();
        commands.wait_until_completed();
        self.newest = next;
        self.frame = self.frame.wrapping_add(1);
        // SAFETY: the buffer is shared memory as long as the frame, written by the GPU before
        // `wait_until_completed` returned, and not written again until the next step.
        unsafe { std::slice::from_raw_parts(out.contents().cast(), width * height) }
    }
}

/// The CPU's picture for comparing.
pub fn on_cpu(program: &Program, lives: &[f32], size: (usize, usize), steps: u32) -> Frame {
    let mut last = Frame::default();
    for frame in 0..steps {
        let inputs = crate::Inputs {
            lives,
            waves: &[],
            images: &[],
            last: &last,
            frame,
        };
        last = crate::render(program, &inputs, size).frame;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;

    fn program(nodes: serde_json::Value, output: serde_json::Value) -> Program {
        let nodes = serde_json::from_value(nodes).unwrap();
        let output: Rgb = serde_json::from_value(output).unwrap();
        Program::new(nodes, (output, Vec::new()), None, (1, 0, 0)).unwrap()
    }

    #[test]
    fn the_gpu_makes_the_picture_the_cpu_makes() {
        // Rings of a live value, smooth noise, white noise and a trail of the step before.
        let program = program(
            serde_json::json!([
                {"op": "x"}, {"op": "y"}, {"op": "live", "index": 0},
                {"op": "constant", "value": 7.0}, {"op": "multiply", "a": 0, "b": 3},
                {"op": "sin", "x": 4}, {"op": "smoothNoise", "x": 4, "y": 1, "z": 2},
                {"op": "noise"}, {"op": "last", "channel": "red", "x": 1, "y": 0},
                {"op": "constant", "value": 0.9}, {"op": "multiply", "a": 8, "b": 9},
                {"op": "max", "a": 5, "b": 10}, {"op": "remainder", "a": 6, "b": 2},
            ]),
            serde_json::json!({"red": 11, "green": 6, "blue": 7, "alpha": 12}),
        );
        let (size, steps) = ((37, 23), 5);
        let cpu = on_cpu(&program, &[0.3], size, steps);
        let mut gpu = Gpu::new(&program).unwrap();
        let mut last = Vec::new();
        for _ in 0..steps {
            last = gpu.render(&[0.3], size).to_vec();
        }
        let most = (cpu.pixels.iter().flatten())
            .zip(last.iter().flatten())
            .map(|(cpu, gpu)| (cpu - gpu).abs())
            .fold(0.0, f32::max);
        assert!(most < 1e-4, "{most}");
    }
}
