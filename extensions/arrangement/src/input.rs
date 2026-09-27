//! Which channels of the input an audio track records.

use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Channels of the default input of the system, counted from 1 as an interface labels them:
/// one alone, `[1]`, or two next to each other for a stereo take, `[1, 2]`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<u16>", into = "Vec<u16>")]
pub struct InputChannels {
    /// The first channel, from 1.
    first: u16,
    stereo: bool,
}

impl InputChannels {
    /// Channel 1 alone: what a track records when its record says nothing.
    pub const FIRST: Self = Self {
        first: 1,
        stereo: false,
    };

    /// One channel, from 1.
    pub fn mono(channel: u16) -> Option<Self> {
        (channel >= 1).then_some(Self {
            first: channel,
            stereo: false,
        })
    }

    /// Two channels next to each other, `left` and the one after it, for a stereo take.
    pub fn pair(left: u16) -> Option<Self> {
        (left >= 1 && left < u16::MAX).then_some(Self {
            first: left,
            stereo: true,
        })
    }

    /// What an input of `channels` offers: each channel alone, then the pairs 1 + 2, 3 + 4.
    pub fn offered(channels: usize) -> Vec<Self> {
        let count = u16::try_from(channels).unwrap_or(u16::MAX);
        let alone = (1..=count).filter_map(Self::mono);
        let pairs = (1..count).step_by(2).filter_map(Self::pair);
        alone.chain(pairs).collect()
    }

    pub fn is_stereo(&self) -> bool {
        self.stereo
    }

    /// The channels of the device, counted from 0.
    pub fn device_channels(&self) -> Range<usize> {
        let first = usize::from(self.first - 1);
        first..first + if self.stereo { 2 } else { 1 }
    }

    /// Whether a record leaves it out: channel 1 alone.
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::FIRST
    }
}

impl Default for InputChannels {
    fn default() -> Self {
        Self::FIRST
    }
}

/// As the input select says it: `In 1`, `In 1 + 2`.
impl fmt::Display for InputChannels {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.stereo {
            false => write!(formatter, "In {}", self.first),
            true => write!(formatter, "In {} + {}", self.first, self.first + 1),
        }
    }
}

impl TryFrom<Vec<u16>> for InputChannels {
    type Error = String;

    fn try_from(channels: Vec<u16>) -> Result<Self, String> {
        let wrong = || {
            format!(
                "input must be one channel counted from 1, such as [1], or two next to each other for a stereo take, such as [1, 2], not {channels:?}"
            )
        };
        match channels.as_slice() {
            [channel] => Self::mono(*channel).ok_or_else(wrong),
            [left, right] if right.checked_sub(*left) == Some(1) => {
                Self::pair(*left).ok_or_else(wrong)
            }
            _ => Err(wrong()),
        }
    }
}

impl From<InputChannels> for Vec<u16> {
    fn from(channels: InputChannels) -> Vec<u16> {
        match channels.stereo {
            false => vec![channels.first],
            true => vec![channels.first, channels.first + 1],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_names_one_channel_or_a_pair_next_to_each_other() {
        let pair: InputChannels = serde_json::from_str("[3, 4]").unwrap();
        assert_eq!(pair.to_string(), "In 3 + 4");
        assert_eq!(pair.device_channels(), 2..4);
        assert_eq!(serde_json::to_string(&pair).unwrap(), "[3,4]");
        let mono: InputChannels = serde_json::from_str("[2]").unwrap();
        assert_eq!((mono.to_string(), mono.device_channels()), ("In 2".into(), 1..2));
        for wrong in ["[]", "[0]", "[1, 3]", "[2, 1]", "[1, 2, 3]", "[0, 1]"] {
            assert!(serde_json::from_str::<InputChannels>(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn an_input_offers_each_channel_alone_then_the_pairs() {
        let offered: Vec<String> = InputChannels::offered(4)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            offered,
            ["In 1", "In 2", "In 3", "In 4", "In 1 + 2", "In 3 + 4"]
        );
        assert_eq!(InputChannels::offered(1), [InputChannels::FIRST]);
        assert_eq!(InputChannels::offered(3).len(), 4);
    }
}
