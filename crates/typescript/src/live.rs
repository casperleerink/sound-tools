//! Live values: numbers of a tree the window works out again on every frame, so what moves on
//! a view moves without Bun. The window gives what they read, see [`Leaves`].

use serde::de::value::MapAccessDeserializer;
use serde::de::{Error, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use sound_core::Clock;

use crate::readout;
pub use crate::tools::Unit;

/// How close an ease comes before it is there, or a smooth one would move for a minute.
const SETTLED: f64 = 1e-6;

/// What a live value reads: the values that read no other, and what eases keep.
pub trait Leaves {
    /// The number of a live value that reads no other, such as the playhead or a peak.
    fn leaf(&self, live: &Live) -> f64;
    /// `value` eased as `ease` says, under `key`: once a frame, the same wherever it is drawn.
    fn ease(&self, key: &str, ease: Ease, value: f64) -> f64;
}

/// The number `number` is now.
pub fn number(number: &Num, leaves: &(impl Leaves + ?Sized)) -> f64 {
    match number {
        Num::Fixed(value) => f64::from(*value),
        Num::Live(live) => evaluate(live, leaves),
    }
}

/// The number `live` is now.
pub fn evaluate(live: &Live, leaves: &(impl Leaves + ?Sized)) -> f64 {
    let both = |a: &Num, b: &Num| (number(a, leaves), number(b, leaves));
    let truth = |holds: bool| f64::from(u8::from(holds));
    match live {
        Live::Plus { a, b } => number(a, leaves) + number(b, leaves),
        Live::Minus { a, b } => number(a, leaves) - number(b, leaves),
        Live::Times { a, b } => number(a, leaves) * number(b, leaves),
        Live::Over { a, b } => {
            let (a, b) = both(a, b);
            if b == 0.0 { 0.0 } else { a / b }
        }
        Live::Min { a, b } => {
            let (a, b) = both(a, b);
            a.min(b)
        }
        Live::Max { a, b } => {
            let (a, b) = both(a, b);
            a.max(b)
        }
        Live::Lt { a, b } => {
            let (a, b) = both(a, b);
            truth(a < b)
        }
        Live::Gt { a, b } => {
            let (a, b) = both(a, b);
            truth(a > b)
        }
        Live::Le { a, b } => {
            let (a, b) = both(a, b);
            truth(a <= b)
        }
        Live::Ge { a, b } => {
            let (a, b) = both(a, b);
            truth(a >= b)
        }
        Live::Eq { a, b } => {
            let (a, b) = both(a, b);
            truth(a == b)
        }
        Live::Ne { a, b } => {
            let (a, b) = both(a, b);
            truth(a != b)
        }
        Live::Mod { a, b } => {
            let (a, b) = both(a, b);
            if b == 0.0 { 0.0 } else { a.rem_euclid(b) }
        }
        Live::Pow { a, b } => {
            let (a, b) = both(a, b);
            let power = a.powf(b);
            if power.is_finite() { power } else { 0.0 }
        }
        Live::Snap { a, b } => {
            let (value, step) = both(a, b);
            snap(value, step)
        }
        Live::Negate { x } => -number(x, leaves),
        Live::Abs { x } => number(x, leaves).abs(),
        Live::Floor { x } => number(x, leaves).floor(),
        Live::Round { x } => number(x, leaves).round(),
        Live::Sin { x } => number(x, leaves).sin(),
        Live::Cos { x } => number(x, leaves).cos(),
        Live::Decibels { x } => (20.0 * number(x, leaves).abs().log10()).max(-120.0),
        Live::Ease { x, ease, key } => leaves.ease(key, *ease, number(x, leaves)),
        Live::Tick
        | Live::Playing
        | Live::Bpm
        | Live::Elapsed
        | Live::Value { .. }
        | Live::Held { .. }
        | Live::Sounding { .. }
        | Live::Peak { .. }
        | Live::DevicePeak { .. }
        | Live::Input { .. }
        | Live::Drag { .. }
        | Live::Plane { .. }
        | Live::Hover { .. }
        | Live::Touched { .. } => leaves.leaf(live),
    }
}

/// What an ease keeps from frame to frame.
#[derive(Clone, Copy, Debug)]
pub struct Eased {
    /// The frame it last moved in.
    pub frame: u64,
    pub value: f64,
    /// How long a hold has held.
    held: f64,
}

impl Eased {
    pub fn new(frame: u64, value: f64) -> Self {
        Self {
            frame,
            value,
            held: 0.0,
        }
    }

    /// One frame `dt` seconds after the last, toward `value`. Whether it still moves.
    pub fn step(&mut self, ease: Ease, value: f64, dt: f64) -> bool {
        let before = self.value;
        match ease {
            Ease::Smooth { ms } => {
                let seconds = f64::from(ms.max(1.0)) / 1000.0;
                self.value += (value - self.value) * (1.0 - (-dt / seconds).exp());
            }
            Ease::Fall { per_second } => {
                self.value = value.max(self.value - f64::from(per_second) * dt);
            }
            Ease::Hold { seconds } => {
                self.held += dt;
                if value >= self.value || self.held > f64::from(seconds) {
                    self.value = value;
                    self.held = 0.0;
                }
            }
        }
        if !self.value.is_finite() || (value - self.value).abs() < SETTLED {
            self.value = value;
        }
        self.value != before || self.value != value
    }
}

/// A number, or a live value the window works out on every frame. Read by hand: serde's
/// untagged enums buffer every number of a tree.
#[derive(Clone, Debug)]
pub enum Num {
    Fixed(f32),
    Live(Box<Live>),
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NumVisitor)
    }
}

struct NumVisitor;

impl<'de> Visitor<'de> for NumVisitor {
    type Value = Num;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a number or a live value")
    }

    fn visit_f64<E: Error>(self, value: f64) -> Result<Num, E> {
        Ok(Num::Fixed(value as f32))
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Num, E> {
        Ok(Num::Fixed(value as f32))
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Num, E> {
        Ok(Num::Fixed(value as f32))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Num, A::Error> {
        let mut live = serde_json::Value::deserialize(MapAccessDeserializer::new(map))?;
        fed_as_before(&mut live);
        let live = Live::deserialize(live).map_err(A::Error::custom)?;
        Ok(Num::Live(Box::new(live)))
    }
}

/// `live.feed` of `sdk/view.ts` as the values the window had before feeds: `peak` of a path, or
/// of the device itself, and `sounding.<pitch>`.
fn fed_as_before(live: &mut serde_json::Value) {
    if live["op"] != "feed" {
        return;
    }
    let path = (live.get("path").and_then(serde_json::Value::as_str)).map(str::to_string);
    let name = live["name"].as_str().unwrap_or_default();
    let pitch = (name.strip_prefix("sounding.")).and_then(|pitch| pitch.parse::<u8>().ok());
    *live = match (name, path, pitch) {
        ("peak", Some(path), _) => serde_json::json!({ "op": "peak", "name": path }),
        ("peak", None, _) => serde_json::json!({ "op": "device_peak" }),
        (_, child, Some(pitch)) => {
            serde_json::json!({ "op": "sounding", "pitch": pitch, "child": child })
        }
        _ => return,
    };
}

impl Num {
    /// Its number, when it is one and not a live value.
    pub fn fixed(&self) -> Option<f32> {
        match self {
            Self::Fixed(number) => Some(*number),
            Self::Live(_) => None,
        }
    }
}

/// A live value, as `sdk.ts`'s `Live` sends it.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Live {
    Tick,
    /// 1 while the piece plays, else 0.
    Playing,
    /// The tempo at the playhead.
    Bpm,
    /// Seconds since the view opened.
    Elapsed,
    /// A number by its name: a field of the record, a number input or a number output of the
    /// sound; of the instance at the path `child` inside it, its field or its output.
    /// A field reads as it plays: what a lane or a cable plays into it, else its record.
    Value {
        name: String,
        child: Option<String>,
        fraction: bool,
    },
    /// 1 while a lane or a cable moves the field `name` and holds its control.
    Held {
        name: String,
        child: Option<String>,
    },
    /// The loudest sample since the last frame: of a `meter` at a path inside, or of peaks
    /// an instance inside it keeps, as `path.name`.
    Peak {
        name: String,
        /// 0 left, 1 right; the louder when left out.
        channel: Option<u8>,
    },
    /// The same, of what the device plays.
    DevicePeak {
        channel: Option<u8>,
    },
    /// The loudest sample of a channel of the audio input at the last poll, while it is open.
    Input {
        channel: usize,
    },
    Drag {
        id: String,
        part: DragPart,
    },
    /// What the plane `key` of the view shows, in its units, and how far it is zoomed.
    Plane {
        key: String,
        part: PlanePart,
    },
    /// 1 while the pointer is over the shape of `key`.
    Hover {
        key: String,
    },
    /// 1 while the key `pitch` is down on the sound of a tool of the project, or of its child
    /// at the path `child`: what a clip, a key or its code plays. Only the new window has it.
    Sounding {
        pitch: u8,
        child: Option<String>,
    },
    /// 1 while the pointer is over a control, the keys are on it or a drag holds it: the space
    /// of the key `name`, or one whose drag turns the field `name`, so a fader shows the
    /// number beside it.
    Touched {
        name: String,
    },
    Plus {
        a: Num,
        b: Num,
    },
    Minus {
        a: Num,
        b: Num,
    },
    Times {
        a: Num,
        b: Num,
    },
    Over {
        a: Num,
        b: Num,
    },
    Mod {
        a: Num,
        b: Num,
    },
    Pow {
        a: Num,
        b: Num,
    },
    Min {
        a: Num,
        b: Num,
    },
    Max {
        a: Num,
        b: Num,
    },
    Lt {
        a: Num,
        b: Num,
    },
    Gt {
        a: Num,
        b: Num,
    },
    Le {
        a: Num,
        b: Num,
    },
    Ge {
        a: Num,
        b: Num,
    },
    Eq {
        a: Num,
        b: Num,
    },
    Ne {
        a: Num,
        b: Num,
    },
    Snap {
        a: Num,
        b: Num,
    },
    Negate {
        x: Num,
    },
    Abs {
        x: Num,
    },
    Floor {
        x: Num,
    },
    Round {
        x: Num,
    },
    Sin {
        x: Num,
    },
    Cos {
        x: Num,
    },
    /// An amplitude in decibels, -120 at most down.
    Decibels {
        x: Num,
    },
    /// What moves from frame to frame: `key` is its live value as `sdk.ts` writes it, so the
    /// same one drawn twice is one, as a signal used twice is.
    Ease {
        x: Num,
        ease: Ease,
        key: String,
    },
}

/// How a live value follows another over time.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Ease {
    /// Glides toward it, about two thirds of the way in `ms`.
    Smooth { ms: f32 },
    /// Rises with it at once and falls at most `per_second`.
    Fall { per_second: f32 },
    /// Rises with it at once and stays for `seconds`, then follows it.
    Hold { seconds: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DragPart {
    StartX,
    StartY,
    Dx,
    Dy,
    X,
    Y,
    Active,
    Alt,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanePart {
    Left,
    Top,
    Right,
    Bottom,
    Zoom,
}

/// How text shows a live number: in a unit, or a tick of the clock.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Readout {
    Unit(Unit),
    Tick(TickReadout),
}

/// How text shows a tick: as a place in the piece, as minutes and seconds, or as the time
/// signature there.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TickReadout {
    Position,
    Time,
    Signature,
    /// The beats of a bar, and the note value of a beat, of the time signature there.
    Beats,
    Note,
}

/// The nearest whole number of `step`s; `value` itself for no step.
fn snap(value: f64, step: f64) -> f64 {
    if step > 0.0 {
        (value / step).round() * step
    } else {
        value
    }
}

/// A number as a readout of `unit`.
fn readout(value: f32, unit: Option<Unit>) -> String {
    match unit {
        Some(Unit::Hz) => readout::hertz(value),
        Some(Unit::Ms) => readout::milliseconds(value),
        Some(Unit::Seconds) => readout::seconds(value),
        Some(Unit::Db) => readout::decibels(value),
        Some(Unit::Percent) => readout::percent(value),
        Some(Unit::Semitones) => readout::semitones(value),
        Some(Unit::Cents) => format!("{} ct", readout::short(value)),
        Some(Unit::Octaves) => format!("{} oct", readout::short(value)),
        Some(Unit::Pitch) => pitch_readout(f64::from(value)),
        Some(Unit::Pan) => readout::pan(value),
        None => readout::short(value),
    }
}

/// The unit the number `field` of a record from `min` to `max` reads in, as `sdk/fields.ts`
/// names it: the unit its name ends in, a pan, or a share of 0 to 1 as a percent.
pub fn field_unit(field: &str, min: f64, max: f64) -> Option<&'static str> {
    const UNITS: [(&str, &str); 5] = [
        ("_hz", "hz"),
        ("_db", "db"),
        ("_ms", "ms"),
        ("_seconds", "seconds"),
        ("_percent", "percent"),
    ];
    let unit = UNITS.iter().find(|(end, _)| field.ends_with(end));
    match unit {
        Some((_, unit)) => Some(unit),
        None if field.rsplit('.').next() == Some("pan") => Some("pan"),
        None if min == 0.0 && max == 1.0 => Some("percent"),
        None => None,
    }
}

/// A live number as text shows it: in its unit, or a tick as a place in the piece (`5.3`), as
/// minutes and seconds (`1:07`), or as the time signature there (`7/8`).
pub fn text_of(value: f64, shown: Option<Readout>, clock: Option<&Clock>) -> String {
    let shown = match shown {
        None => return readout(value as f32, None),
        // A pitch keeps its cents, which an `f32` of a high note would round.
        Some(Readout::Unit(Unit::Pitch)) => return pitch_readout(value),
        Some(Readout::Unit(unit)) => return readout(value as f32, Some(unit)),
        Some(Readout::Tick(shown)) => shown,
    };
    let tick = sound_core::Ticks(value.max(0.0) as u64);
    let Some(clock) = clock else {
        return String::new();
    };
    let signatures = clock.tempo_map().time_signatures();
    match shown {
        TickReadout::Position => {
            let position = signatures.bar_beat_of(tick);
            format!("{}.{}", position.bar, position.beat)
        }
        TickReadout::Time => {
            let seconds = clock.seconds_of(tick).max(0.0) as u64;
            format!("{}:{:02}", seconds / 60, seconds % 60)
        }
        TickReadout::Signature => signatures.bar_at(tick).signature.to_string(),
        TickReadout::Beats => signatures.bar_at(tick).signature.numerator().to_string(),
        TickReadout::Note => signatures.bar_at(tick).signature.denominator().to_string(),
    }
}

/// A pitch as its note, `C2` for 36, and how many cents off it is where it is not whole:
/// `D2 +30`, so two pitches tuned apart do not read the same.
fn pitch_readout(value: f64) -> String {
    let note = value.round();
    let cents = ((value - note) * 100.0).round() as i64;
    match sound_notes::Pitch::try_from(note as i64) {
        Ok(pitch) if cents == 0 => pitch.name(),
        Ok(pitch) => format!("{} {cents:+}", pitch.name()),
        Err(_) => readout(value as f32, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pitch_that_is_not_whole_reads_its_note_and_its_cents() {
        assert_eq!(pitch_readout(36.0), "C2");
        assert_eq!(pitch_readout(38.3), "D2 +30");
        assert_eq!(pitch_readout(37.6), "D2 -40");
        assert_eq!(pitch_readout(36.004), "C2");
    }

    fn live(json: serde_json::Value) -> Num {
        serde_json::from_value(json).unwrap()
    }

    /// A drag of 250 and a number `step` of 3.4; nothing else.
    struct Fixed;

    impl Leaves for Fixed {
        fn leaf(&self, live: &Live) -> f64 {
            match live {
                Live::Drag {
                    id,
                    part: DragPart::Dx,
                } if id == "move" => 250.0,
                Live::Value { name, .. } if name == "step" => 3.4,
                _ => 0.0,
            }
        }

        fn ease(&self, _: &str, _: Ease, value: f64) -> f64 {
            value
        }
    }

    #[test]
    fn a_drag_moves_what_binds_to_it_snapped_and_nothing_bound_to_another() {
        let moved = live(serde_json::json!({
            "op": "plus",
            "a": 480,
            "b": { "op": "snap", "a": { "op": "drag", "id": "move", "part": "dx" }, "b": 240 },
        }));
        assert_eq!(number(&moved, &Fixed), 720.0);
        let other = live(serde_json::json!({ "op": "drag", "id": "resize", "part": "dx" }));
        assert_eq!(number(&other, &Fixed), 0.0);
        let lit = live(serde_json::json!({
            "op": "eq",
            "a": { "op": "floor", "x": { "op": "value", "name": "step", "fraction": false } },
            "b": 3,
        }));
        assert_eq!(number(&lit, &Fixed), 1.0);
    }
}
