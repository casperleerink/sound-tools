//! A tool of the project, as `extensions/*.ts` defines it, made into a [`JsonTool`] of the core:
//! the check of its records from its fields, its doc for agents, and a behaviour that plays
//! the sound graph its code builds as an effect, an instrument or a source.
//!
//! The check runs in Rust, from the fields Bun sent with its load, and so does the sound: Bun
//! sent the graph of every combination of choices then. Nothing here waits for Bun. A choice
//! picks another graph; a knob, a toggle or a pattern only moves a value of the sound that
//! plays.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};
use sound_core::{
    Assets, BehaviourContext, BehaviourError, GraphError, InputEndpoint, InstanceId, JsonTool,
    JsonToolDoc, Node, OutputEndpoint, ParameterInfo, Problem, Ticks, ValueRange, Watch,
};
use sound_notes::{
    AUDIO_INPUT, AUDIO_OUTPUT, Launch, Length, NOTES_INPUT, Note, NoteFeed, NotePlayer,
    NotePlayerUpdate, Pattern, PlayerWatches, Quantize,
};
use sound_signals::{
    ArraySpec, Cable, Code, ControlSpec, Declarations, Graph, Kind, Machine, ParameterSpec,
    Signals, SignalsUpdate, Values,
};

use crate::behaviours::{
    self, ChildNumber, Declaration, FromPort, Own, Player, PlayerPattern, PlayerPort, ToPort,
};
use crate::bun::Bun;
use crate::{FOLDER, samples, tree};

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
    /// Its page shows the transport, and its cards and page draw again as the transport moves.
    pub transport: bool,
    /// It hears the computer keyboard, `onKey`.
    pub keys: bool,
    /// It hears the MIDI keyboard, `onMidi`.
    pub midi: bool,
    /// It owns children and has a behaviour, which declares how they connect.
    pub behaviour: bool,
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
        unit: Option<Unit>,
        label: Option<String>,
    },
    Toggle {
        default: bool,
        label: Option<String>,
    },
    Choice {
        options: Vec<Choice>,
        default: Choice,
        label: Option<String>,
    },
    /// A sound under `assets/audio/`, by its file name, which the sound reads as a list.
    Sample { label: Option<String> },
    /// A list of numbers, such as the steps of a sequence.
    Pattern {
        length: usize,
        min: f32,
        max: f32,
        default: f32,
        label: Option<String>,
    },
    /// Notes in ticks. This and the kinds below are data a behaviour reads: no signal.
    Notes {
        /// `[]` when left out.
        default: Option<Value>,
        label: Option<String>,
    },
    Text {
        default: Option<String>,
        label: Option<String>,
    },
    Number {
        min: f64,
        max: f64,
        #[serde(default)]
        integer: bool,
        /// `min` when left out.
        default: Option<f64>,
        label: Option<String>,
    },
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Control {
    Live {
        min: f32,
        max: f32,
        default: f32,
        label: Option<String>,
    },
    Trigger {
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
            Self::Knob { default, .. } => tree::decimal(*default),
            Self::Toggle { default, .. } => Value::from(*default),
            Self::Choice { default, .. } => default.to_value(),
            Self::Pattern {
                length, default, ..
            } => Value::from(vec![tree::decimal(*default); *length]),
            Self::Sample { .. } => Value::from(""),
            Self::Notes { default, .. } => default.clone().unwrap_or(Value::Array(Vec::new())),
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

    /// Whether it is data a behaviour reads, which the sound does not get.
    pub(crate) fn is_data(&self) -> bool {
        matches!(
            self,
            Self::Notes { .. }
                | Self::Text { .. }
                | Self::Number { .. }
                | Self::List { .. }
                | Self::Object { .. }
        )
    }

    /// Whether `value` is one this field takes. `at` is its path, which the message starts
    /// with, such as `state.lanes[1].notes[3].pitch: ...`.
    fn check(&self, value: &Value, at: &str) -> Result<(), String> {
        let in_range =
            |number: f64, min: f32, max: f32| number >= f64::from(min) && number <= f64::from(max);
        match self {
            Self::Knob { min, max, .. } => {
                let Some(number) = value.as_f64() else {
                    return Err(format!("{at}: must be a number from {min} to {max}"));
                };
                if !in_range(number, *min, *max) {
                    return Err(format!("{at}: {number} is outside [{min}, {max}]"));
                }
            }
            Self::Toggle { .. } => {
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
            Self::Sample { .. } => {
                let Some(file) = value.as_str() else {
                    return Err(format!(
                        "{at}: must be the name of a file under assets/audio/, such as \"voice.wav\", or \"\" for none"
                    ));
                };
                if !file.is_empty() {
                    sound_media::AudioAsset::new(file).map_err(|error| format!("{at}: {error}"))?;
                }
            }
            Self::Pattern {
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
                if let Some((index, number)) =
                    (numbers.iter().enumerate()).find(|(_, number)| !in_range(**number, *min, *max))
                {
                    return Err(format!("{at}[{index}]: {number} is outside [{min}, {max}]"));
                }
            }
            Self::Notes { .. } => {
                let Some(notes) = value.as_array() else {
                    return Err(format!(
                        "{at}: must be a list of notes such as {NOTE_EXAMPLE}"
                    ));
                };
                for (index, note) in notes.iter().enumerate() {
                    if let Err(error) = serde_path_to_error::deserialize::<_, Note>(note) {
                        let path = error.path().to_string();
                        let path = if path == "." {
                            String::new()
                        } else {
                            format!(".{path}")
                        };
                        return Err(format!("{at}[{index}]{path}: {}", error.inner()));
                    }
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

/// A note as a message shows it.
const NOTE_EXAMPLE: &str = r#"{"start": 0, "length": 480, "pitch": 60, "velocity": 100}"#;

/// A number as a record writes it: `4`, not `4.0`, for a whole number field. A default that is
/// no whole number stays as it is, so its check refuses it.
fn number_value(number: f64, integer: bool) -> Value {
    match integer && number.fract() == 0.0 {
        true => Value::from(number as i64),
        false => Value::from(number),
    }
}

impl Named<Field> {
    fn get(&self, name: &str) -> Option<&Field> {
        let mut fields = self.0.iter();
        fields
            .find(|(field, _)| field == name)
            .map(|(_, field)| field)
    }

    fn names(&self) -> String {
        let names: Vec<&str> = self.0.iter().map(|(name, _)| name.as_str()).collect();
        names.join(", ")
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

/// Whether two values are the same, a number also when one writes it `4` and the other `4.0`.
fn same(one: &Value, two: &Value) -> bool {
    match (one, two) {
        (Value::Number(one), Value::Number(two)) => one.as_f64() == two.as_f64(),
        (Value::Array(one), Value::Array(two)) => {
            one.len() == two.len() && one.iter().zip(two).all(|(one, two)| same(one, two))
        }
        (Value::Object(one), Value::Object(two)) => {
            one.len() == two.len()
                && (one.iter())
                    .all(|(name, value)| two.get(name).is_some_and(|two| same(value, two)))
        }
        _ => one == two,
    }
}

impl ToolInfo {
    /// What the `Signals` processor of this tool is.
    pub(crate) fn kind(&self) -> Kind {
        match self.kind {
            ToolKind::Effect => Kind::Effect,
            ToolKind::Instrument => Kind::Instrument {
                voices: sound_signals::MAX_VOICES,
            },
            ToolKind::Source => Kind::Source,
        }
    }

    /// What the record and the card give its sound: the knobs and toggles, the patterns and
    /// samples, and the controls, each in the order the code gives them.
    pub(crate) fn declarations(&self) -> Declarations {
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
                Field::Knob {
                    min, max, default, ..
                } => (declarations.parameters).push(number(name, *default, *min, *max)),
                Field::Toggle { default, .. } => {
                    let default = f32::from(u8::from(*default));
                    declarations
                        .parameters
                        .push(number(name, default, 0.0, 1.0));
                }
                Field::Choice { .. } => {}
                Field::Pattern {
                    length,
                    min,
                    max,
                    default,
                    ..
                } => (declarations.arrays).push(list(name, Some(*length), *default, *min, *max)),
                Field::Sample { .. } => {
                    (declarations.arrays).push(list(name, None, 0.0, -1.0, 1.0))
                }
                Field::Notes { .. }
                | Field::Text { .. }
                | Field::Number { .. }
                | Field::List { .. }
                | Field::Object { .. } => {}
            }
        }
        for (name, control) in &self.controls.0 {
            declarations.controls.push(match control {
                Control::Live {
                    min, max, default, ..
                } => ControlSpec::Live(number(name, *default, *min, *max)),
                Control::Trigger { .. } => ControlSpec::Trigger(name.clone()),
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

    /// The name of its processor. Another kind is another processor, with other ports.
    pub(crate) fn processor(&self) -> &'static str {
        match self.kind {
            ToolKind::Effect => "signals-effect",
            ToolKind::Instrument => "signals-instrument",
            ToolKind::Source => "signals-source",
        }
    }

    /// The index of the live control or the trigger `name` among those of its kind, which is
    /// its index in the sound the tool builds.
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

    /// `edited` without each field `before` leaves out and `edited` holds at its default, so
    /// the record follows a later change of that default in the code.
    pub(crate) fn untouched_defaults_left_out(&self, before: &Value, mut edited: Value) -> Value {
        if let (Value::Object(before), Value::Object(edited)) = (before, &mut edited) {
            edited.retain(|name, value| {
                let default = self.fields.get(name).map(Field::default_value);
                before.contains_key(name) || !default.is_some_and(|default| same(value, &default))
            });
        }
        edited
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
        let (place, example, path) = match (self.behaviour, self.kind) {
            (true, _) => (
                format!(
                    "a tool of this project that owns children. Its record is \
                     `state/{name}/instance.json`, and every record next to it, \
                     `state/{name}/<child>.json`, is a child: any tool, such as a built-in \
                     device whose extension `project.json` enables, or a tool of this project. Its \
                     behaviour connects them, and connects them into its own sound, which is its \
                     `audio` output. At the top of the project, connect that to the speakers in \
                     `project.json`: `{{\"from\": {{\"instance\": \"{name}\", \"port\": \"audio\"}}, \
                     \"to\": {{\"device_output\": 0}}}}`. Or put it in a track's `effects` as \
                     `state/arrangement/<track>/{name}/instance.json`, where it hears the track's \
                     sound and plays into its mixer"
                ),
                "one at its defaults",
                format!("state/{name}/instance.json"),
            ),
            (false, ToolKind::Effect) => (
                "an effect of this project. It goes in a track's `effects` like any effect: the \
                 record sits in the track's folder and its file name is in `effects` of the \
                 track's `instance.json`"
                    .to_string(),
                "a track `pad` with one, at its defaults",
                format!("state/arrangement/pad/{name}.json"),
            ),
            (false, ToolKind::Instrument | ToolKind::Source) => (
                "an instrument of this project. It is what a track plays, like any instrument: \
                 its record is the track's `instrument.json`"
                    .to_string(),
                "a track `pad` with one, at its defaults",
                "state/arrangement/pad/instrument.json".to_string(),
            ),
        };
        let data = (self.fields.0.iter().any(|(_, field)| field.is_data()))
            .then_some(
                " A notes, text, number or list field is data for its behaviour, card and page, \
                 not its sound: a change plays once Bun has run the behaviour again.",
            )
            .unwrap_or_default();
        format!(
            "# {title}\n\n{doc}\n\n\
             `{name}` is {place}. It is defined in `extensions/{file}`, and the `extensions` of \
             `project.json` do not list it. An example, not a record of this project: {example}:\n\n\
             ```json {path}\n{{\n  \"tool\": \"{name}\",\n  \"state\": {{{}}}\n}}\n```\n\n\
             {rows}\n\
             A field left out is at its default, so `\"state\": {{}}` is the tool at its defaults. \
             A knob, a toggle or a pattern changes the sound at once. A choice changes what the \
             sound is made of: the new sound fades in over 10 ms. A knob can be automated by a \
             lane of its track, as the numbers of any device (`agent-docs/arrangement.md`).{data}\n",
            defaults.join(", "),
        )
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
        Field::Notes { .. } => (
            "notes",
            format!(
                "a list of notes such as `{NOTE_EXAMPLE}`: `start` and `length` in ticks, 960 to a \
                 quarter note, `pitch` 0 to 127, `velocity` 1 to 127"
            ),
        ),
        Field::Text { .. } => ("text", "text in quotes".to_string()),
        Field::Number {
            min, max, integer, ..
        } => {
            let whole = if *integer {
                "a whole number"
            } else {
                "a number"
            };
            ("number", format!("{whole} from {min} to {max}"))
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

/// The behaviour of a tool: its sound per combination of choices.
pub(crate) struct Sounds {
    info: Arc<ToolInfo>,
    /// Compiled, by [`ToolInfo::sound_key`], or why that sound does not build.
    sounds: HashMap<String, Result<Arc<Code>, String>>,
    /// The Bun that runs its behaviour, for a tool that owns children.
    bun: Option<Arc<Bun>>,
}

impl Sounds {
    /// The behaviour of `info`, with the graph Bun sent for each combination of its choices
    /// compiled. Takes as long as compiling them all: not on the thread that draws.
    pub(crate) fn new(
        info: &ToolInfo,
        graphs: Vec<(Map<String, Value>, Result<Graph, String>)>,
        bun: &Arc<Bun>,
    ) -> Self {
        let declarations = info.declarations();
        let sounds = (graphs.into_iter())
            .map(|(choices, graph)| {
                let key = info.sound_key(&Value::Object(choices));
                let code = graph.and_then(|graph| {
                    (graph.compile(&declarations).map(Arc::new)).map_err(|error| error.to_string())
                });
                let code = code.map_err(|error| match info.has_choices() {
                    true => format!("its sound with the choices {key} does not build: {error}"),
                    false => format!("its sound does not build: {error}"),
                });
                (key, code)
            })
            .collect();
        Self {
            info: Arc::new(info.clone()),
            sounds,
            bun: info.behaviour.then(|| bun.clone()),
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

    /// The compiled sound for the choices of `state`.
    fn code(&self, state: &Value) -> Result<Arc<Code>, String> {
        let key = self.info.sound_key(state);
        // A checked record has one of the options of each choice, and Bun sent each combination.
        (self.sounds.get(&key).cloned())
            .unwrap_or_else(|| Err(format!("Bun sent no sound for the choices {key}")))
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
    fn lists(&self, code: &Code, state: &Value, (assets, rate, id): Read<'_>) -> Vec<Vec<f32>> {
        let list = |array: &ArraySpec| {
            let Some(length) = array.length else {
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
        (assets, rate, id): Read<'_>,
    ) -> (u64, Vec<String>) {
        let mut hasher = DefaultHasher::new();
        let mut problems = Vec::new();
        for array in &code.arrays {
            let value = state.get(&array.name);
            value.map(Value::to_string).hash(&mut hasher);
            let file = value.and_then(Value::as_str).unwrap_or_default();
            if array.length.is_none() && !file.is_empty() {
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
        let code = self.code(state).map_err(BehaviourError::Other)?;
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
        let sample_rate = context.prepare_config().sample_rate as f32;
        let new_code = context.changed("code", code.hash);
        if new_code || new_lists {
            values.arrays = Some(self.lists(&code, state, reading));
        }
        let made = || Box::new(Machine::new(code.as_ref().clone(), sample_rate));
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
            Signals::new(kind, first.unwrap_or_else(made), values, watches.clone())
        })?;
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
        match kind {
            Kind::Effect => {
                context.input(AUDIO_INPUT, InputEndpoint::new(signals, Signals::INPUT));
            }
            Kind::Instrument { .. } | Kind::Source => {
                context.input(NOTES_INPUT, InputEndpoint::new(signals, Signals::NOTES));
            }
        }
        context.output(AUDIO_OUTPUT, OutputEndpoint::new(signals, Signals::OUTPUT));
        for (index, name) in code.numbers.iter().enumerate() {
            context.output(name, OutputEndpoint::new(signals, Signals::number(index)));
        }
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
            let input = InputEndpoint::new(signals, Signals::AUTOMATION);
            context.runtime_automation(input, numbers)?;
        }
        if let Some(bun) = &self.bun {
            let sound = InputEndpoint::new(signals, Signals::INPUT);
            self.declare(bun, state, sound, context)?;
        }
        Ok(())
    }

    /// Connects the children of an owner as its behaviour declares, and into `sound`, its own
    /// sound: what Bun answered for its record and its children, or the last declaration while
    /// Bun works it out.
    fn declare(
        &self,
        bun: &Bun,
        state: &Value,
        sound: InputEndpoint,
        context: &mut BehaviourContext<'_>,
    ) -> Result<(), BehaviourError> {
        let children = (context.child_records())
            .map(|(name, tool, state)| {
                let state: Value = serde_json::from_str(&state?)?;
                Ok(serde_json::json!({ "name": name, "tool": tool, "state": state }))
            })
            .collect::<Result<Vec<Value>, serde_json::Error>>();
        let children =
            Value::Array(children.map_err(|error| BehaviourError::Other(error.to_string()))?);
        let state = self.info.with_defaults(state.clone());
        let instance = (context.assets(), context.id());
        let declared = behaviours::declared(
            bun,
            instance,
            &self.info.name,
            key(&(&self.info.name, state.to_string(), children.to_string())),
            (&state, &children),
        );
        if let Some(problem) = declared.problem {
            context.problem(format!("{problem}. It plays what it declared last"));
        }
        wire(&declared.declaration, sound, context)
    }
}

/// The name of the processor of the player `name` of an owner, which a launch is sent to.
pub(crate) fn player_processor(name: &str) -> String {
    format!("player:{name}")
}

/// Makes the players of `declaration` and its connections. A connection that names a child, a
/// player or a port the owner does not have, or joins ports of two kinds, is a problem, and the
/// others play.
///
/// What plays into the `notes` of a child goes through a [`NoteFeed`] of that child, which
/// releases what the child holds when that changes: a player that went away or plays elsewhere
/// sends no off there.
fn wire(
    declaration: &Declaration,
    sound: InputEndpoint,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let mut players = BTreeMap::new();
    for (name, player) in &declaration.players {
        players.insert(name.as_str(), play(name, player, context)?);
    }
    let feeds = feeds(context)?;
    let mut cables = BTreeSet::new();
    let mut fed: BTreeMap<&str, BTreeSet<&FromPort>> = BTreeMap::new();
    for (index, connection) in declaration.connections.iter().enumerate() {
        let at = format!("connections[{index}]");
        let numbers = (
            matches!(connection.from, FromPort::Number(_)),
            matches!(connection.to, ToPort::Number(_)),
        );
        if numbers.0 != numbers.1 {
            context.problem(format!(
                "its behaviour declares {at} between a number and a sound or notes: a number goes from {{ child, number }} to {{ child, number }}"
            ));
            continue;
        }
        let (output, to_port) = match &connection.from {
            FromPort::Child(from) => {
                let port = from.port.as_deref().unwrap_or(AUDIO_OUTPUT);
                let Some(output) = context.child_output(&from.child, port) else {
                    let at = format!("{at}.from");
                    let problem = missing(context, &at, (&from.child, port), Side::Output);
                    context.problem(problem);
                    continue;
                };
                (output, AUDIO_INPUT)
            }
            FromPort::Player(PlayerPort { player }) => {
                let Some(node) = players.get(player.as_str()) else {
                    let names: Vec<&str> = players.keys().copied().collect();
                    let players = match names.is_empty() {
                        true => "it declares none".to_string(),
                        false => format!("its players are {}", names.join(", ")),
                    };
                    context.problem(format!(
                        "its behaviour names the player {player:?} at {at}.from, which it does not declare: {players}"
                    ));
                    continue;
                };
                (OutputEndpoint::new(*node, NotePlayer::NOTES), NOTES_INPUT)
            }
            FromPort::Number(from) => {
                let Some(output) = context.child_output(&from.child, &from.number) else {
                    let at = format!("{at}.from");
                    let port = (from.child.as_str(), from.number.as_str());
                    context.problem(missing(context, &at, port, Side::Output));
                    continue;
                };
                (output, AUDIO_INPUT)
            }
        };
        let (input, feed) = match &connection.to {
            ToPort::Own(Own::Sound) => (sound, None),
            ToPort::Child(to) => {
                let port = to.port.as_deref().unwrap_or(to_port);
                let feed = (feeds.get_key_value(to.child.as_str())).filter(|_| port == NOTES_INPUT);
                match (feed, context.child_input(&to.child, port)) {
                    (Some((child, feed)), _) => (
                        InputEndpoint::new(*feed, NoteFeed::INPUT),
                        Some(child.as_str()),
                    ),
                    (None, Some(input)) => (input, None),
                    (None, None) => {
                        let at = format!("{at}.to");
                        let problem = missing(context, &at, (&to.child, port), Side::Input);
                        context.problem(problem);
                        continue;
                    }
                }
            }
            ToPort::Number(to) => match cable((&connection.from, to), &at, &mut cables, context)? {
                Some(input) => (input, None),
                None => continue,
            },
        };
        match context.connect(output.to(input)) {
            Err(BehaviourError::Graph(GraphError::PortTypeMismatch { description, .. })) => {
                context.problem(format!(
                    "its behaviour declares {at}, {description}, between ports of two kinds, such as sound and notes"
                ));
            }
            connected => connected?,
        }
        if let Some(child) = feed {
            fed.entry(child).or_default().insert(&connection.from);
        }
    }
    for (child, feed) in &feeds {
        let senders = fed.get(child.as_str());
        if context.changed(&format!("feed:{child}.senders"), key(&senders)) {
            context.update(*feed, ())?;
        }
    }
    Ok(())
}

/// A [`Cable`] from `from` into the number `to` takes automation for, connected to the
/// automation input of its child. `None` when the child takes no such number, a problem, and
/// for a cable `made` already: the same connection twice is one.
fn cable(
    (from, to): (&FromPort, &ChildNumber),
    at: &str,
    made: &mut BTreeSet<String>,
    context: &mut BehaviourContext<'_>,
) -> Result<Option<InputEndpoint>, BehaviourError> {
    let ChildNumber { child, number } = to;
    let found = context
        .child_automation(child)
        .and_then(|(automation, numbers)| {
            let place = numbers.iter().position(|info| *info.field == **number)?;
            Some((automation, place))
        });
    let Some((automation, place)) = found else {
        let problem = missing(context, &format!("{at}.to"), (child, number), Side::Number);
        context.problem(problem);
        return Ok(None);
    };
    // At most `MAX_AUTOMATED` numbers, far under `u16::MAX`.
    let parameter = u16::try_from(place).unwrap_or(u16::MAX);
    let name = format!("cable:{from:?}>{child:?}.{number:?}");
    if !made.insert(name.clone()) {
        return Ok(None);
    }
    let cable = context.processor(&name, Cable::default)?;
    if context.changed(&name, key(&parameter)) {
        context.update(cable, parameter)?;
    }
    context.connect(OutputEndpoint::new(cable, Cable::OUTPUT).to(automation))?;
    Ok(Some(InputEndpoint::new(cable, Cable::INPUT)))
}

/// The [`NoteFeed`] of every child that has `notes`, by the child's name. Declared whatever
/// plays into it, so it is there to release what a sender that went away left sounding.
fn feeds(
    context: &mut BehaviourContext<'_>,
) -> Result<BTreeMap<String, Node<NoteFeed>>, BehaviourError> {
    let children: Vec<String> = context.child_names().map(str::to_string).collect();
    let mut feeds = BTreeMap::new();
    for child in children {
        let Some(notes) = context.child_input(&child, NOTES_INPUT) else {
            continue;
        };
        let feed = context.processor(&format!("feed:{child}"), NoteFeed::default)?;
        match context.connect(OutputEndpoint::new(feed, NoteFeed::OUTPUT).to(notes)) {
            // An input named so that takes something else than notes.
            Err(BehaviourError::Graph(GraphError::PortTypeMismatch { .. })) => continue,
            connected => connected?,
        }
        feeds.insert(child, feed);
    }
    Ok(feeds)
}

/// Makes the note player `name` of an owner, which shows what it plays in the watches
/// `<name>.playing`, `<name>.queued` and `<name>.phase`. Its patterns are sent when they change.
///
/// What plays before any launch is decided here: the pattern `plays` names is launched when the
/// player is first declared, and again each time `plays` changes, as a launch at the next bar
/// is. While the project does not play that lands at once, so a render from the start plays it
/// from tick 0. A launch of a performer plays until `plays` changes. Nothing of it is saved.
fn play(
    name: &str,
    player: &Player,
    context: &mut BehaviourContext<'_>,
) -> Result<Node<NotePlayer>, BehaviourError> {
    let watches = PlayerWatches {
        playing: context.watch(&format!("{name}.playing")),
        queued: context.watch(&format!("{name}.queued")),
        phase: context.watch(&format!("{name}.phase")),
    };
    let processor = player_processor(name);
    let node = context.processor(&processor, || NotePlayer::watched(watches))?;
    // Checked every run, so a problem stays as long as the pattern does.
    let mut patterns = Vec::with_capacity(player.patterns.len());
    for (index, pattern) in player.patterns.iter().enumerate() {
        let at = format!("players.{name}.patterns[{index}]");
        patterns.push(pattern_of(pattern, &at, context)?);
    }
    if let Some(plays) = player.plays
        && plays >= patterns.len()
    {
        context.problem(format!(
            "its behaviour declares players.{name}.plays {plays}, which is no pattern: it has {}, from 0",
            patterns.len()
        ));
    }
    if context.changed(&format!("{processor}.patterns"), key(&player.patterns)) {
        context.update(node, NotePlayerUpdate::Patterns(Arc::new(patterns)))?;
    }
    if context.changed(&format!("{processor}.plays"), key(&player.plays)) {
        let launch = Launch {
            pattern: player.plays,
            quantize: Quantize::Bar,
        };
        context.update(node, NotePlayerUpdate::Launch(launch))?;
    }
    Ok(node)
}

/// The pattern `declared` at `at` of a declaration. One that cannot play, such as one with a
/// note outside it, is silent and a problem.
fn pattern_of(
    declared: &PlayerPattern,
    at: &str,
    context: &mut BehaviourContext<'_>,
) -> Result<Pattern, BehaviourError> {
    let made = Length::new(Ticks(declared.length))
        .map_err(|error| format!("{at}.{error}"))
        .and_then(|length| {
            Pattern::new(length, &declared.notes).map_err(|error| format!("{at}.{error}"))
        });
    match made {
        Ok(pattern) => Ok(pattern),
        Err(error) => {
            context.problem(format!(
                "its behaviour declares a pattern that cannot play, so it is silent: {error}"
            ));
            let silent = Pattern::new(Length::at_least_one(Ticks(declared.length)), &[]);
            silent.map_err(|error| BehaviourError::Other(error.to_string()))
        }
    }
}

fn key(value: &impl Hash) -> u64 {
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
            "page": false,
            "transport": false,
            "keys": false,
            "midi": false,
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

    /// An owner with lanes of patterns of notes, as a clip launcher keeps them.
    fn band() -> ToolInfo {
        serde_json::from_value(json!({
            "name": "band",
            "file": "band.ts",
            "title": "Band",
            "when": "You want lanes of patterns",
            "doc": "Lanes of patterns.",
            "kind": "effect",
            "fields": {
                "lanes": {
                    "kind": "list",
                    "max": 4,
                    "item": { "kind": "object", "fields": {
                        "name": { "kind": "text" },
                        "patterns": { "kind": "list", "item": { "kind": "object", "fields": {
                            "bars": { "kind": "number", "min": 1, "max": 16, "integer": true },
                            "notes": { "kind": "notes" }
                        } } }
                    } }
                },
                "gain": { "kind": "number", "min": 0, "max": 1, "default": 0.5 }
            },
            "controls": {},
            "tick": false,
            "page": false,
            "transport": false,
            "keys": false,
            "midi": false,
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
                "state.lanes[1].patterns[0].notes[3].pitch: pitch must be from 0 to 127, not 200",
            ),
            (
                lanes(
                    json!([{ "start": 0, "length": 240, "pitch": 60 }]),
                    json!(1),
                ),
                "state.lanes[1].patterns[0].notes[0]: missing field `velocity`",
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
                    "notes": { "kind": "notes" }
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
