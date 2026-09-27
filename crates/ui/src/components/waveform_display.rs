//! Waveform display: a whole audio file in the display inset. The waveform is `alpha/30`, the
//! part outside the start and end lines is shaded with `gray-50` at 72 %, and the start and end
//! are 1 pt `gray-950` lines with hollow handles 8 pt above the bottom, which drag sideways. A
//! green line is where the sound plays. A curve over it, such as gain and fades or an envelope,
//! and its handles follow the rules of every display.
//!
//! The Clip card of the arrangement and the Sampler share it. It knows no clip and no sampler:
//! the owner gives the overview of the file, where it starts and ends in seconds, and hears the
//! start and end handles. A value a handle moves also has a knob, as on every display.
//!
//! A display that takes a file dropped from the Finder has a [`FileDrop`]: while a file is
//! dragged over it, the 2 pt lavender ring and a line that says what a drop does. [`NoFile`] is
//! the display before there is a file: a line and a button in its middle.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, ExternalPaths, Point, SharedString, Window, div, prelude::*, px,
};
use sound_media::{Info, Overview};

use crate::components::display::{Axis, Display, Handle, INSET_HEIGHT};
use crate::components::gesture::{ChangeHandler, ValueChange};
use crate::components::knob::KnobRange;
use crate::theme::ActiveTheme;

/// The handles of the start and end lines sit this far above the bottom of the display.
pub const TRIM_HANDLE_RISE: f32 = 8.;

#[derive(IntoElement)]
pub struct WaveformDisplay {
    id: ElementId,
    width: f32,
    overview: Option<Arc<Overview>>,
    /// How long the file is, in seconds: the width of the display.
    seconds: f32,
    start: f32,
    end: f32,
    on_start: Option<ChangeHandler<f32>>,
    on_end: Option<ChangeHandler<f32>>,
    playhead: Option<f32>,
    curve: Vec<Point<f32>>,
    handles: Vec<Handle>,
    caption: Option<SharedString>,
    children: Vec<AnyElement>,
    drop: Option<FileDrop>,
}

impl WaveformDisplay {
    /// A display of a file of `seconds`, `width` points wide. With no overview yet it shows no
    /// waveform, and the rest as it is.
    pub fn new(
        id: impl Into<ElementId>,
        width: f32,
        overview: Option<Arc<Overview>>,
        seconds: f32,
    ) -> Self {
        Self {
            id: id.into(),
            width,
            overview,
            seconds,
            start: 0.,
            end: seconds,
            on_start: None,
            on_end: None,
            playhead: None,
            curve: Vec::new(),
            handles: Vec::new(),
            caption: None,
            children: Vec::new(),
            drop: None,
        }
    }

    /// Takes a file dropped from the Finder, see [`FileDrop`].
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
        self.on_start = Some(std::rc::Rc::new(f));
        self
    }

    /// Hears the handle of the end line, in seconds.
    pub fn on_end(mut self, f: impl Fn(ValueChange, &mut Window, &mut App) + 'static) -> Self {
        self.on_end = Some(std::rc::Rc::new(f));
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

    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// The handle of a line at `value` seconds, which moves it sideways over the whole file.
    fn trim_handle(
        &self,
        name: &'static str,
        value: f32,
        default: f32,
        on: ChangeHandler<f32>,
    ) -> Handle {
        let range = KnobRange::linear(0., self.seconds.max(f32::MIN_POSITIVE));
        let x = Axis::new(range, value, default);
        let y = Axis::fixed(TRIM_HANDLE_RISE / INSET_HEIGHT);
        Handle::new(name, x, y).hollow(true).on_change(
            move |change: ValueChange<Point<f32>>, window, cx| {
                on(change.map(|at| at.x), window, cx)
            },
        )
    }
}

/// Controls at the top of the display, as on [`Display`].
impl ParentElement for WaveformDisplay {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for WaveformDisplay {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // One column per point of the display.
        let columns = self.width.max(1.) as usize;
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
            handles.push(self.trim_handle("start", self.start, 0., on));
        }
        if let Some(on) = self.on_end.clone() {
            handles.push(self.trim_handle("end", self.end, self.seconds, on));
        }
        let display = Display::new(self.id, self.width)
            .waveform(peaks)
            .kept(start, end)
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
        let display = match self.caption {
            Some(caption) => display.caption(caption),
            None => display,
        };
        let display = match self.drop {
            Some(drop) => display.takes_files(true).overlay(drop),
            None => display,
        };
        display.children(self.children)
    }
}

/// Where a display takes a file dropped from the Finder. While a file is dragged over it, it
/// shows the 2 pt lavender ring of a drop target and a line that says what a drop does, such as
/// `Drop to replace the file`, over what the display shows. A drop gives the paths.
///
/// GPUI turns a drag from the Finder into a drag of [`ExternalPaths`], so the ring is a style of
/// that drag and needs no state of its own.
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
            .rounded(px(6.))
            .border(px(2.))
            .border_color(theme.lavender)
            // Opaque, so the line reads over a waveform.
            .bg(theme.gray_100)
            .text_size(px(12.))
            .line_height(px(14.))
            .text_color(theme.gray_950)
            .child(self.message)
            .when(!self.shown, |d| d.opacity(0.))
            .drag_over::<ExternalPaths>(|style, _, _, _| style.opacity(1.))
            .on_drop(move |paths: &ExternalPaths, window, cx| on_drop(paths.paths(), window, cx))
    }
}

/// A display with no file yet: a line and a button in its middle, such as `Drop an audio file
/// here` over `Choose file`. The button is the way from the keys.
#[derive(IntoElement)]
pub struct NoFile {
    id: ElementId,
    width: f32,
    message: SharedString,
    button: Option<AnyElement>,
    drop: Option<FileDrop>,
}

impl NoFile {
    pub fn new(id: impl Into<ElementId>, width: f32, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            width,
            message: message.into(),
            button: None,
            drop: None,
        }
    }

    pub fn button(mut self, button: impl IntoElement) -> Self {
        self.button = Some(button.into_any_element());
        self
    }

    /// Takes a file dropped from the Finder, see [`FileDrop`].
    pub fn drop_file(mut self, drop: FileDrop) -> Self {
        self.drop = Some(drop);
        self
    }
}

impl RenderOnce for NoFile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let middle = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .text_size(px(12.))
            .line_height(px(14.))
            .text_color(cx.theme().gray_800)
            .child(self.message)
            .children(self.button);
        let display = Display::new(self.id, self.width).overlay(middle);
        match self.drop {
            Some(drop) => display.overlay(drop),
            None => display,
        }
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
