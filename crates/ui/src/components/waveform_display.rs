//! Waveform display: a whole audio file in the display inset. The waveform is `alpha/30`, the
//! part outside the start and end lines is shaded with `gray-50` at 72 %, and the start and end
//! are 1 pt `gray-950` lines with hollow handles 8 pt above the bottom, which drag sideways. A
//! green line is where the sound plays. A curve over it, such as gain and fades or an envelope,
//! and its handles follow the rules of every display.
//!
//! The Clip card of the arrangement and the Sampler share it. It knows no clip and no sampler:
//! the owner gives the overview of the file, where it starts and ends in seconds, and hears the
//! start and end handles. A value a handle moves also has a knob, as on every display.

use std::sync::Arc;

use gpui::{AnyElement, App, ElementId, Point, SharedString, Window, prelude::*};
use sound_media::Overview;

use crate::components::display::{Axis, Display, Handle, INSET_HEIGHT};
use crate::components::gesture::{ChangeHandler, ValueChange};
use crate::components::knob::KnobRange;

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
        }
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
        let display = self
            .handles
            .into_iter()
            .chain(handles)
            .fold(display, Display::handle);
        let display = match self.caption {
            Some(caption) => display.caption(caption),
            None => display,
        };
        display.children(self.children)
    }
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
    fn a_time_of_the_file_is_its_part_of_the_width() {
        assert_eq!(place(0., 10.), 0.);
        assert_eq!(place(2.5, 10.), 0.25);
        assert_eq!(place(12., 10.), 1.);
        assert_eq!(place(1., 0.), 0.);
    }
}
