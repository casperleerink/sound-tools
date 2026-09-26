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
