use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ViewError {
    #[error("an editor name is 1 to 64 lowercase letters, digits, `-` and `_`, not {0:?}")]
    EditorName(String),
    #[error(
        "a view field name is a lowercase letter, then lowercase letters, digits and `_`, not {0:?}"
    )]
    FieldName(String),
}

/// The name of a clip editor, such as `piano-roll`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EditorName(String);

impl EditorName {
    pub fn new(name: &str) -> Result<Self, ViewError> {
        Self::try_from(name.to_owned())
    }

    /// The built-in editor a clip without `editor` opens in.
    pub fn piano_roll() -> Self {
        Self("piano-roll".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EditorName {
    type Error = ViewError;

    fn try_from(name: String) -> Result<Self, ViewError> {
        let allowed = |character: char| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        };
        if (1..=64).contains(&name.len()) && name.chars().all(allowed) {
            Ok(Self(name))
        } else {
            Err(ViewError::EditorName(name))
        }
    }
}

impl From<EditorName> for String {
    fn from(name: EditorName) -> String {
        name.0
    }
}

/// The name of a field of a [`View`]. The same rule as a field name in TypeScript.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FieldName(String);

impl FieldName {
    pub fn new(name: &str) -> Result<Self, ViewError> {
        Self::try_from(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for FieldName {
    type Error = ViewError;

    fn try_from(name: String) -> Result<Self, ViewError> {
        let mut characters = name.chars();
        let first = characters
            .next()
            .is_some_and(|character| character.is_ascii_lowercase());
        let rest = characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        });
        if first && rest {
            Ok(Self(name))
        } else {
            Err(ViewError::FieldName(name))
        }
    }
}

impl From<FieldName> for String {
    fn from(name: FieldName) -> String {
        name.0
    }
}

impl Borrow<str> for FieldName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// One value of a [`View`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(untagged)]
pub enum ViewValue {
    Toggle(bool),
    Number(i64),
    Word(String),
}

/// As JSON writes it: a word in quotes.
impl fmt::Display for ViewValue {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Toggle(toggle) => write!(formatter, "{toggle}"),
            Self::Number(number) => write!(formatter, "{number}"),
            Self::Word(word) => write!(formatter, "{word:?}"),
        }
    }
}

impl<'de> Deserialize<'de> for ViewValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;

        impl Visitor<'_> for ValueVisitor {
            type Value = ViewValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a view value: a word, a whole number, or true or false")
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<ViewValue, E> {
                Ok(ViewValue::Toggle(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<ViewValue, E> {
                Ok(ViewValue::Number(value))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<ViewValue, E> {
                i64::try_from(value)
                    .map(ViewValue::Number)
                    .map_err(|_| E::invalid_value(de::Unexpected::Unsigned(value), &self))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<ViewValue, E> {
                Ok(ViewValue::Word(value.to_owned()))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<ViewValue, E> {
                Ok(ViewValue::Word(value))
            }
        }

        deserializer.deserialize_any(ValueVisitor)
    }
}

/// What a clip editor keeps on a clip or a note, such as `{"scale": "dorian"}`. Flat: a field
/// holds a [`ViewValue`] and never another view. Only an editor reads it; playback never does.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct View(BTreeMap<FieldName, ViewValue>);

impl View {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, field: &str) -> Option<&ViewValue> {
        self.0.get(field)
    }

    pub fn insert(&mut self, field: FieldName, value: ViewValue) {
        self.0.insert(field, value);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&FieldName, &ViewValue)> {
        self.0.iter()
    }
}

/// As JSON writes it, such as `{"scale": "dorian"}`.
impl fmt::Display for View {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let fields: Vec<String> = self
            .iter()
            .map(|(field, value)| format!("{:?}: {value}", field.as_str()))
            .collect();
        write!(formatter, "{{{}}}", fields.join(", "))
    }
}
