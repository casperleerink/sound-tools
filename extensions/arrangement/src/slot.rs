//! One effect slot of a track: the name of the child that holds the effect, whether the slot
//! is bypassed, and the track that keys its sidechain.
//!
//! Bypass and sidechain are saved on the slot and not in the record of the effect, so a plugin
//! and a built-in effect share them and no effect has to know about them. A plain slot is saved
//! as the name, the form every record had before bypass, so such a record is written back the
//! same. Any other is saved as `{"name": "space", "bypass": true}`, or
//! `{"name": "duck", "sidechain": {"track": "kick", "tap": "post_fx"}}`.

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectSlot {
    /// The name of the child record that holds the effect, without `.json`.
    pub name: String,
    /// A bypassed effect is out of the chain: the sound goes past it untouched.
    pub bypass: bool,
    /// The sound that keys the `sidechain` input of the effect. `None` is nothing.
    pub sidechain: Option<Sidechain>,
}

/// Where the sound that keys an effect comes from: a track of the same arrangement, at a tap.
///
/// The arrangement wires it, as only it sees every track. The track is named by its folder
/// name, which a rename does not change, so nothing rewrites it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sidechain {
    /// The folder name of the track.
    pub track: String,
    pub tap: Tap,
}

/// Where on its track a sidechain takes the sound.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tap {
    /// What the instrument or the player makes, before the effects of the track.
    PreFx,
    /// The end of the chain of the track, before its volume, pan, mute and solo.
    PostFx,
    /// What the track sends to the master, after its volume, pan, mute and solo.
    PostMixer,
}

impl EffectSlot {
    /// A slot that is on.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            bypass: false,
            sidechain: None,
        }
    }
}

/// The long form of a slot, and the only one that can say `bypass` or `sidechain`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    bypass: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidechain: Option<Sidechain>,
}

impl Serialize for EffectSlot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !self.bypass && self.sidechain.is_none() {
            return serializer.serialize_str(&self.name);
        }
        Written {
            name: self.name.clone(),
            bypass: self.bypass,
            sidechain: self.sidechain.clone(),
        }
        .serialize(serializer)
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
            r#"the file name of an effect, such as "space", or {{"name": "space", "bypass": true}}, or {{"name": "duck", "sidechain": {{"track": "kick", "tap": "post_fx"}}}}"#
        )
    }

    fn visit_str<E: de::Error>(self, name: &str) -> Result<EffectSlot, E> {
        Ok(EffectSlot::new(name))
    }

    fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<EffectSlot, M::Error> {
        // Through the derived form, so an unknown field or a missing name says so.
        let Written {
            name,
            bypass,
            sidechain,
        } = Written::deserialize(de::value::MapAccessDeserializer::new(map))?;
        Ok(EffectSlot {
            name,
            bypass,
            sidechain,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{EffectSlot, Sidechain, Tap};

    #[test]
    fn a_plain_slot_is_its_name_and_any_other_says_what_it_has() {
        let slots: Vec<EffectSlot> = serde_json::from_str(
            r#"["warmth", {"name": "space", "bypass": true}, {"name": "echo", "sidechain": null}, {"name": "duck", "sidechain": {"track": "kick", "tap": "post_mixer"}}]"#,
        )
        .unwrap();
        let duck = EffectSlot {
            sidechain: Some(Sidechain {
                track: "kick".into(),
                tap: Tap::PostMixer,
            }),
            ..EffectSlot::new("duck")
        };
        let space = EffectSlot {
            bypass: true,
            ..EffectSlot::new("space")
        };
        assert_eq!(
            slots,
            [
                EffectSlot::new("warmth"),
                space,
                EffectSlot::new("echo"),
                duck
            ]
        );
        assert_eq!(
            serde_json::to_string(&slots).unwrap(),
            r#"["warmth",{"name":"space","bypass":true},"echo",{"name":"duck","sidechain":{"track":"kick","tap":"post_mixer"}}]"#
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
