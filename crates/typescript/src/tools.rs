//! A tool of the project, as `extensions/*.ts` defines it, made into a [`JsonTool`] of the core:
//! the check of its records from its fields, its doc for agents, and a behaviour that plays
//! the Hum its code generates.
//!
//! The check runs in Rust, from the fields Bun sent once, so a record loads, and an agent
//! hears what is wrong with it, without asking Bun. Only a new combination of choices asks Bun
//! for Hum; a knob or a toggle only moves a value of the Hum that plays.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};
use sound_core::{
    BehaviourContext, BehaviourError, InputEndpoint, JsonTool, JsonToolDoc, OutputEndpoint,
};
use sound_hum::{Code, Hum, HumUpdate, MAX_PARAMETERS, Machine, compile};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

use crate::bun::Bun;

/// A tool as `host.ts` sends it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolInfo {
    pub name: String,
    /// Its file in `extensions/`.
    pub file: String,
    pub title: String,
    pub when: String,
    pub doc: String,
    /// In the order the code gives them, which is the order of the knobs on the card.
    pub fields: Fields,
}

/// The fields of a record, in order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fields(pub Vec<(String, Field)>);

impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `serde_json` keeps the order of an object, which a map type of ours would not.
        let object = Map::<String, Value>::deserialize(deserializer)?;
        let fields = object
            .into_iter()
            .map(|(name, field)| Ok((name, Field::deserialize(field)?)))
            .collect::<Result<_, serde_json::Error>>()
            .map_err(serde::de::Error::custom)?;
        Ok(Self(fields))
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Field {
    Knob {
        min: f32,
        max: f32,
        default: f32,
        #[serde(default)]
        unit: Option<Unit>,
        #[serde(default)]
        label: Option<String>,
    },
    Toggle {
        default: bool,
        #[serde(default)]
        label: Option<String>,
    },
    Choice {
        options: Vec<Choice>,
        default: Choice,
        #[serde(default)]
        label: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Unit {
    Hz,
    Ms,
    Db,
    Percent,
}

/// An option of a choice: a number or a word.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum Choice {
    Number(f64),
    Word(String),
}

impl Choice {
    fn of(value: &Value) -> Option<Self> {
        match value {
            Value::Number(number) => number.as_f64().map(Self::Number),
            Value::String(word) => Some(Self::Word(word.clone())),
            _ => None,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Number(number) => Value::from(*number),
            Self::Word(word) => Value::from(word.clone()),
        }
    }
}

impl fmt::Display for Choice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => write!(formatter, "{number}"),
            Self::Word(word) => write!(formatter, "{word:?}"),
        }
    }
}

impl Unit {
    fn word(self) -> &'static str {
        match self {
            Self::Hz => " Hz",
            Self::Ms => " ms",
            Self::Db => " dB",
            Self::Percent => " (0 to 1)",
        }
    }
}

impl ToolInfo {
    /// Whether `state` is a record this tool takes. The message names the field.
    pub(crate) fn check(&self, state: &Value) -> Result<(), String> {
        let Value::Object(object) = state else {
            return Err("state: must be an object of the fields".to_string());
        };
        for (name, value) in object {
            let Some(field) = self.field(name) else {
                let names: Vec<&str> = self
                    .fields
                    .0
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect();
                return Err(format!(
                    "state.{name}: {} has no field {name}; its fields are {}",
                    self.name,
                    names.join(", ")
                ));
            };
            let at = format!("state.{name}");
            match field {
                Field::Knob { min, max, .. } => {
                    let Some(number) = value.as_f64() else {
                        return Err(format!("{at}: must be a number from {min} to {max}"));
                    };
                    if number < f64::from(*min) || number > f64::from(*max) {
                        return Err(format!("{at}: {number} is outside [{min}, {max}]"));
                    }
                }
                Field::Toggle { .. } => {
                    if !value.is_boolean() {
                        return Err(format!("{at}: must be true or false"));
                    }
                }
                Field::Choice { options, .. } => {
                    if !Choice::of(value).is_some_and(|choice| options.contains(&choice)) {
                        let options: Vec<String> = options.iter().map(Choice::to_string).collect();
                        return Err(format!("{at}: must be one of {}", options.join(", ")));
                    }
                }
            }
        }
        Ok(())
    }

    fn field(&self, name: &str) -> Option<&Field> {
        let mut fields = self.fields.0.iter();
        fields
            .find(|(field, _)| field == name)
            .map(|(_, field)| field)
    }

    /// The choices of a record, each at its default when the record leaves it out: what the
    /// Hum of a record depends on.
    fn choices(&self, state: &Value) -> Map<String, Value> {
        let mut choices = Map::new();
        for (name, field) in &self.fields.0 {
            if let Field::Choice { default, .. } = field {
                let value = state
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| default.to_value());
                choices.insert(name.clone(), value);
            }
        }
        choices
    }

    /// The doc an agent reads to use the tool: what its author wrote, where its record goes,
    /// and a table of its fields made from the fields themselves, so it is never wrong.
    pub(crate) fn doc(&self) -> String {
        let ToolInfo {
            name,
            file,
            title,
            doc,
            ..
        } = self;
        let mut defaults = Vec::new();
        let mut rows =
            String::from("| Field | Kind | Values | Default |\n| --- | --- | --- | --- |\n");
        for (field_name, field) in &self.fields.0 {
            let (kind, values, default) = match field {
                Field::Knob {
                    min,
                    max,
                    default,
                    unit,
                    ..
                } => {
                    let unit = unit.map_or("", Unit::word);
                    (
                        "knob",
                        format!("{min} to {max}{unit}"),
                        Value::from(*default),
                    )
                }
                Field::Toggle { default, .. } => (
                    "toggle",
                    "`true` or `false`".to_string(),
                    Value::from(*default),
                ),
                Field::Choice {
                    options, default, ..
                } => {
                    let options: Vec<String> = options.iter().map(|o| format!("`{o}`")).collect();
                    ("choice", options.join(", "), default.to_value())
                }
            };
            rows.push_str(&format!(
                "| `{field_name}` | {kind} | {values} | `{default}` |\n"
            ));
            defaults.push(format!("{field_name:?}: {default}"));
        }
        format!(
            "# {title}\n\n{doc}\n\n\
             `{name}` is an effect of this project, defined in `extensions/{file}`. It goes in a \
             track's `effects` like any effect. Here the pad plays through one named `{name}`, \
             at its defaults:\n\n\
             ```json state/arrangement/pad/{name}.json\n{{\n  \"tool\": \"{name}\",\n  \"state\": {{{}}}\n}}\n```\n\n\
             {rows}\n\
             A field left out is at its default, so `\"state\": {{}}` is the tool at its defaults. \
             A knob or a toggle changes the sound at once and glides. A choice changes what the \
             sound is made of: the new sound fades in over 10 ms.\n",
            defaults.join(", "),
        )
    }

    /// The tool for the core. `bun` makes its Hum.
    pub(crate) fn json_tool(&self, bun: &Arc<Bun>) -> JsonTool {
        let info = Arc::new(self.clone());
        let check = Arc::new(move |state: &Value| info.check(state));
        let sounds = Sounds::new(self, bun);
        JsonTool {
            name: self.name.clone(),
            check,
            behaviour: Box::new(move |state, context| sounds.apply(state, context)),
            doc: Some(JsonToolDoc {
                when: self.when.clone(),
                markdown: self.doc(),
            }),
        }
    }
}

/// The behaviour of a tool: its Hum per combination of choices, asked of Bun once each.
pub(crate) struct Sounds {
    info: Arc<ToolInfo>,
    bun: Arc<Bun>,
    /// By the choices as JSON. A failure is kept too, so a record that cannot play does not
    /// ask again on every turn of a knob. A tool defined again starts empty.
    compiled: Rc<RefCell<HashMap<String, Result<Rc<Code>, String>>>>,
}

impl Sounds {
    pub(crate) fn new(info: &ToolInfo, bun: &Arc<Bun>) -> Self {
        Self {
            info: Arc::new(info.clone()),
            bun: bun.clone(),
            compiled: Rc::default(),
        }
    }

    /// The compiled Hum for the choices of `state`.
    pub(crate) fn code(&self, state: &Value) -> Result<Rc<Code>, String> {
        let choices = self.info.choices(state);
        let key = Value::Object(choices.clone()).to_string();
        if let Some(code) = self.compiled.borrow().get(&key) {
            return code.clone();
        }
        let code = self.bun.sound(&self.info.name, &choices).and_then(|lines| {
            let file = &self.info.file;
            compile(&lines).map(Rc::new).map_err(|error| {
                let line = lines.get(error.line).map_or("", String::as_str);
                format!(
                    "the Hum of extensions/{file}, line `{line}`: {}",
                    error.message
                )
            })
        });
        self.compiled.borrow_mut().insert(key, code.clone());
        code
    }

    fn apply(
        &self,
        state: &Value,
        context: &mut BehaviourContext<'_>,
    ) -> Result<(), BehaviourError> {
        // The check of a record is the one of the tool when it loaded, and the tool may have
        // changed since: the new one decides what plays.
        self.info.check(state).map_err(BehaviourError::Other)?;
        let code = self.code(state).map_err(BehaviourError::Other)?;
        let mut values = [0.0; MAX_PARAMETERS];
        for (value, parameter) in values.iter_mut().zip(&code.parameters) {
            // A knob or a toggle of the record, by the name of its `param` line.
            *value = match state.get(&parameter.name) {
                Some(Value::Bool(on)) => f32::from(u8::from(*on)),
                Some(number) => number.as_f64().map_or(parameter.default, |n| n as f32),
                None => parameter.default,
            };
        }
        let sample_rate = context.prepare_config().sample_rate as f32;
        let new_code = context.changed("code", code.hash);
        let made = |code: &Code| Box::new(Machine::new(code.clone(), &values, sample_rate));
        let mut machine = new_code.then(|| made(&code));
        let hum = context.processor("hum", || {
            Hum::new(machine.take().unwrap_or_else(|| made(&code)))
        })?;
        context.update(hum, HumUpdate { machine, values })?;
        context.input(AUDIO_INPUT, InputEndpoint::new(hum, Hum::INPUT));
        context.output(AUDIO_OUTPUT, OutputEndpoint::new(hum, Hum::OUTPUT));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wobble() -> ToolInfo {
        serde_json::from_value(json!({
            "name": "wobble",
            "file": "wobble.ts",
            "title": "Wobble",
            "when": "You want the volume to move",
            "doc": "A tremolo.",
            "fields": {
                "rate": { "kind": "knob", "min": 0.1, "max": 20, "default": 4, "unit": "hz" },
                "shape": { "kind": "choice", "options": ["sine", "square"], "default": "sine" },
                "bypass": { "kind": "toggle", "default": false }
            }
        }))
        .unwrap()
    }

    #[test]
    fn a_record_is_checked_against_the_fields_and_the_message_names_the_field() {
        let wobble = wobble();
        assert_eq!(wobble.check(&json!({})), Ok(()));
        assert_eq!(
            wobble.check(&json!({ "rate": 6, "shape": "square", "bypass": true })),
            Ok(())
        );
        let refused = [
            (json!({ "rate": 25 }), "state.rate: 25 is outside [0.1, 20]"),
            (
                json!({ "shape": "saw" }),
                "state.shape: must be one of \"sine\", \"square\"",
            ),
            (
                json!({ "bypass": 1 }),
                "state.bypass: must be true or false",
            ),
            (
                json!({ "speed": 1 }),
                "state.speed: wobble has no field speed; its fields are rate, shape, bypass",
            ),
        ];
        for (state, message) in refused {
            assert_eq!(wobble.check(&state), Err(message.to_string()));
        }
    }

    #[test]
    fn the_example_of_the_doc_is_a_record_the_tool_takes() {
        let wobble = wobble();
        let doc = wobble.doc();
        let start = doc
            .find("```json state/arrangement/pad/wobble.json\n")
            .unwrap();
        let body = &doc[start..];
        let json = &body[body.find('\n').unwrap() + 1..body.find("\n```\n").unwrap()];
        let record: Value = serde_json::from_str(json).unwrap();
        assert_eq!(record["tool"], "wobble");
        assert_eq!(wobble.check(&record["state"]), Ok(()));
        assert!(
            doc.contains("| `rate` | knob | 0.1 to 20 Hz | `4.0` |"),
            "{doc}"
        );
    }
}
