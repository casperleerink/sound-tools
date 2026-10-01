//! Automation lanes: a number of a device of a track, or the track's own volume or pan, moving
//! over the project timeline.
//!
//! A lane is saved in the track record, in project ticks, with values in the units of its
//! number. The behaviour of the track finds each number by its field name, as the device names
//! it with `BehaviourContext::automation`, and plays the lanes of each device through one
//! [`LanePlayer`] connected to that device alone. So each player sees the transport the device
//! sees, with its latency lead, and its events need no device id. The lanes of the track itself
//! go to its mixer the same way.
//!
//! A clip takes the line under it along when it moves or is copied, see [`moves`].

mod moves;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sound_core::{
    Automation, BehaviourContext, BehaviourError, EventOutput, InputEndpoint, InstanceId,
    OutputEndpoint, ParameterInfo, PlayedLanes, Ports, PrepareConfig, ProcessContext, Processor,
    Project, Ticks, ValueRange,
};
use sound_notes::{Point, check_order, value_at};

pub use moves::{Carried, LaneMove, Moved, Travel, clear, moved, positions, travel_in};
pub(crate) use moves::{ON_THE_LINE, write};

use crate::TrackState;
use crate::decibels;
use crate::mixer::Mixer;

/// The output of a track that carries the lanes of the track itself, to its mixer.
pub(crate) const TRACK_AUTOMATION: &str = "automation";

/// The players of a track: of each device, by its name after this, and of the track itself.
const PLAYER: &str = "automation";

/// One automation lane, saved in the track record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationLane {
    /// The child of the track that holds the number, by its name: `filter` for `filter.json`.
    /// Left out for the volume and the pan of the track itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The field of the number in the record of the device, or `gain_db` or `pan` of the track.
    pub parameter: String,
    /// In project ticks, in tick order, one per tick, at least one. Between two points the
    /// value moves in a straight line on the travel of the number's knob. Before the first
    /// point the lane holds the first value, after the last the last.
    pub points: Vec<Point<AutomationValue>>,
}

/// The value of a point, in the units of its number: a number, or `"-inf"` for the silence of
/// a volume, as the track record saves its gain.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AutomationValue(pub f32);

impl Serialize for AutomationValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        decibels::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for AutomationValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        decibels::deserialize(deserializer).map(Self)
    }
}

impl AutomationLane {
    /// The form of the lanes of a track, which the record alone decides: a device that cannot
    /// be a child, an empty lane, points out of order, and a number with two lanes. A record
    /// with one of them does not load, as a clip whose lane is out of order does not.
    ///
    /// Whether the number exists and its values are in its range is not here: that is up to
    /// the device, which may arrive later, so the behaviour reports it and leaves the lane out.
    /// The volume and the pan of the track follow the same rule, so there is one way a lane
    /// with a wrong number or value fails.
    pub(crate) fn check_all(lanes: &[Self]) -> Result<(), String> {
        for (index, lane) in lanes.iter().enumerate() {
            let field = format!("automation[{index}]");
            if let Some(device) = &lane.device
                && !crate::is_child_name(device)
            {
                return Err(format!(
                    "{field}.device must be the name of a file in the track folder without `.json`: lowercase letters, digits, `-` and `_`, not {device:?}"
                ));
            }
            if lane.points.is_empty() {
                return Err(format!(
                    "{field}.points must hold at least one point. Delete the lane to stop the automation"
                ));
            }
            check_order(&format!("{field}.points"), &lane.points)?;
            let same =
                |other: &Self| other.device == lane.device && other.parameter == lane.parameter;
            if let Some(first) = lanes.iter().take(index).position(same) {
                return Err(format!(
                    "{field} moves the same number as automation[{first}]. One number has one lane: put the points in one of them"
                ));
            }
        }
        Ok(())
    }

    /// An error that names the first point outside the range of `parameter`.
    fn check_values(&self, field: &str, parameter: &ParameterInfo) -> Result<(), String> {
        let range = parameter.range;
        let mut points = self.points.iter().enumerate();
        let Some((index, point)) = points.find(|(_, point)| !range.contains(point.value.0)) else {
            return Ok(());
        };
        let ValueRange { min, max, .. } = range;
        Err(format!(
            "{field}.points[{index}].value must be from {min} to {max}, not {}",
            point.value.0
        ))
    }
}

/// Every number of `track`, whose record is `state`, that a lane can move, as a lane with no
/// points yet: its own volume and pan, then those of its instrument and of its effects in the
/// order of the chain, as their behaviours named them. [`AutomationLane::number`] gives the
/// range and the record value of each.
pub fn automatable(
    project: &Project,
    track: &InstanceId,
    state: &TrackState,
) -> Vec<AutomationLane> {
    let lane = |device: Option<&str>, field: &str| AutomationLane {
        device: device.map(str::to_string),
        parameter: field.to_string(),
        points: Vec::new(),
    };
    let own = Mixer::AUTOMATION.parameters().iter();
    let own = own.map(|parameter| lane(None, parameter.field));
    let effects = state.effects.iter().map(|slot| slot.name.as_str());
    let devices = std::iter::once(crate::INSTRUMENT).chain(effects);
    let devices = devices.filter_map(|name| Some((name, track.child(name).ok()?)));
    let of_devices = devices.flat_map(|(name, device)| {
        let fields = project.automatable(&device);
        fields
            .map(|field| lane(Some(name), field))
            .collect::<Vec<_>>()
    });
    own.chain(of_devices).collect()
}

/// One automation lane as it plays: the index and the field of its number, its range, and its
/// points as places on the travel, so a straight line between them is straight on the knob.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LaneLine {
    parameter: u16,
    field: &'static str,
    range: ValueRange,
    positions: Vec<Point<f32>>,
}

impl LaneLine {
    /// The value at `tick`, not rounded: what the device hears, and what its knob shows.
    fn value_at(&self, tick: Ticks) -> Option<f32> {
        let position = value_at(&self.positions, tick)?;
        Some(self.range.exact(position))
    }
}

/// The lanes of one device, or of the track itself: what its player plays, and what the views
/// show on the knobs of its numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LaneLines(Vec<LaneLine>);

impl PlayedLanes for LaneLines {
    fn values_at(&self, tick: Ticks) -> Vec<(&'static str, f32)> {
        let lanes = self.0.iter();
        let values = lanes.filter_map(|lane| Some((lane.field, lane.value_at(tick)?)));
        values.collect()
    }
}

/// Plays the lanes of one device on the audio thread: every block, the value of each lane at
/// the end of the block, at offset 0, also while the project does not play. The device ramps to
/// it over the block, so a sweep follows the line.
///
/// Every block and not only when a value moved: a device takes a number that hears nothing in a
/// block back to its record, which is how it learns that a lane or its player went away.
///
/// A number has one lane, so a block holds at most as many events as the device has numbers,
/// which `MAX_AUTOMATED` keeps far under the capacity of an event port.
#[derive(Default)]
pub(crate) struct LanePlayer {
    lanes: Arc<LaneLines>,
}

impl LanePlayer {
    pub const OUTPUT: EventOutput<Automation> = EventOutput::new(0);
}

impl Processor for LanePlayer {
    type Update = Arc<LaneLines>;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Self::Update) {
        // The old lanes ride back to the control thread inside the update.
        std::mem::swap(&mut self.lanes, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        // Where the next block starts, so the ramp of this block ends on the line.
        let tick = context.transport.tick_range.end;
        for lane in &self.lanes.0 {
            if let Some(value) = lane.value_at(tick) {
                let event = Automation {
                    parameter: lane.parameter,
                    value,
                };
                context.event_outputs.push(Self::OUTPUT, 0, event);
            }
        }
    }
}

/// The lanes of a track: one player per device that has lanes, connected to its automation
/// input, and one for the track itself, which becomes its [`TRACK_AUTOMATION`] output. A lane
/// whose device or number is not there, or whose values are outside the range of the number,
/// is reported and plays nothing, and the rest plays. The views are shown what each player
/// plays.
pub(crate) fn play(
    lanes: &[AutomationLane],
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let mut players: BTreeMap<Option<&str>, (Option<InputEndpoint>, Vec<LaneLine>)> =
        BTreeMap::new();
    for (index, lane) in lanes.iter().enumerate() {
        match resolve(context, index, lane) {
            Ok((input, played)) => {
                let player = players.entry(lane.device.as_deref());
                player.or_insert_with(|| (input, Vec::new())).1.push(played);
            }
            Err(message) => context.problem(format!("{message}, so the lane moves nothing")),
        }
    }
    for (device, (input, lanes)) in players {
        let name = match device {
            Some(device) => format!("{PLAYER}/{device}"),
            None => PLAYER.to_string(),
        };
        let player = context.processor(&name, LanePlayer::default)?;
        let lanes = Arc::new(LaneLines(lanes));
        context.update(player, lanes.clone())?;
        context.show_lanes(device, lanes);
        let output = OutputEndpoint::new(player, LanePlayer::OUTPUT);
        match input {
            Some(input) => context.connect(output.to(input))?,
            None => context.output(TRACK_AUTOMATION, output),
        }
    }
    Ok(())
}

/// Where a lane goes, `None` for the mixer of the track, and how it plays. The error says why
/// it cannot play.
fn resolve(
    context: &BehaviourContext<'_>,
    index: usize,
    lane: &AutomationLane,
) -> Result<(Option<InputEndpoint>, LaneLine), String> {
    let field = format!("automation[{index}]");
    let track = Mixer::AUTOMATION
        .parameters()
        .map(|parameter| parameter.info());
    let (input, parameters) = match &lane.device {
        Some(name) => {
            let (input, parameters) = context
                .child_automation(name)
                .ok_or_else(|| missing_device(context, &field, name))?;
            (Some(input), parameters)
        }
        None => (None, &track[..]),
    };
    let found = parameters.iter().enumerate();
    let mut found = found.filter(|(_, info)| info.field == lane.parameter);
    let Some((place, info)) = found.next() else {
        let fields: Vec<&str> = parameters.iter().map(|info| info.field).collect();
        return Err(format!(
            "{field}.parameter is {:?}, and {} takes no automation of a number of that name. It takes {}",
            lane.parameter,
            lane.device.as_deref().unwrap_or("the track"),
            fields.join(", ")
        ));
    };
    lane.check_values(&field, info)?;
    let parameter = u16::try_from(place)
        .map_err(|_| format!("{field}.parameter is past the first {} numbers", u16::MAX))?;
    let played = LaneLine {
        parameter,
        field: info.field,
        range: info.range,
        positions: positions(&lane.points, info.range),
    };
    Ok((input, played))
}

/// Why a lane has no device behind it: no record at all, or one that takes no automation.
fn missing_device(context: &BehaviourContext<'_>, field: &str, name: &str) -> String {
    match context.child_names().any(|child| child == name) {
        true => format!("{field}.device is {name:?}, and {name}.json takes no automation"),
        false => format!(
            "{field}.device is {name:?}, and this track has no {name}.json. Write that record, or take the lane out"
        ),
    }
}
