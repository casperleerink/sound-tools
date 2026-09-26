//! The math and the text of the steadiness control in the transport. Pure functions, like the
//! tempo next to it, so the drag, the keys and the readout cannot disagree about a value.
//!
//! Steadiness is one number of the project's fit: 0 % is the tempo as the take was played and
//! 100 % is one steady tempo. The control shows only when the project has a fit, so a project
//! that was never fitted has the transport it always had.

/// Percent per point of a plain drag. With shift it is a tenth, as for every drag.
pub const DRAG_PER_POINT: f64 = 1.0;
/// The step a plain drag moves by, from the value it began on. It does not snap the result.
/// With shift it is a tenth.
pub const DRAG_STEP: f64 = 1.0;
/// What one arrow key adds, plain and with shift.
pub const KEY_STEP: f64 = 5.0;
pub const FINE_KEY_STEP: f64 = 1.0;

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
    fn the_readout_is_whole_percent_and_the_saved_value_is_a_fraction() {
        assert_eq!(percent_text(0.0), "0%");
        assert_eq!(percent_text(62.0), "62%");
        assert_eq!(percent_text(100.0), "100%");
        assert_eq!(percent_of(0.5), 50.0);
        assert_eq!(steadiness_of(50.0), 0.5);
        assert_eq!(steadiness_of(percent_of(0.25)), 0.25);
    }
}
