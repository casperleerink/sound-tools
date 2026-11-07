//! How long a step of a heavy canvas takes on the CPU and on the GPU, at two sizes:
//! `cargo run -p sound-canvas --example timing`.
#![allow(clippy::unwrap_used)]

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    use std::time::Instant;

    use sound_canvas::gpu::Gpu;
    use sound_canvas::{Frame, Inputs, Program, render};

    // 60 sines a pixel, then a trail of the step before.
    let mut nodes = vec![
        serde_json::json!({"op": "x"}),
        serde_json::json!({"op": "y"}),
    ];
    for index in 2..60 {
        nodes.push(serde_json::json!({"op": "sin", "x": index - 1}));
    }
    nodes.push(serde_json::json!({"op": "last", "channel": "red", "x": 1, "y": 0}));
    nodes.push(serde_json::json!({"op": "max", "a": 59, "b": 60}));
    let nodes = serde_json::from_value(serde_json::Value::Array(nodes)).unwrap();
    let output =
        serde_json::from_value(serde_json::json!({"red": 61, "green": 58, "blue": 57})).unwrap();
    let program = Program::new(nodes, (output, Vec::new()), Some(480), (0, 0, 0)).unwrap();
    let mut gpu = Gpu::new(&program).unwrap();
    for size in [(480, 270), (1920, 1080)] {
        let mut last = Frame::default();
        let started = Instant::now();
        for frame in 0..20 {
            let inputs = Inputs {
                lives: &[],
                waves: &[],
                images: &[],
                last: &last,
                frame,
            };
            last = render(&program, &inputs, size).frame;
        }
        let cpu = started.elapsed() / 20;
        let inputs = Inputs {
            lives: &[],
            waves: &[],
            images: &[],
            last: &last,
            frame: 0,
        };
        gpu.render(&inputs, size);
        let started = Instant::now();
        for _ in 0..100 {
            gpu.render(&inputs, size);
        }
        let gpu_time = started.elapsed() / 100;
        println!("{size:?}: CPU {cpu:?} a step, GPU {gpu_time:?} a step");
    }
}
