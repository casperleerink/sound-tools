//! A press that may become a drag.

use super::{Axis, DragSpec};

impl DragSpec {
    /// Where a drag by `dx`, `dy` leaves the element: along its axis, within its bounds in
    /// either order, as a view may give them swapped.
    pub(super) fn offset(self, dx: f32, dy: f32) -> [f32; 2] {
        let (low, high) = (self.min.min(self.max), self.min.max(self.max));
        match self.axis {
            Axis::X => [dx.clamp(low, high), 0.0],
            Axis::Y => [0.0, dy.clamp(low, high)],
        }
    }
}

/// How far the pointer travels before a press is a drag, in points: a drag never also presses.
const DRAG_SLOP: f32 = 3.0;
/// How much of the pointer's travel a drag moves with Shift held.
const FINE: f32 = 0.1;

/// A press on an element that may become a drag.
pub(crate) struct Gesture {
    pub(super) key: String,
    /// Where the pointer was from the press, and how far it moved the element: with Shift held,
    /// a tenth as far.
    last: [f32; 2],
    pub(super) moved: [f32; 2],
    /// It went past the slop: a drag, and no press.
    pub(super) far: bool,
}

impl Gesture {
    /// The element a drag moves now, once past the slop.
    pub(crate) fn dragging(&self) -> Option<&str> {
        self.far.then_some(self.key.as_str())
    }

    pub(super) fn of(key: &str) -> Self {
        Self {
            key: key.to_string(),
            last: [0.0; 2],
            moved: [0.0; 2],
            far: false,
        }
    }

    pub(super) fn follow(&mut self, d: &rune::Drag, fine: bool) {
        let by = if fine { FINE } else { 1.0 };
        self.moved[0] += (d.dx - self.last[0]) * by;
        self.moved[1] += (d.dy - self.last[1]) * by;
        self.last = [d.dx, d.dy];
        self.far |= far(d);
    }
}

fn far(d: &rune::Drag) -> bool {
    d.dx.abs().max(d.dy.abs()) > DRAG_SLOP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_with_bounds_swapped_stays_between_them() {
        let spec = DragSpec {
            axis: Axis::Y,
            min: 4.0,
            max: -4.0,
        };
        assert_eq!(spec.offset(0.0, 10.0), [0.0, 4.0]);
        assert_eq!(spec.offset(0.0, -10.0), [0.0, -4.0]);
    }
}
