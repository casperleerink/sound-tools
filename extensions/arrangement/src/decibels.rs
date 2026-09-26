//! A gain in decibels as a record saves it: a number, or `"-inf"` for silence.
//!
//! JSON has no infinity, and silence is the one gain a fader needs that no number gives. So the
//! bottom of a volume is saved as the string `"-inf"`, and every other gain as the number it
//! always was. A record of before this reads as it did and is written back the same.

use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};

/// What a record says for silence.
pub const SILENCE: &str = "-inf";

/// For `#[serde(with = "decibels")]` on an `f32` in decibels.
pub fn serialize<S: Serializer>(db: &f32, serializer: S) -> Result<S::Ok, S::Error> {
    if *db == f32::NEG_INFINITY {
        serializer.serialize_str(SILENCE)
    } else {
        serializer.serialize_f32(*db)
    }
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    deserializer.deserialize_any(GainVisitor)
}

struct GainVisitor;

impl Visitor<'_> for GainVisitor {
    type Value = f32;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            formatter,
            "a number of decibels, or \"{SILENCE}\" for silence"
        )
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<f32, E> {
        Ok(value as f32)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<f32, E> {
        Ok(value as f32)
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<f32, E> {
        Ok(value as f32)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<f32, E> {
        match value {
            SILENCE => Ok(f32::NEG_INFINITY),
            _ => Err(E::invalid_value(de::Unexpected::Str(value), &self)),
        }
    }
}

/// The factor a gain multiplies the samples by: 0 dB is exactly 1 and `-inf` exactly 0.
pub fn amplitude(db: f32) -> f32 {
    // In f64, so that 0 dB is exactly 1 and leaves every sample as it was.
    10_f64.powf(f64::from(db) / 20.0) as f32
}

/// Whether a gain can be saved and played: at most `max`, and a number or `-inf`.
pub fn check(field: &str, db: f32, max: f32) -> Result<(), String> {
    if db <= max {
        return Ok(());
    }
    Err(format!(
        "{field} must be a number of decibels up to {max}, or \"{SILENCE}\" for silence, not {db}"
    ))
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::amplitude;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Gain {
        #[serde(with = "super")]
        gain_db: f32,
    }

    #[test]
    fn a_number_stays_a_number_and_silence_is_the_string() {
        for (text, db) in [
            (r#"{"gain_db":-6.0}"#, -6.0),
            (r#"{"gain_db":"-inf"}"#, f32::NEG_INFINITY),
        ] {
            let gain: Gain = serde_json::from_str(text).unwrap();
            assert_eq!(gain, Gain { gain_db: db });
            assert_eq!(serde_json::to_string(&gain).unwrap(), text);
        }
        assert_eq!(
            serde_json::from_str::<Gain>(r#"{"gain_db":-3}"#).unwrap(),
            Gain { gain_db: -3.0 }
        );
        let wrong = serde_json::from_str::<Gain>(r#"{"gain_db":"loud"}"#).unwrap_err();
        assert!(
            wrong.to_string().contains(r#"or "-inf" for silence"#),
            "{wrong}"
        );
    }

    #[test]
    fn zero_db_is_exactly_one_and_minus_inf_exactly_zero() {
        assert_eq!(amplitude(0.0), 1.0);
        assert_eq!(amplitude(f32::NEG_INFINITY), 0.0);
        assert!((amplitude(-6.0) - 0.501_187).abs() < 1e-6);
    }
}
