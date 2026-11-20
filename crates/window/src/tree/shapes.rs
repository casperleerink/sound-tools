//! Lines and rectangles a view draws: its numbers as vectors, and list feeds into pixels, which
//! change every frame without a commit, as Rune's shapes cannot.

use rune::{El, LineCap, Shape, canvas, el};
use serde::Deserialize;

use crate::feeds::ListFeed;
use crate::lives::Shows;
use crate::theme::Theme;

use super::{Scope, ShapesNode};

/// The numbers of a line or rects, or a list feed that gives them every frame.
#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Points {
    Fixed(Vec<f32>),
    Fed(ListFeed),
}

/// One axis of `units`: a length from 0, or what its start and its end are, such as
/// `[18, -18]` dB down.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(untagged)]
pub(super) enum Span {
    Length(f32),
    Ends([f32; 2]),
}

impl Span {
    fn ends(self) -> [f32; 2] {
        match self {
            Self::Length(length) => [0.0, length],
            Self::Ends(ends) => ends,
        }
    }
}

/// The list feeds of a `shapes`, as it draws them: also the name of their binding.
#[derive(Clone, Debug)]
pub(crate) struct Listed {
    /// Its width and height in points, where they are numbers.
    size: [Option<f32>; 2],
    /// What its edges are in its units: left and right, top and bottom.
    units: Option<[[f32; 2]; 2]>,
    /// The part of a list it shows, where `units` gives its ends across.
    range: Option<[f64; 2]>,
    parts: Vec<Part>,
}

#[derive(Clone, Debug)]
struct Part {
    feed: ListFeed,
    color: String,
    /// A line, and the colour of the area below it; else rects.
    line: Option<(f32, Option<String>)>,
}

/// Device pixels a point: list feeds draw as sharp on a display of twice the pixels.
const SCALE: f32 = 2.0;

impl Listed {
    pub(crate) fn feeds(&self) -> impl Iterator<Item = &ListFeed> {
        self.parts.iter().map(|part| &part.feed)
    }

    /// How many numbers across a list of it reads: one a point of its width.
    pub(crate) fn points(&self) -> usize {
        self.size[0].map_or(0, |w| w.round().max(2.0) as usize)
    }

    pub(crate) fn range(&self) -> Option<[f64; 2]> {
        self.range
    }

    /// `lists`, one for each of its feeds, drawn: width, height and straight RGBA of sRGB.
    pub(crate) fn raster(&self, theme: &Theme, lists: &[Vec<f32>]) -> (u32, u32, Vec<u8>) {
        let longest = lists.iter().map(Vec::len).max().unwrap_or(0) as f32;
        let [w, h] = [
            self.size[0].unwrap_or(longest),
            self.size[1].unwrap_or(longest),
        ];
        let mut pixels = Raster::new((w * SCALE).ceil() as usize, (h * SCALE).ceil() as usize);
        // From the units the numbers are in to pixels, along each axis.
        let [across, down] = self.units.unwrap_or([[0.0, w], [0.0, h]]);
        let along = |[from, to]: [f32; 2], pixels: usize| {
            let scale = pixels as f32 / (to - from);
            move |unit: f32| (unit - from) * scale
        };
        let (x, y) = (along(across, pixels.w), along(down, pixels.h));
        for (part, list) in self.parts.iter().zip(lists) {
            let colour = srgb(theme.color(&part.color));
            match &part.line {
                Some((width, fill)) => {
                    let fill = fill.as_ref().map(|fill| srgb(theme.color(fill)));
                    pixels.line(list, &y, width * SCALE, colour, fill);
                }
                None => {
                    for rect in list.chunks_exact(4) {
                        let ([left, right], [top, bottom]) = (
                            [x(rect[0]), x(rect[0] + rect[2])],
                            [y(rect[1]), y(rect[1] + rect[3])],
                        );
                        let rect = [
                            left.min(right),
                            top.min(bottom),
                            left.max(right),
                            top.max(bottom),
                        ];
                        pixels.rect(rect, colour);
                    }
                }
            }
        }
        (pixels.w as u32, pixels.h as u32, pixels.rgba)
    }
}

/// The list feeds of `d`, if it has any.
pub(crate) fn listed(d: &ShapesNode) -> Option<Listed> {
    let lines = (d.lines.iter()).filter_map(|lines| match &lines.points {
        Points::Fed(feed) => Some(Part {
            feed: feed.clone(),
            color: lines.color.clone(),
            line: Some((lines.width, lines.fill.clone())),
        }),
        Points::Fixed(_) => None,
    });
    let rects = (d.rects.iter()).filter_map(|rects| match &rects.rects {
        Points::Fed(feed) => Some(Part {
            feed: feed.clone(),
            color: rects.color.clone(),
            line: None,
        }),
        Points::Fixed(_) => None,
    });
    let parts: Vec<Part> = lines.chain(rects).collect();
    let units = d.units.map(|units| units.map(Span::ends));
    (!parts.is_empty()).then(|| Listed {
        size: [d.w.fixed(), d.h.fixed()],
        units,
        range: match d.units {
            Some([Span::Ends([from, to]), _]) => Some([f64::from(from), f64::from(to)]),
            _ => None,
        },
        parts,
    })
}

pub(super) fn drawing(scope: &Scope, d: &ShapesNode) -> El {
    let theme = &scope.state.theme;
    let mut shape = Shape::new();
    // Points in units go from 0 to 1 across the box, which an open line fills down to.
    let units = d.units.map(|units| units.map(Span::ends));
    let unit = |[x, y]: [f32; 2]| match units {
        Some([[left, right], [top, bottom]]) => {
            [(x - left) / (right - left), (y - top) / (bottom - top)]
        }
        None => [x, y],
    };
    let bottom = match units {
        Some(_) => 1.0,
        None => d.h.fixed().unwrap_or(0.0),
    };
    for lines in &d.lines {
        let Points::Fixed(points) = &lines.points else {
            continue;
        };
        let colour = theme.color(&lines.color);
        let runs = runs(points, unit);
        if let Some(fill) = &lines.fill {
            let fill = theme.color(fill);
            for run in &runs {
                shape = shape.fill(filled(run, bottom), fill);
            }
        }
        for run in runs {
            // Round, so the joints of a run, such as an arc, show no seams; but round joints
            // overlap, which shows in a see-through colour.
            let cap = match run.len() > 2 && colour.a >= 1.0 {
                true => LineCap::Round,
                false => LineCap::Butt,
            };
            shape = shape.stroke_with(run, false, lines.width, cap, colour);
        }
    }
    for rects in &d.rects {
        let Points::Fixed(numbers) = &rects.rects else {
            continue;
        };
        let colour = theme.color(&rects.color);
        for rect in numbers.chunks_exact(4) {
            let [x, y, w, h] = [rect[0], rect[1], rect[2], rect[3]];
            let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
            shape = shape.fill(corners.map(unit).to_vec(), colour);
        }
    }
    if units.is_some() {
        shape = shape.view_box(1.0, 1.0);
    }
    let mut trim = None;
    match d.trim.as_ref().map(|[from, to]| (from.fixed(), to.fixed())) {
        Some((Some(from), Some(to))) => shape = shape.trimmed(from, to),
        Some(_) => trim = d.trim.clone(),
        None => {}
    }
    let mut e = canvas(shape);
    if let (Some(w), Some(h)) = (d.w.fixed(), d.h.fixed()) {
        e = e.size(w, h);
    }
    if let Some(trim) = trim {
        e = scope.bind(e, Shows::Trim(trim));
    }
    if let Some(listed) = listed(d) {
        // Over its numbers, the same size.
        let fed = scope.bind(
            el().absolute(0.0, 0.0).w_full().h_full(),
            Shows::Lists(listed),
        );
        e = e.child(fed);
    }
    if let Some(key) = &d.key {
        e = e.key(scope.key(key));
    }
    scope.placed(
        e,
        d.at.as_ref(),
        (Some(&d.w), Some(&d.h)),
        d.opacity.as_ref(),
    )
}

/// The area a run of a line closes: itself when it ends where it starts, else down to `bottom`
/// below its ends.
fn filled(run: &[[f32; 2]], bottom: f32) -> Vec<[f32; 2]> {
    let mut area = run.to_vec();
    if let (Some(&[first_x, _]), Some(&[last_x, _])) = (run.first(), run.last())
        && run.first() != run.last()
    {
        area.extend([[last_x, bottom], [first_x, bottom]]);
    }
    area
}

/// Line segments (x1 y1 x2 y2 each) as strokes: a segment that starts where the last ended goes
/// on with it, so an arc of short segments draws as one line, without seams.
fn runs(points: &[f32], unit: impl Fn([f32; 2]) -> [f32; 2]) -> Vec<Vec<[f32; 2]>> {
    let mut runs: Vec<Vec<[f32; 2]>> = Vec::new();
    for segment in points.chunks_exact(4) {
        let (from, to) = (
            unit([segment[0], segment[1]]),
            unit([segment[2], segment[3]]),
        );
        match runs.last_mut() {
            Some(run) if run.last() == Some(&from) => run.push(to),
            _ => runs.push(vec![from, to]),
        }
    }
    runs
}

/// A colour as the bytes of a pixel: sRGB, straight alpha.
fn srgb(colour: rune::Color) -> [f32; 4] {
    let encode = |c: f32| rune::color::srgb_encode(c.clamp(0.0, 1.0)) * 255.0;
    [
        encode(colour.r),
        encode(colour.g),
        encode(colour.b),
        colour.a,
    ]
}

/// Pixels drawn a column or a row at a time: a list draws a column a pixel.
struct Raster {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

impl Raster {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            rgba: vec![0; w * h * 4],
        }
    }

    /// `values` evenly from the left edge to the right, each at the pixel `y` gives it: a line
    /// `width` pixels thick, and the area below it in `fill`.
    fn line(
        &mut self,
        values: &[f32],
        y: impl Fn(f32) -> f32,
        width: f32,
        colour: [f32; 4],
        fill: Option<[f32; 4]>,
    ) {
        let Some(&first) = values.first() else {
            return;
        };
        let (last, across) = (values.len().saturating_sub(1) as f32, self.w as f32);
        let at = |x: f32| {
            let place = (x / across * last).clamp(0.0, last);
            let (index, part) = (place.floor() as usize, place.fract());
            let next = values.get(index + 1).copied().unwrap_or(values[index]);
            y(values[index] + (next - values[index]) * part)
        };
        let half = width / 2.0;
        let mut left = y(first);
        for column in 0..self.w {
            let right = at(column as f32 + 1.0);
            if let Some(fill) = fill {
                self.column(column, (left + right) / 2.0, self.h as f32, fill);
            }
            self.column(
                column,
                left.min(right) - half,
                left.max(right) + half,
                colour,
            );
            left = right;
        }
    }

    /// Rows `from` to `to` of `column`, the rows they cut through in part.
    fn column(&mut self, column: usize, from: f32, to: f32, colour: [f32; 4]) {
        let (from, to) = (from.max(0.0), to.min(self.h as f32));
        if to <= from {
            return;
        }
        let (first, last) = (from.floor() as usize, to.ceil() as usize);
        let solid = colour.map(|channel| channel as u8);
        let solid = [
            solid[0],
            solid[1],
            solid[2],
            (colour[3] * 255.0).round() as u8,
        ];
        let mut index = (first * self.w + column) * 4;
        for row in first..last {
            let edge = row == first || row + 1 == last;
            let covered = match edge {
                true => (to.min(row as f32 + 1.0) - from.max(row as f32)).clamp(0.0, 1.0),
                false => 1.0,
            };
            // Most of an area: all of the pixel, nothing under it.
            if covered == 1.0 && self.rgba[index + 3] == 0 {
                self.rgba[index..index + 4].copy_from_slice(&solid);
            } else {
                self.blend(index, colour, covered);
            }
            index += self.w * 4;
        }
    }

    /// [left, top, right, bottom] in pixels, its edges in part.
    fn rect(&mut self, [left, top, right, bottom]: [f32; 4], colour: [f32; 4]) {
        let (left, right) = (left.max(0.0), right.min(self.w as f32));
        let mut column = left.floor();
        while column < right {
            let covered = (right.min(column + 1.0) - left.max(column)).clamp(0.0, 1.0);
            let alpha = [colour[0], colour[1], colour[2], colour[3] * covered];
            self.column(column as usize, top, bottom, alpha);
            column += 1.0;
        }
    }

    /// `colour`, `covered` 0 to 1 of the pixel at `index`, over what is there.
    fn blend(&mut self, index: usize, [r, g, b, a]: [f32; 4], covered: f32) {
        let alpha = a * covered;
        let pixel = &mut self.rgba[index..index + 4];
        let under = f32::from(pixel[3]) / 255.0 * (1.0 - alpha);
        let out = alpha + under;
        if out <= 0.0 {
            return;
        }
        for (channel, over) in pixel[..3].iter_mut().zip([r, g, b]) {
            *channel = ((over * alpha + f32::from(*channel) * under) / out) as u8;
        }
        pixel[3] = (out * 255.0).round() as u8;
    }
}
