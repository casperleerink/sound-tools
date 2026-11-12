//! The soft edge of a div that scrolls: what it scrolls fades into the surface under it on each
//! side where more waits. Painted from what the layout of the div gives, so it costs a frame
//! nothing: a page paints it on the layer over itself, as the cards and meters of a row that
//! scrolls draw there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, ContentMask, Hsla, Pixels, ScrollHandle, Window, canvas, fill,
    linear_color_stop, linear_gradient, point, prelude::*, px, size,
};

use crate::tree::ScrollAxis;

/// How long an edge fades at most, in points.
pub(super) const FADE: f32 = 24.;

/// The edges a page paints over itself, by the name of what the div scrolls in.
pub(super) type Edges = Rc<RefCell<HashMap<String, Edge>>>;

/// Where a div scrolls in the window, and how much of what it scrolls waits past each end.
#[derive(Clone, Copy)]
pub(super) struct Edge {
    bounds: Bounds<Pixels>,
    /// Where the page cuts the div off, as a div it scrolls in does.
    mask: Bounds<Pixels>,
    axis: ScrollAxis,
    waits: [Pixels; 2],
    ground: Hsla,
}

impl Edge {
    /// As the layout of this frame left `scroller`, read while what it scrolls is laid out.
    fn of(scroller: &ScrollHandle, axis: ScrollAxis, ground: Hsla, window: &Window) -> Self {
        let (offset, most) = (scroller.offset(), scroller.max_offset());
        let (before, all) = match axis {
            ScrollAxis::X => (-offset.x, most.x),
            ScrollAxis::Y => (-offset.y, most.y),
        };
        Self {
            bounds: scroller.bounds(),
            mask: window.content_mask().bounds,
            axis,
            waits: [before, all - before],
            ground,
        }
    }

    #[cfg(feature = "test-support")]
    pub(super) fn room(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// How much of [`FADE`] it fades at its start and at its end: as much as waits there, so
    /// nothing fades where nothing waits and the fade grows as the div scrolls away from an end.
    pub(super) fn fades(&self) -> [f32; 2] {
        self.waits.map(|waits| (waits / px(FADE)).clamp(0., 1.))
    }

    pub(super) fn paint(&self, window: &mut Window) {
        let Bounds { origin, size: room } = self.bounds;
        let along = match self.axis {
            ScrollAxis::X => room.width,
            ScrollAxis::Y => room.height,
        };
        let [start, end] = (self.fades()).map(|fades| (px(FADE) * fades).min(along / 2.));
        // Each from the edge it lies at, with the way it runs out as the angle of a gradient:
        // 0 is up, 90 to the right.
        let sides = match self.axis {
            ScrollAxis::X => [
                (origin, size(start, room.height), 90.),
                (
                    point(origin.x + along - end, origin.y),
                    size(end, room.height),
                    270.,
                ),
            ],
            ScrollAxis::Y => [
                (origin, size(room.width, start), 180.),
                (
                    point(origin.x, origin.y + along - end),
                    size(room.width, end),
                    0.,
                ),
            ],
        };
        let mask = ContentMask { bounds: self.mask };
        window.with_content_mask(Some(mask), |window| {
            for (at, side, angle) in sides {
                if side.width <= px(0.) || side.height <= px(0.) {
                    continue;
                }
                let fade = linear_gradient(
                    angle,
                    linear_color_stop(self.ground, 0.),
                    linear_color_stop(self.ground.opacity(0.), 1.),
                );
                window.paint_quad(fill(Bounds::new(at, side), fade));
            }
        });
    }
}

/// The soft edges of the div that scrolls with `scroller` in the box `name`, for that box,
/// after what it scrolls. With `over`, the layer over the page paints them, see [`Edges`].
pub(super) fn soft_edges(
    name: String,
    scroller: ScrollHandle,
    axis: ScrollAxis,
    ground: Hsla,
    over: Option<Edges>,
) -> AnyElement {
    canvas(
        move |_, window, _| {
            let edge = Edge::of(&scroller, axis, ground, window);
            match over {
                Some(edges) => {
                    edges.borrow_mut().insert(name.clone(), edge);
                    None
                }
                None => Some(edge),
            }
        },
        |_, edge, window, _| {
            if let Some(edge) = edge {
                edge.paint(window);
            }
        },
    )
    .absolute()
    .into_any_element()
}

/// Paints the edges a page left for the layer over it, as they are when it paints.
pub(super) fn painted_over(edges: Edges) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            for edge in edges.borrow().values() {
                edge.paint(window);
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}
