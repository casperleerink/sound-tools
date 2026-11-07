//! Pictures: a graph of signals the window runs once per pixel on every frame, as `sdk.ts`'s
//! `picture` sends it. What it reads from outside are live values, worked out once a frame.

use std::sync::{Arc, Mutex};

use gpui::{Bounds, Pixels, RenderImage, Window};
use serde::{Deserialize, Deserializer};

use sound_pictures::{Frame, Inputs, MAX_ACROSS, Node, Program, Rendered, Rgb, render};

use crate::tree::{List, Num, Uses};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    nodes: Vec<Node>,
    lives: Vec<Num>,
    #[serde(default)]
    waves: Vec<List>,
    #[serde(default)]
    videos: Vec<VideoSource>,
    output: Rgb,
    /// The number inputs of its tool each frame moves to the mean of a node over the picture.
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

/// A video a picture reads: a file under `assets/video/`, at a second that is a live value.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VideoSource {
    pub asset: String,
    pub at: Num,
    /// It plays round and round, else it holds its first and last frame outside its length.
    pub loops: bool,
}

/// A picture of a shape: its graph, checked, and what it showed last.
#[derive(Clone, Debug)]
pub(crate) struct Picture(Arc<Shown>);

#[derive(Debug)]
struct Shown {
    program: Program,
    lives: Vec<Num>,
    waves: Vec<List>,
    videos: Vec<VideoSource>,
    /// The number inputs it moves, in the order of the means it renders.
    measures: Vec<String>,
    memory: Mutex<Memory>,
}

#[derive(Default)]
struct Memory {
    last: Frame,
    /// The step of its time it showed last.
    stepped: Option<u64>,
    /// What it measured in its last frame, until its card sends it.
    measured: Option<Vec<f32>>,
    frame: u32,
    /// The images painted, newest last: one stays in the atlas a frame longer, as a cached
    /// view may paint the frame before again.
    images: Vec<Arc<RenderImage>>,
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Memory")
    }
}

impl<'de> Deserialize<'de> for Picture {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Graph {
            nodes,
            lives,
            waves,
            videos,
            output,
            measures,
            across,
        } = Graph::deserialize(deserializer)?;
        let counts = (lives.len(), waves.len(), videos.len());
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
            videos,
            measures: names,
            memory: Mutex::default(),
        })))
    }
}

impl Picture {
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

    pub(crate) fn videos(&self) -> &[VideoSource] {
        &self.0.videos
    }

    pub(crate) fn add_uses(&self, uses: &mut Uses) {
        (self.0.videos.iter()).for_each(|video| video.at.add_uses(uses));
        self.0.lives.iter().for_each(|live| live.add_uses(uses));
        self.0.waves.iter().for_each(|list| list.add_uses(uses));
        uses.animates = true;
    }

    /// Paints the picture across `bounds` as it is at `seen.time`: it steps 60 times a second
    /// of that time, however often the window draws, so its feedback moves alike on any screen,
    /// in a screenshot and in a film.
    pub(crate) fn paint(&self, bounds: Bounds<Pixels>, seen: &Seen, window: &mut Window) {
        let Shown {
            program, memory, ..
        } = &*self.0;
        let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        if width < 1.0 || height < 1.0 {
            return;
        }
        let across = program.across() as usize;
        let down =
            ((across as f32 * height / width).round() as usize).clamp(1, MAX_ACROSS as usize);
        let Ok(mut memory) = memory.lock() else {
            return;
        };
        let due = (seen.time * STEPS_PER_SECOND).floor() as u64;
        let resized = (memory.last.width, memory.last.height) != (across, down);
        let steps = match memory.stepped {
            Some(stepped) if !resized => due.saturating_sub(stepped).min(MOST_STEPS),
            _ => 1,
        };
        memory.stepped = Some(due);
        let videos: Vec<&Frame> = seen.videos.iter().map(|frame| &**frame).collect();
        for _ in 0..steps {
            let inputs = Inputs {
                lives: &seen.lives,
                waves: &seen.waves,
                videos: &videos,
                last: &memory.last,
                frame: memory.frame,
            };
            let Rendered { frame, means } = render(program, &inputs, (across, down));
            memory.measured = Some(means);
            memory.last = frame;
            memory.frame = memory.frame.wrapping_add(1);
        }
        let image = match (steps, memory.images.last()) {
            (0, Some(image)) => image.clone(),
            _ => {
                let image = Arc::new(image_of(&memory.last));
                memory.images.push(image.clone());
                image
            }
        };
        if let Err(error) = window.paint_image(bounds, bounds, Default::default(), image, 0, false)
        {
            eprintln!("error: a picture did not paint: {error}");
        }
        while memory.images.len() > 2 {
            let old = memory.images.remove(0);
            if let Err(error) = window.drop_image(old) {
                eprintln!("error: a picture's last frame stayed: {error}");
            }
        }
    }
}

/// How often a picture steps, in its time.
const STEPS_PER_SECOND: f64 = 60.0;
/// The most steps one draw makes up for, after the window was away a while.
const MOST_STEPS: u64 = 4;

/// What a picture reads for a draw: its live values, lists and videos as they are now, and the
/// time.
pub(crate) struct Seen {
    pub lives: Vec<f32>,
    pub waves: Vec<Vec<f32>>,
    pub videos: Vec<Arc<Frame>>,
    /// Seconds since the card opened.
    pub time: f64,
}

/// The frame as GPUI paints an image.
fn image_of(frame: &Frame) -> RenderImage {
    let buffer = image::RgbaImage::from_raw(frame.width as u32, frame.height as u32, frame.bgra())
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    RenderImage::new(vec![image::Frame::new(buffer)])
}
