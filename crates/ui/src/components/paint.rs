//! Shapes that controls paint on a canvas: circles, rings, arcs and lines. Angles are in degrees,
//! clockwise from the top, as on a dial.

use gpui::{
    BorderStyle, Bounds, Hsla, PathBuilder, Pixels, Point, Window, fill, outline, point, px, size,
};

/// The point at `angle` on a circle.
pub(crate) fn on_circle(centre: Point<Pixels>, radius: f32, angle: f32) -> Point<Pixels> {
    let radians = angle.to_radians();
    centre + point(px(radius * radians.sin()), px(-radius * radians.cos()))
}

fn square(centre: Point<Pixels>, radius: f32) -> Bounds<Pixels> {
    let origin = centre - point(px(radius), px(radius));
    Bounds::new(origin, size(px(radius * 2.), px(radius * 2.)))
}

pub(crate) fn circle(window: &mut Window, centre: Point<Pixels>, radius: f32, color: Hsla) {
    window.paint_quad(fill(square(centre, radius), color).corner_radii(px(radius)));
}

/// A ring whose outer edge has this radius.
pub(crate) fn ring(
    window: &mut Window,
    centre: Point<Pixels>,
    radius: f32,
    width: f32,
    color: Hsla,
) {
    let quad = outline(square(centre, radius), color, BorderStyle::Solid);
    window.paint_quad(quad.corner_radii(px(radius)).border_widths(px(width)));
}

/// A stroke along the circle from one angle to the other, either way round. With `round`, its
/// ends are round, which only an opaque colour can have: the caps are circles painted over the
/// ends, and a see-through colour would show where they overlap.
pub(crate) fn arc(
    window: &mut Window,
    centre: Point<Pixels>,
    radius: f32,
    width: f32,
    (from, to): (f32, f32),
    color: Hsla,
    round: bool,
) {
    if (to - from).abs() < 0.01 {
        return;
    }
    let (start, end) = (
        on_circle(centre, radius, from),
        on_circle(centre, radius, to),
    );
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(start);
    let radii = point(px(radius), px(radius));
    // Clockwise on the screen is the positive way round of SVG, with y down.
    path.arc_to(radii, px(0.), (to - from).abs() > 180., to > from, end);
    // A path that does not tessellate paints nothing, which is all there is to do about it.
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
    if round {
        circle(window, start, width / 2., color);
        circle(window, end, width / 2., color);
    }
}

/// A straight stroke with round ends, for an opaque colour, as [`arc`].
pub(crate) fn line(
    window: &mut Window,
    from: Point<Pixels>,
    to: Point<Pixels>,
    width: f32,
    color: Hsla,
) {
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(from);
    path.line_to(to);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
    circle(window, from, width / 2., color);
    circle(window, to, width / 2., color);
}

/// A waveform mirrored about `middle` as one shape: `reaches` are how far it goes up and down at
/// columns `column` wide from `left`. One path and not a quad per column, so a screen full of
/// waveforms stays a few hundred shapes for the scene, whatever the zoom.
pub(crate) fn mirrored_waveform(
    window: &mut Window,
    (left, middle): (f32, f32),
    column: f32,
    reaches: &[f32],
    color: Hsla,
) {
    let (Some(first), Some(last)) = (reaches.first(), reaches.last()) else {
        return;
    };
    let at = |x: f32, y: f32| point(px(x), px(y));
    let right = left + column * reaches.len() as f32;
    let mut shape = PathBuilder::fill();
    shape.move_to(at(left, middle - first));
    for (index, reach) in reaches.iter().enumerate() {
        shape.line_to(at(left + column * (index as f32 + 0.5), middle - reach));
    }
    shape.line_to(at(right, middle - last));
    shape.line_to(at(right, middle + last));
    for (index, reach) in reaches.iter().enumerate().rev() {
        shape.line_to(at(left + column * (index as f32 + 0.5), middle + reach));
    }
    shape.line_to(at(left, middle + first));
    shape.close();
    // A shape that does not tessellate paints nothing, which is all there is to do about it.
    if let Ok(path) = shape.build() {
        window.paint_path(path, color);
    }
}
