//! One effect slot of a track: the name of the child that holds the effect, and whether the
//! slot is bypassed.
//!
//! Bypass is saved on the slot and not in the record of the effect, so a plugin and a built-in
//! effect share it and no effect has to know about it. A slot that is on is saved as the plain
//! name, the form every record had before bypass, so such a record is written back the same.
//! A bypassed one is saved as `{"name": "space", "bypass": true}`.

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectSlot {
    /// The name of the child record that holds the effect, without `.json`.
    pub name: String,
    /// A bypassed effect is out of the chain: the sound goes past it untouched.
    pub bypass: bool,
}

impl EffectSlot {
    /// A slot that is on.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            bypass: false,
        }
    }
}

/// The long form of a slot, and the only one that can say `bypass`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    name: String,
    #[serde(default)]
    bypass: bool,
}

impl Serialize for EffectSlot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.bypass {
            false => serializer.serialize_str(&self.name),
            true => Written {
                name: self.name.clone(),
                bypass: true,
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for EffectSlot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SlotVisitor)
    }
}

struct SlotVisitor;

impl<'de> Visitor<'de> for SlotVisitor {
    type Value = EffectSlot;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            formatter,
            r#"the file name of an effect, such as "space", or {{"name": "space", "bypass": true}}"#
        )
    }

    fn visit_str<E: de::Error>(self, name: &str) -> Result<EffectSlot, E> {
        Ok(EffectSlot::new(name))
    }

    fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<EffectSlot, M::Error> {
        // Through the derived form, so an unknown field or a missing name says so.
        let Written { name, bypass } =
            Written::deserialize(de::value::MapAccessDeserializer::new(map))?;
        Ok(EffectSlot { name, bypass })
    }
}

#[cfg(test)]
mod tests {
    use super::EffectSlot;

    #[test]
    fn a_slot_that_is_on_is_its_name_and_a_bypassed_one_says_so() {
        let slots: Vec<EffectSlot> = serde_json::from_str(
            r#"["warmth", {"name": "space", "bypass": true}, {"name": "echo"}]"#,
        )
        .unwrap();
        assert_eq!(
            slots,
            [
                EffectSlot::new("warmth"),
                EffectSlot {
                    name: "space".into(),
                    bypass: true
                },
                EffectSlot::new("echo"),
            ]
        );
        assert_eq!(
            serde_json::to_string(&slots).unwrap(),
            r#"["warmth",{"name":"space","bypass":true},"echo"]"#
        );
    }

    #[test]
    fn a_wrong_slot_says_what_a_slot_is() {
        let wrong = serde_json::from_str::<EffectSlot>("3").unwrap_err();
        assert!(
            wrong.to_string().contains("the file name of an effect"),
            "{wrong}"
        );
        let unknown =
            serde_json::from_str::<EffectSlot>(r#"{"name": "space", "off": true}"#).unwrap_err();
        assert!(
            unknown.to_string().contains("unknown field `off`"),
            "{unknown}"
        );
    }
}
