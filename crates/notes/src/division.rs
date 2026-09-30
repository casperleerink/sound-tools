//! Note lengths that follow the tempo, for a time a composer picks as a note: the repeat of a
//! delay, the cycle of an LFO.

use serde::{Deserialize, Serialize};

/// The note a synced time lasts, before its feel. Saved as the fraction, `"1/8"`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Division {
    #[serde(rename = "1/32")]
    ThirtySecond,
    #[serde(rename = "1/16")]
    Sixteenth,
    #[serde(rename = "1/8")]
    Eighth,
    #[serde(rename = "1/4")]
    Quarter,
    #[serde(rename = "1/2")]
    Half,
    #[serde(rename = "1/1")]
    Whole,
}

impl Division {
    /// From the shortest to the longest, each twice the one before.
    pub const ALL: [Self; 6] = [
        Self::ThirtySecond,
        Self::Sixteenth,
        Self::Eighth,
        Self::Quarter,
        Self::Half,
        Self::Whole,
    ];

    /// How many quarter notes, the beats of the tempo, it lasts.
    pub fn quarters(self) -> f32 {
        match self {
            Self::ThirtySecond => 0.125,
            Self::Sixteenth => 0.25,
            Self::Eighth => 0.5,
            Self::Quarter => 1.0,
            Self::Half => 2.0,
            Self::Whole => 4.0,
        }
    }

    /// How many quarter notes it lasts with `feel`.
    pub fn quarters_with(self, feel: Feel) -> f32 {
        self.quarters() * feel.factor()
    }

    /// The fraction, as it is saved.
    pub fn name(self) -> &'static str {
        match self {
            Self::ThirtySecond => "1/32",
            Self::Sixteenth => "1/16",
            Self::Eighth => "1/8",
            Self::Quarter => "1/4",
            Self::Half => "1/2",
            Self::Whole => "1/1",
        }
    }
}

/// Whether a synced time is the note, one and a half of it, or two thirds of it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feel {
    Straight,
    Dotted,
    Triplet,
}

impl Feel {
    pub const ALL: [Self; 3] = [Self::Straight, Self::Dotted, Self::Triplet];

    /// What it makes of the length of the note.
    pub fn factor(self) -> f32 {
        match self {
            Self::Straight => 1.0,
            Self::Dotted => 1.5,
            Self::Triplet => 2.0 / 3.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each division is twice the one before, and the names are what a record saves.
    #[test]
    fn the_divisions_double_and_save_as_their_names() {
        for pair in Division::ALL.windows(2) {
            assert_eq!(pair[1].quarters(), 2.0 * pair[0].quarters());
        }
        for division in Division::ALL {
            let saved = serde_json::to_string(&division).unwrap();
            assert_eq!(saved, format!("\"{}\"", division.name()));
        }
    }
}
