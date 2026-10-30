//! The shapes of a space in points: painted in as few layers as their order allows, and found
//! under the pointer.

use std::rc::Rc;

use gpui::{
    App, Bounds, Hitbox, HitboxBehavior, Hsla, Pixels, Point, SharedString, TextAlign, TextRun,
    Window, fill, outline, point, px, size,
};
use sound_ui::typography;

use crate::tree::{Align, Cursor, Weight};

use super::shapes::{Area, Bend, Geometry, Notes, TextPart, Wave};
use super::space::SpaceFrame;

/// A shape placed in points, as it paints and as the pointer finds it.
pub(super) struct Painted {
    index: usize,
    shape: PaintedShape,
    color: Hsla,
    hitbox: Option<(Hitbox, Cursor)>,
    /// What of the window it shows in, inside the box of its groups.
    clip: Option<Bounds<Pixels>>,
}

enum PaintedShape {
    Rect(Bounds<Pixels>, f32, Option<(Hsla, f32)>),
    Circle(Point<Pixels>, f32),
    Line(Point<Pixels>, Point<Pixels>, f32),
    Arc(Point<Pixels>, f32, (f32, f32), f32),
    Text(Point<Pixels>, SharedString, Rc<[TextPart]>, f32, Align),
    Wave(Bounds<Pixels>, Rc<Wave>),
    Notes(Bounds<Pixels>, Rc<Notes>),
    Path(Rc<[Bend<Point<Pixels>>]>, Option<f32>),
}

impl PaintedShape {
    fn bounds(&self) -> Bounds<Pixels> {
        let square = |centre: Point<Pixels>, radius: f32| {
            Bounds::new(
                centre - point(px(radius), px(radius)),
                size(px(radius * 2.), px(radius * 2.)),
            )
        };
        match self {
            Self::Rect(bounds, ..) | Self::Wave(bounds, _) | Self::Notes(bounds, _) => *bounds,
            Self::Path(bends, outline) => {
                let (mut low, mut high) = (
                    point(px(f32::MAX), px(f32::MAX)),
                    point(px(f32::MIN), px(f32::MIN)),
                );
                for each in bends.iter().flat_map(|bend| bend.points()) {
                    low = point(low.x.min(each.x), low.y.min(each.y));
                    high = point(high.x.max(each.x), high.y.max(each.y));
                }
                let pad = outline.map_or(px(0.), |width| px(width / 2. + 2.));
                match bends.is_empty() {
                    true => Bounds::default(),
                    false => Bounds::from_corners(low - point(pad, pad), high + point(pad, pad)),
                }
            }
            Self::Circle(centre, radius) => square(*centre, *radius),
            Self::Arc(centre, radius, _, width) => square(*centre, radius + width / 2.),
            Self::Line(from, to, width) => {
                let (left, top) = (from.x.min(to.x), from.y.min(to.y));
                let (right, bottom) = (from.x.max(to.x), from.y.max(to.y));
                let pad = px(width / 2. + 2.);
                Bounds::from_corners(
                    point(left - pad, top - pad),
                    point(right + pad, bottom + pad),
                )
            }
            Self::Text(origin, text, _, font, align) => {
                let width = px(text.len() as f32 * font * 0.6);
                let left = match align {
                    Align::Left => origin.x,
                    Align::Center => origin.x - width / 2.,
                    Align::Right => origin.x - width,
                };
                Bounds::new(point(left, origin.y), size(width, px(font * 1.3)))
            }
        }
    }

    /// Where GPUI paints its primitives among those of the same order: quads, paths, text.
    fn batch_rank(&self) -> u8 {
        match self {
            Self::Rect(..) | Self::Circle(..) | Self::Notes(..) => 0,
            Self::Line(..) | Self::Arc(..) | Self::Wave(..) | Self::Path(..) => 1,
            Self::Text(..) => 2,
        }
    }

    /// Whether GPUI paints it as a filled path: a band of a wave, or a path.
    fn is_filled_path(&self) -> bool {
        match self {
            Self::Wave(_, wave) => wave.band,
            Self::Path(_, outline) => outline.is_none(),
            _ => false,
        }
    }

    fn contains(&self, position: Point<Pixels>) -> bool {
        match self {
            Self::Circle(centre, radius) => {
                let (dx, dy) = (
                    f32::from(position.x - centre.x),
                    f32::from(position.y - centre.y),
                );
                dx * dx + dy * dy <= radius * radius
            }
            _ => self.bounds().contains(&position),
        }
    }
}

/// The shapes of a frame in points, those that show, with a hitbox for each that shows a cursor.
pub(super) fn prepaint(
    frame: &SpaceFrame,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) -> (Hitbox, Rc<Vec<Painted>>) {
    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
    let mut frame_view = frame.view;
    frame_view.width = f64::from(f32::from(bounds.size.width));
    frame_view.height = f64::from(f32::from(bounds.size.height));
    let (across, down) = frame_view.scale();
    let at = |(x, y): (f64, f64)| frame.point_at(bounds, (x, y));
    let mut painted = Vec::new();
    for placed in &frame.placed {
        let shape = match &placed.geometry {
            Geometry::Rect {
                x,
                y,
                width,
                height,
                radius,
                border,
            } => {
                let origin = at((*x, *y));
                let size = size(px((width * across) as f32), px((height * down) as f32));
                PaintedShape::Rect(Bounds::new(origin, size), *radius, *border)
            }
            Geometry::Circle { x, y, radius } => PaintedShape::Circle(at((*x, *y)), *radius),
            Geometry::Line { from, to, width } => PaintedShape::Line(at(*from), at(*to), *width),
            Geometry::Arc {
                x,
                y,
                radius,
                turns,
                width,
            } => PaintedShape::Arc(at((*x, *y)), *radius, *turns, *width),
            Geometry::Text {
                x,
                y,
                text,
                parts,
                size,
                align,
            } => PaintedShape::Text(at((*x, *y)), text.clone(), parts.clone(), *size, *align),
            Geometry::Wave {
                x,
                y,
                width,
                height,
                wave,
            } => {
                let size = size(px((width * across) as f32), px((height * down) as f32));
                PaintedShape::Wave(Bounds::new(at((*x, *y)), size), wave.clone())
            }
            Geometry::Notes {
                x,
                y,
                width,
                height,
                notes,
            } => {
                let size = size(px((width * across) as f32), px((height * down) as f32));
                PaintedShape::Notes(Bounds::new(at((*x, *y)), size), notes.clone())
            }
            Geometry::Path { bends, outline } => {
                PaintedShape::Path(bends.iter().map(|bend| bend.map(at)).collect(), *outline)
            }
        };
        let clip = (placed.clip).map(|Area([left, top, right, bottom])| {
            Bounds::from_corners(at((left, top)), at((right, bottom))).intersect(&bounds)
        });
        let shown = clip.unwrap_or(bounds).intersect(&shape.bounds());
        // What does not show is not painted and not found.
        if shown.is_empty() {
            continue;
        }
        let cursor = (frame.pointed.get(placed.index))
            .and_then(Option::as_ref)
            .and_then(|pointer| pointer.cursor);
        let hitbox =
            cursor.map(|cursor| (window.insert_hitbox(shown, HitboxBehavior::Normal), cursor));
        painted.push(Painted {
            index: placed.index,
            shape,
            color: placed.color,
            hitbox,
            clip,
        });
    }
    (hitbox, Rc::new(painted))
}

pub(super) fn paint(
    frame: &SpaceFrame,
    bounds: Bounds<Pixels>,
    hitbox: &Hitbox,
    painted: &[Painted],
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(background) = frame.background {
        window.paint_quad(fill(bounds, background).corner_radii(px(frame.radius)));
    }
    if let Some(cursor) = frame.cursor {
        window.set_cursor_style(cursor.style(), hitbox);
    }
    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        // GPUI orders each primitive by what it overlaps, which costs most of a frame of many
        // shapes. Inside a layer they share one order and paint as their kinds come: quads, then
        // paths, then text. So a run of shapes whose kinds come in that order is one layer, and a
        // shape of an earlier kind starts the next one, which paints over the run before.
        // A filled path ends its layer too: GPUI draws the paths of one layer over each other's
        // boxes, so a path over another one would show the one under it twice.
        let mut runs: Vec<&[Painted]> = Vec::new();
        let mut start = 0;
        let mut latest = 0;
        let mut filled = false;
        for (index, each) in painted.iter().enumerate() {
            let rank = each.shape.batch_rank();
            if rank < latest || filled {
                runs.extend(painted.get(start..index));
                start = index;
            }
            latest = rank;
            filled = each.shape.is_filled_path();
        }
        runs.extend(painted.get(start..));
        for run in runs {
            window.paint_layer(bounds, |window| {
                for each in run {
                    // A shape with no colour is there for the pointer, such as the handle at the
                    // end of a note.
                    if each.color.a > 0.0 {
                        match each.clip {
                            Some(clip) => window.with_content_mask(
                                Some(gpui::ContentMask { bounds: clip }),
                                |window| paint_shape(&each.shape, each.color, window, cx),
                            ),
                            None => paint_shape(&each.shape, each.color, window, cx),
                        }
                    }
                    if let Some((hitbox, cursor)) = &each.hitbox {
                        window.set_cursor_style(cursor.style(), hitbox);
                    }
                }
            });
        }
    });
}

fn paint_shape(shape: &PaintedShape, color: Hsla, window: &mut Window, cx: &mut App) {
    match shape {
        PaintedShape::Rect(bounds, radius, border) => {
            window.paint_quad(fill(*bounds, color).corner_radii(px(*radius)));
            if let Some((border, width)) = border {
                let quad = outline(*bounds, *border, gpui::BorderStyle::Solid);
                window.paint_quad(quad.corner_radii(px(*radius)).border_widths(px(*width)));
            }
        }
        PaintedShape::Circle(centre, radius) => {
            let bounds = Bounds::new(
                *centre - point(px(*radius), px(*radius)),
                size(px(radius * 2.), px(radius * 2.)),
            );
            window.paint_quad(fill(bounds, color).corner_radii(px(*radius)));
        }
        PaintedShape::Line(from, to, width) => {
            let mut line = gpui::PathBuilder::stroke(px(*width));
            line.move_to(*from);
            line.line_to(*to);
            if let Ok(path) = line.build() {
                window.paint_path(path, color);
            }
        }
        PaintedShape::Arc(centre, radius, (from, to), width) => {
            if (to - from).abs() < 0.0001 {
                return;
            }
            let on_circle = |turns: f32| {
                let radians = turns * std::f32::consts::TAU;
                *centre + point(px(radius * radians.sin()), px(-radius * radians.cos()))
            };
            let mut path = gpui::PathBuilder::stroke(px(*width));
            path.move_to(on_circle(*from));
            // Clockwise on the screen is the positive way round of SVG, with y down.
            let radii = point(px(*radius), px(*radius));
            path.arc_to(
                radii,
                px(0.),
                (to - from).abs() > 0.5,
                to > from,
                on_circle(*to),
            );
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        PaintedShape::Wave(bounds, wave) => paint_wave(*bounds, wave, color, window),
        PaintedShape::Notes(bounds, notes) => paint_notes(*bounds, notes, color, window),
        PaintedShape::Path(bends, outline) => {
            let mut path = match outline {
                Some(width) => gpui::PathBuilder::stroke(px(*width)),
                None => gpui::PathBuilder::fill(),
            };
            for (index, bend) in bends.iter().enumerate() {
                match (index, *bend) {
                    (0, bend) => path.move_to(bend.end()),
                    (_, Bend::To(to)) => path.line_to(to),
                    (_, Bend::Quadratic(control, to)) => path.curve_to(to, control),
                    (_, Bend::Cubic(first, second, to)) => path.cubic_bezier_to(to, first, second),
                }
            }
            let ends_at_start =
                bends.first().map(|bend| bend.end()) == bends.last().map(|bend| bend.end());
            if outline.is_none() || ends_at_start {
                path.close();
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        PaintedShape::Text(origin, text, parts, font_size, align) => {
            let run = |len: usize, color: Hsla, weight: Option<Weight>| TextRun {
                len,
                font: gpui::Font {
                    weight: match weight {
                        None | Some(Weight::Normal) => gpui::FontWeight::NORMAL,
                        Some(Weight::Medium) => gpui::FontWeight::MEDIUM,
                        Some(Weight::Semibold) => gpui::FontWeight::SEMIBOLD,
                    },
                    ..typography::ui_font()
                },
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs: Vec<TextRun> = match parts.is_empty() {
                true => vec![run(text.len(), color, None)],
                false => (parts.iter())
                    .filter(|part| part.len > 0)
                    .map(|part| run(part.len, part.color.unwrap_or(color), part.weight))
                    .collect(),
            };
            let mut at = 0;
            let runs: Vec<TextRun> = (runs.iter())
                .flat_map(|run| {
                    let part = text.get(at..at + run.len).unwrap_or_default();
                    at += run.len;
                    typography::tabular_runs(part, run)
                })
                .collect();
            let shaped = window
                .text_system()
                .shape_line(text.clone(), px(*font_size), &runs, None);
            let left = match align {
                Align::Left => origin.x,
                Align::Center => origin.x - shaped.width / 2.,
                Align::Right => origin.x - shaped.width,
            };
            let line_height = px(font_size * 1.3);
            // Parts next to each other with one border share a box.
            let mut boxes: Vec<(usize, usize, Hsla)> = Vec::new();
            let mut start = 0;
            for part in parts.iter() {
                let end = start + part.len;
                match (part.border, boxes.last_mut()) {
                    (Some(border), Some(last)) if last.1 == start && last.2 == border => {
                        last.1 = end
                    }
                    (Some(border), _) => boxes.push((start, end, border)),
                    (None, _) => {}
                }
                start = end;
            }
            for (start, end, border) in boxes {
                let (from, to) = (shaped.x_for_index(start), shaped.x_for_index(end));
                let bounds = Bounds::from_corners(
                    point(left + from - px(BOXED.0), origin.y - px(BOXED.1)),
                    point(
                        left + to + px(BOXED.0),
                        origin.y + line_height + px(BOXED.1),
                    ),
                );
                let back = border.alpha(border.a * 0.15);
                window.paint_quad(fill(bounds, back).corner_radii(px(4.)));
                let quad = outline(bounds, border, gpui::BorderStyle::Solid);
                window.paint_quad(quad.corner_radii(px(4.)).border_widths(px(1.)));
            }
            if let Err(error) = shaped.paint(
                point(left, origin.y),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            ) {
                eprintln!("error: a label of a card: {error}");
            }
        }
    }
}

/// Paints each note a line across `bounds`, 2 to 4 points thick, and a point long at least.
fn paint_notes(bounds: Bounds<Pixels>, notes: &Notes, color: Hsla, window: &mut Window) {
    let step = f32::from(bounds.size.height) / notes.shown;
    let thick = step.clamp(2.0, 4.0);
    let foot = f32::from(bounds.bottom()) - notes.under * step;
    let width = f32::from(bounds.size.width);
    for (from, to, above) in &notes.lines {
        let middle = foot - (above + 0.5) * step;
        let left = bounds.origin.x + px(width * from);
        let length = px((width * (to - from)).max(1.0));
        let line = Bounds::new(
            point(left, px(middle - thick / 2.0)),
            size(length, px(thick)),
        );
        window.paint_quad(gpui::fill(line, color));
    }
}

/// Paints a wave across `bounds`: a line through one value per point, or a band from the lowest
/// to the highest value under each point.
fn paint_wave(bounds: Bounds<Pixels>, wave: &Wave, color: Hsla, window: &mut Window) {
    let values = &wave.values;
    let columns = (f32::from(bounds.size.width).round() as usize).clamp(2, 4096);
    if values.len() < 2 {
        return;
    }
    let span = wave.high - wave.low;
    let height = f32::from(bounds.size.height);
    let y_of = |value: f32| {
        let along = if span == 0.0 {
            0.5
        } else {
            (value - wave.low) / span
        };
        bounds.origin.y + px(height * (1.0 - along.clamp(0.0, 1.0)))
    };
    let x_of = |column: usize, of: usize| {
        bounds.origin.x + bounds.size.width * (column as f32 / (of - 1).max(1) as f32)
    };
    let columns = columns.min(values.len());
    // The values under each column.
    let under = |column: usize| {
        let start = column * values.len() / columns;
        let end = ((column + 1) * values.len() / columns).max(start + 1);
        values.get(start..end.min(values.len())).unwrap_or_default()
    };
    if wave.band {
        // One shape through the middle of each column, top edge then bottom edge back, so its
        // outline is smooth.
        let middle = (wave.low + wave.high) / 2.0;
        let edges: Vec<(Pixels, Pixels, Pixels)> = (0..columns)
            .map(|column| {
                let under = under(column);
                let fraction = (column as f32 + 0.5) / columns as f32;
                let factor = wave.level_at(fraction);
                let scaled = |value: f32| middle + (value - middle) * factor;
                let low = scaled(under.iter().copied().fold(f32::INFINITY, f32::min));
                let high = scaled(under.iter().copied().fold(f32::NEG_INFINITY, f32::max));
                let (top, bottom) = (y_of(high), y_of(low));
                // At least a point thick, so silence shows as a line.
                let thin = (px(1.0) - (bottom - top)).max(px(0.0)) / 2.0;
                let x = bounds.origin.x + bounds.size.width * fraction;
                (x, top - thin, bottom + thin)
            })
            .collect();
        let (Some(first), Some(last)) = (edges.first(), edges.last()) else {
            return;
        };
        let right = bounds.origin.x + bounds.size.width;
        let mut shape = gpui::PathBuilder::fill();
        shape.move_to(point(bounds.origin.x, first.1));
        for (x, top, _) in &edges {
            shape.line_to(point(*x, *top));
        }
        shape.line_to(point(right, last.1));
        shape.line_to(point(right, last.2));
        for (x, _, bottom) in edges.iter().rev() {
            shape.line_to(point(*x, *bottom));
        }
        shape.line_to(point(bounds.origin.x, first.2));
        shape.close();
        if let Ok(path) = shape.build() {
            window.paint_path(path, color);
        }
        return;
    }
    let mut path = gpui::PathBuilder::stroke(px(wave.line));
    let middle = (wave.low + wave.high) / 2.0;
    for column in 0..columns {
        let value = under(column).first().copied().unwrap_or_default();
        let factor = wave.level_at(column as f32 / (columns - 1).max(1) as f32);
        let value = middle + (value - middle) * factor;
        let at = point(x_of(column, columns), y_of(value));
        if column == 0 {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The topmost shape at `position` that hears the pointer: its place among the shapes.
pub(super) fn found(
    frame: &SpaceFrame,
    painted: &[Painted],
    position: Point<Pixels>,
) -> Option<usize> {
    (painted.iter().rev())
        .filter(|each| frame.pointed.get(each.index).is_some_and(Option::is_some))
        .find(|each| {
            each.shape.contains(position) && each.clip.is_none_or(|clip| clip.contains(&position))
        })
        .map(|each| each.index)
}

/// How far a boxed part's box reaches past its text, across and down.
const BOXED: (f32, f32) = (5., 2.);
