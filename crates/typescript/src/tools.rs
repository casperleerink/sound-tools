//! A tool of the project, as `extensions/*.ts` defines it, made into a [`JsonTool`] of the core:
//! the check of its records from its fields, its doc for agents, and a behaviour that plays
//! the sound graph its code builds, with one voice or a voice per note.
//!
//! The check runs in Rust, from the fields Bun sent with its load, and so does the sound: Bun
//! sent the graph of every combination of choices then. Nothing here waits for Bun. A choice
//! picks another graph; a number, a boolean or a list of numbers only moves a value of the
//! sound that plays.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Deserialize;
use serde_json::{Map, Value};
use sound_core::{
    Assets, AudioThru, Automation, BehaviourContext, BehaviourError, Event, EventThru, GraphError,
    InputEndpoint, InstanceId, JsonTool, JsonToolDoc, Node, NodeId, OutputEndpoint, ParameterInfo,
    Problem, RecordStamp, ToolSchema, ValueRange, Watch,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT, NOTES_INPUT, NoteEvent, NoteFeed, NoteFeedUpdate};
use sound_signals::{
    ArraySpec, Bang, Cable, Code, ControlSpec, Declarations, DeviceUse, Graph, Kind, LinkFrom,
    LinkTo, Machine, Number, ParameterSpec, Placed, Program, Shown, Signals, SignalsUpdate, Sound,
    Values,
};

use crate::behaviours::{self, Declaration, End, Key};
use crate::bun::Bun;
use crate::feeds::played_name;
use crate::{FOLDER, samples};

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
    /// How many voices its sound plays at once: one that runs all the time, or one per note.
    pub voices: u8,
    /// Its sound reads `input`, so it has an audio input.
    pub input: bool,
    /// The saved fields of its record, in the order the code gives them, which is the order
    /// of the knobs of its default view.
    pub fields: Named<Field>,
    /// What plays it and nothing saves.
    pub inputs: Named<Input>,
    /// It has a control loop, which runs while the window is open.
    pub tick: bool,
    /// It draws its own view; without one, its default view is a card, and it has no page.
    pub view: bool,
    /// It hears the computer keyboard, `onKey`.
    pub keys: bool,
    /// The keys it takes, by name such as `delete` or `cmd-c`; every plain key when `None`.
    #[serde(default)]
    pub takes: Option<Vec<String>>,
    /// The keys it hears wherever the focus is, while its view is the main view: `space`.
    #[serde(default)]
    pub global_keys: Vec<String>,
    /// It hears note events, `onNotes`: what reaches its `notes`.
    pub notes: bool,
    /// It owns children and has a behaviour, which declares how they connect.
    pub behaviour: bool,
    /// The tools of its children by name, when it names them. An empty map allows none.
    #[serde(default)]
    pub children: Option<Named<String>>,
    /// A sound of it plays through a device, so its processors are the stages of a
    /// [`Program`].
    #[serde(skip)]
    pub staged: bool,
}

/// What a tool is to a track, which goes in its `effects` or plays as its instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolKind {
    /// One voice whose sound reads what comes in.
    Effect,
    /// A sound that hears nothing coming in, or a voice per note.
    Instrument,
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
    /// A sound that reads it hears it as a signal; a lane or a cable moves it unless `integer`.
    Number {
        min: f64,
        max: f64,
        #[serde(default)]
        integer: bool,
        /// `min` when left out.
        default: Option<f64>,
        unit: Option<Unit>,
        label: Option<String>,
    },
    Boolean {
        default: bool,
        label: Option<String>,
    },
    Choice {
        options: Vec<Choice>,
        default: Choice,
        label: Option<String>,
    },
    Text {
        default: Option<String>,
        label: Option<String>,
    },
    /// A sound under `assets/audio/`, by its file name, which the sound reads as a list.
    File { label: Option<String> },
    /// A sound reads a list of numbers as a table.
    List {
        item: Box<Field>,
        max: Option<usize>,
        /// `[]` when left out.
        default: Option<Value>,
        label: Option<String>,
    },
    /// An item of a list that holds several fields.
    Object { fields: Named<Field> },
}

/// What plays a tool and nothing saves.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Input {
    /// A number that starts at its default and glides where it is put.
    Number {
        min: f32,
        max: f32,
        default: Option<f32>,
        #[serde(default)]
        integer: bool,
        unit: Option<Unit>,
        label: Option<String>,
    },
    /// A signal that is 1 for one sample.
    Bang { label: Option<String> },
}

impl Input {
    /// Where a number input starts: its default, else its min.
    pub(crate) fn start(&self) -> Option<f32> {
        match self {
            Self::Number { min, default, .. } => Some(default.unwrap_or(*min)),
            Self::Bang { .. } => None,
        }
    }
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
            // the sound that is kept per choice.
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
            Self::Boolean { default, .. } => Value::from(*default),
            Self::Choice { default, .. } => default.to_value(),
            Self::File { .. } => Value::from(""),
            Self::List { default, .. } => {
                let items = default.clone().unwrap_or(Value::Array(Vec::new()));
                self.filled(items)
            }
            Self::Text { default, .. } => Value::from(default.clone().unwrap_or_default()),
            Self::Number {
                min,
                default,
                integer,
                ..
            } => number_value(default.unwrap_or(*min), *integer),
            Self::Object { fields } => fields.with_defaults(Value::Object(Map::new())),
        }
    }

    /// How a number turns on a knob and a lane: over its range, on a log scale for Hz.
    pub(crate) fn range(&self) -> Option<ValueRange> {
        match self {
            Self::Number { min, max, unit, .. } => {
                Some(knob_range(*min as f32, *max as f32, *unit))
            }
            _ => None,
        }
    }

    /// Whether `value` is one this field takes. `at` is its path, which the message starts
    /// with, such as `state.lanes[1].notes[3].pitch: ...`.
    fn check(&self, value: &Value, at: &str) -> Result<(), String> {
        match self {
            Self::Boolean { .. } => {
                if !value.is_boolean() {
                    return Err(format!("{at}: must be true or false"));
                }
            }
            Self::Choice { options, .. } => {
                if !Choice::of(value).is_some_and(|choice| options.contains(&choice)) {
                    let options: Vec<String> = options.iter().map(Choice::to_string).collect();
                    return Err(format!("{at}: must be one of {}", options.join(", ")));
                }
            }
            Self::File { .. } => {
                let Some(file) = value.as_str() else {
                    return Err(format!(
                        "{at}: must be the name of a file under assets/audio/, such as \"voice.wav\", or \"\" for none"
                    ));
                };
                if !file.is_empty() {
                    sound_media::AudioAsset::new(file).map_err(|error| format!("{at}: {error}"))?;
                }
            }
            Self::Text { .. } => {
                if !value.is_string() {
                    return Err(format!("{at}: must be text in quotes"));
                }
            }
            Self::Number {
                min, max, integer, ..
            } => {
                let whole = if *integer {
                    "a whole number"
                } else {
                    "a number"
                };
                let Some(number) = value.as_f64() else {
                    return Err(format!("{at}: must be {whole} from {min} to {max}"));
                };
                if *integer && number.fract() != 0.0 {
                    return Err(format!("{at}: {number} is no whole number"));
                }
                if number < *min || number > *max {
                    return Err(format!("{at}: {number} is outside [{min}, {max}]"));
                }
            }
            Self::List { item, max, .. } => {
                let Some(items) = value.as_array() else {
                    return Err(format!("{at}: must be a list"));
                };
                if let Some(max) = max
                    && items.len() > *max
                {
                    return Err(format!("{at}: holds {} items, at most {max}", items.len()));
                }
                for (index, value) in items.iter().enumerate() {
                    item.check(value, &format!("{at}[{index}]"))?;
                }
            }
            Self::Object { fields } => fields.check(value, at)?,
        }
        Ok(())
    }

    /// `value` with what it leaves out at its default, inside its lists and items too.
    fn filled(&self, value: Value) -> Value {
        match (self, value) {
            (Self::List { item, .. }, Value::Array(items)) => {
                Value::Array(items.into_iter().map(|value| item.filled(value)).collect())
            }
            (Self::Object { fields }, value) => fields.with_defaults(value),
            (_, value) => value,
        }
    }
}

/// A number as a record writes it: `4`, not `4.0`, for a whole number field. A default that is
/// no whole number stays as it is, so its check refuses it.
fn number_value(number: f64, integer: bool) -> Value {
    match integer && number.fract() == 0.0 {
        true => Value::from(number as i64),
        false => Value::from(number),
    }
}

impl<T> Named<T> {
    fn names(&self) -> String {
        let names: Vec<&str> = self.0.iter().map(|(name, _)| name.as_str()).collect();
        names.join(", ")
    }
}

impl Named<Field> {
    fn get(&self, name: &str) -> Option<&Field> {
        let mut fields = self.0.iter();
        fields
            .find(|(field, _)| field == name)
            .map(|(_, field)| field)
    }

    /// Whether `value` is an object of these fields. `at` is its path.
    fn check(&self, value: &Value, at: &str) -> Result<(), String> {
        let Value::Object(object) = value else {
            return Err(format!(
                "{at}: must be an object of the fields {}",
                self.names()
            ));
        };
        for (name, value) in object {
            let Some(field) = self.get(name) else {
                return Err(format!(
                    "{at}.{name}: there is no field {name}; the fields are {}",
                    self.names()
                ));
            };
            field.check(value, &format!("{at}.{name}"))?;
        }
        Ok(())
    }

    /// Whether the default of each field is a value it takes, inside the items of lists too.
    /// `prefix` starts the path of each.
    pub(crate) fn check_defaults(&self, prefix: &str) -> Result<(), String> {
        for (name, field) in &self.0 {
            let mut path = format!("{prefix}{name}");
            field.check(&field.default_value(), &format!("{path}.default"))?;
            let mut item = field;
            while let Field::List { item: inner, .. } = item {
                item = inner;
                path.push_str("[]");
                item.check(&item.default_value(), &format!("{path}.default"))?;
            }
            if let Field::Object { fields } = item {
                fields.check_defaults(&format!("{path}."))?;
            }
        }
        Ok(())
    }

    /// `value` with each field it leaves out at its default, inside lists and items too.
    fn with_defaults(&self, mut value: Value) -> Value {
        if let Value::Object(object) = &mut value {
            for (name, field) in &self.0 {
                let filled = match object.remove(name) {
                    Some(value) => field.filled(value),
                    None => field.default_value(),
                };
                object.insert(name.clone(), filled);
            }
        }
        value
    }
}

impl ToolInfo {
    /// What the `Signals` processor of this tool is.
    /// What the tool is to a track: an effect when it has one voice whose sound reads what
    /// comes in, as a parent's does; else an instrument.
    pub(crate) fn tool_kind(&self) -> ToolKind {
        match self.voices == 1 && (self.input || self.behaviour) {
            true => ToolKind::Effect,
            false => ToolKind::Instrument,
        }
    }

    /// What the `Signals` processor of this tool is.
    pub(crate) fn kind(&self) -> Kind {
        match self.voices > 1 {
            true => Kind::Instrument {
                voices: usize::from(self.voices).min(sound_signals::MAX_VOICES),
            },
            false => Kind::One,
        }
    }

    /// What the record and the inputs give a sound of `graph`: the numbers and booleans it
    /// reads, the files and lists of numbers, and the inputs, each in the order the code gives
    /// them. Only the numbers it reads, so a tool may hold any number of numbers it does not.
    pub(crate) fn declarations(&self, graph: &Graph) -> Declarations {
        let read = graph.params();
        let mut declarations = Declarations::default();
        let number = |name: &str, default, min, max| ParameterSpec {
            name: name.to_string(),
            default,
            min,
            max,
        };
        let list = |name: &str, length, default, min, max| ArraySpec {
            name: name.to_string(),
            length,
            default,
            min,
            max,
        };
        for (name, field) in &self.fields.0 {
            match field {
                Field::Number {
                    min, max, default, ..
                } if read.contains(name.as_str()) => {
                    let (min, max) = (*min as f32, *max as f32);
                    let default = default.map_or(min, |default| default as f32);
                    declarations
                        .parameters
                        .push(number(name, default, min, max));
                }
                Field::Boolean { default, .. } if read.contains(name.as_str()) => {
                    let default = f32::from(u8::from(*default));
                    declarations
                        .parameters
                        .push(number(name, default, 0.0, 1.0));
                }
                Field::List { item, max, .. } => {
                    if let Field::Number {
                        min,
                        max: top,
                        default,
                        ..
                    } = &**item
                    {
                        let (min, top) = (*min as f32, *top as f32);
                        let default = default.map_or(min, |default| default as f32);
                        (declarations.arrays).push(list(name, *max, default, min, top));
                    }
                }
                Field::File { .. } => (declarations.arrays).push(list(name, None, 0.0, -1.0, 1.0)),
                _ => {}
            }
        }
        for (name, input) in &self.inputs.0 {
            declarations.controls.push(match input {
                Input::Number { min, max, .. } => {
                    let start = input.start().unwrap_or(*min);
                    ControlSpec::Live(number(name, start, *min, *max))
                }
                Input::Bang { .. } => ControlSpec::Trigger(name.clone()),
            });
        }
        declarations
    }

    /// A problem of the tool, listed under its file.
    pub(crate) fn problem(&self, message: impl fmt::Display) -> Problem {
        Problem {
            path: format!("{FOLDER}/{}", self.file),
            message: format!("tool {}: {message}", self.name),
        }
    }

    /// The name of its processor, the first stage of its sound. Another kind is another
    /// processor, and so is a stage, which has other ports.
    pub(crate) fn processor(&self) -> &'static str {
        match (self.kind(), self.staged) {
            (Kind::One, false) => "signals",
            (Kind::One, true) => "signals-stage",
            (Kind::Instrument { .. }, _) => "signals-voices",
        }
    }

    /// The index of the input `name` among those of its kind, which is its index in the sound
    /// the tool builds.
    pub(crate) fn input(&self, name: &str) -> Option<(usize, &Input)> {
        let same_kind = |input: &Input, other: &Input| {
            std::mem::discriminant(input) == std::mem::discriminant(other)
        };
        let (position, (_, input)) =
            (self.inputs.0.iter().enumerate()).find(|(_, (input, _))| input == name)?;
        let index = (self.inputs.0[..position].iter())
            .filter(|(_, other)| same_kind(input, other))
            .count();
        Some((index, input))
    }

    /// Whether `state` is a record this tool takes. The message names the field.
    pub(crate) fn check(&self, state: &Value) -> Result<(), String> {
        if let Value::Object(object) = state
            && let Some(name) = object.keys().find(|name| self.field(name).is_none())
        {
            return Err(format!(
                "state.{name}: {} has no field {name}; its fields are {}",
                self.name,
                self.fields.names()
            ));
        }
        self.fields.check(state, "state")
    }

    pub(crate) fn field(&self, name: &str) -> Option<&Field> {
        self.fields.get(name)
    }

    /// `state` with each field it leaves out at its default: the record as it plays, in the
    /// order of the fields, and inside lists too, as a behaviour reads it.
    pub(crate) fn with_defaults(&self, state: Value) -> Value {
        self.fields.with_defaults(state)
    }

    fn has_choices(&self) -> bool {
        (self.fields.0.iter()).any(|(_, field)| matches!(field, Field::Choice { .. }))
    }

    /// The choices of a record, each at its default when the record leaves it out, as JSON:
    /// what picks its sound. A record and Bun may write an option apart, `4.0` and `4`.
    fn sound_key(&self, state: &Value) -> String {
        let mut choices = Map::new();
        for (name, field) in &self.fields.0 {
            if let Field::Choice { default, .. } = field {
                let choice = state.get(name).and_then(Choice::of);
                let value = choice.as_ref().unwrap_or(default).to_value();
                choices.insert(name.clone(), value);
            }
        }
        Value::Object(choices).to_string()
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
        let mut rows =
            String::from("| Field | Kind | Values | Default |\n| --- | --- | --- | --- |\n");
        field_rows(&self.fields, "", &mut rows);
        let defaults: Vec<String> = (self.fields.0.iter())
            .map(|(name, field)| format!("{name:?}: {}", field.default_value()))
            .collect();
        let (place, example, path) = match (self.behaviour, self.tool_kind()) {
            (true, _) => (
                format!(
                    "a tool of this project that owns children. Its record is \
                     `state/{name}/instance.json`, and every record next to it, \
                     `state/{name}/<child>.json`, is a child: any tool, such as a built-in \
                     device whose extension `project.json` enables, or a tool of this project. Its \
                     behaviour connects them and its own ports, such as its `audio` output. At \
                     the top of the project, connect that to the speakers in \
                     `project.json`: `{{\"from\": {{\"instance\": \"{name}\", \"port\": \"audio\"}}, \
                     \"to\": {{\"device_output\": 0}}}}`. Or put it in a track's `effects` as \
                     `state/arrangement/<track>/{name}/instance.json`, where it hears the track's \
                     sound and notes and plays into its mixer{}",
                    self.children_doc()
                ),
                "one at its defaults",
                format!("state/{name}/instance.json"),
            ),
            (false, ToolKind::Effect) => (
                "an effect of this project: one voice whose sound hears what comes in. It goes in \
                 a track's `effects` like any effect: the record sits in the track's folder and \
                 its file name is in `effects` of the track's `instance.json`"
                    .to_string(),
                "a track `pad` with one, at its defaults",
                format!("state/arrangement/pad/{name}.json"),
            ),
            (false, ToolKind::Instrument) => (
                "an instrument of this project: its sound plays notes and hears nothing coming in. \
                 It is what a track plays, like any instrument: its record is the track's \
                 `instrument.json`"
                    .to_string(),
                "a track `pad` with one, at its defaults",
                "state/arrangement/pad/instrument.json".to_string(),
            ),
        };
        format!(
            "# {title}\n\n{doc}\n\n\
             `{name}` is {place}. It is defined in `extensions/{file}`, and the `extensions` of \
             `project.json` do not list it. An example, not a record of this project: {example}:\n\n\
             ```json {path}\n{{\n  \"tool\": \"{name}\",\n  \"state\": {{{}}}\n}}\n```\n\n\
             {rows}\n\
             A field left out is at its default, so `\"state\": {{}}` is the tool at its defaults. \
             What reads a field follows it: its sound glides to a number or a boolean at once, \
             and a choice changes what the sound is made of, the new sound fading in over 10 ms; \
             its behaviour and view run again once Bun has heard of it. A number its sound reads \
             that is not whole can be automated by a lane of its track, as the numbers of any \
             device (`agent-docs/arrangement.md`).\n",
            defaults.join(", "),
        )
    }

    /// The children it names, as its doc says them, or nothing.
    fn children_doc(&self) -> String {
        let Some(children) = &self.children else {
            return String::new();
        };
        if children.0.is_empty() {
            return ". It has no children".to_string();
        }
        let children: Vec<String> = (children.0.iter())
            .map(|(name, tool)| match name.as_str() {
                ANY_NAME => format!(
                    "any number of `<name>.json` or `<name>/instance.json`, each a `{tool}`"
                ),
                _ => format!("`{name}.json`, a `{tool}`"),
            })
            .collect();
        format!(". Its children are {}, and no others", children.join("; "))
    }
}

/// The rows of the doc of `fields`, whose paths start with `prefix`. The fields of the items
/// of a list follow it, as `lanes[].name`.
fn field_rows(fields: &Named<Field>, prefix: &str, rows: &mut String) {
    for (name, field) in &fields.0 {
        let path = format!("{prefix}{name}");
        let (kind, values) = described(field);
        let default = field.default_value();
        rows.push_str(&format!("| `{path}` | {kind} | {values} | `{default}` |\n"));
        let (mut item, mut items) = (field, path);
        while let Field::List { item: inner, .. } = item {
            item = inner;
            items.push_str("[]");
        }
        if let Field::Object { fields } = item {
            field_rows(fields, &format!("{items}."), rows);
        }
    }
}

/// The kind of a field and the values it takes, as the doc says them.
fn described(field: &Field) -> (&'static str, String) {
    match field {
        Field::Boolean { .. } => ("boolean", "`true` or `false`".to_string()),
        Field::Choice { options, .. } => {
            let options: Vec<String> = options.iter().map(|o| format!("`{o}`")).collect();
            ("choice", options.join(", "))
        }
        Field::File { .. } => (
            "file",
            "the name of a file under `assets/audio/`, such as `voice.wav`".to_string(),
        ),
        Field::Text { .. } => ("text", "text in quotes".to_string()),
        Field::Number {
            min,
            max,
            integer,
            unit,
            ..
        } => {
            let whole = if *integer {
                "a whole number"
            } else {
                "a number"
            };
            let unit = unit.map_or("", Unit::word);
            ("number", format!("{whole} from {min} to {max}{unit}"))
        }
        Field::List { item, max, .. } => {
            let items = match max {
                Some(max) => format!("up to {max} items"),
                None => "items".to_string(),
            };
            ("list", format!("{items}, each {}", described(item).1))
        }
        Field::Object { .. } => ("object", "with the fields below".to_string()),
    }
}

/// Where the samples of an instance are read: its assets, the rate of the engine and itself,
/// which waits for a sample read on its thread.
type Read<'a> = (&'a Assets, u32, &'a InstanceId);

/// The name in a parent's `children` that stands for any other name.
const ANY_NAME: &str = "*";

/// The tools of one load by name, so a parent finds the inputs of a child.
pub(crate) type Tools = BTreeMap<String, ToolInfo>;

/// The behaviour of a tool: its sound per combination of choices.
pub(crate) struct Sounds {
    info: Arc<ToolInfo>,
    /// Compiled, by [`ToolInfo::sound_key`], or why that sound does not build.
    sounds: HashMap<String, Result<Arc<Program>, String>>,
    /// The Bun that runs its behaviour and the tools of its load, for a tool that owns children.
    parent: Option<(Arc<Bun>, Arc<Tools>)>,
}

impl Sounds {
    /// The behaviour of `info`, with the graph Bun sent for each combination of its choices
    /// compiled. Takes as long as compiling them all: not on the thread that draws.
    pub(crate) fn new(
        info: &ToolInfo,
        graphs: Vec<(Map<String, Value>, Result<Graph, String>)>,
        (bun, tools): (&Arc<Bun>, &Arc<Tools>),
        schemas: &[ToolSchema],
    ) -> Self {
        let sounds: HashMap<String, Result<Arc<Program>, String>> = (graphs.into_iter())
            .map(|(choices, graph)| {
                let key = info.sound_key(&Value::Object(choices));
                let code = graph.and_then(|graph| {
                    let declarations = info.declarations(&graph);
                    let program =
                        graph.program(&declarations).map_err(|error| error.to_string())?;
                    let voices = info.voices > 1;
                    let events = (program.stages.iter())
                        .any(|code| !code.bangs.is_empty() || !code.notes.is_empty());
                    if events && voices {
                        return Err("a sound of more voices than one sends no bangs or notes, as each of its voices would: give it one voice, which follows the newest note".to_string());
                    }
                    if !program.devices.is_empty() && voices {
                        return Err("a sound of more voices than one plays through no device, as each of its voices would play one: filter a voice with lowpass, highpass or bandpass, or put the device after it, in the track's effects or in a parent".to_string());
                    }
                    for device in &program.devices {
                        check_device(device, schemas)?;
                    }
                    Ok(Arc::new(program))
                });
                let code = code.map_err(|error| match info.has_choices() {
                    true => format!("its sound with the choices {key} does not build: {error}"),
                    false => format!("its sound does not build: {error}"),
                });
                (key, code)
            })
            .collect();
        let staged = (sounds.values().flatten()).any(|program| !program.devices.is_empty());
        Self {
            info: Arc::new(ToolInfo {
                staged,
                ..info.clone()
            }),
            sounds,
            parent: info.behaviour.then(|| (bun.clone(), tools.clone())),
        }
    }

    pub(crate) fn info(&self) -> &ToolInfo {
        &self.info
    }

    /// Why each sound that does not build does not.
    pub(crate) fn failures(&self) -> impl Iterator<Item = &String> {
        self.sounds.values().filter_map(|code| code.as_ref().err())
    }

    /// The tool for the core.
    pub(crate) fn json_tool(self) -> JsonTool {
        let info = self.info.clone();
        JsonTool {
            name: info.name.clone(),
            check: Arc::new({
                let info = info.clone();
                move |state: &Value| info.check(state)
            }),
            doc: Some(JsonToolDoc {
                when: info.when.clone(),
                markdown: info.doc(),
            }),
            behaviour: Box::new(move |state, context| self.apply(state, context)),
            // A sample its record names may come after the record, as an audio clip's may.
            asset_folders: vec![sound_media::AUDIO_FOLDER],
            owns_children: info.behaviour,
        }
    }

    /// The numbers of the tool a lane moves, in order, with the range it moves each one over
    /// and their default: those `code` reads that are not whole.
    fn moved<'a>(&'a self, code: &'a Code) -> impl Iterator<Item = (&'a str, ValueRange, f32)> {
        let fields = self.info.fields.0.iter();
        fields.filter_map(move |(name, field)| match field {
            Field::Number {
                min,
                default,
                integer: false,
                ..
            } if code
                .parameters
                .iter()
                .any(|parameter| parameter.name == *name) =>
            {
                let default = default.unwrap_or(*min) as f32;
                Some((name.as_str(), field.range()?, default))
            }
            _ => None,
        })
    }

    /// The compiled sound for the choices of `state`.
    fn program(&self, state: &Value) -> Result<Arc<Program>, String> {
        let key = self.info.sound_key(state);
        // A checked record has one of the options of each choice, and Bun sent each combination.
        (self.sounds.get(&key).cloned())
            .unwrap_or_else(|| Err(format!("Bun sent no sound for the choices {key}")))
    }

    /// Where the params and lists of `code` stand in `state`: a number or a boolean by the name
    /// of its `param` line, a list by its name.
    fn values(&self, code: &Code, state: &Value) -> Values {
        let mut values = Values::default();
        // The numbers a lane moves, in the order of `automated`: each one's param.
        values.automated = (self.moved(code))
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

    /// The lists of `code` from `state`: a list of numbers as the record holds it, a file as its
    /// sound, which is silence while it cannot be read.
    fn lists(&self, code: &Code, state: &Value, (assets, rate, id): Read<'_>) -> Vec<Vec<f32>> {
        let list = |array: &ArraySpec| {
            if let Some(Field::File { .. }) = self.info.field(&array.name) {
                let file = state
                    .get(&array.name)
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if file.is_empty() {
                    return Vec::new();
                }
                let sound = samples::sample(assets, file, rate, id);
                // Silent while it is read, or when it cannot be.
                return (sound.ok().flatten()).map_or_else(Vec::new, |sound| sound.to_vec());
            };
            let items = state.get(&array.name).and_then(Value::as_array);
            let numbers = items
                .into_iter()
                .flatten()
                .take(sound_signals::MAX_ARRAY_LENGTH);
            let numbers = numbers.map(|item| item.as_f64().map_or(array.default, |n| n as f32));
            numbers.collect()
        };
        code.arrays.iter().map(list).collect()
    }

    /// What the lists of `state` are made of: the lists of numbers, and the files with whether
    /// each could be read, so a file that arrives after its record is sent then. With a problem
    /// for each file that cannot be read.
    fn lists_key(
        &self,
        code: &Code,
        state: &Value,
        (assets, rate, id): Read<'_>,
    ) -> (u64, Vec<String>) {
        let mut hasher = DefaultHasher::new();
        let mut problems = Vec::new();
        for array in &code.arrays {
            let value = state.get(&array.name);
            value.map(Value::to_string).hash(&mut hasher);
            let file = value.and_then(Value::as_str).unwrap_or_default();
            let is_file = matches!(self.info.field(&array.name), Some(Field::File { .. }));
            if is_file && !file.is_empty() {
                let read = samples::sample(assets, file, rate, id);
                // Which read it is, not its length: a file read again at the same length is a
                // new sound too.
                read.as_ref()
                    .map(|sound| sound.as_ref().map(|sound| Arc::as_ptr(sound).addr()))
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
        let program = self.program(state).map_err(BehaviourError::Other)?;
        let Some(code) = program.stages.first() else {
            return Err(BehaviourError::Other("its sound has no stage".to_string()));
        };
        let kind = self.info.kind();
        let mut values = self.values(&code, state);
        let (assets, rate, id) = (
            context.assets().clone(),
            context.prepare_config().sample_rate,
            context.id().clone(),
        );
        let reading = (&assets, rate, &id);
        // A list, a sample most of all, is sent only when it changed: not at every knob turn.
        let (lists_key, problems) = self.lists_key(&code, state, reading);
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
        // Only a tool that hears them writes what reaches its notes.
        let tap = (self.info.notes).then(|| context.tap::<NoteEvent>(NOTES_TAP));
        let new_tap = context.changed("tap", u64::from(tap.is_some()));
        let sample_rate = context.prepare_config().sample_rate as f32;
        let new_code = context.changed("code", code.hash);
        if new_code || new_lists {
            values.arrays = Some(self.lists(&code, state, reading));
        }
        let made = || Box::new(Machine::new(code.clone(), sample_rate));
        // Made once and cloned for every voice: scheduling the code is not free.
        let mut machines: Vec<Option<Box<Machine>>> = match new_code {
            true => vec![Some(made()); kind.machines()],
            false => Vec::new(),
        };
        let mut created = false;
        let signals = context.processor(self.info.processor(), || {
            created = true;
            let first = machines.first_mut().and_then(Option::take);
            let lists = (values.arrays.take()).unwrap_or_else(|| self.lists(&code, state, reading));
            let values = Values {
                arrays: Some(lists),
                ..values.clone()
            };
            let signals = Signals::new(kind, first.unwrap_or_else(made), values, watches.clone());
            match self.info.staged {
                true => signals.with_stage_ports(),
                false => signals,
            }
        })?;
        if created || new_tap {
            context.update(signals, SignalsUpdate::Tap(tap))?;
        }
        // A new processor already plays this code with these values.
        if !created {
            let watches = if new_code { watches } else { Vec::new() };
            context.update(
                signals,
                SignalsUpdate::Set {
                    machines,
                    values: Box::new(values),
                    watches,
                },
            )?;
        }
        // A parent's own inputs are ends of its connections, see `declare`. One voice hears
        // what comes in, also when its sound does not read it, as an effect that plays notes.
        if self.parent.is_none() {
            if kind == Kind::One {
                context.input(AUDIO_INPUT, InputEndpoint::new(signals, Signals::INPUT));
            }
            context.input(NOTES_INPUT, InputEndpoint::new(signals, Signals::NOTES));
        }
        let mut stages = vec![signals];
        for (index, code) in program.stages.iter().enumerate().skip(1) {
            stages.push(self.stage(index, code, context)?);
        }
        let sound = devices(&program, &stages, context)?;
        if let Some(sound) = sound {
            context.output(AUDIO_OUTPUT, sound);
        }
        for (signals, code) in stages.iter().zip(&program.stages) {
            for (index, name) in code.numbers.iter().enumerate() {
                context.output(name, OutputEndpoint::new(*signals, Signals::number(index)));
            }
            for (index, name) in code.bangs.iter().enumerate() {
                context.output(name, OutputEndpoint::new(*signals, Signals::bang(index)));
            }
            for (index, name) in code.notes.iter().enumerate() {
                context.output(name, OutputEndpoint::new(*signals, Signals::notes(index)));
            }
        }
        // A parent's cable reaches an input here; the place comes with it.
        for (name, _) in &self.info.inputs.0 {
            context.input(name, InputEndpoint::new(signals, Signals::CONTROLS));
        }
        // Every number the sound reads can be automated, as a number of any device, but a whole
        // number, a boolean or a list, which are no straight line.
        let numbers: Vec<(ParameterInfo, f32)> = (self.moved(code))
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
            let input = InputEndpoint::new(signals, Signals::AUTOMATION);
            context.runtime_automation(input, numbers)?;
        }
        if let Some(parent) = &self.parent {
            let own = own_ports(signals, context)?;
            self.declare(parent, state, own, context)?;
        }
        Ok(())
    }

    /// The processor of the stage `index` of its sound, which plays `code`: it hears only what
    /// the stages and devices before give it.
    fn stage(
        &self,
        index: usize,
        code: &Code,
        context: &mut BehaviourContext<'_>,
    ) -> Result<Node<Signals>, BehaviourError> {
        let name = format!("{}~{index}", self.info.processor());
        let watches: Vec<Watch> = (code.watches.iter())
            .map(|watch| context.watch(watch))
            .collect();
        let new_code = context.changed(&name, code.hash);
        let sample_rate = context.prepare_config().sample_rate as f32;
        let made = || Box::new(Machine::new(code.clone(), sample_rate));
        let mut created = false;
        let signals = context.processor(&name, || {
            created = true;
            let watches = watches.clone();
            Signals::new(Kind::One, made(), Values::default(), watches).with_stage_ports()
        })?;
        if new_code && !created {
            let update = SignalsUpdate::Set {
                machines: vec![Some(made())],
                values: Box::default(),
                watches,
            };
            context.update(signals, update)?;
        }
        Ok(signals)
    }

    /// Connects the children of an owner and its `own` ports as its behaviour declares: what
    /// Bun answered for its record and its children, or the last declaration while Bun works
    /// it out.
    fn declare(
        &self,
        (bun, tools): &(Arc<Bun>, Arc<Tools>),
        state: &Value,
        own: Own,
        context: &mut BehaviourContext<'_>,
    ) -> Result<(), BehaviourError> {
        let id = context.id().clone();
        let reads = behaviours::reads_of(context.assets(), &id);
        let child_fields = reads.as_ref().and_then(|reads| reads.children.as_deref());
        let children = kept_children(
            (context.assets(), &id),
            context.child_stamps(),
            child_fields,
        )
        .map_err(|error| BehaviourError::Other(error.to_string()))?;
        let read = reads.as_ref().map(|reads| {
            let own = match &reads.state {
                Some(fields) => picked(state, fields),
                None => state.to_string(),
            };
            key(&(&self.info.name, &own, children.read_key))
        });
        let state = state.to_string();
        let key = (key(&(&self.info.name, &state, children.key)), read);
        let tell = || {
            let told = (children.children.iter())
                .map(|child| (&child.id, child.tool, child.key, &*child.json));
            bun.tell_owner((&id, &self.info.name, &state), told)
        };
        self.check_children(&children.children, context);
        let instance = (context.assets(), &id);
        let declared = behaviours::declared(bun, instance, &self.info.name, key, tell);
        if let Some(problem) = declared.problem {
            context.problem(format!("{problem}. It plays what it declared last"));
        }
        // Only the children a connection plays a number or a bang into.
        let controlled = (declared.declaration.connections.iter())
            .filter(|connection| matches!(connection.to.port(), Some((Key::Number | Key::Bang, _))))
            .filter_map(|connection| connection.to.child.as_deref());
        let child_tools: BTreeMap<String, &ToolInfo> = controlled
            .filter_map(|name| {
                let child = (children.children.iter()).find(|child| child.id.name() == name)?;
                Some((name.to_string(), tools.get(child.tool)?))
            })
            .collect();
        wire(&declared.declaration, own, &child_tools, context)
    }

    /// A problem for each child that is not of the tool its `children` name, and each they
    /// name that it does not have.
    fn check_children(&self, children: &[KeptChild], context: &mut BehaviourContext<'_>) {
        let Some(named) = &self.info.children else {
            return;
        };
        // `*` stands for any name it does not name.
        let tool_of = |name: &str| {
            let find = |wanted: &str| named.0.iter().find(|(named, _)| named == wanted);
            find(name).or_else(|| find(ANY_NAME)).map(|(_, tool)| tool)
        };
        for child in children {
            let name = child.id.name();
            match tool_of(name) {
                Some(tool) if tool == child.tool => {}
                Some(tool) => context.problem(format!(
                    "its child {name} is a {}, and its `children` say {tool}",
                    child.tool
                )),
                None if named.0.is_empty() => context.problem(format!(
                    "its child {name} is none of its `children`, which name none"
                )),
                None => context.problem(format!(
                    "its child {name} is none of its `children`: {}",
                    named.names()
                )),
            }
        }
        for (name, tool) in &named.0 {
            if name != ANY_NAME && !children.iter().any(|child| child.id.name() == name) {
                context.problem(format!(
                    "it has no child {name}, a {tool}, as its `children` say"
                ));
            }
        }
    }
}

/// Whether the record of `device` without the numbers a signal moves is one its tool of
/// `schemas` takes: what it plays with is known as the tool loads.
fn check_device(device: &DeviceUse, schemas: &[ToolSchema]) -> Result<(), String> {
    let tool = &device.tool;
    let schema = (schemas.iter()).find(|schema| schema.tool == tool && schema.in_code);
    let Some(schema) = schema else {
        return Err(format!("it plays a {tool:?}, which is no device"));
    };
    (schema.check)(Value::Object(device.state.clone())).map_err(|error| {
        let error = (error.strip_prefix("state."))
            .or_else(|| error.strip_prefix("state: "))
            .unwrap_or(&error);
        format!("devices.{tool}: {error}")
    })
}

/// Plays the devices of `program` between its `stages`, with a cable for each number a signal
/// moves, and gives where the sound of the tool comes from. A device that cannot play is a
/// problem, and what goes through it is silent.
fn devices(
    program: &Program,
    stages: &[Node<Signals>],
    context: &mut BehaviourContext<'_>,
) -> Result<Option<OutputEndpoint>, BehaviourError> {
    let mut parts = Vec::new();
    for (index, device) in program.devices.iter().enumerate() {
        let state = Value::Object(device.state.clone());
        parts.push(context.part(&format!("device{index}"), &device.tool, state)?);
    }
    let output = |from: LinkFrom| match from {
        LinkFrom::Export { stage, port } => {
            (stages.get(stage)).map(|stage| OutputEndpoint::new(*stage, Signals::export(port)))
        }
        LinkFrom::Device(device) => (parts.get(device).and_then(Option::as_ref))
            .and_then(|part| part.outputs.get(AUDIO_OUTPUT).copied()),
    };
    for link in &program.links {
        let input = match link.to {
            LinkTo::Import { stage, port } => {
                (stages.get(stage)).map(|stage| InputEndpoint::new(*stage, Signals::import(port)))
            }
            LinkTo::Device(device) => (parts.get(device).and_then(Option::as_ref))
                .and_then(|part| part.inputs.get(AUDIO_INPUT).copied()),
        };
        if let (Some(output), Some(input)) = (output(link.from), input) {
            context.connect(output.to(input))?;
        }
    }
    let mut made = BTreeSet::new();
    for (index, (device, part)) in program.devices.iter().zip(&parts).enumerate() {
        let Some(part) = part else {
            continue;
        };
        for number in &device.numbers {
            let place = part.automation.as_ref().and_then(|(input, numbers)| {
                let place = numbers
                    .iter()
                    .position(|info| *info.field == *number.field)?;
                Some((*input, u16::try_from(place).unwrap_or(u16::MAX)))
            });
            let Some(into) = place else {
                context.problem(format!(
                    "the {} of its sound has no number {:?} that a signal moves",
                    device.tool, number.field
                ));
                continue;
            };
            let name = format!("device{index}:{}", number.field);
            let cable = cable::<Number, Automation>(name, into, None, &mut made, context)?;
            if let (Some(cable), Some(stage)) = (cable, stages.get(number.stage)) {
                let sent = OutputEndpoint::new(*stage, Signals::device_number(number.index));
                context.connect(sent.to(cable))?;
            }
        }
    }
    Ok(match program.sound {
        Sound::Stage(stage) => {
            (stages.get(stage)).map(|stage| OutputEndpoint::new(*stage, Signals::OUTPUT))
        }
        Sound::Device(device) => output(LinkFrom::Device(device)),
    })
}

/// The children of an owner as its behaviour last read them, in name order, and a key of all
/// of them.
#[derive(Default)]
struct Kept {
    children: Vec<KeptChild>,
    key: u64,
    /// Of the children's names and tools, and the fields of their states in `fields`.
    read_key: u64,
    /// The fields of a child's state the behaviour read; `None` for all.
    fields: Option<Vec<String>>,
}

struct KeptChild {
    id: InstanceId,
    tool: &'static str,
    stamp: RecordStamp,
    json: Arc<str>,
    /// Of its tool and its state.
    key: u64,
    /// Of its tool and the fields of its state the behaviour read.
    read_key: u64,
}

/// The fields `fields` of `state`, as text to make a key of: a missing one is `null`.
fn picked(state: &Value, fields: &[String]) -> String {
    let values: Vec<(&str, Option<&Value>)> = (fields.iter())
        .map(|field| (field.as_str(), state.get(field)))
        .collect();
    serde_json::to_string(&values).unwrap_or_default()
}

/// The key of the fields `fields` of a child's state, `json`; of all of it with no `fields`.
fn read_key(
    tool: &str,
    json: &str,
    full: u64,
    fields: Option<&[String]>,
) -> Result<u64, serde_json::Error> {
    let Some(fields) = fields else {
        return Ok(full);
    };
    let state: Value = serde_json::from_str(json)?;
    Ok(key(&(tool, picked(&state, fields))))
}

/// What each owner of every project made of the state of each child, so its behaviour reads a
/// child again only when it changed: a parent of a thousand takes edits its own record about
/// as fast as one of none.
static KEPT: Mutex<BTreeMap<(Assets, InstanceId), Arc<Kept>>> = Mutex::new(BTreeMap::new());

/// The children of `owner` from their `stamps`, each read again only when its state changed
/// since the last time.
fn kept_children<'a>(
    (assets, owner): (&Assets, &InstanceId),
    stamps: impl Iterator<Item = (&'a str, &'static str, RecordStamp)>,
    fields: Option<&[String]>,
) -> Result<Arc<Kept>, Box<dyn std::error::Error>> {
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    let entry = (assets.clone(), owner.clone());
    let last = kept.get(&entry).cloned().unwrap_or_default();
    let stamps: Vec<(&str, &'static str, RecordStamp)> = stamps.collect();
    let is_known = |(name, tool, stamp): &(&str, &str, RecordStamp), child: &KeptChild| {
        child.id.name() == *name && child.tool == *tool && child.stamp == *stamp
    };
    let same_fields = last.fields.as_deref() == fields;
    let unchanged = same_fields
        && stamps.len() == last.children.len()
        && (stamps.iter().zip(&last.children)).all(|(stamp, child)| is_known(stamp, child));
    if unchanged {
        return Ok(last);
    }
    let mut children = Vec::with_capacity(stamps.len());
    let mut known = last.children.iter().peekable();
    for (name, tool, stamp) in stamps {
        // Both in name order.
        while known.next_if(|child| child.id.name() < name).is_some() {}
        let child = match known.next_if(|child| is_known(&(name, tool, stamp.clone()), child)) {
            Some(child) => KeptChild {
                id: child.id.clone(),
                tool,
                stamp,
                json: child.json.clone(),
                key: child.key,
                read_key: match same_fields {
                    true => child.read_key,
                    false => read_key(tool, &child.json, child.key, fields)?,
                },
            },
            None => {
                let json: Arc<str> = stamp.state_json()?.into();
                let key = crate::bun::record_key(tool, &json);
                let id = owner.child(name)?;
                KeptChild {
                    id,
                    tool,
                    stamp,
                    read_key: read_key(tool, &json, key, fields)?,
                    json,
                    key,
                }
            }
        };
        children.push(child);
    }
    let keys: Vec<(&str, u64)> = (children.iter())
        .map(|child| (child.id.name(), child.key))
        .collect();
    let read_keys: Vec<(&str, u64)> = (children.iter())
        .map(|child| (child.id.name(), child.read_key))
        .collect();
    let fresh = Arc::new(Kept {
        key: key(&keys),
        read_key: key(&read_keys),
        fields: fields.map(<[String]>::to_vec),
        children,
    });
    kept.insert(entry, fresh.clone());
    Ok(fresh)
}

/// The name of the tap of what reaches the `notes` of a tool that hears them.
pub(crate) const NOTES_TAP: &str = "notes";

/// The name of the [`NoteFeed`] of the child `name` of an owner, which a performer plays it
/// through.
pub(crate) fn feed_processor(name: &str) -> String {
    format!("feed:{name}")
}

/// What a connection carries. Each end of one names its kind, and both ends name the same.
#[derive(Clone, Copy, PartialEq)]
enum Carries {
    Sound,
    Notes,
    Number,
    Bang,
}

impl Key {
    fn carries(self) -> Carries {
        match self {
            Key::Port => Carries::Sound,
            Key::Notes => Carries::Notes,
            Key::Number => Carries::Number,
            Key::Bang => Carries::Bang,
        }
    }
}

/// The ports of an owner itself, as ends of its connections: what reaches its `audio` input and
/// its `notes`, and its own sound, which is its `audio` output. Its other outputs are made as
/// its connections name them.
pub(crate) struct Own {
    audio: OutputEndpoint,
    notes: OutputEndpoint,
    sound: InputEndpoint,
}

/// Declares the inputs of an owner whose own sound is `signals`. Its `notes` reach its sound
/// too, as every tool hears its `notes`.
fn own_ports(
    signals: Node<Signals>,
    context: &mut BehaviourContext<'_>,
) -> Result<Own, BehaviourError> {
    let audio = context.processor("in:audio", AudioThru::default)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(audio, AudioThru::INPUT));
    let notes = context.processor("in:notes", EventThru::<NoteEvent>::default)?;
    let input = InputEndpoint::new(notes, EventThru::<NoteEvent>::INPUT);
    context.input(NOTES_INPUT, input);
    let notes = OutputEndpoint::new(notes, EventThru::<NoteEvent>::OUTPUT);
    context.connect(notes.to(InputEndpoint::new(signals, Signals::NOTES)))?;
    Ok(Own {
        audio: OutputEndpoint::new(audio, AudioThru::OUTPUT),
        notes,
        sound: InputEndpoint::new(signals, Signals::INPUT),
    })
}

/// What a connection may name, for a problem that says so.
const ENDS: &str = "a sound goes from { child, port? } or { port: \"audio\" } to the same, notes from { child, notes } or { notes: \"notes\" } to { child?, notes }, a number from { child, number } to { child?, number }, a bang from { child, bang } to { child?, bang }";

/// Makes the connections of `declaration`, between the children and the `own` ports of the
/// owner, with the tool of each child that is a tool of this project in `child_tools`. A
/// connection that names a child or a port the owner does not have, or joins two kinds, is a
/// problem, and the others play.
///
/// What plays into the `notes` of a child goes through a [`NoteFeed`] of that child, which
/// releases what the child holds when that changes: a sender that went away or plays elsewhere
/// sends no off there.
fn wire(
    declaration: &Declaration,
    own: Own,
    child_tools: &BTreeMap<String, &ToolInfo>,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let fed = (declaration.connections.iter()).filter_map(|connection| {
        let to = &connection.to;
        let notes = matches!(to.port(), Some((Key::Port | Key::Notes, NOTES_INPUT)));
        to.child.as_deref().filter(|_| notes)
    });
    let feeds = feeds(fed, context)?;
    let mut cables = BTreeSet::new();
    let mut outputs = BTreeMap::new();
    // By its processor too, so a sender made again is another sender; not by its port, which
    // new code of the same sender may move.
    let mut fed: BTreeMap<&str, BTreeSet<(&End, NodeId)>> = BTreeMap::new();
    for (index, connection) in declaration.connections.iter().enumerate() {
        let at = format!("connections[{index}]");
        let (Some((from_key, from_name)), Some((to_key, to_name))) =
            (connection.from.port(), connection.to.port())
        else {
            context.problem(format!(
                "its behaviour declares {at} with an end that names no port, or more than one: {ENDS}"
            ));
            continue;
        };
        let from_child = connection.from.child.as_deref();
        let to_child = connection.to.child.as_deref();
        if from_key.carries() != to_key.carries() {
            let (from, to) = (&connection.from, &connection.to);
            context.problem(format!(
                "its behaviour declares {at} from {from} to {to}, which do not connect: {ENDS}"
            ));
            continue;
        }
        let output = match (from_child, from_key) {
            (Some(child), _) => context.child_output(child, from_name),
            (None, Key::Port) if from_name == AUDIO_INPUT => Some(own.audio),
            (None, Key::Notes) if from_name == NOTES_INPUT => Some(own.notes),
            (None, _) => None,
        };
        let Some(output) = output else {
            let problem = match from_child {
                Some(child) => missing(
                    context,
                    &format!("{at}.from"),
                    (child, from_name),
                    Side::Output,
                ),
                None => format!(
                    "its behaviour declares {at} from its own {from_name:?}, which it does not have: its own ports to connect from are {{ port: \"audio\" }} and {{ notes: \"notes\" }}"
                ),
            };
            context.problem(problem);
            continue;
        };
        let at = format!("{at}.to");
        let name = format!("cable:{:?}>{:?}", connection.from, connection.to);
        let into = match (to_child, to_key) {
            (None, Key::Port) if to_name == AUDIO_OUTPUT => Some((own.sound, None)),
            (None, Key::Port) => {
                context.problem(format!(
                    "its behaviour declares {at}, its own sound output {to_name:?}, which it does not have: its own sound output is {{ port: \"audio\" }}"
                ));
                None
            }
            (None, Key::Notes) => own_output::<NoteEvent>(to_name, &mut outputs, &at, context)?,
            (None, Key::Number) => own_output::<Number>(to_name, &mut outputs, &at, context)?,
            (None, Key::Bang) => own_output::<Bang>(to_name, &mut outputs, &at, context)?,
            (Some(child), Key::Port | Key::Notes) => {
                input_of(&feeds, (child, to_name), &at, context)
            }
            // A number input plays the number; else a lane's input moves the field.
            (Some(child), Key::Number) if takes_number(child, to_name, child_tools, context) => {
                let number = (child, to_name, true);
                (control::<Number>(name, number, child_tools, &mut cables, &at, context)?)
                    .map(|input| (input, None))
            }
            (Some(child), Key::Number) => {
                match automation(child, to_name, &at, child_tools, context) {
                    Some(into) => {
                        // What it plays, for a view that shows the number as it plays.
                        let shown = context.watch(&played_name(&format!("{child}.{to_name}")));
                        (cable::<Number, Automation>(
                            name,
                            into,
                            Some(shown),
                            &mut cables,
                            context,
                        )?)
                        .map(|input| (input, None))
                    }
                    None => None,
                }
            }
            (Some(child), Key::Bang) => {
                let bang = (child, to_name, false);
                (control::<Bang>(name, bang, child_tools, &mut cables, &at, context)?)
                    .map(|input| (input, None))
            }
        };
        let Some((input, feed)) = into else {
            continue;
        };
        match context.connect(output.to(input)) {
            Err(BehaviourError::Graph(GraphError::PortTypeMismatch { description, .. })) => {
                context.problem(format!(
                    "its behaviour declares {at}, {description}, between ports of two kinds, such as sound and notes"
                ));
                continue;
            }
            connected => connected?,
        }
        if let Some(child) = feed {
            let sender = (&connection.from, output.node);
            fed.entry(child).or_default().insert(sender);
        }
    }
    for (child, feed) in &feeds {
        let senders = fed.get(child.as_str());
        if context.changed(&format!("feed:{child}.senders"), key(&senders)) {
            context.update(*feed, NoteFeedUpdate::Senders)?;
        }
    }
    Ok(())
}

/// Where a connection `at` of events `E` goes out of the owner as its own output `name`, which
/// the first connection to it makes and every one meets at. `None` when its sound has an output
/// of that name, a problem.
fn own_output<'a, E: Event>(
    name: &str,
    outputs: &mut BTreeMap<String, InputEndpoint>,
    at: &str,
    context: &mut BehaviourContext<'_>,
) -> Result<Option<(InputEndpoint, Option<&'a str>)>, BehaviourError> {
    if let Some(input) = outputs.get(name) {
        return Ok(Some((*input, None)));
    }
    if context.has_output(name) {
        context.problem(format!(
            "its behaviour declares {at}, its own output {name:?}, which its sound has: give it another name"
        ));
        return Ok(None);
    }
    let thru = context.processor(&format!("out:{name}"), EventThru::<E>::default)?;
    context.output(name, OutputEndpoint::new(thru, EventThru::<E>::OUTPUT));
    let input = InputEndpoint::new(thru, EventThru::<E>::INPUT);
    outputs.insert(name.to_string(), input);
    Ok(Some((input, None)))
}

/// Where a connection `at` into the port `port` of `child` goes: its feed for its `notes`, with
/// the name of the child it feeds. `None` when the child has no such input, a problem.
fn input_of<'a>(
    feeds: &'a BTreeMap<String, Node<NoteFeed>>,
    (child, port): (&str, &str),
    at: &str,
    context: &mut BehaviourContext<'_>,
) -> Option<(InputEndpoint, Option<&'a str>)> {
    let feed = feeds.get_key_value(child).filter(|_| port == NOTES_INPUT);
    match (feed, context.child_input(child, port)) {
        (Some((child, feed)), _) => Some((
            InputEndpoint::new(*feed, NoteFeed::INPUT),
            Some(child.as_str()),
        )),
        (None, Some(input)) => Some((input, None)),
        (None, None) => {
            let problem = missing(context, at, (child, port), Side::Input);
            context.problem(problem);
            None
        }
    }
}

/// The automation input of `child` and the place of `number` there, the number field or the
/// number of a built-in device a lane moves. `None` when it takes no such number, a problem,
/// which names the number inputs too of a tool of this project in `child_tools`.
fn automation(
    child: &str,
    number: &str,
    at: &str,
    child_tools: &BTreeMap<String, &ToolInfo>,
    context: &mut BehaviourContext<'_>,
) -> Option<(InputEndpoint, u16)> {
    let found = context
        .child_automation(child)
        .and_then(|(automation, numbers)| {
            let place = numbers.iter().position(|info| *info.field == *number)?;
            // At most `MAX_AUTOMATED` numbers, far under `u16::MAX`.
            Some((automation, u16::try_from(place).unwrap_or(u16::MAX)))
        });
    if found.is_none() {
        let mut problem = missing(context, at, (child, number), Side::Number);
        if let Some(info) = child_tools.get(child) {
            let inputs: Vec<&str> = (info.inputs.0.iter())
                .filter(|(_, input)| matches!(input, Input::Number { .. }))
                .map(|(name, _)| name.as_str())
                .collect();
            let inputs = match inputs.is_empty() {
                true => "none".to_string(),
                false => inputs.join(", "),
            };
            problem.push_str(&format!("; its number inputs are {inputs}"));
        }
        context.problem(problem);
    }
    found
}

/// Whether `child` takes `name` as a number input, which a cable plays as a performer would,
/// and not as a field a lane moves: an input of a tool of this project, or a number of another
/// tool that a lane does not move, such as `launch` of a timeline.
fn takes_number(
    child: &str,
    name: &str,
    child_tools: &BTreeMap<String, &ToolInfo>,
    context: &BehaviourContext<'_>,
) -> bool {
    match child_tools.get(child) {
        Some(info) => matches!(info.input(name), Some((_, Input::Number { .. }))),
        None => {
            let moved = (context.child_automation(child))
                .is_some_and(|(_, numbers)| numbers.iter().any(|info| *info.field == *name));
            !moved && context.child_input(child, name).is_some()
        }
    }
}

/// Where a connection `at` into the number input `name` of `child` goes, or into its bang when
/// not `number`. A tool of this project in `child_tools` takes all of them on one input, so a
/// cable says which; another, such as a timeline, takes each on an input of its own.
fn control<From: Event + Shown>(
    cable_name: String,
    (child, name, number): (&str, &str, bool),
    child_tools: &BTreeMap<String, &ToolInfo>,
    made: &mut BTreeSet<String>,
    at: &str,
    context: &mut BehaviourContext<'_>,
) -> Result<Option<InputEndpoint>, BehaviourError>
where
    sound_signals::Control: Placed<From>,
{
    let input = context.child_input(child, name);
    let Some(info) = child_tools.get(child) else {
        if input.is_none() {
            let problem = missing(context, at, (child, name), Side::Input);
            context.problem(problem);
        }
        return Ok(input);
    };
    match (info.input(name), input) {
        (Some((index, found)), Some(input)) if matches!(found, Input::Number { .. }) == number => {
            // At most `MAX_LIVES` of each kind, far under `u16::MAX`.
            let place = u16::try_from(index).unwrap_or(u16::MAX);
            let into = (input, place);
            cable::<From, sound_signals::Control>(cable_name, into, None, made, context)
        }
        _ => {
            let kind = if number { "number input" } else { "bang input" };
            let names: Vec<&str> = (info.inputs.0.iter())
                .filter(|(_, input)| matches!(input, Input::Number { .. }) == number)
                .map(|(name, _)| name.as_str())
                .collect();
            let names = match names.is_empty() {
                true => "none".to_string(),
                false => names.join(", "),
            };
            context.problem(format!(
                "its behaviour names the {kind} {name:?} of {child:?} at {at}, which it does not have: its {kind}s are {names}"
            ));
            Ok(None)
        }
    }
}

/// A [`Cable`] named `name` that delivers what reaches it at `place` of `into`. `None` for a
/// cable `made` already: the same connection twice is one.
fn cable<From: Event + Shown, To: Placed<From>>(
    name: String,
    (into, place): (InputEndpoint, u16),
    shown: Option<Watch>,
    made: &mut BTreeSet<String>,
    context: &mut BehaviourContext<'_>,
) -> Result<Option<InputEndpoint>, BehaviourError> {
    if !made.insert(name.clone()) {
        return Ok(None);
    }
    let cable = context.processor(&name, || match shown {
        Some(watch) => {
            // Nothing shows until it delivers.
            watch.set(f32::NAN);
            Cable::<From, To>::showing(watch)
        }
        None => Cable::<From, To>::default(),
    })?;
    if context.changed(&name, key(&place)) {
        context.update(cable, place)?;
    }
    context.connect(OutputEndpoint::new(cable, Cable::<From, To>::OUTPUT).to(into))?;
    Ok(Some(InputEndpoint::new(cable, Cable::<From, To>::INPUT)))
}

/// The children of every project that have had a [`NoteFeed`]: their feed stays, to release
/// what a sender that went away left sounding.
static FED: Mutex<BTreeSet<(Assets, InstanceId)>> = Mutex::new(BTreeSet::new());

/// Asks for a feed for `child`, for a performer to play it. Whether it had none: then its owner
/// must run its behaviour again to make it.
pub(crate) fn feed_child(assets: &Assets, child: &InstanceId) -> bool {
    let mut fed = FED.lock().unwrap_or_else(PoisonError::into_inner);
    fed.insert((assets.clone(), child.clone()))
}

/// The [`NoteFeed`] of each child that has `notes` and is `fed` now or was before, by the
/// child's name. Only those, so a parent of a thousand takes declares no thousand feeds.
fn feeds<'a>(
    fed: impl Iterator<Item = &'a str>,
    context: &mut BehaviourContext<'_>,
) -> Result<BTreeMap<String, Node<NoteFeed>>, BehaviourError> {
    let owner = context.id().clone();
    let assets = context.assets().clone();
    let children: BTreeSet<String> = {
        let mut known = FED.lock().unwrap_or_else(PoisonError::into_inner);
        for child in fed {
            if let Ok(id) = owner.child(child) {
                known.insert((assets.clone(), id));
            }
        }
        let start = (assets.clone(), owner.clone());
        (known.range(start..))
            .take_while(|(project, _)| *project == assets)
            .filter(|(_, id)| id.parent().as_ref() == Some(&owner))
            .map(|(_, id)| id.name().to_string())
            .collect()
    };
    let mut feeds = BTreeMap::new();
    for child in children {
        let Some(notes) = context.child_input(&child, NOTES_INPUT) else {
            continue;
        };
        let feed = context.processor(&feed_processor(&child), NoteFeed::default)?;
        match context.connect(OutputEndpoint::new(feed, NoteFeed::OUTPUT).to(notes)) {
            // An input named so that takes something else than notes.
            Err(BehaviourError::Graph(GraphError::PortTypeMismatch { .. })) => continue,
            connected => connected?,
        }
        feeds.insert(child, feed);
    }
    Ok(feeds)
}

pub(crate) fn key(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Copy)]
enum Side {
    Output,
    Input,
    /// A number the child takes automation for.
    Number,
}

/// What is wrong with the port `(child, port)` at `at` of a declaration, whose child or port is
/// not there.
fn missing(
    context: &BehaviourContext<'_>,
    at: &str,
    (child, port): (&str, &str),
    side: Side,
) -> String {
    let Some((outputs, inputs)) = context.child_ports(child) else {
        let children: Vec<&str> = context.child_names().collect();
        let children = match children.is_empty() {
            true => "it owns none".to_string(),
            false => format!("its children are {}", children.join(", ")),
        };
        return format!(
            "its behaviour names the child {child:?} at {at}, which it does not own: {children}"
        );
    };
    let (side, ports) = match side {
        Side::Output => ("output", outputs),
        Side::Input => ("input", inputs),
        Side::Number => {
            let numbers = context.child_automation(child).map(|(_, numbers)| numbers);
            let numbers = numbers.unwrap_or_default().iter();
            ("number", numbers.map(|info| &*info.field).collect())
        }
    };
    let ports = match ports.is_empty() {
        true => "none".to_string(),
        false => ports.join(", "),
    };
    format!(
        "its behaviour names the {side} {port:?} of {child:?} at {at}, which it does not have: its {side}s are {ports}"
    )
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

    /// A list of notes, as `notes()` of the SDK makes it.
    fn notes() -> Value {
        let number = |min: u32, max: u64, default: u32| json!({ "kind": "number", "min": min, "max": max, "default": default, "integer": true });
        json!({ "kind": "list", "item": { "kind": "object", "fields": {
            "start": number(0, 1 << 31, 0),
            "length": number(1, 1 << 31, 1),
            "pitch": number(0, 127, 60),
            "velocity": number(1, 127, 100)
        } } })
    }

    fn wobble() -> ToolInfo {
        serde_json::from_value(json!({
            "name": "wobble",
            "file": "wobble.ts",
            "title": "Wobble",
            "when": "You want the volume to move",
            "doc": "A tremolo.",
            "voices": 1,
            "input": true,
            "fields": {
                "rate": { "kind": "number", "min": 0.1, "max": 20, "default": 4, "unit": "hz" },
                "shape": { "kind": "choice", "options": ["sine", "square"], "default": "sine" },
                "bypass": { "kind": "boolean", "default": false },
                "accents": {
                    "kind": "list", "max": 4, "default": [0, 0, 0, 0],
                    "item": { "kind": "number", "min": 0, "max": 1 }
                }
            },
            "inputs": {
                "x": { "kind": "number", "min": 0, "max": 1, "default": 0 },
                "hit": { "kind": "bang" },
                "y": { "kind": "number", "min": 0, "max": 1, "default": 0 }
            },
            "tick": false,
            "view": false,
            "keys": false,
            "notes": false,
            "behaviour": false
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
                json!({ "accents": [1, 0, 1, 0, 1] }),
                "state.accents: holds 5 items, at most 4",
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
            "rate": 6, "shape": "sine", "bypass": false, "accents": [0, 0, 0, 0]
        });
        assert_eq!(played, expected);
    }

    #[test]
    fn an_input_is_found_by_its_place_among_those_of_its_kind() {
        let wobble = wobble();
        assert!(matches!(wobble.input("x"), Some((0, Input::Number { .. }))));
        assert!(matches!(wobble.input("hit"), Some((0, Input::Bang { .. }))));
        assert!(matches!(wobble.input("y"), Some((1, Input::Number { .. }))));
        assert!(wobble.input("z").is_none());
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
            doc.contains("| `rate` | number | a number from 0.1 to 20 Hz | `4.0` |"),
            "{doc}"
        );
    }

    /// An owner with lanes of patterns of notes, as a clip launcher keeps them.
    fn band() -> ToolInfo {
        serde_json::from_value(json!({
            "name": "band",
            "file": "band.ts",
            "title": "Band",
            "when": "You want lanes of patterns",
            "doc": "Lanes of patterns.",
            "voices": 1,
            "input": true,
            "fields": {
                "lanes": {
                    "kind": "list",
                    "max": 4,
                    "item": { "kind": "object", "fields": {
                        "name": { "kind": "text" },
                        "patterns": { "kind": "list", "item": { "kind": "object", "fields": {
                            "bars": { "kind": "number", "min": 1, "max": 16, "integer": true },
                            "notes": notes()
                        } } }
                    } }
                },
                "gain": { "kind": "number", "min": 0, "max": 1, "default": 0.5 }
            },
            "inputs": {},
            "tick": false,
            "view": false,
            "keys": false,
            "notes": false,
            "behaviour": true
        }))
        .unwrap()
    }

    #[test]
    fn data_is_checked_without_bun_and_the_message_names_its_path() {
        let band = band();
        let note =
            |pitch: u32| json!({ "start": 0, "length": 240, "pitch": pitch, "velocity": 100 });
        let lanes = |notes: Value, bars: Value| {
            let lane = json!({ "name": "bass", "patterns": [{ "bars": bars, "notes": notes }] });
            json!({ "lanes": [{ "name": "lead" }, lane] })
        };
        let good = lanes(json!([note(60), note(64)]), json!(2));
        assert_eq!(band.check(&good), Ok(()));
        let refused = [
            (
                lanes(json!([note(60), note(60), note(60), note(200)]), json!(1)),
                "state.lanes[1].patterns[0].notes[3].pitch: 200 is outside [0, 127]",
            ),
            (
                lanes(
                    json!([{ "start": 0, "length": 0, "pitch": 60, "velocity": 100 }]),
                    json!(1),
                ),
                "state.lanes[1].patterns[0].notes[0].length: 0 is outside [1, 2147483648]",
            ),
            (
                lanes(json!([]), json!(1.5)),
                "state.lanes[1].patterns[0].bars: 1.5 is no whole number",
            ),
            (
                json!({ "lanes": [{ "nme": "lead" }] }),
                "state.lanes[0].nme: there is no field nme; the fields are name, patterns",
            ),
            (
                json!({ "lanes": [{}, {}, {}, {}, {}] }),
                "state.lanes: holds 5 items, at most 4",
            ),
            (
                json!({ "gain": "loud" }),
                "state.gain: must be a number from 0 to 1",
            ),
        ];
        for (state, message) in refused {
            assert_eq!(band.check(&state), Err(message.to_string()));
        }
    }

    #[test]
    fn a_behaviour_reads_every_field_at_its_default_inside_lists_too() {
        let state = json!({ "lanes": [{ "patterns": [{}] }] });
        let expected = json!({
            "lanes": [{ "name": "", "patterns": [{ "bars": 1, "notes": [] }] }],
            "gain": 0.5
        });
        assert_eq!(band().with_defaults(state), expected);
    }

    #[test]
    fn a_default_of_a_list_is_filled_like_a_record_and_checked_like_one() {
        let fields = |default: Value| -> Named<Field> {
            serde_json::from_value(json!({
                "scenes": { "kind": "list", "default": default, "item": { "kind": "object", "fields": {
                    "bars": { "kind": "number", "min": 1, "max": 16, "integer": true },
                    "notes": notes()
                } } }
            }))
            .unwrap()
        };
        let good = fields(json!([{ "notes": [] }]));
        assert_eq!(good.check_defaults("state."), Ok(()));
        let filled = good.with_defaults(json!({}));
        assert_eq!(filled, json!({ "scenes": [{ "bars": 1, "notes": [] }] }));
        let bad = fields(json!([{ "bars": 0 }]));
        let error = "state.scenes.default[0].bars: 0 is outside [1, 16]";
        assert_eq!(bad.check_defaults("state."), Err(error.to_string()));
    }

    #[test]
    fn a_default_of_a_whole_number_field_that_is_no_whole_number_is_refused() {
        let fields: Named<Field> = serde_json::from_value(json!({
            "bars": { "kind": "number", "min": 1, "max": 16, "integer": true, "default": 1.5 }
        }))
        .unwrap();
        let error = "state.bars.default: 1.5 is no whole number";
        assert_eq!(fields.check_defaults("state."), Err(error.to_string()));
    }

    #[test]
    fn the_doc_of_an_owner_says_where_its_children_go_and_lists_the_fields_of_its_items() {
        let band = band();
        let doc = band.doc();
        let start = doc.find("```json state/band/instance.json\n").unwrap();
        let body = &doc[start..];
        let json = &body[body.find('\n').unwrap() + 1..body.find("\n```\n").unwrap()];
        let record: Value = serde_json::from_str(json).unwrap();
        assert_eq!(band.check(&record["state"]), Ok(()));
        assert!(doc.contains("`state/band/<child>.json`"), "{doc}");
        let row = "| `lanes[].patterns[].bars` | number | a whole number from 1 to 16 | `1` |";
        assert!(doc.contains(row), "{doc}");
    }
}
