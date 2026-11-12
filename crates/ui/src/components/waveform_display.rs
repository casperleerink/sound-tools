//! Waveform display: a whole audio file in the display inset. The part outside the start and
//! end lines is shaded, and their handles drag sideways. A part that repeats, from a loop start
//! to the end line, is tinted, with a handle of its own at the top. A green line is where the
//! sound plays. A curve over it, such as gain and fades or an envelope, and its handles follow
//! the rules of every display.
//!
//! It knows no clip and no sampler: the owner gives the overview of the file, where it starts and
//! ends in seconds, and hears the start and end handles.
//!
//! A display that takes a file dropped from the file manager has a [`FileDrop`]: while a file is
//! dragged over it, the ring and a line that says what a drop does.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, ElementId, ExternalPaths, Point, SharedString, Window, div, prelude::*, px};
use sound_media::{Info, Overview};

use crate::components::display::{Axis, Display, Handle, INSET_HEIGHT, WIDE_WIDTH};
use crate::components::gesture::{ChangeHandler, ValueChange};
use crate::components::knob::{KnobRange, seconds_readout};
use crate::scale::radius;
use crate::theme::ActiveTheme;
use crate::typography::{TextVariant, Typography};

/// The handles of the start and end lines sit this far above the bottom of the display.
const TRIM_HANDLE_RISE: f32 = 8.;

#[derive(IntoElement)]
pub struct WaveformDisplay {
    id: ElementId,
    overview: Option<Arc<Overview>>,
    /// How long the file is, in seconds: the width of the display.
    seconds: f32,
    start: f32,
    end: f32,
    on_start: Option<ChangeHandler<f32>>,
    on_end: Option<ChangeHandler<f32>>,
    loop_start: Option<(f32, ChangeHandler<f32>)>,
    playhead: Option<f32>,
    curve: Vec<Point<f32>>,
    handles: Vec<Handle>,
    drop: Option<FileDrop>,
}

impl WaveformDisplay {
    /// A wide display of a file of `seconds`. With no overview yet it shows no waveform, and
    /// the rest as it is.
    pub fn new(id: impl Into<ElementId>, overview: Option<Arc<Overview>>, seconds: f32) -> Self {
        Self {
            id: id.into(),
            overview,
            seconds,
            start: 0.,
            end: seconds,
            on_start: None,
            on_end: None,
            loop_start: None,
            playhead: None,
            curve: Vec::new(),
            handles: Vec::new(),
            drop: None,
        }
    }

    /// Takes a file dropped from the file manager, see [`FileDrop`].
    pub fn drop_file(mut self, drop: FileDrop) -> Self {
        self.drop = Some(drop);
        self
    }

    /// The part of the file that plays, in seconds.
    pub fn trim(mut self, start: f32, end: f32) -> Self {
        self.start = start;
        self.end = end;
        self
    }

    /// Hears the handle of the start line, in seconds.
    pub fn on_start(mut self, f: impl Fn(ValueChange, &mut Window, &mut App) + 'static) -> Self {
        self.on_start = Some(Rc::new(f));
        self
    }

    /// Hears the handle of the end line, in seconds.
    pub fn on_end(mut self, f: impl Fn(ValueChange, &mut Window, &mut App) + 'static) -> Self {
        self.on_end = Some(Rc::new(f));
        self
    }

    /// Where the part that repeats up to the end line starts, in seconds, and what hears its
    /// handle.
    pub fn loop_start(
        mut self,
        seconds: f32,
        f: impl Fn(ValueChange, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.loop_start = Some((seconds, Rc::new(f)));
        self
    }

    /// Where the sound plays now, in seconds of the file: a green line. `None` when it plays
    /// nowhere in it.
    pub fn playhead(mut self, seconds: Option<f32>) -> Self {
        self.playhead = seconds;
        self
    }

    /// The curve over the waveform, as on [`Display::curve`].
    pub fn curve(mut self, points: impl IntoIterator<Item = Point<f32>>) -> Self {
        self.curve = points.into_iter().collect();
        self
    }

    pub fn handle(mut self, handle: Handle) -> Self {
        self.handles.push(handle);
        self
    }

    /// The handle of a line at `value` seconds, `rise` points above the bottom, which moves it
    /// sideways over the whole file. `says` is its name in the line under the display.
    fn line_handle(
        &self,
        (name, says): (&'static str, &'static str),
        (value, default): (f32, f32),
        rise: f32,
        on: ChangeHandler<f32>,
    ) -> Handle {
        let range = KnobRange::linear(0., self.seconds.max(f32::MIN_POSITIVE));
        let x = Axis::new(range, value, default);
        let y = Axis::fixed(rise / INSET_HEIGHT);
        Handle::new(name, x, y)
            .hollow(true)
            .caption(format!("{says} {}", seconds_readout(value)))
            .on_change(move |change: ValueChange<Point<f32>>, window, cx| {
                on(change.map(|at| at.x), window, cx)
            })
    }
}

impl RenderOnce for WaveformDisplay {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // One column per point of the display.
        let columns = WIDE_WIDTH as usize;
        let peaks = self.overview.as_ref().map_or_else(Vec::new, |overview| {
            overview.columns(0., overview.seconds(), columns)
        });
        let (start, end) = (
            place(self.start, self.seconds),
            place(self.end, self.seconds),
        );
        let signal = self.playhead.map(|seconds| place(seconds, self.seconds));
        let mut handles = Vec::new();
        if let Some(on) = self.on_start.clone() {
            let start = ("start", "Start");
            handles.push(self.line_handle(start, (self.start, 0.), TRIM_HANDLE_RISE, on));
        }
        if let Some(on) = self.on_end.clone() {
            let values = (self.end, self.seconds);
            handles.push(self.line_handle(("end", "End"), values, TRIM_HANDLE_RISE, on));
        }
        // At the top, clear of the start handle when the loop starts at the start.
        if let Some((seconds, on)) = self.loop_start.clone() {
            let rise = INSET_HEIGHT - TRIM_HANDLE_RISE;
            let loop_start = ("loop-start", "Loop start");
            handles.push(self.line_handle(loop_start, (seconds, self.start), rise, on));
        }
        let looped = self
            .loop_start
            .as_ref()
            .map(|(seconds, _)| place(*seconds, self.seconds));
        let display = Display::new(self.id)
            .wide()
            .waveform(peaks)
            .kept(start, end)
            .looped(looped)
            .signal_line(signal)
            .curve(self.curve);
        // A drop shown for a gallery hides the handles, as a file dragged over it does.
        let shown = self.drop.as_ref().is_some_and(|drop| drop.shown);
        let display = self
            .handles
            .into_iter()
            .chain(handles)
            .filter(|_| !shown)
            .fold(display, Display::handle);
        match self.drop {
            Some(drop) => display.takes_files(true).overlay(drop),
            None => display,
        }
    }
}

/// Where a display takes a file dropped from the file manager. While a file is dragged over it, it
/// shows the 2 pt lavender ring of a drop target and a line that says what a drop does, such as
/// `Drop to replace the file`, over what the display shows. A drop gives the paths.
///
/// GPUI turns a drag from the file manager into a drag of [`ExternalPaths`], so the ring is a
/// style of that drag and needs no state of its own.
#[derive(IntoElement)]
pub struct FileDrop {
    message: SharedString,
    on_drop: Rc<dyn Fn(&[PathBuf], &mut Window, &mut App)>,
    /// Shown whether a file is dragged over or not, for a gallery.
    shown: bool,
}

impl FileDrop {
    pub fn new(
        message: impl Into<SharedString>,
        on_drop: impl Fn(&[PathBuf], &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            message: message.into(),
            on_drop: Rc::new(on_drop),
            shown: false,
        }
    }

    /// Shows the ring and the line as if a file were dragged over it.
    pub fn shown(mut self, shown: bool) -> Self {
        self.shown = shown;
        self
    }
}

impl RenderOnce for FileDrop {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let on_drop = self.on_drop;
        div()
            .id("file-drop")
            .debug_selector(|| "file-drop".into())
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::MD)
            .border(px(2.))
            .border_color(theme.lavender)
            // Opaque, so the line reads over a waveform.
            .bg(theme.background())
            .text_variant(TextVariant::Label)
            .line_height(px(14.))
            .text_color(theme.text())
            .child(self.message)
            .when(!self.shown, |d| d.opacity(0.))
            .drag_over::<ExternalPaths>(|style, _, _, _| style.opacity(1.))
            .on_drop(move |paths: &ExternalPaths, window, cx| on_drop(paths.paths(), window, cx))
    }
}

/// The shortest part of a file that a start and an end leave to play, in seconds: the start
/// and end lines and their knobs keep this much between them, for a clip and a sampler alike.
pub const SHORTEST_SECONDS: f64 = 0.01;

/// The latest start of a file that plays up to `end`, and never before `earliest`.
pub fn latest_start(earliest: f64, end: f64) -> f64 {
    (end - SHORTEST_SECONDS).max(earliest)
}

/// A start in seconds of the file, from `earliest` to the latest before `end`.
pub fn clamped_start(seconds: f64, earliest: f64, end: f64) -> f64 {
    seconds.clamp(earliest, latest_start(earliest, end))
}

/// An end as a record keeps it: at least [`SHORTEST_SECONDS`] after `start`, and `None`, the
/// end of the file, at the end of the file or within one frame of it.
pub fn clamped_end(seconds: f64, start: f64, file: &Info) -> Option<f64> {
    let seconds = seconds.max(start + SHORTEST_SECONDS);
    let frame = 1.0 / f64::from(file.sample_rate.max(1));
    (seconds < file.seconds() - frame).then_some(seconds)
}

/// Where a time of a file of `file_seconds` is across the display, from 0 to 1, so an owner
/// puts its curve where the waveform is.
pub fn place(seconds: f32, file_seconds: f32) -> f32 {
    match file_seconds > 0. {
        true => (seconds / file_seconds).clamp(0., 1.),
        false => 0.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_and_an_end_keep_the_shortest_part_between_them_and_the_end_of_the_file_is_none() {
        let file = Info {
            frames: 48_000,
            channels: 1,
            sample_rate: 48_000,
            container: sound_media::Container::Wav,
        };
        assert_eq!(clamped_start(0.7, 0., 0.5), 0.49);
        assert_eq!(clamped_start(-1., 0., 0.5), 0.);
        assert_eq!(clamped_start(0.3, 0.4, 0.405), 0.4);
        assert_eq!(clamped_end(0.001, 0., &file), Some(SHORTEST_SECONDS));
        assert_eq!(clamped_end(0.99999, 0., &file), None);
        assert_eq!(clamped_end(2., 0., &file), None);
        assert_eq!(clamped_end(0.8, 0., &file), Some(0.8));
    }

    #[test]
    fn a_time_of_the_file_is_its_part_of_the_width() {
        assert_eq!(place(0., 10.), 0.);
        assert_eq!(place(2.5, 10.), 0.25);
        assert_eq!(place(12., 10.), 1.);
        assert_eq!(place(1., 0.), 0.);
    }
}
