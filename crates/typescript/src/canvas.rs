//! Canvases: a graph of signals the window runs once per pixel on every frame, as `sdk.ts`'s
//! `canvas` sends it. What it reads from outside are live values, worked out once a frame.

use std::sync::{Arc, Mutex};

use gpui::{Bounds, Pixels, RenderImage, Window, WindowId};
use serde::{Deserialize, Deserializer};

use sound_canvas::gpu::Gpu;
use sound_canvas::{
    Frame, Inputs, MAX_ACROSS, MAX_ACROSS_ON_GPU, Node, Program, Rendered, Rgb, render,
};

use crate::tree::{List, Num, Uses};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    nodes: Vec<Node>,
    lives: Vec<Num>,
    #[serde(default)]
    waves: Vec<List>,
    #[serde(default)]
    images: Vec<ImageSource>,
    output: Rgb,
    /// The number inputs of its tool each frame moves to the mean of a node over the canvas.
    #[serde(default)]
    measures: Vec<Measure>,
    across: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Measure {
    name: String,
    node: usize,
}

/// An image a canvas reads: a video, a photo, or the last step of another canvas by its key.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum ImageSource {
    Video(VideoSource),
    Canvas(String),
    /// A photo under `assets/images/`.
    Photo(String),
}

/// A video a canvas reads: a file under `assets/video/`, at a second that is a live value.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VideoSource {
    pub asset: String,
    pub at: Num,
    /// It plays round and round, else it holds its first and last frame outside its length.
    pub loops: bool,
}

/// A canvas of a shape: its graph, checked, and what it showed last.
#[derive(Clone, Debug)]
pub(crate) struct Canvas(Arc<Shown>);

#[derive(Debug)]
struct Shown {
    program: Program,
    lives: Vec<Num>,
    waves: Vec<List>,
    images: Vec<ImageSource>,
    /// The number inputs it moves, in the order of the means it renders.
    measures: Vec<String>,
    memory: Mutex<Memory>,
}

#[derive(Default)]
struct Memory {
    last: Arc<Frame>,
    /// Where it runs on the GPU, once it was tried: not off macOS.
    gpu: Option<Option<Gpu>>,
    /// The step of its time it showed last.
    stepped: Option<u64>,
    /// What it measured in its last frame, until its card sends it.
    measured: Option<Vec<f32>>,
    frame: u32,
    /// The images painted, newest last: one stays in the atlas a frame longer, as a cached
    /// view may paint the frame before again.
    images: Vec<(WindowId, Arc<RenderImage>)>,
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Memory")
    }
}

impl<'de> Deserialize<'de> for Canvas {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Graph {
            nodes,
            lives,
            waves,
            images,
            output,
            measures,
            across,
        } = Graph::deserialize(deserializer)?;
        let counts = (lives.len(), waves.len(), images.len());
        let (names, measured) = measures
            .into_iter()
            .map(|each| (each.name, each.node))
            .unzip();
        let program = Program::new(nodes, (output, measured), across, counts)
            .map_err(serde::de::Error::custom)?;
        Ok(Self(Arc::new(Shown {
            program,
            lives,
            waves,
            images,
            measures: names,
            memory: Mutex::default(),
        })))
    }
}

impl Canvas {
    pub(crate) fn lives(&self) -> &[Num] {
        &self.0.lives
    }

    pub(crate) fn waves(&self) -> &[List] {
        &self.0.waves
    }

    /// The number inputs to move and where, measured since this was last asked.
    pub(crate) fn take_measured(&self) -> Vec<(&str, f32)> {
        let measured = (self.0.memory.lock())
            .ok()
            .and_then(|mut memory| memory.measured.take());
        let names = self.0.measures.iter().map(String::as_str);
        names.zip(measured.unwrap_or_default()).collect()
    }

    pub(crate) fn images(&self) -> &[ImageSource] {
        &self.0.images
    }

    /// What it showed at its last step, for another canvas that reads it.
    pub(crate) fn last(&self) -> Arc<Frame> {
        (self.0.memory.lock()).map_or_else(|_| Arc::default(), |memory| memory.last.clone())
    }

    pub(crate) fn add_uses(&self, uses: &mut Uses) {
        for image in &self.0.images {
            if let ImageSource::Video(video) = image {
                video.at.add_uses(uses);
            }
        }
        self.0.lives.iter().for_each(|live| live.add_uses(uses));
        self.0.waves.iter().for_each(|list| list.add_uses(uses));
        uses.animates = true;
    }

    /// Paints the canvas across `bounds` as it is at `seen.time`: it steps 60 times a second
    /// of that time, however often the window draws, so its feedback moves alike on any screen,
    /// and in a screenshot.
    pub(crate) fn paint(&self, bounds: Bounds<Pixels>, seen: &Seen, window: &mut Window) {
        let Shown {
            program, memory, ..
        } = &*self.0;
        let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        if width < 1.0 || height < 1.0 {
            return;
        }
        let Ok(mut memory) = memory.lock() else {
            return;
        };
        if memory.gpu.is_none() {
            memory.gpu = Some(Gpu::new(program).ok());
        }
        let on_gpu = matches!(memory.gpu, Some(Some(_)));
        let most = if on_gpu {
            MAX_ACROSS_ON_GPU
        } else {
            MAX_ACROSS
        };
        let across = program.across().min(most) as usize;
        let down = ((across as f32 * height / width).round() as usize).clamp(1, most as usize);
        let due = (seen.time * STEPS_PER_SECOND).floor() as u64;
        let resized = (memory.last.width, memory.last.height) != (across, down);
        let steps = match memory.stepped {
            Some(stepped) if !resized => due.saturating_sub(stepped).min(MOST_STEPS),
            _ => 1,
        };
        memory.stepped = Some(due);
        let images: Vec<&Frame> = seen.images.iter().map(|frame| &**frame).collect();
        let first = memory.frame;
        if let Some(Some(gpu)) = &mut memory.gpu {
            let mut pixels: &[[f32; 4]] = &[];
            let mut means = Vec::new();
            for frame in 0..steps {
                let inputs = Inputs {
                    lives: &seen.lives,
                    waves: &seen.waves,
                    images: &images,
                    last: &Frame::default(),
                    frame: first.wrapping_add(frame as u32),
                };
                (pixels, means) = gpu.render(&inputs, (across, down));
            }
            if steps > 0 {
                let pixels = pixels.to_vec();
                memory.last = Arc::new(Frame {
                    width: across,
                    height: down,
                    pixels,
                });
                memory.measured = Some(means);
                memory.frame = memory.frame.wrapping_add(steps as u32);
            }
        }
        for _ in 0..if on_gpu { 0 } else { steps } {
            let inputs = Inputs {
                lives: &seen.lives,
                waves: &seen.waves,
                images: &images,
                last: &memory.last,
                frame: memory.frame,
            };
            let Rendered { frame, means } = render(program, &inputs, (across, down));
            memory.measured = Some(means);
            memory.last = Arc::new(frame);
            memory.frame = memory.frame.wrapping_add(1);
        }
        let here = window.window_handle().window_id();
        let image = match (steps, memory.images.last()) {
            (0, Some((painted_in, image))) if *painted_in == here => image.clone(),
            _ => {
                let image = Arc::new(image_of(&memory.last));
                memory.images.push((here, image.clone()));
                image
            }
        };
        if let Err(error) = window.paint_image(bounds, bounds, Default::default(), image, 0, false)
        {
            eprintln!("error: a canvas did not paint: {error}");
        }
        let over = memory.images.len().saturating_sub(2);
        let old: Vec<_> = memory.images.drain(..over).collect();
        drop(memory);
        let mut orphans = ORPHANS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        orphans.extend(old);
        // Each window frees what was painted in it.
        for (painted_in, image) in std::mem::take(&mut *orphans) {
            if painted_in != here {
                orphans.push((painted_in, image));
            } else if let Err(error) = window.drop_image(image) {
                eprintln!("error: a canvas's last frame stayed: {error}");
            }
        }
    }
}

/// Images of canvases no longer shown, by the window they were painted in, which frees them at
/// its next paint of a canvas: the atlas of a window is freed only in that window.
static ORPHANS: Mutex<Vec<(WindowId, Arc<RenderImage>)>> = Mutex::new(Vec::new());

impl Drop for Shown {
    fn drop(&mut self) {
        let images = match self.memory.get_mut() {
            Ok(memory) => std::mem::take(&mut memory.images),
            Err(_) => return,
        };
        (ORPHANS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
        .extend(images);
    }
}

/// How often a canvas steps, in its time.
const STEPS_PER_SECOND: f64 = 60.0;
/// The most steps one draw makes up for, after the window was away a while.
const MOST_STEPS: u64 = 4;

/// What a canvas reads for a draw: its live values, lists and images as they are now, and the
/// time.
pub(crate) struct Seen {
    pub lives: Vec<f32>,
    pub waves: Vec<Vec<f32>>,
    pub images: Vec<Arc<Frame>>,
    /// Seconds since the card opened.
    pub time: f64,
}

/// The frame as GPUI paints an image.
fn image_of(frame: &Frame) -> RenderImage {
    let buffer = image::RgbaImage::from_raw(frame.width as u32, frame.height as u32, frame.bgra())
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    RenderImage::new(vec![image::Frame::new(buffer)])
}
