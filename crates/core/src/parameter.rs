//! One number of a tool's saved state, with its range and its default, written once.

/// One number of the saved state `S`: its field, its range and its default. A tool writes one
/// constant per number and nothing else says the range or the default again: `validate`,
/// `Default`, the knobs of its view, what a double click resets to and a test of its docs all
/// read the constant.
///
/// This is the small part of the declarative parameters of ARCHITECTURE.md that the synth and
/// the built-in effects need. Units, knob travel and labels are the view's business.
pub struct Parameter<S> {
    /// The name of the field in the record, for messages and docs.
    pub field: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub get: fn(&S) -> f32,
    pub set: fn(&mut S, f32),
}

impl<S> Parameter<S> {
    /// An error that names the field when its value is outside the range, or not a number.
    pub fn check(&self, state: &S) -> Result<(), String> {
        let (
            Self {
                field, min, max, ..
            },
            value,
        ) = (self, (self.get)(state));
        if (*min..=*max).contains(&value) {
            return Ok(());
        }
        Err(format!("{field} must be from {min} to {max}, not {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::Parameter;

    const GAIN: Parameter<f32> = Parameter {
        field: "gain",
        min: 0.,
        max: 1.,
        default: 0.5,
        get: |state| *state,
        set: |state, value| *state = value,
    };

    #[test]
    fn a_value_outside_the_range_is_named_with_its_field() {
        assert_eq!(GAIN.check(&0.), Ok(()));
        assert_eq!(GAIN.check(&1.), Ok(()));
        assert_eq!(
            GAIN.check(&1.5),
            Err("gain must be from 0 to 1, not 1.5".into())
        );
        assert!(GAIN.check(&f32::NAN).is_err());
    }
}
