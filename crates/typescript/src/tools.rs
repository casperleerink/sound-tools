//! A tool of the project, as `extensions/*.ts` defines it, made into a [`JsonTool`] of the core:
//! the check of its records from its fields, its doc for agents, and a behaviour that plays
//! the Hum its code generates as an effect, an instrument or a source.
//!
//! The check runs in Rust, from the fields Bun sent once, so a record loads, and an agent
//! hears what is wrong with it, without asking Bun. Only a new combination of choices asks Bun
//! for Hum; a knob, a toggle or a pattern only moves a value of the Hum that plays.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};
use sound_core::{
    Assets, BehaviourContext, BehaviourError, InputEndpoint, JsonTool, JsonToolDoc, OutputEndpoint,
    ParameterInfo, ValueRange, Watch,
};
use sound_hum::{ArraySpec, Code, Hum, HumUpdate, Kind, Machine, Values, compile};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT, NOTES_INPUT};

use crate::bun::Bun;
use crate::samples::Samples;

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
    pub kind: ToolKind,
    /// The saved fields of its record, in the order the code gives them, which is the order
    /// of the knobs on the card.
    pub fields: Named<Field>,
    /// What its interface plays and nothing saves.
    pub controls: Named<Control>,
    /// It has a control loop, which runs while the window is open.
    pub tick: bool,
    /// It has a page: a view of the whole window, for an instance at the top of the project.
    pub page: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolKind {
    Effect,
    Instrument,
    Source,
}

/// Named things in the order the code gives them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Named<T>(pub Vec<(String, T)>);

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Named<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `serde_json` keeps the order of an object, which a map type of ours would not.
        let object = Map::<String, Value>::deserialize(deserializer)?;
        let named = object
            .into_iter()
            .map(|(name, value)| Ok((name, T::deserialize(value)?)))
            .collect::<Result<_, serde_json::Error>>()
            .map_err(serde::de::Error::custom)?;
        Ok(Self(named))
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
    /// A sound under `assets/audio/`, by its file name, which the Hum reads as a list.
    Sample {
        #[serde(default)]
        label: Option<String>,
    },
    /// A list of numbers, such as the steps of a sequence.
    Pattern {
        length: usize,
        min: f32,
        max: f32,
        default: f32,
        #[serde(default)]
        label: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Control {
    Live {
        min: f32,
        max: f32,
        default: f32,
        #[serde(default)]
        label: Option<String>,
    },
    Trigger {
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
            // As a record writes it: `4`, not `4.0`. The two are one option, and one key of
            // the Hum that is kept per choice.
            Self::Number(number) if number.fract() == 0.0 && number.abs() < 1e15 => {
                Value::from(*number as i64)
            }
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

impl Field {
    /// The value a record that leaves the field out has.
    fn default_value(&self) -> Value {
        match self {
            Self::Knob { default, .. } => Value::from(*default),
            Self::Toggle { default, .. } => Value::from(*default),
            Self::Choice { default, .. } => default.to_value(),
            Self::Pattern {
                length, default, ..
            } => Value::from(vec![*default; *length]),
            Self::Sample { .. } => Value::from(""),
        }
    }
}

impl ToolInfo {
    /// What the Hum processor of this tool is.
    pub(crate) fn kind(&self) -> Kind {
        match self.kind {
            ToolKind::Effect => Kind::Effect,
            ToolKind::Instrument => Kind::Instrument {
                voices: sound_hum::MAX_VOICES,
            },
            ToolKind::Source => Kind::Source,
        }
    }

    /// The name of its processor. Another kind is another processor, with other ports.
    pub(crate) fn processor(&self) -> &'static str {
        match self.kind {
            ToolKind::Effect => "hum-effect",
            ToolKind::Instrument => "hum-instrument",
            ToolKind::Source => "hum-source",
        }
    }

    /// The index of the live control or the trigger `name` among those of its kind, which is
    /// its index in the Hum the tool generates.
    pub(crate) fn control(&self, name: &str) -> Option<(usize, &Control)> {
        let same_kind = |control: &Control, other: &Control| {
            std::mem::discriminant(control) == std::mem::discriminant(other)
        };
        let (position, (_, control)) =
            (self.controls.0.iter().enumerate()).find(|(_, (control, _))| control == name)?;
        let index = (self.controls.0[..position].iter())
            .filter(|(_, other)| same_kind(control, other))
            .count();
        Some((index, control))
    }

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
            let in_range = |number: f64, min: f32, max: f32| {
                number >= f64::from(min) && number <= f64::from(max)
            };
            match field {
                Field::Knob { min, max, .. } => {
                    let Some(number) = value.as_f64() else {
                        return Err(format!("{at}: must be a number from {min} to {max}"));
                    };
                    if !in_range(number, *min, *max) {
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
                Field::Sample { .. } => {
                    let Some(file) = value.as_str() else {
                        return Err(format!(
                            "{at}: must be the name of a file under assets/audio/, such as \"voice.wav\", or \"\" for none"
                        ));
                    };
                    if !file.is_empty() {
                        sound_media::AudioAsset::new(file)
                            .map_err(|error| format!("{at}: {error}"))?;
                    }
                }
                Field::Pattern {
                    length, min, max, ..
                } => {
                    let numbers = value.as_array().map(|items| {
                        items
                            .iter()
                            .map(Value::as_f64)
                            .collect::<Option<Vec<f64>>>()
                    });
                    let Some(Some(numbers)) = numbers else {
                        return Err(format!("{at}: must be a list of {length} numbers"));
                    };
                    if numbers.len() != *length {
                        return Err(format!(
                            "{at}: must be a list of {length} numbers, not {}",
                            numbers.len()
                        ));
                    }
                    if let Some((index, number)) = (numbers.iter().enumerate())
                        .find(|(_, number)| !in_range(**number, *min, *max))
                    {
                        return Err(format!("{at}[{index}]: {number} is outside [{min}, {max}]"));
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn field(&self, name: &str) -> Option<&Field> {
        let mut fields = self.fields.0.iter();
        fields
            .find(|(field, _)| field == name)
            .map(|(_, field)| field)
    }

    /// `state` with each field it leaves out at its default: the record as it plays.
    pub(crate) fn with_defaults(&self, mut state: Value) -> Value {
        if let Value::Object(object) = &mut state {
            for (name, field) in &self.fields.0 {
                if !object.contains_key(name) {
                    object.insert(name.clone(), field.default_value());
                }
            }
        }
        state
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
            let (kind, values) = match field {
                Field::Knob { min, max, unit, .. } => (
                    "knob",
                    format!("{min} to {max}{}", unit.map_or("", Unit::word)),
                ),
                Field::Toggle { .. } => ("toggle", "`true` or `false`".to_string()),
                Field::Choice { options, .. } => {
                    let options: Vec<String> = options.iter().map(|o| format!("`{o}`")).collect();
                    ("choice", options.join(", "))
                }
                Field::Pattern {
                    length, min, max, ..
                } => (
                    "pattern",
                    format!("a list of {length} numbers from {min} to {max}"),
                ),
                Field::Sample { .. } => (
                    "sample",
                    "the name of a file under `assets/audio/`, such as `voice.wav`".to_string(),
                ),
            };
            let default = field.default_value();
            rows.push_str(&format!(
                "| `{field_name}` | {kind} | {values} | `{default}` |\n"
            ));
            defaults.push(format!("{field_name:?}: {default}"));
        }
        let (place, path) = match self.kind {
            ToolKind::Effect => (
                "an effect of this project. It goes in a track's `effects` like any effect: the \
                 record sits in the track's folder and its file name is in `effects` of the \
                 track's `instance.json`",
                format!("state/arrangement/pad/{name}.json"),
            ),
            ToolKind::Instrument | ToolKind::Source => (
                "an instrument of this project. It is what a track plays, like any instrument: \
                 its record is the track's `instrument.json`",
                "state/arrangement/pad/instrument.json".to_string(),
            ),
        };
        format!(
            "# {title}\n\n{doc}\n\n\
             `{name}` is {place}. It needs no entry in `project.json`, and is defined in \
             `extensions/{file}`. An example, not a record of this project: a track `pad` \
             with one, at its defaults:\n\n\
             ```json {path}\n{{\n  \"tool\": \"{name}\",\n  \"state\": {{{}}}\n}}\n```\n\n\
             {rows}\n\
             A field left out is at its default, so `\"state\": {{}}` is the tool at its defaults. \
             A knob, a toggle or a pattern changes the sound at once. A choice changes what the \
             sound is made of: the new sound fades in over 10 ms. A knob can be automated by a \
             lane of its track, as the numbers of any device (`agent-docs/arrangement.md`).\n",
            defaults.join(", "),
        )
    }

    /// The tool for the core. `bun` makes its Hum.
    pub(crate) fn json_tool(&self, bun: &Arc<Bun>) -> JsonTool {
        let sounds = Sounds::new(self, bun);
        let info = sounds.info.clone();
        JsonTool {
            name: self.name.clone(),
            check: Arc::new(move |state: &Value| info.check(state)),
            behaviour: Box::new(move |state, context| sounds.apply(state, context)),
            doc: Some(JsonToolDoc {
                when: self.when.clone(),
                markdown: self.doc(),
            }),
            // A sample its record names may come after the record, as an audio clip's may.
            asset_folders: vec![sound_media::AUDIO_FOLDER],
        }
    }
}

/// The behaviour of a tool: its Hum per combination of choices, asked of Bun once each.
pub(crate) struct Sounds {
    info: Arc<ToolInfo>,
    bun: Arc<Bun>,
    /// By the choices as JSON. A failure is kept too, so a record that cannot play does not
    /// ask again on every turn of a knob. A tool defined again starts empty.
    compiled: RefCell<HashMap<String, Result<Rc<Code>, String>>>,
    /// The sounds of its `sample` fields, read once.
    samples: Samples,
}

impl Sounds {
    pub(crate) fn new(info: &ToolInfo, bun: &Arc<Bun>) -> Self {
        Self {
            info: Arc::new(info.clone()),
            bun: bun.clone(),
            compiled: RefCell::default(),
            samples: Samples::default(),
        }
    }

    /// The knobs of the tool, in order, with the range a lane moves each one over and their
    /// default.
    fn knobs(&self) -> impl Iterator<Item = (&str, ValueRange, f32)> {
        let fields = self.info.fields.0.iter();
        fields.filter_map(|(name, field)| match field {
            Field::Knob {
                min,
                max,
                unit,
                default,
                ..
            } => Some((name.as_str(), knob_range(*min, *max, *unit), *default)),
            _ => None,
        })
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
                    "the sound of extensions/{file} does not build: {} (in the Hum it makes: `{line}`)",
                    error.message
                )
            })
        });
        self.compiled.borrow_mut().insert(key, code.clone());
        code
    }

    /// Where the params and lists of `code` stand in `state`: a knob or a toggle by the name of
    /// its `param` line, a pattern by the name of its list.
    fn values(&self, code: &Code, state: &Value) -> Values {
        let mut values = Values::default();
        // The knobs, in the order of `automated`: each one's param.
        values.automated = (self.knobs())
            .filter_map(|(name, ..)| code.parameters.iter().position(|p| p.name == name))
            .map(|index| index as u16)
            .collect();
        for (value, parameter) in values.parameters.iter_mut().zip(&code.parameters) {
            *value = match state.get(&parameter.name) {
                Some(Value::Bool(on)) => f32::from(u8::from(*on)),
                Some(number) => number.as_f64().map_or(parameter.default, |n| n as f32),
                None => parameter.default,
            };
        }
        values
    }

    /// The lists of `code` from `state`: a pattern as the record holds it, a sample as its
    /// sound, which is silence while it cannot be read.
    fn lists(&self, code: &Code, state: &Value, assets: &Assets, rate: u32) -> Vec<Vec<f32>> {
        let list = |array: &ArraySpec| {
            let Some(length) = array.length else {
                let file = state
                    .get(&array.name)
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let sound = self.samples.get(assets, file, rate);
                return sound.map(|sound| sound.to_vec()).unwrap_or_default();
            };
            let items = state.get(&array.name).and_then(Value::as_array);
            let numbers = items.into_iter().flatten();
            let numbers = numbers.map(|item| item.as_f64().map_or(array.default, |n| n as f32));
            let mut numbers: Vec<f32> = numbers.collect();
            numbers.resize(length, array.default);
            numbers
        };
        code.arrays.iter().map(list).collect()
    }

    /// What the lists of `state` are made of: the patterns, and the files of the samples with
    /// whether each could be read, so a sample that arrives after its record is sent then. With
    /// a problem for each sample that cannot be read.
    fn lists_key(
        &self,
        code: &Code,
        state: &Value,
        assets: &Assets,
        rate: u32,
    ) -> (u64, Vec<String>) {
        let mut hasher = DefaultHasher::new();
        let mut problems = Vec::new();
        for array in &code.arrays {
            let value = state.get(&array.name);
            value.map(Value::to_string).hash(&mut hasher);
            let file = value.and_then(Value::as_str).unwrap_or_default();
            if array.length.is_none() && !file.is_empty() {
                let read = self.samples.get(assets, file, rate);
                read.as_ref()
                    .map(|sound| sound.len())
                    .ok()
                    .hash(&mut hasher);
                if let Err(problem) = read {
                    problems.push(format!("state.{}: {problem}", array.name));
                }
            }
        }
        (hasher.finish(), problems)
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
        let kind = self.info.kind();
        let mut values = self.values(&code, state);
        let (assets, rate) = (
            context.assets().clone(),
            context.prepare_config().sample_rate,
        );
        // A list, a sample most of all, is sent only when it changed: not at every knob turn.
        let (lists_key, problems) = self.lists_key(&code, state, &assets, rate);
        for problem in problems {
            context.problem(problem);
        }
        let new_lists = context.changed("lists", lists_key);
        // Declared every run, so they stay the same while the code keeps them.
        let watches: Vec<Watch> = code
            .watches
            .iter()
            .map(|name| context.watch(name))
            .collect();
        let sample_rate = context.prepare_config().sample_rate as f32;
        let new_code = context.changed("code", code.hash);
        if new_code || new_lists {
            values.arrays = Some(self.lists(&code, state, &assets, rate));
        }
        let made = || Box::new(Machine::new(code.as_ref().clone(), sample_rate));
        let mut machines: Vec<Option<Box<Machine>>> = match new_code {
            true => (0..kind.machines()).map(|_| Some(made())).collect(),
            false => Vec::new(),
        };
        let mut created = false;
        let hum = context.processor(self.info.processor(), || {
            created = true;
            let first = machines.first_mut().and_then(Option::take);
            let lists =
                (values.arrays.take()).unwrap_or_else(|| self.lists(&code, state, &assets, rate));
            let values = Values {
                arrays: Some(lists),
                ..values.clone()
            };
            Hum::new(kind, first.unwrap_or_else(made), values, watches.clone())
        })?;
        // A new processor already plays this code with these values.
        if !created {
            let watches = if new_code { watches } else { Vec::new() };
            context.update(
                hum,
                HumUpdate::Set {
                    machines,
                    values: Box::new(values),
                    watches,
                },
            )?;
        }
        match kind {
            Kind::Effect => {
                context.input(AUDIO_INPUT, InputEndpoint::new(hum, Hum::INPUT));
            }
            Kind::Instrument { .. } | Kind::Source => {
                context.input(NOTES_INPUT, InputEndpoint::new(hum, Hum::NOTES));
            }
        }
        context.output(AUDIO_OUTPUT, OutputEndpoint::new(hum, Hum::OUTPUT));
        // Every knob can be automated, as a number of any device. A toggle or a pattern is no
        // straight line, and a lane cannot move it.
        let numbers: Vec<(ParameterInfo, f32)> = (self.knobs())
            .map(|(name, range, default)| {
                let record = state.get(name).and_then(Value::as_f64);
                let info = ParameterInfo {
                    field: name.into(),
                    range,
                };
                (info, record.map_or(default, |value| value as f32))
            })
            .collect();
        if !numbers.is_empty() {
            let input = InputEndpoint::new(hum, Hum::AUTOMATION);
            context.runtime_automation(input, numbers)?;
        }
        Ok(())
    }
}

/// How a knob turns: a frequency on a log scale, as it is heard, anything else straight.
pub(crate) fn knob_range(min: f32, max: f32, unit: Option<Unit>) -> ValueRange {
    match unit {
        Some(Unit::Hz) if min > 0.0 => ValueRange::logarithmic(min, max),
        _ => ValueRange::linear(min, max),
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
            "kind": "effect",
            "fields": {
                "rate": { "kind": "knob", "min": 0.1, "max": 20, "default": 4, "unit": "hz" },
                "shape": { "kind": "choice", "options": ["sine", "square"], "default": "sine" },
                "bypass": { "kind": "toggle", "default": false },
                "accents": { "kind": "pattern", "length": 4, "min": 0, "max": 1, "default": 0 }
            },
            "controls": {
                "x": { "kind": "live", "min": 0, "max": 1, "default": 0 },
                "hit": { "kind": "trigger" },
                "y": { "kind": "live", "min": 0, "max": 1, "default": 0 }
            },
            "tick": false,
            "page": false
        }))
        .unwrap()
    }

    #[test]
    fn a_record_is_checked_against_the_fields_and_the_message_names_the_field() {
        let wobble = wobble();
        assert_eq!(wobble.check(&json!({})), Ok(()));
        let all = json!({ "rate": 6, "shape": "square", "bypass": true, "accents": [1, 0, 0, 1] });
        assert_eq!(wobble.check(&all), Ok(()));
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
                json!({ "accents": [1, 0] }),
                "state.accents: must be a list of 4 numbers, not 2",
            ),
            (
                json!({ "accents": [1, 0, 2, 0] }),
                "state.accents[2]: 2 is outside [0, 1]",
            ),
            (
                json!({ "speed": 1 }),
                "state.speed: wobble has no field speed; its fields are rate, shape, bypass, accents",
            ),
        ];
        for (state, message) in refused {
            assert_eq!(wobble.check(&state), Err(message.to_string()));
        }
    }

    #[test]
    fn a_record_that_leaves_fields_out_plays_them_at_their_defaults() {
        let played = wobble().with_defaults(json!({ "rate": 6 }));
        let expected = json!({
            "rate": 6, "shape": "sine", "bypass": false, "accents": [0.0, 0.0, 0.0, 0.0]
        });
        assert_eq!(played, expected);
    }

    #[test]
    fn a_control_is_found_by_its_place_among_those_of_its_kind() {
        let wobble = wobble();
        assert!(matches!(
            wobble.control("x"),
            Some((0, Control::Live { .. }))
        ));
        assert!(matches!(
            wobble.control("hit"),
            Some((0, Control::Trigger { .. }))
        ));
        assert!(matches!(
            wobble.control("y"),
            Some((1, Control::Live { .. }))
        ));
        assert!(wobble.control("z").is_none());
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
