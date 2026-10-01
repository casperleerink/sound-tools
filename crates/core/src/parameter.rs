//! One number of a tool's saved state, with its range, its default and its scale, written once.

/// One number of the saved state `S`: its field, its range, its default and its scale. A tool
/// writes one constant per number and nothing else says them again: `validate`, `Default`, the
/// knobs of its view, what a double click resets to, an automation lane and a test of its docs
/// all read the constant.
///
/// This is the small part of the declarative parameters of ARCHITECTURE.md that the synth and
/// the built-in effects need. Units and labels are the view's business.
pub struct Parameter<S> {
    /// The name of the field in the record, for messages and docs.
    pub field: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// How the range spreads over the travel of a knob, and so how an automation lane moves
    /// between two points.
    pub scale: Scale,
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
        if self.info().contains(value) {
            return Ok(());
        }
        Err(format!("{field} must be from {min} to {max}, not {value}"))
    }

    /// The parameter without its state type, for an owner that knows the tool only by its
    /// ports, such as the arrangement that automates the devices of a track.
    pub const fn info(&self) -> ParameterInfo {
        ParameterInfo {
            field: self.field,
            min: self.min,
            max: self.max,
            scale: self.scale,
        }
    }
}

/// A [`Parameter`] without its state type: its field, its range and its scale.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ParameterInfo {
    pub field: &'static str,
    pub min: f32,
    pub max: f32,
    pub scale: Scale,
}

impl ParameterInfo {
    /// Whether `value` is in the range. Not a number is not.
    pub fn contains(&self, value: f32) -> bool {
        (self.min..=self.max).contains(&value)
    }

    /// Where `value` is on the travel, from 0 to 1. See [`Scale::position`].
    pub fn position(&self, value: f32) -> f32 {
        self.scale.position(self.min, self.max, value)
    }

    /// The value at a place on the travel. See [`Scale::value`].
    pub fn value(&self, position: f32) -> f32 {
        self.scale.value(self.min, self.max, position)
    }
}

/// Where 0 dB sits on the travel of [`Scale::Fader`].
pub const FADER_UNITY: f32 = 0.8;
/// Decibels of one tenfold step of the place on the travel of [`Scale::Fader`], so that +6 dB
/// is at the top.
const FADER_DECADE: f32 = 61.94;

/// How the values of a range spread over the travel of a knob, from 0 to 1. An automation
/// lane is a straight line on the travel, so a sweep of a cutoff sounds even.
///
/// No method here panics, whatever the numbers, so the audio thread can call them.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Scale {
    Linear,
    /// Equal travel for equal ratios: for frequencies and times. The range must be above zero.
    Logarithmic,
    /// Decibels on the volume of a track: `-inf` at the bottom, 0 dB at 80 % of the travel and
    /// +6 dB at the top. A power law on the amplitude, which gives the lower decibels room and
    /// reaches the bottom at `-inf`. The meters draw on the same scale.
    Fader,
}

impl Scale {
    /// Where `value` is on the travel of `min` to `max`, from 0 to 1. A value outside the
    /// range is at an end, and not a number is at the bottom.
    pub fn position(self, min: f32, max: f32, value: f32) -> f32 {
        // Not `clamp`: it panics on a NaN, and nothing may panic on the audio thread.
        let held = value.max(min).min(max);
        let position = match self {
            Self::Linear => (held - min) / (max - min),
            Self::Logarithmic => (held / min).ln() / (max / min).ln(),
            Self::Fader if value.is_nan() => 0.,
            // The decade is rounded, so the top would be a hair under the end.
            Self::Fader if value >= max => 1.,
            Self::Fader => FADER_UNITY * 10_f32.powf(value / FADER_DECADE),
        };
        position.clamp(0., 1.)
    }

    /// The value at a place on the travel of `min` to `max`. The ends are exact, and a place
    /// outside the travel is at an end.
    pub fn value(self, min: f32, max: f32, position: f32) -> f32 {
        let position = match position.is_nan() {
            true => 0.,
            false => position.clamp(0., 1.),
        };
        match self {
            Self::Linear => min + (max - min) * position,
            Self::Logarithmic => min * (max / min).powf(position),
            Self::Fader if position <= 0. => f32::NEG_INFINITY,
            Self::Fader => (FADER_DECADE * (position / FADER_UNITY).log10()).min(max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Parameter, Scale};

    const GAIN: Parameter<f32> = Parameter {
        field: "gain",
        min: 0.,
        max: 1.,
        default: 0.5,
        scale: Scale::Linear,
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

    #[test]
    fn the_ends_of_the_travel_are_the_ends_of_the_range() {
        for (scale, min, max) in [
            (Scale::Linear, -50., 50.),
            (Scale::Logarithmic, 20., 20_000.),
            (Scale::Fader, f32::NEG_INFINITY, 6.),
        ] {
            assert_eq!(scale.value(min, max, 0.), min, "{scale:?}");
            assert_eq!(scale.value(min, max, 1.), max, "{scale:?}");
            assert_eq!(scale.position(min, max, min), 0., "{scale:?}");
            assert_eq!(scale.position(min, max, max), 1., "{scale:?}");
            assert_eq!(scale.position(min, max, f32::NAN), 0., "{scale:?}");
            assert_eq!(scale.value(min, max, f32::NAN), min, "{scale:?}");
        }
    }

    #[test]
    fn the_middle_of_a_logarithmic_travel_is_the_middle_ratio() {
        let middle = Scale::Logarithmic.value(20., 20_000., 0.5);
        assert!((middle - 632.456).abs() < 0.01, "{middle}");
        let unity = Scale::Fader.position(f32::NEG_INFINITY, 6., 0.);
        assert_eq!(unity, super::FADER_UNITY);
    }
}
