use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
    marker::PhantomData,
};

pub trait State: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {}
impl<T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static> State for T {}

pub struct Tool<S> {
    name: &'static str,
    marker: PhantomData<fn() -> S>,
}

impl<S> Copy for Tool<S> {}
impl<S> Clone for Tool<S> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S> Tool<S> {
    pub fn name(self) -> &'static str {
        self.name
    }
}

type Decode = Box<dyn Fn(Value) -> Result<Box<dyn Any + Send + Sync>> + Send + Sync>;

struct Definition {
    state_type: TypeId,
    decode: Decode,
}

#[derive(Default)]
pub struct Registry {
    definitions: BTreeMap<&'static str, Definition>,
}

impl Registry {
    pub fn register<S: State>(
        &mut self,
        name: &'static str,
        validate: fn(&S) -> Result<()>,
    ) -> Result<Tool<S>> {
        if name.is_empty() || self.definitions.contains_key(name) {
            return Err(Error(format!("Empty or duplicate tool name: {name}")));
        }
        self.definitions.insert(
            name,
            Definition {
                state_type: TypeId::of::<S>(),
                decode: Box::new(move |value| {
                    let state: S = serde_json::from_value(value)?;
                    validate(&state)?;
                    Ok(Box::new(state))
                }),
            },
        );
        Ok(Tool {
            name,
            marker: PhantomData,
        })
    }

    pub fn decode<S: State>(&self, tool: Tool<S>, value: Value) -> Result<S> {
        let definition = self
            .definitions
            .get(tool.name)
            .ok_or_else(|| Error(format!("Missing tool: {}", tool.name)))?;
        if definition.state_type != TypeId::of::<S>() {
            return Err(Error(format!("Wrong state type for {}", tool.name)));
        }
        (definition.decode)(value)?
            .downcast::<S>()
            .map(|state| *state)
            .map_err(|_| Error(format!("Wrong state type for {}", tool.name)))
    }

    pub fn validate(&self, name: &str, value: Value) -> Result<()> {
        let definition = self
            .definitions
            .get(name)
            .ok_or_else(|| Error(format!("Missing tool: {name}")))?;
        (definition.decode)(value)?;
        Ok(())
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.definitions.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
    struct Gain {
        level: f32,
    }

    fn validate_gain(state: &Gain) -> Result<()> {
        if state.level.is_finite() && (0.0..=2.0).contains(&state.level) {
            Ok(())
        } else {
            Err(Error("Gain must be between zero and two".into()))
        }
    }

    #[test]
    fn typed_decode_validates_external_records() {
        let mut registry = Registry::default();
        let gain = registry.register("test.gain", validate_gain).unwrap();
        assert_eq!(
            registry.decode(gain, json!({"level": 0.5})).unwrap(),
            Gain { level: 0.5 }
        );
        assert!(registry.decode(gain, json!({"level": 3.0})).is_err());
        assert!(registry.decode(gain, json!({"level": "loud"})).is_err());
        assert!(registry.validate("missing", json!({})).is_err());
        assert!(registry.register("test.gain", validate_gain).is_err());
    }

    #[test]
    fn handles_from_another_registry_cannot_confuse_types() {
        let mut first = Registry::default();
        let tool = first.register("shared", validate_gain).unwrap();
        let mut second = Registry::default();
        second.register::<String>("shared", |_| Ok(())).unwrap();
        assert!(second.decode(tool, json!("not a gain")).is_err());
    }
}
