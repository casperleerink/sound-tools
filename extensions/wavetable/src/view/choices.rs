//! The words of every choice of the card: the options of its segmented controls and selects,
//! and the switches of its sections. A control gives an option as its `Debug` name, such as
//! `Osc1Position`, which is also how a test finds it.

use std::fmt::Debug;

use gpui::SharedString;
use sound_core::{FilterSlope, FilterType, LfoShape};
use sound_notes::{Division, Feel};
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};

use crate::state::{Effect, Routing, SubOctave};
use crate::{Category, Destination, Source, Table};

/// A value a segmented control or a select picks.
pub(super) trait Choice: Copy + PartialEq + Debug + 'static {
    /// Every option, in the order the control shows them.
    const ALL: &'static [Self];

    /// The word the composer sees.
    fn label(self) -> &'static str;

    /// What the control gives for it.
    fn key(self) -> SharedString {
        format!("{self:?}").into()
    }

    /// The option a control gave.
    fn of(key: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|option| option.key().as_ref() == key)
    }

    /// Every option as a row of a select, in one group.
    fn rows() -> Vec<MenuEntry> {
        let items = Self::ALL
            .iter()
            .map(|option| MenuItem::new(option.key(), option.label()));
        vec![MenuEntry::Group(MenuGroup::new().items(items))]
    }
}

/// Rows of a select in labelled groups, each with the options of `ALL` it takes.
fn grouped<C: Choice, G: PartialEq>(
    groups: &[(G, &'static str)],
    group_of: impl Fn(C) -> G,
) -> Vec<MenuEntry> {
    groups
        .iter()
        .map(|(group, label)| {
            let items = C::ALL
                .iter()
                .filter(|option| group_of(**option) == *group)
                .map(|option| MenuItem::new(option.key(), option.label()));
            MenuEntry::Group(MenuGroup::new().label(*label).items(items))
        })
        .collect()
}

impl Choice for Table {
    const ALL: &'static [Self] = &Table::ALL;

    fn label(self) -> &'static str {
        self.name()
    }

    fn rows() -> Vec<MenuEntry> {
        let categories = [
            (Category::Basic, "Basic"),
            (Category::Additive, "Additive"),
            (Category::Vocal, "Vocal"),
            (Category::Spectral, "Spectral"),
            (Category::Synthesis, "Synthesis"),
            (Category::Digital, "Digital"),
        ];
        grouped(&categories, Table::category)
    }
}

impl Choice for Effect {
    const ALL: &'static [Self] = &Effect::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Fm => "FM",
            Self::Sync => "Sync",
            Self::Warp => "Warp",
            Self::Fold => "Fold",
        }
    }
}

impl Choice for SubOctave {
    const ALL: &'static [Self] = &SubOctave::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::Down1 => "-1",
            Self::Down2 => "-2",
        }
    }
}

impl Choice for FilterType {
    const ALL: &'static [Self] = &FilterType::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::LowPass => "Low",
            Self::BandPass => "Band",
            Self::HighPass => "High",
            Self::Notch => "Notch",
        }
    }
}

impl Choice for FilterSlope {
    const ALL: &'static [Self] = &FilterSlope::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::Twelve => "12",
            Self::TwentyFour => "24",
        }
    }
}

impl Choice for Routing {
    const ALL: &'static [Self] = &Routing::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::Serial => "Serial",
            Self::Parallel => "Parallel",
            Self::Split => "Split",
        }
    }
}

impl Choice for LfoShape {
    const ALL: &'static [Self] = &LfoShape::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Triangle => "Triangle",
            Self::SawUp => "Saw up",
            Self::SawDown => "Saw down",
            Self::Square => "Square",
            Self::SampleAndHold => "Sample & hold",
        }
    }
}

impl Choice for Division {
    const ALL: &'static [Self] = &Division::ALL;

    fn label(self) -> &'static str {
        self.name()
    }
}

impl Choice for Feel {
    const ALL: &'static [Self] = &Feel::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::Straight => "Straight",
            Self::Dotted => "Dotted",
            Self::Triplet => "Triplet",
        }
    }
}

impl Choice for Source {
    const ALL: &'static [Self] = &Source::ALL;

    fn label(self) -> &'static str {
        self.name()
    }

    fn rows() -> Vec<MenuEntry> {
        let played = |source| {
            !matches!(
                source,
                Source::Env2 | Source::Env3 | Source::Lfo1 | Source::Lfo2
            )
        };
        grouped(&[(false, "Envelopes and LFOs"), (true, "Played")], played)
    }
}

impl Choice for Destination {
    const ALL: &'static [Self] = &Destination::ALL;

    fn label(self) -> &'static str {
        self.name()
    }

    fn rows() -> Vec<MenuEntry> {
        use Destination::*;
        let group = |destination| match destination {
            Osc1Position | Osc2Position | Osc1Effect | Osc2Effect | Osc1Pitch | Osc2Pitch
            | Osc1Gain | Osc2Gain | SubGain => 0,
            Filter1Cutoff | Filter2Cutoff | Filter1Resonance | Filter2Resonance => 1,
            AmpLevel | Pan | Lfo1Rate | Lfo2Rate | UnisonAmount => 2,
        };
        grouped(&[(0, "Oscillators"), (1, "Filters"), (2, "Voice")], group)
    }
}

/// Which sections the expanded card shows, as the tabs in its header pick. Interface state:
/// not saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// The rest of the first oscillator and the second.
    #[default]
    Osc,
    /// The sub, the unison and the voicing.
    Voice,
    Filter,
    Env,
    Lfo,
    Matrix,
}

impl Choice for Page {
    const ALL: &'static [Self] = &[
        Self::Osc,
        Self::Voice,
        Self::Filter,
        Self::Env,
        Self::Lfo,
        Self::Matrix,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Osc => "Osc",
            Self::Voice => "Voice",
            Self::Filter => "Filter",
            Self::Env => "Env",
            Self::Lfo => "LFO",
            Self::Matrix => "Matrix",
        }
    }
}

/// Which envelope the envelope section shows. Interface state: not saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnvelopeShown {
    #[default]
    Amp,
    Env2,
    Env3,
}

impl Choice for EnvelopeShown {
    const ALL: &'static [Self] = &[Self::Amp, Self::Env2, Self::Env3];

    fn label(self) -> &'static str {
        match self {
            Self::Amp => "Amp",
            Self::Env2 => "Env 2",
            Self::Env3 => "Env 3",
        }
    }
}

/// Which LFO the LFO section shows. Interface state: not saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LfoShown {
    #[default]
    Lfo1,
    Lfo2,
}

impl Choice for LfoShown {
    const ALL: &'static [Self] = &[Self::Lfo1, Self::Lfo2];

    fn label(self) -> &'static str {
        match self {
            Self::Lfo1 => "LFO 1",
            Self::Lfo2 => "LFO 2",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_option_comes_back_from_its_key<C: Choice>() {
        for option in C::ALL {
            assert_eq!(C::of(&option.key()), Some(*option));
        }
    }

    #[test]
    fn every_option_of_every_choice_comes_back_from_its_key() {
        every_option_comes_back_from_its_key::<Table>();
        every_option_comes_back_from_its_key::<Effect>();
        every_option_comes_back_from_its_key::<SubOctave>();
        every_option_comes_back_from_its_key::<FilterType>();
        every_option_comes_back_from_its_key::<FilterSlope>();
        every_option_comes_back_from_its_key::<Routing>();
        every_option_comes_back_from_its_key::<LfoShape>();
        every_option_comes_back_from_its_key::<Division>();
        every_option_comes_back_from_its_key::<Feel>();
        every_option_comes_back_from_its_key::<Source>();
        every_option_comes_back_from_its_key::<Destination>();
        every_option_comes_back_from_its_key::<Page>();
        every_option_comes_back_from_its_key::<EnvelopeShown>();
        every_option_comes_back_from_its_key::<LfoShown>();
    }
}
