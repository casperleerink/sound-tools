//! Display: the inset at the left of a device card that shows what the device does to the
//! sound. A curve is drawn over a fill, or over bands of colour by height for a level
//! ([`Display::fill_bands`]). A control such as a segmented control sits in a strip above the
//! drawing ([`Display::control`]), so it lies over none of it.
//!
//! A display explains itself by what it draws: the marks of its scale stand faintly at their
//! lines ([`Display::scale`]). Words come only while there is something to say, as one line
//! inside it at its foot: what moves ([`Display::caption`]), and what a handle moves while that
//! is pointed at or dragged ([`Handle::caption`]). The line never lies over a handle or the
//! curve: where the middle of the foot is taken it stands at a side, or at the top.
//!
//! What is bright is what the composer drags: the curve and its handles. What the sound does,
//! a level, a history, a reduction, is one soft fill under at most one line
//! ([`Display::columns`]).
//!
//! A display is as tall as the two rows of cells beside it, and [`WIDTH`] wide, or
//! [`WIDE_WIDTH`] for one that needs the room across: a file, the bands of an EQ. Both are whole
//! columns of cells, so the cells of every card of a rack stand in the same columns.
//!
//! The display is not only a picture: its handles drag. A [`Handle`] moves one or two values,
//! sideways and up and down, each on a [`KnobRange`] across the width or the height of the
//! display, with the gesture of the knob, see [`gesture`](super::gesture). A handle is not a tab
//! stop: every value a handle moves also has a knob or a hidden control, which is the path for
//! the keys.
//!
//! A handle may carry a number, such as the band of an EQ: its dot is then larger, with the
//! number in it, and [`Handle::on_press`] hears every press on it, so that a click selects it.
//! A handle may also be the whole display, with no dot ([`Handle::area`]): a drag anywhere on
//! it moves the value, as the position of a wavetable moves. A handle that moves a number an automation lane moves shows where the
//! lane is and does not drag; the knob of that number carries the mark and says why.
//!
//! The display knows no device. The owner gives the curve as points on the display and the
//! handles with their values, and hears what a handle moves.
//!
//! A display can also show a waveform under its curve, with the part outside two lines shaded
//! and a green line where the sound plays, see [`waveform_display`](super::waveform_display).
//!
//! A device with nothing to draw has no display: a [`ControlStrip`] takes its place.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, ContentMask, CursorStyle, DispatchPhase, Div, ElementId, Entity,
    ExternalPaths, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseUpEvent, PathBuilder,
    Pixels, Point, SharedString, Window, canvas, div, fill, point, prelude::*, px, size,
};

use crate::components::cell::{CELL_WIDTH, CONTROL_HEIGHT, ROW_HEIGHT};
use crate::components::gesture::{self, ChangeHandler, GestureState, Travel, ValueChange};
use crate::components::knob::KnobRange;
use crate::components::paint;
use crate::components::waveform_display::FileDrop;
use crate::scale::radius;
use crate::theme::ActiveTheme;
use crate::typography;
use crate::typography::{TextVariant, Typography};

/// Between a display and the first column of cells after it.
pub const GAP: f32 = 8.;
/// Four columns of cells with the gap, and six.
pub const WIDTH: f32 = 4. * CELL_WIDTH - GAP;
pub const WIDE_WIDTH: f32 = 6. * CELL_WIDTH - GAP;
pub const INSET_HEIGHT: f32 = 2. * ROW_HEIGHT;
/// The strip of the controls above the drawing: as tall as the room of a knob, so a control in
/// it is on the line of the knobs of the first row.
pub const STRIP_HEIGHT: f32 = CONTROL_HEIGHT;
/// The line at the foot of the inset: from its edge, and how tall its words and its plate are.
const CAPTION_INSET: f32 = 6.;
const CAPTION_HEIGHT: f32 = 14.;
const CAPTION_PLATE: f32 = CAPTION_HEIGHT + 4.;
const CAPTION_PADDING: f32 = 6.;
/// What a letter of the line takes across, a little more than most do.
const CAPTION_LETTER: f32 = 6.5;
const CURVE_WIDTH: f32 = 1.5;
/// A mark and a dashed line are thin, under the curve in weight.
const THIN_WIDTH: f32 = 1.;
const DASH: f32 = 3.;
const HANDLE: f32 = 10.;
/// The dot of a handle with a number in it.
const NUMBERED_HANDLE: f32 = 16.;
const HANDLE_RING: f32 = 1.5;
/// The quiet lines behind the curve, such as the frames of a wavetable.
const LINE_WIDTH: f32 = 1.;
/// The target of a handle is larger than its dot: a trackpad is not a mouse.
const HANDLE_TARGET: f32 = 18.;
/// A full-scale waveform stops this far from the top and the bottom of the display.
const WAVEFORM_MARGIN: f32 = 6.;
/// The group of the inset of a display that takes files, which its handles hide under.
const INSET_GROUP: &str = "display-inset";
/// The lines that make a round corner of the fill.
const CORNER_STEPS: usize = 8;
/// From the sides of the inset to the controls of its strip: as the air above and below them.
const STRIP_PADDING: f32 = 6.;
/// A mark of the scale: from its line to its word, and from the top of the drawing.
const SCALE_GAP: f32 = 4.;
const SCALE_TOP: f32 = 3.;
/// The most a letter or a digit of a mark takes across.
const SCALE_LETTER: f32 = 7.;

/// One value a handle moves: where it is on a range, and what a double click sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub range: KnobRange,
    pub value: f32,
    pub default: f32,
    /// Whether the handle moves this value. A handle that only moves sideways still has a
    /// place on the other axis.
    pub drags: bool,
}

impl Axis {
    pub fn new(range: KnobRange, value: f32, default: f32) -> Self {
        let drags = true;
        Self {
            range,
            value,
            default,
            drags,
        }
    }

    /// A place on the display that does not move, from 0 to 1.
    pub fn fixed(position: f32) -> Self {
        let range = KnobRange::linear(0., 1.);
        let drags = false;
        Self {
            range,
            value: position,
            default: position,
            drags,
        }
    }

    fn position(&self) -> f32 {
        self.range.position(self.value)
    }
}

/// The values of a handle: `x` sideways, `y` up and down.
type HandleValues = Point<f32>;

#[derive(Clone)]
pub struct Handle {
    id: ElementId,
    x: Axis,
    y: Axis,
    hollow: bool,
    label: Option<SharedString>,
    dimmed: bool,
    area: bool,
    automated: bool,
    caption: Option<SharedString>,
    on_press: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_change: Option<ChangeHandler<HandleValues>>,
}

impl Handle {
    pub fn new(id: impl Into<ElementId>, x: Axis, y: Axis) -> Self {
        Self {
            id: id.into(),
            x,
            y,
            hollow: false,
            label: None,
            dimmed: false,
            area: false,
            automated: false,
            caption: None,
            on_press: None,
            on_change: None,
        }
    }

    /// What the line of the display says while the pointer is on the handle or drags it: what
    /// it moves, by name and value, such as `Decay 400 ms · Sustain 55%`. Only for a value no
    /// knob in view says: beside its knob a handle says nothing.
    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// A number or a letter in the dot, which makes it 16 pt.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// At 40 %, for what does nothing now, such as a band that is off. It still drags.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    /// Hears every press on the handle, a click as well as the start of a drag, before the
    /// drag does.
    pub fn on_press(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_press = Some(Rc::new(f));
        self
    }

    /// A handle that is the whole drawing, with no dot: a drag anywhere on it moves what it
    /// moves, from where that is. Under the other handles.
    pub fn area(mut self) -> Self {
        self.area = true;
        self
    }

    /// An automation lane moves a value of the handle: give the value it plays. The handle does
    /// not drag, and still hears its presses.
    pub fn automated(mut self, automated: bool) -> Self {
        self.automated = automated;
        self
    }

    /// A secondary handle: a ring and no fill.
    pub fn hollow(mut self, hollow: bool) -> Self {
        self.hollow = hollow;
        self
    }

    pub fn on_change(
        mut self,
        f: impl Fn(ValueChange<HandleValues>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl FluentBuilder for Handle {}

#[derive(IntoElement)]
pub struct Display {
    base: Div,
    id: ElementId,
    width: f32,
    /// Points from left to right, `x` and `y` from 0 to 1, `y` up.
    curve: Vec<Point<f32>>,
    /// Vertical grid lines at these places across, horizontal ones at these places up.
    grid: (Vec<f32>, Vec<f32>),
    /// The grid lines of [`Display::scale`], each at its place across with its word.
    scale: Vec<(f32, SharedString)>,
    /// The 0 dB line, at this place up.
    zero: Option<f32>,
    /// Thin lines up from the bottom, each at a place across to a height.
    marks: Vec<Point<f32>>,
    /// Soft columns up from the bottom, see [`Display::columns`].
    columns: Vec<(Range<f32>, f32)>,
    /// A second line, dashed and not filled, from left to right.
    dashed: Vec<Point<f32>>,
    /// Quiet lines behind the curve.
    lines: Vec<Vec<Point<f32>>>,
    /// Whether the curve is filled down to the bottom.
    filled: bool,
    /// The colours of the fill by height, see [`Display::fill_bands`].
    bands: Vec<(f32, Hsla)>,
    waveform: Waveform,
    handles: Vec<Handle>,
    caption: Option<SharedString>,
    /// How far from the right edge the line stands, see [`Display::caption_at_right`].
    caption_right: Option<f32>,
    controls: Vec<AnyElement>,
    strip: bool,
    children: Vec<AnyElement>,
    overlays: Vec<AnyElement>,
    takes_files: bool,
}

impl Display {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            width: WIDTH,
            curve: Vec::new(),
            grid: (Vec::new(), Vec::new()),
            scale: Vec::new(),
            zero: None,
            marks: Vec::new(),
            columns: Vec::new(),
            dashed: Vec::new(),
            lines: Vec::new(),
            filled: true,
            bands: Vec::new(),
            waveform: Waveform::default(),
            handles: Vec::new(),
            caption: None,
            caption_right: None,
            controls: Vec::new(),
            strip: false,
            children: Vec::new(),
            overlays: Vec::new(),
            takes_files: false,
        }
    }

    /// [`WIDE_WIDTH`] wide.
    pub fn wide(mut self) -> Self {
        self.width = WIDE_WIDTH;
        self
    }

    /// A control in the strip above the drawing, from the left, such as the type of a filter.
    /// With one, the drawing is [`STRIP_HEIGHT`] less tall.
    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.controls.push(control.into_any_element());
        self.strip = true;
        self
    }

    /// Keeps the room of the strip while it holds no control, for a display whose control
    /// comes and goes: its drawing then stays the size it is.
    pub fn strip(mut self) -> Self {
        self.strip = true;
        self
    }

    /// The size of the drawing of a display, in points: the whole inset, or what a control
    /// above it leaves.
    pub fn drawing_size(wide: bool, has_control: bool) -> (f32, f32) {
        let width = if wide { WIDE_WIDTH } else { WIDTH };
        let strip = if has_control { STRIP_HEIGHT } else { 0. };
        (width, INSET_HEIGHT - strip)
    }

    /// A display that takes a file dropped from the file manager: while a file is dragged over it,
    /// its handles hide, so none shows past the ring of the drop.
    pub fn takes_files(mut self, takes_files: bool) -> Self {
        self.takes_files = takes_files;
        self
    }

    /// An element over the whole inset, above the handles, with no padding: such as the ring and
    /// the words of a file dragged over it. Place it absolutely, at the top left, full size.
    pub fn overlay(mut self, overlay: impl IntoElement) -> Self {
        self.overlays.push(overlay.into_any_element());
        self
    }

    /// The curve, as points from left to right with `x` and `y` from 0 to 1, `y` up. It is
    /// filled down to the bottom of the display.
    pub fn curve(mut self, points: impl IntoIterator<Item = Point<f32>>) -> Self {
        self.curve = points.into_iter().collect();
        self
    }

    /// Grid lines: vertical ones at places across, horizontal ones at places up, 0 to 1.
    pub fn grid(mut self, across: Vec<f32>, up: Vec<f32>) -> Self {
        self.grid = (across, up);
        self
    }

    /// The scale across: a grid line at each place, 0 to 1, with what it is in faint words at
    /// its top, such as `1k` on a scale of frequencies.
    pub fn scale(
        mut self,
        marks: impl IntoIterator<Item = (f32, impl Into<SharedString>)>,
    ) -> Self {
        self.scale = (marks.into_iter())
            .map(|(across, word)| (across, word.into()))
            .collect();
        self
    }

    /// The 0 dB line, at a place up.
    pub fn zero_line(mut self, up: f32) -> Self {
        self.zero = Some(up);
        self
    }

    /// Thin lines up from the bottom, each at `x` across to `y` up, such as the early
    /// reflections of a reverb.
    pub fn marks(mut self, marks: impl IntoIterator<Item = Point<f32>>) -> Self {
        self.marks = marks.into_iter().collect();
        self
    }

    /// Soft columns up from the bottom, each from a place across to another and to a height, a
    /// hairline apart: parts of the sound side by side, such as the bands of a vocoder.
    pub fn columns(mut self, columns: impl IntoIterator<Item = (Range<f32>, f32)>) -> Self {
        self.columns = columns.into_iter().collect();
        self
    }

    /// A second line, dashed and not filled, as points from left to right, such as how the
    /// highs of a reverb fall.
    pub fn dashed(mut self, points: impl IntoIterator<Item = Point<f32>>) -> Self {
        self.dashed = points.into_iter().collect();
        self
    }

    /// Quiet lines behind the curve and not filled, each from left to right, such as the
    /// frames of a wavetable around the one that plays.
    pub fn lines(mut self, lines: impl IntoIterator<Item = Vec<Point<f32>>>) -> Self {
        self.lines = lines.into_iter().collect();
        self
    }

    /// Whether the curve is filled down to the bottom, as a level is. A wave is not.
    pub fn filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }

    /// The fill in colours by height, as on a meter, instead of one quiet tint: each colour from
    /// its place up, 0 to 1, to the place of the next. So a colour is always at the same height,
    /// such as the level of a spectrum. The line takes the colours too, at full strength: a
    /// level is not the composer's to drag.
    pub fn fill_bands(mut self, bands: impl IntoIterator<Item = (f32, Hsla)>) -> Self {
        self.bands = bands.into_iter().collect();
        self
    }

    pub fn handle(mut self, handle: Handle) -> Self {
        self.handles.push(handle);
        self
    }

    /// A waveform under the curve: the peak of each column across the display, 0 to 1, drawn
    /// mirrored about the middle.
    pub fn waveform(mut self, peaks: Vec<f32>) -> Self {
        self.waveform.peaks = peaks;
        self
    }

    /// The part of the display from `from` to `to` across, 0 to 1, between two lines, with what
    /// is outside it shaded: the part of a file that plays.
    pub fn kept(mut self, from: f32, to: f32) -> Self {
        self.waveform.kept = Some((from, to));
        self
    }

    /// The part from `from` across to the end of the kept part that repeats: tinted, with a thin
    /// line at its start.
    pub fn looped(mut self, from: Option<f32>) -> Self {
        self.waveform.looped = from;
        self
    }

    /// A green line at a place across, where the sound plays now. Level and signal are green.
    pub fn signal_line(mut self, at: Option<f32>) -> Self {
        self.waveform.signal = at;
        self
    }

    /// The line at the foot of the display. A card at rest has none: give it while what it
    /// says moves, such as how far a compressor turns the sound down now.
    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// The line stands at the right of the foot, `inset` from the edge, wherever the handles
    /// are: beside what it speaks of there, such as the bar of a reduction.
    pub fn caption_at_right(mut self, inset: f32) -> Self {
        self.caption_right = Some(inset);
        self
    }
}

/// More of the drawing, over the curve and under the handles: each child places itself in the
/// drawing, absolutely.
impl ParentElement for Display {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// The colours of what the display paints.
#[derive(Clone, Copy)]
struct Ink {
    curve: Hsla,
    fill: Hsla,
    grid: Hsla,
    zero: Hsla,
    thin: Hsla,
    soft: Hsla,
    waveform: Hsla,
    shade: Hsla,
    tint: Hsla,
    signal: Hsla,
}

/// What a waveform display draws under the curve.
#[derive(Clone, Default)]
struct Waveform {
    /// The peak of each column across, 0 to 1.
    peaks: Vec<f32>,
    /// The part that plays, between two lines, with the rest shaded.
    kept: Option<(f32, f32)>,
    /// Where the part that repeats starts, up to the end of the kept part.
    looped: Option<f32>,
    /// The green line of where the sound is.
    signal: Option<f32>,
}

/// What the display draws, in places from 0 to 1.
struct Drawing {
    curve: Vec<Point<f32>>,
    grid: (Vec<f32>, Vec<f32>),
    zero: Option<f32>,
    marks: Vec<Point<f32>>,
    columns: Vec<(Range<f32>, f32)>,
    dashed: Vec<Point<f32>>,
    lines: Vec<Vec<Point<f32>>>,
    filled: bool,
    bands: Vec<(f32, Hsla)>,
    waveform: Waveform,
}

/// A line through `points`, `width` wide, dashed or not.
fn stroke(
    bounds: Bounds<Pixels>,
    points: &[Point<f32>],
    width: f32,
    dash: Option<f32>,
) -> Option<gpui::Path<Pixels>> {
    let (first, rest) = points.split_first()?;
    let mut stroke = PathBuilder::stroke(px(width));
    if let Some(dash) = dash {
        stroke = stroke.dash_array(&[px(dash)]);
    }
    stroke.move_to(at(bounds, *first));
    for place in rest {
        stroke.line_to(at(bounds, *place));
    }
    // A path that does not tessellate paints nothing, which is all there is to do about it.
    stroke.build().ok()
}

/// A place on the display in points, from `x` and `y` from 0 to 1 with `y` up.
fn at(bounds: Bounds<Pixels>, place: Point<f32>) -> Point<Pixels> {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    bounds.origin + point(px(place.x * width), px((1. - place.y) * height))
}

fn paint_display(bounds: Bounds<Pixels>, drawing: &Drawing, ink: Ink, window: &mut Window) {
    let Drawing {
        curve,
        grid: (across, up),
        zero,
        marks,
        columns,
        dashed,
        lines,
        filled,
        bands,
        waveform,
    } = drawing;
    let mask = ContentMask { bounds };
    window.with_content_mask(Some(mask), |window| {
        let line = |window: &mut Window, from: Point<Pixels>, to: Point<Pixels>, color| {
            let size = size((to.x - from.x).max(px(1.)), (to.y - from.y).max(px(1.)));
            window.paint_quad(fill(Bounds::new(from, size), color));
        };
        paint_waveform(bounds, waveform, ink, window);
        for x in across {
            let top = at(bounds, point(*x, 1.));
            line(window, top, at(bounds, point(*x, 0.)), ink.grid);
        }
        for y in up {
            let left = at(bounds, point(0., *y));
            line(window, left, at(bounds, point(1., *y)), ink.grid);
        }
        if let Some(y) = *zero {
            line(
                window,
                at(bounds, point(0., y)),
                at(bounds, point(1., y)),
                ink.zero,
            );
        }
        for (across, up) in columns {
            let top = at(bounds, point(across.start, *up));
            let bottom = at(bounds, point(across.end, 0.));
            let width = (bottom.x - top.x - px(THIN_WIDTH)).max(px(THIN_WIDTH));
            let area = Bounds::new(top, size(width, bottom.y - top.y));
            window.paint_quad(fill(area, ink.soft));
        }
        for mark in marks {
            let bottom = at(bounds, point(mark.x, 0.));
            let top = at(bounds, *mark);
            let area = Bounds::new(point(top.x, top.y), size(px(THIN_WIDTH), bottom.y - top.y));
            window.paint_quad(fill(area, ink.thin));
        }
        if let Some(path) = stroke(bounds, dashed, THIN_WIDTH, Some(DASH)) {
            window.paint_path(path, ink.thin);
        }
        for line in lines {
            if let Some(path) = stroke(bounds, line, LINE_WIDTH, None) {
                window.paint_path(path, ink.soft);
            }
        }
        let (Some(first), Some(last)) = (curve.first(), curve.last()) else {
            return;
        };
        if !filled {
            if let Some(stroke) = stroke(bounds, curve, CURVE_WIDTH, None) {
                window.paint_path(stroke, ink.curve);
            }
            return;
        }
        // The fill keeps inside the round corners at the bottom of the inset.
        let floor = |x: Pixels| bounds.bottom() - corner_rise(x - bounds.left(), bounds.size.width);
        let mut area = PathBuilder::fill();
        let start = at(bounds, point(first.x, 0.));
        area.move_to(point(start.x, floor(start.x)));
        for place in curve {
            let place = at(bounds, *place);
            area.line_to(point(place.x, place.y.min(floor(place.x))));
        }
        let end = at(bounds, point(last.x, 0.)).x;
        area.line_to(point(end, floor(end)));
        let steps = (0..=CORNER_STEPS).map(|step| step as f32 / CORNER_STEPS as f32);
        let corner = px(f32::from(radius::MD));
        let right = steps.clone().map(|part| bounds.right() - corner * part);
        let left = steps.map(|part| bounds.left() + corner * (1. - part));
        for x in right.chain(left).filter(|x| *x < end && *x > start.x) {
            area.line_to(point(x, floor(x)));
        }
        area.close();
        if let Ok(area) = area.build() {
            if bands.is_empty() {
                window.paint_path(area, ink.fill);
            } else {
                paint_bands(bounds, &area, curve, bands, window);
            }
        }
        if let Some(stroke) = stroke(bounds, curve, CURVE_WIDTH, None) {
            if bands.is_empty() {
                window.paint_path(stroke, ink.curve);
            } else {
                let opaque: Vec<_> = (bands.iter())
                    .map(|(from, color)| (*from, color.alpha(1.)))
                    .collect();
                paint_bands(bounds, &stroke, curve, &opaque, window);
            }
        }
    });
}

/// How far the round corner of the inset is above its bottom edge, `x` from its left side: 0
/// between the two corners.
fn corner_rise(x: Pixels, width: Pixels) -> Pixels {
    let radius = f32::from(radius::MD);
    let inside = f32::from(x.min(width - x)).clamp(0., radius);
    px(radius - (radius * radius - (radius - inside).powi(2)).sqrt())
}

/// The area under the curve, or its line, once per band, under a mask of the band's heights. A
/// band above the curve has nothing to paint.
fn paint_bands(
    bounds: Bounds<Pixels>,
    area: &gpui::Path<Pixels>,
    curve: &[Point<f32>],
    bands: &[(f32, Hsla)],
    window: &mut Window,
) {
    let top = curve.iter().fold(0_f32, |top, place| top.max(place.y));
    let ends = bands.iter().skip(1).map(|(from, _)| *from).chain([1.]);
    for ((from, color), to) in bands.iter().zip(ends) {
        if *from >= top {
            break;
        }
        let band = Bounds::from_corners(at(bounds, point(0., to)), at(bounds, point(1., *from)));
        window.with_content_mask(Some(ContentMask { bounds: band }), |window| {
            window.paint_path(area.clone(), *color);
        });
    }
}

/// The waveform, the shade outside the part that plays with its two lines, the tint of the part
/// that repeats, and the green line of where the sound is. Under the grid and the curve.
fn paint_waveform(bounds: Bounds<Pixels>, waveform: &Waveform, ink: Ink, window: &mut Window) {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let middle = height / 2.;
    let count = waveform.peaks.len();
    if count > 0 {
        let column = width / count as f32;
        // At least a hairline, so silence still shows where the file is.
        let reaches: Vec<f32> = waveform
            .peaks
            .iter()
            .map(|peak| (peak.clamp(0., 1.) * (middle - WAVEFORM_MARGIN)).max(0.5))
            .collect();
        let origin = (f32::from(bounds.left()), f32::from(bounds.top()) + middle);
        paint::mirrored_waveform(window, origin, column, &reaches, ink.waveform);
    }
    if let Some((from, to)) = waveform.kept {
        let (left, right) = (from.clamp(0., 1.) * width, to.clamp(0., 1.) * width);
        let shade = |window: &mut Window, x: f32, shaded: f32| {
            let area = Bounds::new(
                bounds.origin + point(px(x), px(0.)),
                size(px(shaded), bounds.size.height),
            );
            window.paint_quad(fill(area, ink.shade));
        };
        shade(window, 0., left);
        shade(window, right, width - right);
        for x in [left, right] {
            let area = Bounds::new(
                bounds.origin + point(px(x.round() - 0.5), px(0.)),
                size(px(1.), bounds.size.height),
            );
            window.paint_quad(fill(area, ink.curve));
        }
    }
    if let Some(from) = waveform.looped {
        let to = waveform.kept.map_or(1., |(_, to)| to).clamp(0., 1.) * width;
        let from = from.clamp(0., 1.) * width;
        let area = Bounds::new(
            bounds.origin + point(px(from), px(0.)),
            size(px((to - from).max(0.)), bounds.size.height),
        );
        window.paint_quad(fill(area, ink.tint));
        let line = Bounds::new(
            bounds.origin + point(px(from.round() - 0.5), px(0.)),
            size(px(THIN_WIDTH), bounds.size.height),
        );
        window.paint_quad(fill(line, ink.thin));
    }
    if let Some(at) = waveform.signal {
        let area = Bounds::new(
            bounds.origin + point(px((at.clamp(0., 1.) * width).round() - 0.5), px(0.)),
            size(px(1.), bounds.size.height),
        );
        window.paint_quad(fill(area, ink.signal));
    }
}

/// The value of one axis of a handle for the pointer now.
fn moved(axis: Axis, travel: &mut Travel, pointer: f32, fine: bool) -> f32 {
    match travel.position(pointer, fine) {
        Some(position) if axis.drags => axis.range.value(position),
        _ => axis.value,
    }
}

/// The side of the drawing the line of a display stands at.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Middle,
    Right,
    Left,
}

/// Where the line of a display stands: at the top of the drawing or at its foot, and its side.
type CaptionPlace = (bool, Side);

/// In the order the line takes them: the foot before the top, the middle before the sides.
const CAPTION_PLACES: [CaptionPlace; 6] = [
    (false, Side::Middle),
    (false, Side::Right),
    (false, Side::Left),
    (true, Side::Middle),
    (true, Side::Right),
    (true, Side::Left),
];

/// The first place where a line `plate` wide lies over nothing of `taken`: what the composer
/// drags, as points of the drawing from its top left, each with the room it keeps clear. The
/// middle of the foot when every place is taken.
fn caption_place(
    plate: f32,
    (width, height): (f32, f32),
    taken: &[(Point<f32>, f32)],
) -> CaptionPlace {
    let free = |(top, side): &CaptionPlace| {
        let left = match side {
            Side::Middle => (width - plate) / 2.,
            Side::Right => width - CAPTION_INSET - plate,
            Side::Left => CAPTION_INSET,
        };
        let above = match top {
            true => CAPTION_INSET,
            false => height - CAPTION_INSET - CAPTION_PLATE,
        };
        taken.iter().all(|(place, clear)| {
            place.x < left - clear
                || place.x > left + plate + clear
                || place.y < above - clear
                || place.y > above + CAPTION_PLATE + clear
        })
    };
    (CAPTION_PLACES.into_iter().find(free)).unwrap_or(CAPTION_PLACES[0])
}

/// The curve as points a few points of the display apart, each with the room its line takes.
fn along_curve(curve: &[Point<f32>], (width, height): (f32, f32)) -> Vec<(Point<f32>, f32)> {
    const APART: f32 = 4.;
    let place = |at: &Point<f32>| point(at.x * width, (1. - at.y) * height);
    let between = curve.windows(2).flat_map(|ends| {
        let (from, to) = (place(&ends[0]), place(&ends[1]));
        let steps = ((to.x - from.x).hypot(to.y - from.y) / APART)
            .ceil()
            .max(1.) as usize;
        (0..=steps).map(move |step| {
            let part = step as f32 / steps as f32;
            point(
                from.x + (to.x - from.x) * part,
                from.y + (to.y - from.y) * part,
            )
        })
    });
    between.map(|at| (at, CURVE_WIDTH)).collect()
}

/// Whether the pointer is on a handle that speaks, or drags it, wherever the pointer went
/// since. Kept in element state under the handle, so it goes when the handle does.
#[derive(Clone, Copy, Default)]
struct Pointing {
    on: bool,
    drags: bool,
}

/// The key of what a handle keeps in element state. Under the id of the display, so that two
/// displays with handles of one name, such as two filters in a rack, keep a drag and a focus
/// each. A display draws before its element pushes its id, so the key names the display itself.
fn handle_key(display: &ElementId, handle: &Handle) -> ElementId {
    ElementId::NamedChild(Arc::new(display.clone()), handle.id.to_string().into())
}

/// Hears the end of the drag of a handle that speaks, as an element of no size: the line of
/// the display then says it no longer, though nothing else is drawn again at a mouse up.
fn drag_end_listener(pointing: Entity<Pointing>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                let ends = phase == DispatchPhase::Capture && event.button == MouseButton::Left;
                if ends && pointing.read(cx).drags {
                    pointing.update(cx, |pointing, cx| {
                        pointing.drags = false;
                        cx.notify();
                    });
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// The element of one handle, a dot in a larger target centred on its place.
fn handle_element(
    display: &ElementId,
    handle: Handle,
    hides_under_files: bool,
    (width, height): (f32, f32),
    (dot, ring): (Hsla, Hsla),
    pointing: Option<Entity<Pointing>>,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let key = handle_key(display, &handle);
    let state = window.use_keyed_state(key, cx, |_, cx| GestureState::new(cx));
    let focus_handle = state.read(cx).focus_handle.clone();
    let held = handle.automated;
    let (x, y) = (handle.x, handle.y);
    let place = point(x.position(), y.position());
    let value = point(x.value, y.value);
    // A double click resets what the handle moves, and leaves an axis it does not move alone.
    let reset_of = |axis: Axis| if axis.drags { axis.default } else { axis.value };
    let reset = point(reset_of(x), reset_of(y));
    let cursor = match (x.drags, y.drags) {
        (true, false) => CursorStyle::ResizeLeftRight,
        (false, true) => CursorStyle::ResizeUpDown,
        _ => CursorStyle::Crosshair,
    };
    let (fill, border) = match handle.hollow {
        true => (ring, dot),
        false => (dot, ring),
    };
    let selector = handle.id.clone();
    let dot = match handle.label {
        Some(_) => NUMBERED_HANDLE,
        None => HANDLE,
    };
    let on_press = handle.on_press;
    let area = handle.area;
    div()
        .id(handle.id)
        .debug_selector(move || format!("handle-{selector}"))
        // A handle that speaks tells the display while the pointer is on it or drags it.
        .when_some(pointing.clone(), |target, pointing| {
            target
                .on_hover({
                    let pointing = pointing.clone();
                    move |on, _, cx| {
                        pointing.update(cx, |pointing, cx| {
                            pointing.on = *on;
                            cx.notify();
                        });
                    }
                })
                .child(drag_end_listener(pointing))
        })
        .absolute()
        .map(|target| match area {
            true => target.top_0().left_0().w(px(width)).h(px(height)),
            false => target
                .left(px(place.x * width - HANDLE_TARGET / 2.))
                .top(px((1. - place.y) * height - HANDLE_TARGET / 2.))
                .size(px(HANDLE_TARGET)),
        })
        .flex()
        .items_center()
        .justify_center()
        .when(handle.dimmed, |d| d.opacity(0.4))
        .when(hides_under_files, |d| {
            d.group_drag_over::<ExternalPaths>(INSET_GROUP, |style| style.opacity(0.))
        })
        .when(!area, |target| {
            target.child(
                div()
                    .size(px(dot))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(fill)
                    .border(px(HANDLE_RING))
                    .border_color(border)
                    .font(typography::tabular())
                    .text_variant(TextVariant::Label)
                    .line_height(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    // The number is in the colour of the ring on a full dot, and of the dot on a
                    // hollow one: always the other of the two.
                    .text_color(border)
                    .children(handle.label),
            )
        })
        // A handle that does not drag still hears its presses. One that drags hears them in
        // its own listener, before the drag, which stops the press there.
        .when_some(
            on_press
                .clone()
                .filter(|_| held || handle.on_change.is_none()),
            |d, on_press| {
                d.on_mouse_down(MouseButton::Left, move |_, window, cx| on_press(window, cx))
            },
        )
        // A held handle keeps its focus, so that a lane arriving during a drag ends the drag
        // and leaves nothing open.
        .when_some(handle.on_change.clone().filter(|_| held), |d, on_change| {
            d.track_focus(&focus_handle)
                .child(gesture::held_listeners(state.clone(), on_change))
        })
        .when_some(handle.on_change.filter(|_| !held), |d, on_change| {
            let on_mouse_down = {
                let (state, on_change) = (state.clone(), on_change.clone());
                let press_focus = focus_handle.clone();
                move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                    if let Some(on_press) = &on_press {
                        on_press(window, cx);
                    }
                    let pointer = event.position;
                    let (px_x, px_y) = (f32::from(pointer.x), -f32::from(pointer.y));
                    let mut across = Travel::new(px_x, place.x, width);
                    let mut up = Travel::new(px_y, place.y, height);
                    let value_at = move |pointer: Point<Pixels>, fine| {
                        point(
                            moved(x, &mut across, f32::from(pointer.x), fine),
                            moved(y, &mut up, -f32::from(pointer.y), fine),
                        )
                    };
                    let reset = Some(reset);
                    let took = gesture::press(
                        &state, event, value, reset, value_at, &on_change, window, cx,
                    );
                    // A press that opened a drag or reset is the handle's, not the card's. Stopping
                    // it also stops GPUI giving the handle the focus, which escape needs, so the
                    // handle takes it itself.
                    if took {
                        if let Some(pointing) = &pointing {
                            pointing.update(cx, |pointing, _| pointing.drags = true);
                        }
                        window.focus(&press_focus, cx);
                        cx.stop_propagation();
                    }
                }
            };
            // Only escape: the keys of a value are those of its knob.
            let on_key_down = {
                let (state, on_change) = (state.clone(), on_change.clone());
                move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                    gesture::key_down(&state, event, None, None, &on_change, window, cx);
                }
            };
            d.cursor(cursor)
                .track_focus(&focus_handle)
                .on_key_down(on_key_down)
                .on_mouse_down(MouseButton::Left, on_mouse_down)
                .child(gesture::drag_listeners(state, on_change))
        })
}

impl RenderOnce for Display {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let inset = theme.well().opacity(0.7);
        let ink = Ink {
            curve: theme.text(),
            fill: theme.fill(),
            grid: theme.border(),
            zero: theme.border_strong(),
            thin: theme.text().opacity(0.5),
            soft: paint::soft_fill(theme),
            waveform: theme.text().opacity(0.3),
            shade: theme.well().opacity(0.72),
            tint: theme.selected(),
            signal: theme.green,
        };
        let handle_colors = (theme.text(), theme.well());
        let (caption_color, scale_color) = (theme.text_secondary(), theme.text_muted());
        let plate = theme.well().opacity(0.85);
        let strip = match self.strip {
            true => STRIP_HEIGHT,
            false => 0.,
        };
        let size = (self.width, INSET_HEIGHT - strip);
        let (mut across, up) = self.grid;
        across.extend(self.scale.iter().map(|(across, _)| *across));
        let grid = (across, up);
        let scale = self.scale.into_iter().map(|(across, word)| {
            let line = across * self.width;
            // After its line, or before it where the display ends too soon after it.
            let fits = line + SCALE_GAP + word.chars().count() as f32 * SCALE_LETTER <= self.width;
            div()
                .absolute()
                .top(px(SCALE_TOP))
                .map(|mark| match fits {
                    true => mark.left(px(line + SCALE_GAP)),
                    false => mark.right(px(self.width - line + SCALE_GAP)),
                })
                .text_variant(TextVariant::Label)
                .line_height(px(CAPTION_HEIGHT))
                .whitespace_nowrap()
                .child(typography::tabular_text(word, scale_color))
        });
        let scale: Vec<_> = scale.collect();
        // In the order they are drawn, the topmost last, each with its pointer if it speaks.
        let (areas, handles): (Vec<_>, Vec<_>) =
            self.handles.into_iter().partition(|handle| handle.area);
        let handles: Vec<_> = (areas.into_iter().chain(handles))
            .map(|handle| {
                let key = handle_key(&self.id, &handle);
                let speaks = handle.caption.is_some();
                let pointing =
                    speaks.then(|| window.use_keyed_state(key, cx, |_, _| Pointing::default()));
                (handle, pointing)
            })
            .collect();
        // The handle a drag has speaks, or else the topmost under the pointer.
        let speaking = |wanted: fn(Pointing) -> bool| {
            let speaking = handles.iter().rev().find(|(_, pointing)| {
                (pointing.as_ref()).is_some_and(|pointing| wanted(*pointing.read(cx)))
            });
            speaking.and_then(|(handle, _)| handle.caption.clone())
        };
        let handle =
            speaking(|pointing| pointing.drags).or_else(|| speaking(|pointing| pointing.on));
        let caption: Option<SharedString> = match (handle, self.caption) {
            (Some(handle), Some(caption)) => Some(format!("{handle} · {caption}").into()),
            (Some(says), None) | (None, Some(says)) => Some(says),
            (None, None) => None,
        };
        let caption = caption.map(|caption| {
            let (top, side, inset) = match self.caption_right {
                Some(inset) => (false, Side::Right, inset),
                None => {
                    let dots = (handles.iter().filter(|(handle, _)| !handle.area))
                        .map(|(handle, _)| (handle.x.position(), handle.y.position()))
                        .map(|(x, y)| (point(x * size.0, (1. - y) * size.1), HANDLE_TARGET / 2.));
                    let taken: Vec<_> = dots.chain(along_curve(&self.curve, size)).collect();
                    let plate =
                        caption.chars().count() as f32 * CAPTION_LETTER + 2. * CAPTION_PADDING;
                    let (top, side) = caption_place(plate, size, &taken);
                    (top, side, CAPTION_INSET)
                }
            };
            // For tests, which find the line by what it says: `caption-<words>`.
            let selector = caption.clone();
            let words = div()
                .debug_selector(move || format!("caption-{selector}"))
                .h(px(CAPTION_PLATE))
                .px(px(CAPTION_PADDING))
                .flex()
                .items_center()
                .rounded(radius::SM)
                .bg(plate)
                .text_variant(TextVariant::Label)
                .line_height(px(CAPTION_HEIGHT))
                .whitespace_nowrap()
                .child(typography::tabular_text(caption, caption_color));
            div()
                .absolute()
                .left_0()
                .w_full()
                .px(px(inset))
                .map(|line| match top {
                    true => line.top(px(CAPTION_INSET)),
                    false => line.bottom(px(CAPTION_INSET)),
                })
                .flex()
                .map(|line| match side {
                    Side::Middle => line.justify_center(),
                    Side::Right => line.justify_end(),
                    Side::Left => line.justify_start(),
                })
                .child(words)
        });
        let drawing = Drawing {
            curve: self.curve,
            grid,
            zero: self.zero,
            marks: self.marks,
            columns: self.columns,
            dashed: self.dashed,
            lines: self.lines,
            filled: self.filled,
            bands: self.bands,
            waveform: self.waveform,
        };
        let drawing = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_display(bounds, &drawing, ink, window),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let handles = handles.into_iter().map(|(handle, pointing)| {
            let (hides, colors) = (self.takes_files, handle_colors);
            let area = handle.area;
            let element =
                handle_element(&self.id, handle, hides, size, colors, pointing, window, cx);
            (area, element)
        });
        let (areas, handles): (Vec<_>, Vec<_>) = handles.partition(|(area, _)| *area);
        let element = |(_, element)| element;
        let areas: Vec<_> = areas.into_iter().map(element).collect();
        let handles: Vec<_> = handles.into_iter().map(element).collect();
        let controls = (!self.controls.is_empty()).then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(STRIP_HEIGHT))
                .px(px(STRIP_PADDING))
                .flex()
                .items_center()
                .gap(px(4.))
                .children(self.controls)
        });

        self.base
            .id(self.id)
            .flex_none()
            .flex()
            .flex_col()
            .w(px(self.width))
            .child(
                div()
                    .id("inset")
                    .when(self.takes_files, |d| d.group(INSET_GROUP))
                    .relative()
                    .h(px(INSET_HEIGHT))
                    .rounded(radius::MD)
                    .bg(inset)
                    .children(controls)
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(strip))
                            .w_full()
                            .h(px(INSET_HEIGHT - strip))
                            .child(drawing)
                            .children(scale)
                            .children(areas)
                            .children(self.children)
                            .children(handles)
                            .children(caption),
                    )
                    .children(self.overlays),
            )
    }
}

/// A display with words in place of a drawing: a line, a button or both in its middle, such as
/// `Drop an audio file here` over `Choose file`. The button is the way from the keys.
#[derive(IntoElement)]
pub struct TextDisplay {
    display: Display,
    message: Option<SharedString>,
    button: Option<AnyElement>,
    drop: Option<FileDrop>,
}

impl TextDisplay {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            display: Display::new(id),
            message: None,
            button: None,
            drop: None,
        }
    }

    /// [`WIDE_WIDTH`] wide.
    pub fn wide(mut self) -> Self {
        self.display = self.display.wide();
        self
    }

    /// The line. A long one wraps inside the display.
    pub fn message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn button(mut self, button: impl IntoElement) -> Self {
        self.button = Some(button.into_any_element());
        self
    }

    /// Takes a file dropped from the file manager, see [`FileDrop`].
    pub fn drop_file(mut self, drop: FileDrop) -> Self {
        self.drop = Some(drop);
        self
    }
}

impl RenderOnce for TextDisplay {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let middle = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .px(px(12.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .text_variant(TextVariant::Label)
            .line_height(px(CAPTION_HEIGHT))
            .text_color(cx.theme().text_secondary())
            .text_center()
            .children(self.message)
            .children(self.button);
        let display = self.display.child(middle);
        match self.drop {
            Some(drop) => display.overlay(drop),
            None => display,
        }
    }
}

/// The place of a display on the card of a device with nothing to draw, such as a plugin, which
/// draws itself in a window of its own. No inset: only the controls a strip would hold, on the
/// line of the knobs beside them, and under them what is wrong, in words. Two controls of two
/// cells each fill it.
#[derive(IntoElement, Default)]
pub struct ControlStrip {
    controls: Vec<AnyElement>,
    message: Option<SharedString>,
}

impl ControlStrip {
    pub fn new() -> Self {
        Self::default()
    }

    /// A control, after those it has.
    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.controls.push(control.into_any_element());
        self
    }

    /// The line under the controls. A long one wraps.
    pub fn message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
    }
}

impl RenderOnce for ControlStrip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // Under controls the words start on the label line of a cell. Alone, their first line
        // is centred where the controls would be.
        let above = match self.controls.is_empty() {
            true => (STRIP_HEIGHT - CAPTION_HEIGHT) / 2.,
            false => 2.,
        };
        let controls = (!self.controls.is_empty()).then(|| {
            div()
                .flex_none()
                .h(px(STRIP_HEIGHT))
                .flex()
                .items_center()
                .gap(px(GAP))
                .children(self.controls)
        });
        let message = self.message.map(|message| {
            div()
                .pt(px(above))
                .text_variant(TextVariant::Label)
                .line_height(px(CAPTION_HEIGHT))
                .text_color(cx.theme().text_secondary())
                .child(message)
        });
        div()
            .flex_none()
            .w(px(WIDTH))
            .h(px(INSET_HEIGHT))
            .flex()
            .flex_col()
            .children(controls)
            .children(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (WIDTH, INSET_HEIGHT);

    #[test]
    fn the_line_takes_the_first_place_that_is_over_no_handle() {
        let handle = |x: f32, y: f32| (point(x, y), HANDLE_TARGET / 2.);
        assert_eq!(caption_place(100., SIZE, &[]), (false, Side::Middle));
        // A handle up in the drawing is in no place of the line.
        let high = [handle(WIDTH / 2., 60.)];
        assert_eq!(caption_place(100., SIZE, &high), (false, Side::Middle));
        let foot = [handle(WIDTH / 2., INSET_HEIGHT)];
        assert_eq!(caption_place(100., SIZE, &foot), (false, Side::Right));
        // A long line reaches the handle from either side of the foot, so it stands at the top,
        // and beside a handle that is there.
        assert_eq!(caption_place(180., SIZE, &foot), (true, Side::Middle));
        let envelope = [handle(WIDTH / 2., INSET_HEIGHT), handle(30., 12.)];
        assert_eq!(caption_place(180., SIZE, &envelope), (true, Side::Right));
    }
}
