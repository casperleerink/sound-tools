//! The math and the text of the steadiness control in the transport. Pure functions, like the
//! tempo next to it, so the drag, the keys and the readout cannot disagree about a value.
//!
//! Steadiness is one number of the project's fit: 0 % is the tempo as the take was played and
//! 100 % is one steady tempo. The control shows only when the project has a fit, so a project
//! that was never fitted has the transport it always had.

/// Percent per pixel of a plain drag, and of a fine drag with shift.
const DRAG_PER_PIXEL: f64 = 1.0;
const FINE_DRAG_PER_PIXEL: f64 = 0.2;
/// The step a drag moves by, from the value it began on. It does not snap the result.
const DRAG_STEP: f64 = 1.0;
const FINE_DRAG_STEP: f64 = 0.2;
/// What one arrow key adds, plain and with shift.
pub const KEY_STEP: f64 = 5.0;
pub const FINE_KEY_STEP: f64 = 1.0;

/// The steadiness a drag of `pixels` up from `start` asks for, as a percentage from 0 to 100.
/// It moves by whole steps from where it began, as a tempo drag does, so a drag there and back
/// ends on exactly the value it started from.
pub fn dragged_percent(start: f64, pixels: f32, fine: bool) -> f64 {
    let (per_pixel, step) = match fine {
        true => (FINE_DRAG_PER_PIXEL, FINE_DRAG_STEP),
        false => (DRAG_PER_PIXEL, DRAG_STEP),
    };
    let steps = (f64::from(pixels) * per_pixel / step).round();
    (start + steps * step).clamp(0.0, 100.0)
}

/// The percentage as the transport shows it: whole percent, so the readout is short and the
/// pill does not move while it is dragged.
pub fn percent_text(percent: f64) -> String {
    format!("{}%", percent.round() as i64)
}

/// A saved steadiness, 0 to 1, as the percentage the control works in.
pub fn percent_of(steadiness: f32) -> f64 {
    f64::from(steadiness.clamp(0.0, 1.0)) * 100.0
}

/// The percentage as a saved steadiness, 0 to 1.
pub fn steadiness_of(percent: f64) -> f32 {
    (percent.clamp(0.0, 100.0) / 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_moves_by_whole_percent_from_where_it_began_and_stops_at_the_ends() {
        assert_eq!(dragged_percent(0.0, 0.0, false), 0.0);
        assert_eq!(dragged_percent(0.0, 25.0, false), 25.0);
        assert_eq!(dragged_percent(40.0, -10.0, false), 30.0);
        assert_eq!(dragged_percent(40.0, 0.4, false), 40.0);
        // Past an end it stops there, and the value it began on comes back.
        assert_eq!(dragged_percent(0.0, -50.0, false), 0.0);
        assert_eq!(dragged_percent(90.0, 50.0, false), 100.0);
        // A fine drag steps by a fifth of a percent and needs five times the travel.
        assert_eq!(dragged_percent(10.0, 5.0, true), 11.0);
        assert_eq!(dragged_percent(10.0, 0.0, true), 10.0);
    }

    #[test]
    fn the_readout_is_whole_percent_and_the_saved_value_is_a_fraction() {
        assert_eq!(percent_text(0.0), "0%");
        assert_eq!(percent_text(62.0), "62%");
        assert_eq!(percent_text(100.0), "100%");
        assert_eq!(percent_of(0.5), 50.0);
        assert_eq!(steadiness_of(50.0), 0.5);
        assert_eq!(steadiness_of(percent_of(0.25)), 0.25);
    }
}
