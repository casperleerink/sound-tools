//! One number of a tool's saved state, with its range, its default and its scale, written once.

use std::sync::Arc;

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
        if ValueRange::of(self).contains(value) {
            return Ok(());
        }
        Err(format!("{field} must be from {min} to {max}, not {value}"))
    }

    /// The parameter without its state type, for an owner that knows the tool only by its
    /// ports, such as the arrangement that automates the devices of a track.
    pub fn info(&self) -> ParameterInfo {
        ParameterInfo {
            field: self.field.into(),
            range: ValueRange::of(self),
        }
    }

    /// This number of one object of a nested record `T`, such as a band of an EQ: the same
    /// range, default and scale, named by its `path` in `T`, such as `bands[0].gain_db`. So an
    /// automation lane can name it, and the object type keeps one parameter for all its objects.
    pub const fn at<T>(
        &self,
        path: &'static str,
        get: fn(&T) -> f32,
        set: fn(&mut T, f32),
    ) -> Parameter<T> {
        Parameter {
            field: path,
            min: self.min,
            max: self.max,
            default: self.default,
            scale: self.scale,
            get,
            set,
        }
    }
}

/// Numbers of the objects of a nested record `S` as numbers of the whole record, with
/// [`Parameter::at`], for the lanes of a device: `lanes![S: object.field: PARAMETER, ...]`, or
/// `object[index].field` for an object in a list. Each is named by its path in the saved
/// record, which is the path of the field here, and has the range of the parameter of its
/// object type. Gives an array, in the order written.
#[macro_export]
macro_rules! lanes {
    ($state:ty: $($object:ident $([$index:literal])? . $field:ident : $parameter:expr),+ $(,)?) => {
        [$($parameter.at(
            concat!(stringify!($object), $("[", stringify!($index), "]",)? ".", stringify!($field)),
            |state: &$state| state.$object$([$index])?.$field,
            |state: &mut $state, value| state.$object$([$index])?.$field = value,
        )),+]
    };
}

/// A number as an owner that automates it sees it: its name and its range. The name of a
/// [`Parameter`] is its field. A device whose numbers are known only as its behaviour runs, such
/// as the parameters of a plugin, makes its own, so the name is not a constant.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterInfo {
    pub field: Arc<str>,
    pub range: ValueRange,
}

/// A number that an instance takes automation for, as its owner sees it: its range, and its
/// value in the record, which plays where no lane moves it. No value when the record is not the
/// state the numbers read.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AutomatedNumber {
    pub range: ValueRange,
    pub record: Option<f32>,
}

/// The values of a number and how they spread over the travel of its knob, from 0 to 1. A knob
/// of a [`Parameter`] and an automation lane of it both take it with [`ValueRange::of`], so
/// they agree. Other controls, such as the axes of a display, make their own.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ValueRange {
    pub min: f32,
    pub max: f32,
    pub scale: Scale,
}

impl ValueRange {
    pub const fn linear(min: f32, max: f32) -> Self {
        let scale = Scale::Linear;
        Self { min, max, scale }
    }

    pub const fn logarithmic(min: f32, max: f32) -> Self {
        let scale = Scale::Logarithmic;
        Self { min, max, scale }
    }

    /// The range and the scale of a parameter.
    pub const fn of<S>(parameter: &Parameter<S>) -> Self {
        let Parameter {
            min, max, scale, ..
        } = *parameter;
        Self { min, max, scale }
    }

    /// Whether `value` is in the range. Not a number is not.
    pub fn contains(&self, value: f32) -> bool {
        (self.min..=self.max).contains(&value)
    }

    /// Where a value is on the travel, from 0 to 1. A value outside the range is at an end.
    pub fn position(&self, value: f32) -> f32 {
        self.scale.position(self.min, self.max, value)
    }

    /// The value at a place on the travel, with three significant digits, as a knob gives it:
    /// so a readout and a saved file stay short. The ends are exact.
    pub fn value(&self, position: f32) -> f32 {
        three_digits(self.exact(position)).clamp(self.min, self.max)
    }

    /// The value at a place on the travel, not rounded: what an automation lane plays.
    pub fn exact(&self, position: f32) -> f32 {
        self.scale.value(self.min, self.max, position)
    }
}

fn three_digits(value: f32) -> f32 {
    if value == 0. || !value.is_finite() {
        return value;
    }
    // In f64, so that the result is the f32 nearest to the short decimal number.
    let value = f64::from(value);
    let unit = 10_f64.powf(2. - value.abs().log10().floor());
    ((value * unit).round() / unit) as f32
}

/// Where 0 dB sits on the travel of [`Scale::Fader`].
pub const FADER_UNITY: f32 = 0.8;
/// The top of [`Scale::Fader`], in decibels.
pub const FADER_TOP_DB: f32 = 6.;
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
    ///
    /// The scale is always `-inf` to +6 dB: it does not read the min and the max it is given.
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
            Self::Fader if value >= FADER_TOP_DB => 1.,
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
            Self::Fader => (FADER_DECADE * (position / FADER_UNITY).log10()).min(FADER_TOP_DB),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Parameter, Scale};

    #[derive(Default)]
    struct Nested {
        filter: f32,
        bands: [f32; 2],
    }

    #[derive(Default)]
    struct Record {
        filter: Nested,
        bands: [Nested; 2],
    }

    /// Each lane is named by the path of its field, and reads and writes that field alone.
    #[test]
    fn lanes_are_named_by_their_path_in_the_record() {
        const NUMBER: Parameter<Nested> = Parameter {
            field: "filter",
            min: 0.,
            max: 1.,
            default: 0.5,
            scale: Scale::Linear,
            get: |nested| nested.filter,
            set: |nested, value| nested.filter = value,
        };
        let lanes: [Parameter<Record>; 2] =
            lanes![Record: filter.filter: NUMBER, bands[1].filter: NUMBER];
        let fields = lanes.each_ref().map(|lane| lane.field);
        assert_eq!(fields, ["filter.filter", "bands[1].filter"]);
        let mut record = Record::default();
        (lanes[1].set)(&mut record, 0.25);
        assert_eq!(record.bands[1].filter, 0.25);
        assert_eq!((lanes[1].get)(&record), 0.25);
        assert_eq!(record.bands[0].filter + record.filter.filter, 0.);
        assert_eq!(record.filter.bands, [0.; 2]);
        assert_eq!((lanes[0].max, lanes[0].scale), (1., Scale::Linear));
    }

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
    fn values_have_three_significant_digits() {
        assert_eq!(super::three_digits(2143.55), 2140.);
        assert_eq!(super::three_digits(0.0123456), 0.0123);
        assert_eq!(super::three_digits(0.15549), 0.155);
        assert_eq!(super::three_digits(-12.34), -12.3);
        assert_eq!(super::three_digits(0.), 0.);
    }

    #[test]
    fn the_middle_of_a_logarithmic_travel_is_the_middle_ratio() {
        let middle = Scale::Logarithmic.value(20., 20_000., 0.5);
        assert!((middle - 632.456).abs() < 0.01, "{middle}");
        let unity = Scale::Fader.position(f32::NEG_INFINITY, 6., 0.);
        assert_eq!(unity, super::FADER_UNITY);
    }
}
