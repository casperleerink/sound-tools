//! The named parts of a piece, such as "Intro" or "Chorus". They change no sound.

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SectionError {
    #[error("a section needs a name")]
    BlankName,
    #[error("sections must be sorted by bar, each bar used once: see bar {0}")]
    NotSorted(NonZeroU64),
}

/// The name of a section as the composer reads it. Never blank.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SectionName(String);

impl SectionName {
    pub fn new(name: impl Into<String>) -> Result<Self, SectionError> {
        let name = name.into();
        match name.trim().is_empty() {
            true => Err(SectionError::BlankName),
            false => Ok(Self(name)),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SectionName {
    type Error = SectionError;

    fn try_from(name: String) -> Result<Self, SectionError> {
        Self::new(name)
    }
}

impl From<SectionName> for String {
    fn from(name: SectionName) -> String {
        name.0
    }
}

/// A part of the piece from the start of bar `bar`, counted from 1, up to the next section.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub bar: NonZeroU64,
    pub name: SectionName,
}

/// The sections of a piece, sorted by bar, each bar used once. The last one runs to the end.
/// Saved as the list:
///
/// ```json
/// [{ "bar": 1, "name": "Intro" }, { "bar": 9, "name": "Chorus" }]
/// ```
///
/// The `with_` and `without_` functions give the list after one edit, valid by construction, or
/// `None` when that edit does not fit, like the tempo change functions of
/// [`TempoMap`](crate::TempoMap).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<Section>", into = "Vec<Section>")]
pub struct Sections(Vec<Section>);

impl Sections {
    pub fn new(sections: Vec<Section>) -> Result<Self, SectionError> {
        let unsorted = sections
            .windows(2)
            .find(|pair| matches!(pair, [earlier, later] if later.bar <= earlier.bar));
        match unsorted {
            Some([_, later]) => Err(SectionError::NotSorted(later.bar)),
            _ => Ok(Self(sections)),
        }
    }

    pub fn as_slice(&self) -> &[Section] {
        &self.0
    }

    /// Each section with the bar the next one starts on: `None` for the last.
    pub fn spans(&self) -> impl Iterator<Item = (&Section, Option<NonZeroU64>)> {
        let next = self.0.iter().skip(1).map(|section| Some(section.bar));
        self.0.iter().zip(next.chain([None]))
    }

    /// A new section from `bar`. `None` when one starts there already.
    pub fn with_section_at(&self, bar: NonZeroU64, name: SectionName) -> Option<Self> {
        let index = self
            .0
            .binary_search_by_key(&bar, |section| section.bar)
            .err()?;
        let mut sections = self.0.clone();
        sections.insert(index, Section { bar, name });
        Some(Self(sections))
    }

    /// The section that starts at `bar` with another name. `None` when none starts there.
    pub fn with_name_at(&self, bar: NonZeroU64, name: SectionName) -> Option<Self> {
        let index = self.index_of(bar)?;
        let mut sections = self.0.clone();
        sections.get_mut(index)?.name = name;
        Some(Self(sections))
    }

    /// The section that starts at `from` moved to start at `to`, past other sections if need
    /// be. `None` when none starts at `from`, or another starts at `to`.
    pub fn with_section_moved(&self, from: NonZeroU64, to: NonZeroU64) -> Option<Self> {
        let name = self.0.get(self.index_of(from)?)?.name.clone();
        if from == to {
            return Some(self.clone());
        }
        self.without_section_at(from)?.with_section_at(to, name)
    }

    /// `None` when no section starts at `bar`.
    pub fn without_section_at(&self, bar: NonZeroU64) -> Option<Self> {
        let index = self.index_of(bar)?;
        let mut sections = self.0.clone();
        sections.remove(index);
        Some(Self(sections))
    }

    fn index_of(&self, bar: NonZeroU64) -> Option<usize> {
        self.0
            .binary_search_by_key(&bar, |section| section.bar)
            .ok()
    }
}

impl TryFrom<Vec<Section>> for Sections {
    type Error = SectionError;

    fn try_from(sections: Vec<Section>) -> Result<Self, SectionError> {
        Self::new(sections)
    }
}

impl From<Sections> for Vec<Section> {
    fn from(sections: Sections) -> Self {
        sections.0
    }
}
