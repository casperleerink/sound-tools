//! Lines and rectangles a view draws: its numbers as vectors, and list feeds into pixels, which
//! change every frame without a commit, as Rune's shapes cannot.

use rune::{El, LineCap, Shape, canvas, el};
use serde::Deserialize;
use sound_core::InstanceId;
use sound_typescript::live::Num;

use crate::app::State;
use crate::feeds::ListFeed;
use crate::lives::Shows;
use crate::theme::Theme;

use super::{Scope, ShapesNode};

/// The numbers of a line or rects, some of them live, or a list feed that gives them every
/// frame.
#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Points {
    Fixed(Vec<f32>),
    Fed(ListFeed),
    Moving(Vec<Num>),
}

impl Points {
    /// Its numbers as they are now in the view of `scope`, but a list feed's.
    fn now(&self, scope: &Scope) -> Option<Vec<f32>> {
        let state = scope.state;
        match self {
            Self::Fixed(numbers) => Some(numbers.clone()),
            Self::Moving(numbers) => Some(
                numbers
                    .iter()
                    .map(|num| state.lives.value(state, scope.instance, num))
                    .collect(),
            ),
            Self::Fed(_) => None,
        }
    }

    /// Its live numbers.
    pub(super) fn moving(&self) -> &[Num] {
        match self {
            Self::Moving(numbers) => numbers,
            _ => &[],
        }
    }
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
    /// The plane it is placed in, by its key in the window, and its place there: it reads and
    /// draws only the part the plane shows.
    plane: Option<(String, [Num; 4])>,
    parts: Vec<Part>,
}

/// What of a [`Listed`] is read and drawn now: all of it, or in a plane the part that shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Seen {
    size: [Option<f32>; 2],
    units: Option<[[f32; 2]; 2]>,
    range: Option<[f64; 2]>,
    /// In a plane, where that part is in the shapes, in points: [x, y, w, h].
    rect: Option<[f32; 4]>,
}

impl Seen {
    /// How many numbers across a list of it reads: one a point of its width.
    pub(crate) fn points(&self) -> usize {
        self.size[0].map_or(0, |w| w.round().max(2.0) as usize)
    }

    pub(crate) fn range(&self) -> Option<[f64; 2]> {
        self.range
    }

    pub(crate) fn rect(&self) -> Option<[f32; 4]> {
        self.rect
    }
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

    pub(crate) fn plane(&self) -> Option<&str> {
        self.plane.as_ref().map(|(plane, _)| plane.as_str())
    }

    /// What of it is read and drawn now, in the view of `instance`: none where its plane shows
    /// none of it.
    pub(crate) fn seen(&self, state: &State, instance: &InstanceId) -> Option<Seen> {
        let all = Seen {
            size: self.size,
            units: self.units,
            range: self.range,
            rect: None,
        };
        let Some((plane, place)) = &self.plane else {
            return Some(all);
        };
        let [x, _, w, h] = place
            .each_ref()
            .map(|num| state.lives.value(state, instance, num));
        let (Some(view), [per_x, per_y]) = (state.planes.view(plane), state.planes.per(plane))
        else {
            return Some(all);
        };
        let [from, to] = [view.left, view.right].map(|edge| ((edge - x) / w).clamp(0.0, 1.0));
        if to <= from || w <= 0.0 {
            return None;
        }
        // Placed, it is in the units of its place, as its vectors are.
        let [[left, right], down] = self.units.unwrap_or([[0.0, w], [0.0, h]]);
        let across = [from, to].map(|part| left + (right - left) * part);
        let [width, height] = [w * per_x * (to - from), (h * per_y).abs()];
        Some(Seen {
            size: [Some(width), Some(height)],
            units: Some([across, down]),
            range: match [from, to] == [0.0, 1.0] {
                true => self.range,
                false => Some(across.map(f64::from)),
            },
            rect: Some([w * per_x * from, 0.0, width, height]),
        })
    }

    /// The pixels `lists` of `seen` draw into: width and height.
    pub(crate) fn pixels(seen: &Seen, lists: &[Vec<f32>]) -> (u32, u32) {
        let [w, h] = Self::extent(seen, lists).map(|points| (points * SCALE).ceil() as u32);
        (w, h)
    }

    /// The size `lists` of `seen` draw at, in points: a list is a point a number where it has
    /// none.
    fn extent(seen: &Seen, lists: &[Vec<f32>]) -> [f32; 2] {
        let longest = lists.iter().map(Vec::len).max().unwrap_or(0) as f32;
        seen.size.map(|size| size.unwrap_or(longest))
    }

    /// `lists`, one for each of its feeds, drawn as `seen`: width, height and straight RGBA of
    /// sRGB.
    pub(crate) fn raster(
        &self,
        theme: &Theme,
        seen: &Seen,
        lists: &[Vec<f32>],
    ) -> (u32, u32, Vec<u8>) {
        let [w, h] = Self::extent(seen, lists);
        let mut pixels = Raster::new((w * SCALE).ceil() as usize, (h * SCALE).ceil() as usize);
        // From the units the numbers are in to pixels, along each axis.
        let [across, down] = seen.units.unwrap_or([[0.0, w], [0.0, h]]);
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

/// The list feeds of `d`, if it has any, placed in the plane `plane` (its key in the window).
pub(crate) fn listed(d: &ShapesNode, plane: Option<&str>) -> Option<Listed> {
    let lines = (d.lines.iter()).filter_map(|lines| match &lines.points {
        Points::Fed(feed) => Some(Part {
            feed: feed.clone(),
            color: lines.color.clone(),
            line: Some((lines.width, lines.fill.clone())),
        }),
        Points::Fixed(_) | Points::Moving(_) => None,
    });
    let rects = (d.rects.iter()).filter_map(|rects| match &rects.rects {
        Points::Fed(feed) => Some(Part {
            feed: feed.clone(),
            color: rects.color.clone(),
            line: None,
        }),
        Points::Fixed(_) | Points::Moving(_) => None,
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
        plane: match (plane, &d.place) {
            (Some(plane), Some(super::Place::Box(place))) => {
                Some((plane.to_string(), place.clone()))
            }
            _ => None,
        },
        parts,
    })
}

pub(super) fn drawing(scope: &Scope, d: &ShapesNode) -> El {
    let theme = &scope.state.theme;
    let mut shape = Shape::new();
    // Points in units go from 0 to 1 across the box, which an open line fills down to. Placed,
    // they are in the units of its place.
    let units = (d.units.map(|units| units.map(Span::ends)))
        .or_else(|| (scope.place_units(d.place.as_ref())).map(|units| units.map(|to| [0.0, to])));
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
        let Some(points) = lines.points.now(scope) else {
            continue;
        };
        let colour = theme.color(&lines.color);
        let runs = runs(&points, unit);
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
        let Some(numbers) = rects.rects.now(scope) else {
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
    let moving = moving(d);
    if !moving.is_empty() {
        #[cfg(test)]
        (scope.state.lives.drawings).set(scope.state.lives.drawings.get() + 1);
        e = scope.bind(e, Shows::Drawn(moving));
    }
    if let Some(listed) = listed(d, scope.plane.as_deref()) {
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
    // Placed, the plane sizes it.
    let size = d.place.is_none().then_some((Some(&d.w), Some(&d.h)));
    scope.placed(
        e,
        d.at.as_ref(),
        size.unwrap_or_default(),
        d.opacity.as_ref(),
    )
}

/// The live numbers of the lines and rects of `d`: while one moves, it is drawn again.
pub(super) fn moving(d: &ShapesNode) -> Vec<Num> {
    let lines = d.lines.iter().map(|lines| &lines.points);
    let rects = d.rects.iter().map(|rects| &rects.rects);
    lines
        .chain(rects)
        .flat_map(Points::moving)
        .cloned()
        .collect()
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
        // Where the line crosses each column: from its left edge to its right.
        let mut spans = Vec::with_capacity(self.w);
        let mut left = y(first);
        for column in 0..self.w {
            let right = at(column as f32 + 1.0);
            spans.push((left.min(right), left.max(right), (left + right) / 2.0));
            left = right;
        }
        // As a round brush draws it: a column takes the line of each column within half its
        // width, less the further away, so a steep line is as thick as a flat one.
        let reach = half.ceil() as usize;
        for column in 0..self.w {
            if let Some(fill) = fill {
                self.column(column, spans[column].2, self.h as f32, fill);
            }
            let near = column.saturating_sub(reach)..(column + reach + 1).min(self.w);
            let (top, bottom) = (spans[near.clone()].iter().zip(near))
                .map(|(&(top, bottom, _), other)| {
                    let apart = other.abs_diff(column) as f32;
                    let thick = (half * half - apart * apart).max(0.0).sqrt();
                    (top - thick, bottom + thick)
                })
                .fold(
                    (f32::MAX, f32::MIN),
                    |(top, bottom), (each_top, each_bottom)| {
                        (top.min(each_top), bottom.max(each_bottom))
                    },
                );
            self.column(column, top, bottom, colour);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The alpha of the pixel `[x, y]` of a drawing `w` pixels wide.
    fn alpha(rgba: &[u8], w: u32, [x, y]: [u32; 2]) -> u8 {
        rgba[((y * w + x) * 4 + 3) as usize]
    }

    #[test]
    fn a_list_in_rects_is_its_rects_and_one_in_lines_stands_in_the_ends_of_its_units() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/template");
        let (theme, _) = Theme::of(std::path::Path::new(root));
        let feed = serde_json::json!({ "op": "feed", "name": "notes" });
        let node = serde_json::json!({
            "w": 10, "h": 10, "units": [10, [10, 0]],
            "rects": [{ "color": "text", "rects": feed }],
            "lines": [{ "color": "text", "width": 1, "points": feed }],
        });
        let listed = listed(&serde_json::from_value(node).unwrap(), None).unwrap();
        // The line first, then the rects: a line along 10, the top, and a rect from 0 to 5 up.
        let lists = [vec![10.0, 10.0], vec![0.0, 0.0, 5.0, 5.0]];
        let seen = Seen {
            size: listed.size,
            units: listed.units,
            range: listed.range,
            rect: None,
        };
        let (w, h, rgba) = listed.raster(&theme, &seen, &lists);
        assert_eq!((w, h), (20, 20));
        assert_eq!(alpha(&rgba, w, [10, 0]), 255, "the line at the top");
        assert_eq!(
            alpha(&rgba, w, [5, 15]),
            255,
            "inside the rect, at the bottom left"
        );
        assert_eq!(alpha(&rgba, w, [15, 15]), 0, "right of the rect");
        assert_eq!(alpha(&rgba, w, [5, 5]), 0, "above the rect, under the line");
    }

    /// The synth's attack: a line that rises a long way in a few columns is as thick across as
    /// a flat one is from top to bottom.
    #[test]
    fn a_steep_list_line_is_as_thick_as_a_flat_one() {
        let lit = |rgba: &[u8], pixels: &mut dyn Iterator<Item = usize>| {
            pixels.filter(|index| rgba[index * 4 + 3] > 128).count()
        };
        let (w, h) = (40, 200);
        let mut flat = Raster::new(w, h);
        flat.line(&[100.0, 100.0], |value| value, 4.0, [255.0; 4], None);
        let down = lit(&flat.rgba, &mut (0..h).map(|row| row * w + 20));
        let mut steep = Raster::new(w, h);
        // From the bottom to the top in 4 columns, then flat.
        let values: Vec<f32> = (0..=40)
            .map(|x| (200.0 - 50.0 * x as f32).max(0.0))
            .collect();
        steep.line(&values, |value| value, 4.0, [255.0; 4], None);
        let across = lit(&steep.rgba, &mut (0..w).map(|column| 100 * w + column));
        assert!(across + 1 >= down, "{across} across, {down} down");
    }
}
