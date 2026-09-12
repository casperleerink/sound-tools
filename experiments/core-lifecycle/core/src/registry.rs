use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{any::Any, collections::BTreeMap, marker::PhantomData};

pub trait State: Serialize + DeserializeOwned + Clone + 'static {}
impl<T: Serialize + DeserializeOwned + Clone + 'static> State for T {}

/// Audio behaviour is optional. State and UI do not need to implement it.
/// apply runs on the control path; render must only process prepared data.
pub trait Processor<S>: 'static {
    fn apply(&mut self, state: &S);
    fn render(&mut self, output: &mut [f32], sample_rate: f32);
}

pub struct Tool<S> {
    pub(crate) name: &'static str,
    validate: fn(&S) -> Result<()>,
    marker: PhantomData<fn() -> S>,
}
impl<S> Tool<S> {
    pub fn name(&self) -> &'static str {
        self.name
    }
}
impl<S> Copy for Tool<S> {}
impl<S> Clone for Tool<S> {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Debug)]
pub struct Instance<S> {
    pub(crate) id: String,
    marker: PhantomData<fn() -> S>,
}
impl<S> Clone for Instance<S> {
    fn clone(&self) -> Self {
        Self::new(self.id.clone())
    }
}
impl<S> Instance<S> {
    pub(crate) fn new(id: String) -> Self {
        Self {
            id,
            marker: PhantomData,
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
}

pub(crate) trait Live {
    fn state(&self) -> &dyn Any;
    fn json(&self) -> Result<Value>;
    fn replace(&mut self, value: Value) -> Result<()>;
    fn render(&mut self, output: &mut [f32], sample_rate: f32);
}
struct Typed<S> {
    state: S,
    processor: Option<Box<dyn Processor<S>>>,
    validate: fn(&S) -> Result<()>,
}
impl<S: State> Live for Typed<S> {
    fn state(&self) -> &dyn Any {
        &self.state
    }
    fn json(&self) -> Result<Value> {
        Ok(serde_json::to_value(&self.state)?)
    }
    fn replace(&mut self, value: Value) -> Result<()> {
        let next = serde_json::from_value(value)?;
        (self.validate)(&next)?;
        if let Some(processor) = &mut self.processor {
            processor.apply(&next);
        }
        self.state = next;
        Ok(())
    }
    fn render(&mut self, output: &mut [f32], sample_rate: f32) {
        if let Some(processor) = &mut self.processor {
            processor.render(output, sample_rate);
        }
    }
}

type Factory = Box<dyn Fn(Value) -> Result<Box<dyn Live>>>;
pub(crate) struct Definition {
    pub output: Option<&'static str>,
    pub factory: Factory,
}
#[derive(Default)]
pub struct Registry {
    pub(crate) definitions: BTreeMap<&'static str, Definition>,
}
impl Registry {
    pub fn register<S: State>(
        &mut self,
        name: &'static str,
        validate: fn(&S) -> Result<()>,
    ) -> Tool<S> {
        assert!(
            !self.definitions.contains_key(name),
            "duplicate tool {name}"
        );
        self.definitions.insert(
            name,
            Definition {
                output: None,
                factory: Box::new(move |value| {
                    let state = serde_json::from_value(value)?;
                    validate(&state)?;
                    Ok(Box::new(Typed::<S> {
                        state,
                        processor: None,
                        validate,
                    }))
                }),
            },
        );
        Tool {
            name,
            validate,
            marker: PhantomData,
        }
    }
    /// This prototype exposes one mono audio output per processor.
    pub fn processor<S: State>(
        &mut self,
        tool: Tool<S>,
        output: &'static str,
        create: fn(&S) -> Box<dyn Processor<S>>,
    ) {
        let validate = tool.validate;
        let definition = self
            .definitions
            .get_mut(tool.name)
            .expect("register tool first");
        definition.output = Some(output);
        definition.factory = Box::new(move |value| {
            let state = serde_json::from_value(value)?;
            validate(&state)?;
            let processor = Some(create(&state));
            Ok(Box::new(Typed::<S> {
                state,
                processor,
                validate,
            }))
        });
    }
    pub(crate) fn definition(&self, name: &str) -> Result<&Definition> {
        self.definitions
            .get(name)
            .ok_or_else(|| Error(format!("Missing tool: {name}")))
    }
}
